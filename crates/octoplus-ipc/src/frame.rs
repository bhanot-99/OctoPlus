//! Framing (Architecture §5.1): 4-byte little-endian length, then one
//! protobuf `Envelope`. The length is checked against the limit **before**
//! any allocation, so a hostile length cannot exhaust memory.

use octoplus_proto::Message;
use octoplus_proto::v1::Envelope;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::IpcError;

/// Size of the length prefix.
pub const PREFIX_LEN: usize = 4;

/// Encodes one frame (prefix + payload).
///
/// # Errors
/// [`IpcError::FrameTooLarge`] if the encoded Envelope exceeds `max`.
pub fn encode(env: &Envelope, max: u32) -> Result<Vec<u8>, IpcError> {
    let len = env.encoded_len();
    let len32 = u32::try_from(len).map_err(|_| IpcError::FrameTooLarge { len: u32::MAX, max })?;
    if len32 > max {
        return Err(IpcError::FrameTooLarge { len: len32, max });
    }
    let mut out = Vec::with_capacity(PREFIX_LEN + len);
    out.extend_from_slice(&len32.to_le_bytes());
    // Encoding into a Vec cannot run out of space; the error arm is defensive.
    env.encode(&mut out)
        .map_err(|_| IpcError::Protocol("encode buffer"))?;
    Ok(out)
}

/// Reads one frame. `Ok(None)` means the peer closed cleanly between frames.
///
/// # Errors
/// [`IpcError::Truncated`] when the stream ends inside a frame,
/// [`IpcError::FrameTooLarge`] for a declared length above `max`,
/// [`IpcError::Decode`] for a payload that is not an Envelope.
pub async fn read_frame<R: AsyncRead + Unpin>(
    r: &mut R,
    max: u32,
) -> Result<Option<Envelope>, IpcError> {
    let mut prefix = [0u8; PREFIX_LEN];
    let mut got = 0;
    while got < PREFIX_LEN {
        let n = r.read(&mut prefix[got..]).await?;
        if n == 0 {
            return if got == 0 {
                Ok(None)
            } else {
                Err(IpcError::Truncated)
            };
        }
        got += n;
    }
    let len = u32::from_le_bytes(prefix);
    if len > max {
        return Err(IpcError::FrameTooLarge { len, max });
    }
    let mut payload = vec![0u8; len as usize];
    match r.read_exact(&mut payload).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(IpcError::Truncated);
        }
        Err(e) => return Err(IpcError::Io(e)),
    }
    Ok(Some(Envelope::decode(payload.as_slice())?))
}

/// Writes one frame and flushes it.
///
/// # Errors
/// Oversized Envelope or socket failure.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    env: &Envelope,
    max: u32,
) -> Result<(), IpcError> {
    let bytes = encode(env, max)?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use octoplus_proto::v1::Ping;
    use octoplus_proto::v1::envelope::Body;
    use proptest::prelude::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
    }

    fn ping(payload: Vec<u8>) -> Envelope {
        Envelope {
            schema_version: 1,
            request_id: 7,
            body: Some(Body::Ping(Ping { payload })),
            ..Envelope::default()
        }
    }

    #[test]
    fn empty_stream_is_clean_eof() {
        let mut r: &[u8] = &[];
        assert!(rt().block_on(read_frame(&mut r, 64)).unwrap().is_none());
    }

    #[test]
    fn truncated_prefix_and_payload() {
        for bytes in [vec![1u8], vec![1, 0, 0], vec![5, 0, 0, 0, 1, 2]] {
            let mut r: &[u8] = &bytes;
            let res = rt().block_on(read_frame(&mut r, 64));
            assert!(matches!(res, Err(IpcError::Truncated)), "{bytes:?}");
        }
    }

    #[test]
    fn oversized_length_rejected_before_allocation() {
        let bytes = u32::MAX.to_le_bytes();
        let mut r: &[u8] = &bytes;
        let res = rt().block_on(read_frame(&mut r, 1024));
        assert!(matches!(
            res,
            Err(IpcError::FrameTooLarge {
                len: u32::MAX,
                max: 1024
            })
        ));
    }

    #[test]
    fn maximum_frame_is_accepted_and_one_more_rejected() {
        let env = ping(vec![0xAB; 100]);
        let max = u32::try_from(env.encoded_len()).unwrap();
        let bytes = encode(&env, max).unwrap();
        let mut r: &[u8] = &bytes;
        assert_eq!(
            rt().block_on(read_frame(&mut r, max)).unwrap(),
            Some(env.clone())
        );
        assert!(matches!(
            encode(&env, max - 1),
            Err(IpcError::FrameTooLarge { .. })
        ));
        let mut r: &[u8] = &bytes;
        assert!(matches!(
            rt().block_on(read_frame(&mut r, max - 1)),
            Err(IpcError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn zero_length_frame_is_an_empty_envelope() {
        let bytes = [0u8, 0, 0, 0];
        let mut r: &[u8] = &bytes;
        assert_eq!(
            rt().block_on(read_frame(&mut r, 8)).unwrap(),
            Some(Envelope::default())
        );
    }

    #[test]
    fn malformed_payload_is_decode_error() {
        let bytes = [3u8, 0, 0, 0, 0x0f, 0xff, 0xff];
        let mut r: &[u8] = &bytes;
        assert!(matches!(
            rt().block_on(read_frame(&mut r, 64)),
            Err(IpcError::Decode(_))
        ));
    }

    /// Yields `prefix`, then fails with an I/O error (not EOF).
    struct FailAfter(Vec<u8>);

    impl AsyncRead for FailAfter {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.0.is_empty() {
                return std::task::Poll::Ready(Err(std::io::Error::other("boom")));
            }
            let n = self.0.len().min(buf.remaining());
            buf.put_slice(&self.0[..n]);
            self.0.drain(..n);
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn io_errors_are_reported_not_hidden() {
        let rt = rt();
        // Failure inside the prefix and inside the payload.
        for prefix in [vec![], vec![1u8], vec![9, 0, 0, 0, 1, 2]] {
            let mut r = FailAfter(prefix.clone());
            let res = rt.block_on(read_frame(&mut r, 64));
            assert!(matches!(res, Err(IpcError::Io(_))), "{prefix:?}: {res:?}");
        }
    }

    #[test]
    fn write_then_read() {
        let rt = rt();
        let env = ping(b"hi".to_vec());
        let mut buf = Vec::new();
        rt.block_on(write_frame(&mut buf, &env, 64)).unwrap();
        let mut r: &[u8] = &buf;
        assert_eq!(rt.block_on(read_frame(&mut r, 64)).unwrap(), Some(env));
        assert!(rt.block_on(read_frame(&mut r, 64)).unwrap().is_none());
    }

    proptest! {
        #[test]
        fn round_trip(payloads in proptest::collection::vec(
            proptest::collection::vec(any::<u8>(), 0..512), 0..8)) {
            let rt = rt();
            let envs: Vec<Envelope> = payloads.into_iter().map(ping).collect();
            let mut buf = Vec::new();
            for e in &envs {
                buf.extend(encode(e, 4096).unwrap());
            }
            let mut r: &[u8] = &buf;
            for e in &envs {
                prop_assert_eq!(rt.block_on(read_frame(&mut r, 4096)).unwrap(), Some(e.clone()));
            }
            prop_assert!(rt.block_on(read_frame(&mut r, 4096)).unwrap().is_none());
        }

        #[test]
        fn any_cut_is_truncated_or_clean(payload in proptest::collection::vec(any::<u8>(), 1..256),
                                         cut in 0usize..300) {
            let rt = rt();
            let bytes = encode(&ping(payload), 4096).unwrap();
            let cut = cut.min(bytes.len());
            let mut r: &[u8] = &bytes[..cut];
            let res = rt.block_on(read_frame(&mut r, 4096));
            if cut == 0 {
                prop_assert!(matches!(res, Ok(None)));
            } else if cut == bytes.len() {
                prop_assert!(matches!(res, Ok(Some(_))));
            } else {
                prop_assert!(matches!(res, Err(IpcError::Truncated)));
            }
        }
    }
}
