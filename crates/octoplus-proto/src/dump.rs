//! The recorded-dump file format (FR-MON-12, P3-S06). It lives here, not in the monitor,
//! because C1 writes it and `octoplusctl` and C3's replay harness read it (AB-1).
//!
//! Layout: `MAGIC` · `u32 LE` version · repeated (`u32 LE` length · `MonitorEvent` bytes) ·
//! `u32 LE` [`END_MARK`] · `u64 LE` event count. The end record makes truncation
//! detectable (AB-12): a dump without it is incomplete.

use std::io::{self, Read, Write};

use prost::Message;

use crate::v1::MonitorEvent;

/// First eight bytes of every dump.
pub const MAGIC: &[u8; 8] = b"NOSDUMP\0";
/// The only format version this code reads and writes.
pub const VERSION: u32 = 1;
/// Length value that introduces the end record instead of an event.
pub const END_MARK: u32 = u32::MAX;
/// Largest single event accepted; a larger length means a corrupt file.
pub const MAX_EVENT_BYTES: u32 = 1 << 20;

/// Why a dump could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum DumpError {
    /// The file system failed.
    #[error("dump i/o failed: {0}")]
    Io(#[from] io::Error),
    /// The file does not start with the dump magic.
    #[error("not a octoplus dump (bad magic)")]
    BadMagic,
    /// The dump was written by a version this build does not know.
    #[error("unsupported dump version {0}")]
    UnsupportedVersion(u32),
    /// An event length is larger than any event can be.
    #[error("corrupt dump: event length {0} exceeds the limit")]
    TooLarge(u32),
    /// An event did not decode.
    #[error("corrupt dump: undecodable event")]
    BadEvent,
    /// The file ended before the end record (a crashed or killed recorder).
    #[error("dump is truncated: no end record after {events} events")]
    Truncated {
        /// Events read before the file ended.
        events: u64,
    },
    /// The end record's count differs from the events actually present.
    #[error("corrupt dump: end record says {declared} events, found {found}")]
    CountMismatch {
        /// Count in the end record.
        declared: u64,
        /// Events actually read.
        found: u64,
    },
}

/// Writes a dump. Call [`DumpWriter::finish`] to write the end record.
#[derive(Debug)]
pub struct DumpWriter<W: Write> {
    out: W,
    count: u64,
}

impl<W: Write> DumpWriter<W> {
    /// Starts a dump by writing the header.
    ///
    /// # Errors
    /// [`DumpError::Io`] if the header cannot be written.
    pub fn new(mut out: W) -> Result<Self, DumpError> {
        out.write_all(MAGIC)?;
        out.write_all(&VERSION.to_le_bytes())?;
        Ok(Self { out, count: 0 })
    }

    /// Appends one event.
    ///
    /// # Errors
    /// [`DumpError::TooLarge`] for an event over the limit, [`DumpError::Io`] on write failure.
    pub fn write(&mut self, event: &MonitorEvent) -> Result<(), DumpError> {
        let len = u32::try_from(event.encoded_len()).map_err(|_| DumpError::TooLarge(u32::MAX))?;
        if len > MAX_EVENT_BYTES {
            return Err(DumpError::TooLarge(len));
        }
        self.out.write_all(&len.to_le_bytes())?;
        self.out.write_all(&event.encode_to_vec())?;
        self.count += 1;
        Ok(())
    }

    /// Events written so far.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Flushes buffered bytes without ending the dump.
    ///
    /// # Errors
    /// [`DumpError::Io`] on failure.
    pub fn flush(&mut self) -> Result<(), DumpError> {
        Ok(self.out.flush()?)
    }

    /// Writes the end record and flushes. Returns the event count.
    ///
    /// # Errors
    /// [`DumpError::Io`] on failure.
    pub fn finish(mut self) -> Result<u64, DumpError> {
        self.out.write_all(&END_MARK.to_le_bytes())?;
        self.out.write_all(&self.count.to_le_bytes())?;
        self.out.flush()?;
        Ok(self.count)
    }
}

/// Reads a dump written by [`DumpWriter`].
#[derive(Debug)]
pub struct DumpReader<R: Read> {
    input: R,
    count: u64,
    done: bool,
}

fn read_exact_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..])? {
            0 if filled == 0 => return Ok(false),
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => filled += n,
        }
    }
    Ok(true)
}

impl<R: Read> DumpReader<R> {
    /// Checks the header.
    ///
    /// # Errors
    /// [`DumpError::BadMagic`], [`DumpError::UnsupportedVersion`], or I/O failure
    /// (a file shorter than the header is [`DumpError::BadMagic`]).
    pub fn new(mut input: R) -> Result<Self, DumpError> {
        let mut magic = [0u8; 8];
        if !read_exact_or_eof(&mut input, &mut magic).map_err(|_| DumpError::BadMagic)?
            || &magic != MAGIC
        {
            return Err(DumpError::BadMagic);
        }
        let mut ver = [0u8; 4];
        input
            .read_exact(&mut ver)
            .map_err(|_| DumpError::BadMagic)?;
        let version = u32::from_le_bytes(ver);
        if version != VERSION {
            return Err(DumpError::UnsupportedVersion(version));
        }
        Ok(Self {
            input,
            count: 0,
            done: false,
        })
    }

    /// The next event, or `None` after a valid end record.
    ///
    /// # Errors
    /// Truncation, a bad length or event, or a count mismatch. After an error the reader
    /// returns `None`.
    pub fn next_event(&mut self) -> Result<Option<MonitorEvent>, DumpError> {
        if self.done {
            return Ok(None);
        }
        let result = self.read_one();
        if !matches!(result, Ok(Some(_))) {
            self.done = true;
        }
        result
    }

    fn read_one(&mut self) -> Result<Option<MonitorEvent>, DumpError> {
        let truncated = |events| DumpError::Truncated { events };
        let mut len_buf = [0u8; 4];
        match read_exact_or_eof(&mut self.input, &mut len_buf) {
            Ok(true) => {}
            Ok(false) | Err(_) => return Err(truncated(self.count)),
        }
        let len = u32::from_le_bytes(len_buf);
        if len == END_MARK {
            let mut n = [0u8; 8];
            self.input
                .read_exact(&mut n)
                .map_err(|_| truncated(self.count))?;
            let declared = u64::from_le_bytes(n);
            return if declared == self.count {
                Ok(None)
            } else {
                Err(DumpError::CountMismatch {
                    declared,
                    found: self.count,
                })
            };
        }
        if len > MAX_EVENT_BYTES {
            return Err(DumpError::TooLarge(len));
        }
        let mut body = vec![0u8; len as usize];
        self.input
            .read_exact(&mut body)
            .map_err(|_| truncated(self.count))?;
        let ev = MonitorEvent::decode(body.as_slice()).map_err(|_| DumpError::BadEvent)?;
        self.count += 1;
        Ok(Some(ev))
    }
}
