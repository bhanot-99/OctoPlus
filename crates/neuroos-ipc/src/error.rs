//! IPC errors. Every variant maps to a wire `ErrorCode` and is either retryable
//! or fatal (rules.md §6.4). Messages never contain user content (§6.3).

use neuroos_proto::v1::{Error as WireError, ErrorCode};

/// Errors from framing, the server or the client.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Socket I/O failed.
    #[error("socket i/o: {0}")]
    Io(#[from] std::io::Error),
    /// A frame's declared length is above the limit.
    #[error("frame of {len} bytes exceeds the {max}-byte limit")]
    FrameTooLarge {
        /// Declared length.
        len: u32,
        /// Configured maximum.
        max: u32,
    },
    /// The stream ended in the middle of a frame.
    #[error("connection closed mid-frame")]
    Truncated,
    /// The payload is not a valid Envelope.
    #[error("malformed envelope: {0}")]
    Decode(#[from] neuroos_proto::DecodeError),
    /// The peer speaks another schema version.
    #[error("unsupported schema version {0}")]
    UnsupportedVersion(u32),
    /// The connection is gone.
    #[error("connection closed")]
    Closed,
    /// No server is reachable right now (client side, AB-10).
    #[error("server unavailable")]
    Unavailable,
    /// The request's deadline passed.
    #[error("deadline exceeded")]
    DeadlineExceeded,
    /// The request was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The peer failed the UID or unit check (AB-9).
    #[error("peer rejected: {0}")]
    PeerRejected(&'static str),
    /// The other side broke the protocol.
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    /// The server answered with an Error frame.
    #[error("remote error {code:?}: {message}")]
    Remote {
        /// Wire code.
        code: ErrorCode,
        /// Server-provided text (no user content by rule).
        message: String,
        /// Whether retrying may help.
        retryable: bool,
    },
}

impl IpcError {
    /// Whether the caller should retry with backoff (rules.md §6.4).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            IpcError::Io(_)
            | IpcError::Closed
            | IpcError::Unavailable
            | IpcError::DeadlineExceeded
            | IpcError::Truncated => true,
            IpcError::Remote { retryable, .. } => *retryable,
            IpcError::FrameTooLarge { .. }
            | IpcError::Decode(_)
            | IpcError::UnsupportedVersion(_)
            | IpcError::Cancelled
            | IpcError::PeerRejected(_)
            | IpcError::Protocol(_) => false,
        }
    }

    /// The wire error a server sends for this failure.
    #[must_use]
    pub fn to_wire(&self) -> WireError {
        let code = match self {
            IpcError::Io(_) | IpcError::Closed | IpcError::Unavailable => ErrorCode::Unavailable,
            IpcError::FrameTooLarge { .. } => ErrorCode::FrameTooLarge,
            IpcError::Truncated | IpcError::Decode(_) | IpcError::Protocol(_) => {
                ErrorCode::InvalidRequest
            }
            IpcError::UnsupportedVersion(_) => ErrorCode::UnsupportedVersion,
            IpcError::DeadlineExceeded => ErrorCode::DeadlineExceeded,
            IpcError::Cancelled => ErrorCode::Cancelled,
            IpcError::PeerRejected(_) => ErrorCode::PermissionDenied,
            IpcError::Remote { code, .. } => *code,
        };
        WireError {
            code: code.into(),
            message: self.to_string(),
            retryable: self.is_retryable(),
        }
    }

    /// Builds the client-side error for a received Error frame.
    #[must_use]
    pub fn from_wire(e: &WireError) -> Self {
        IpcError::Remote {
            code: ErrorCode::try_from(e.code).unwrap_or(ErrorCode::Unspecified),
            message: e.message.clone(),
            retryable: e.retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_and_code_table() {
        let cases: Vec<(IpcError, bool, ErrorCode)> = vec![
            (IpcError::Closed, true, ErrorCode::Unavailable),
            (IpcError::Unavailable, true, ErrorCode::Unavailable),
            (
                IpcError::DeadlineExceeded,
                true,
                ErrorCode::DeadlineExceeded,
            ),
            (IpcError::Truncated, true, ErrorCode::InvalidRequest),
            (
                IpcError::FrameTooLarge { len: 9, max: 8 },
                false,
                ErrorCode::FrameTooLarge,
            ),
            (
                IpcError::UnsupportedVersion(2),
                false,
                ErrorCode::UnsupportedVersion,
            ),
            (IpcError::Cancelled, false, ErrorCode::Cancelled),
            (
                IpcError::PeerRejected("uid"),
                false,
                ErrorCode::PermissionDenied,
            ),
            (IpcError::Protocol("x"), false, ErrorCode::InvalidRequest),
            (
                IpcError::Io(std::io::Error::other("x")),
                true,
                ErrorCode::Unavailable,
            ),
        ];
        for (err, retry, code) in cases {
            assert_eq!(err.is_retryable(), retry, "{err}");
            let w = err.to_wire();
            assert_eq!(w.code, i32::from(code), "{err}");
            assert_eq!(w.retryable, retry);
        }
    }

    #[test]
    fn remote_round_trip() {
        let w = WireError {
            code: ErrorCode::Busy.into(),
            message: "busy".into(),
            retryable: true,
        };
        let e = IpcError::from_wire(&w);
        assert!(e.is_retryable());
        assert_eq!(e.to_wire().code, i32::from(ErrorCode::Busy));
        let unknown = IpcError::from_wire(&WireError {
            code: 9999,
            message: String::new(),
            retryable: false,
        });
        assert!(matches!(
            unknown,
            IpcError::Remote {
                code: ErrorCode::Unspecified,
                ..
            }
        ));
    }

    #[test]
    fn decode_error_is_fatal() {
        use neuroos_proto::Message;
        let decode_err = neuroos_proto::v1::Envelope::decode(&[0x0f_u8, 0xff][..]).unwrap_err();
        let e = IpcError::from(decode_err);
        assert!(!e.is_retryable());
        assert_eq!(e.to_wire().code, i32::from(ErrorCode::InvalidRequest));
    }
}
