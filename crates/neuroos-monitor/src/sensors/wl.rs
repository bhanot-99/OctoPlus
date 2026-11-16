//! A tiny Wayland client loop shared by the focus and idle sensors. Wayland dispatch blocks,
//! so each sensor runs it on its own thread and talks to the async side over a bounded
//! channel (AB-11). The loop wakes at least twice a second to notice a stop request, so a
//! cancelled sensor never leaves a thread behind.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::{Connection, EventQueue};

/// How long one wait for compositor data lasts before the stop flag is re-checked.
const WAKE: Duration = Duration::from_millis(500);

/// Why the loop ended abnormally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lost {
    /// The compositor closed the connection or sent garbage.
    Connection,
}

/// Dispatches events into `state` until `stop` is set (`Ok`) or the connection is lost.
///
/// # Errors
/// [`Lost::Connection`] if reading, flushing or dispatching fails.
pub fn pump<S>(queue: &mut EventQueue<S>, state: &mut S, stop: &AtomicBool) -> Result<(), Lost> {
    let timeout = Timespec {
        tv_sec: 0,
        tv_nsec: i64::try_from(WAKE.as_nanos()).unwrap_or(500_000_000),
    };
    while !stop.load(Ordering::Acquire) {
        queue
            .dispatch_pending(state)
            .map_err(|_| Lost::Connection)?;
        queue.flush().map_err(|_| Lost::Connection)?;
        // `None` means events arrived since the last dispatch: go round again.
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let ready = {
            let fd = guard.connection_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            poll(&mut fds, Some(&timeout)).unwrap_or(0)
        };
        if ready > 0 {
            guard.read().map_err(|_| Lost::Connection)?;
        } else {
            drop(guard);
        }
    }
    Ok(())
}

/// Connects to the compositor named by the environment (`WAYLAND_DISPLAY`).
///
/// # Errors
/// [`Lost::Connection`] if there is no compositor to connect to.
pub fn connect() -> Result<Connection, Lost> {
    Connection::connect_to_env().map_err(|_| Lost::Connection)
}

/// Sets the flag when dropped: a cancelled sensor future stops its Wayland thread.
#[derive(Debug)]
pub struct StopOnDrop(pub std::sync::Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
