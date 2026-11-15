//! Test fixtures shared across the workspace.
//!
//! * [`EchoHandler`] — the request language used by IPC tests and the
//!   `neuroos-echo` service: a Ping payload of `echo:x`, `stream:N`,
//!   `slowstream:MS`, `sleep:MS` or `fail`.
//! * `neuroos-echo` — a minimal real service: echo socket, health socket,
//!   Landlock sandbox, clean shutdown on SIGTERM. Used to prove the unit,
//!   sandbox and reconnect machinery before any real part exists.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    Ack, ComputePath, ErrorCode, EvaluateResult, Evidence, GenerateDone, HybridResult, Pong,
    StopReason, Tier, TokenChunk,
};

/// Mock C6 kernel handler for testing capability evaluations.
#[derive(Debug, Clone, Default)]
pub struct MockKernel {
    pub allowed: bool,
    pub last_capability: Arc<Mutex<Option<String>>>,
    pub last_taint: Arc<Mutex<Option<u32>>>,
}

impl MockKernel {
    /// Creates a mock kernel that allows evaluations.
    #[must_use]
    pub fn allowing() -> Self {
        Self {
            allowed: true,
            last_capability: Arc::new(Mutex::new(None)),
            last_taint: Arc::new(Mutex::new(None)),
        }
    }

    /// Creates a mock kernel that rejects evaluations.
    #[must_use]
    pub fn rejecting() -> Self {
        Self {
            allowed: false,
            last_capability: Arc::new(Mutex::new(None)),
            last_taint: Arc::new(Mutex::new(None)),
        }
    }
}

impl Handler for MockKernel {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        let Body::EvaluateRequest(eval) = req.body else {
            return Err(HandlerError::unsupported());
        };
        if let Ok(mut lock) = self.last_capability.lock() {
            *lock = Some(eval.capability.clone());
        }
        if let Some(t) = &eval.context_taint {
            if let Ok(mut lock) = self.last_taint.lock() {
                *lock = Some(t.flags);
            }
        } else {
            return Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "context_taint is required",
                false,
            ));
        }

        Ok(Body::EvaluateResult(EvaluateResult {
            tier: if self.allowed {
                Tier::Safe as i32
            } else {
                Tier::Review as i32
            },
            allowed_without_dialog: self.allowed,
        }))
    }
}

/// Mock C3 storage handler for testing hybrid queries.
#[derive(Debug, Clone, Default)]
pub struct MockStorage {
    pub evidence: Vec<Evidence>,
    pub fail: bool,
}

impl MockStorage {
    /// Creates a mock storage that returns `evidence`.
    #[must_use]
    pub fn with_evidence(evidence: Vec<Evidence>) -> Self {
        Self {
            evidence,
            fail: false,
        }
    }

    /// Creates a mock storage that fails calls.
    #[must_use]
    pub fn failing() -> Self {
        Self {
            evidence: Vec::new(),
            fail: true,
        }
    }
}

impl Handler for MockStorage {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        let Body::QueryHybrid(_) = req.body else {
            return Err(HandlerError::unsupported());
        };
        if self.fail {
            return Err(HandlerError::new(
                ErrorCode::Unavailable,
                "storage unavailable",
                true,
            ));
        }
        Ok(Body::HybridResult(HybridResult {
            evidence: self.evidence.clone(),
            stale_index: false,
        }))
    }
}

/// Mock C4 inference handler for testing streaming generation.
#[derive(Debug, Clone, Default)]
pub struct MockInference {
    pub tokens: Vec<String>,
    pub delay: Duration,
    pub fail: bool,
    pub received_prompt: Arc<Mutex<Option<String>>>,
    pub received_stable_prefix: Arc<AtomicU32>,
    pub cancelled: Arc<AtomicBool>,
}

impl MockInference {
    /// Creates a mock inference handler that streams `tokens`.
    #[must_use]
    pub fn with_tokens(tokens: Vec<String>) -> Self {
        Self {
            tokens,
            delay: Duration::ZERO,
            fail: false,
            received_prompt: Arc::new(Mutex::new(None)),
            received_stable_prefix: Arc::new(AtomicU32::new(0)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a mock inference handler with a delay between tokens.
    #[must_use]
    pub fn with_delay(tokens: Vec<String>, delay: Duration) -> Self {
        Self {
            tokens,
            delay,
            fail: false,
            received_prompt: Arc::new(Mutex::new(None)),
            received_stable_prefix: Arc::new(AtomicU32::new(0)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a mock inference handler that fails.
    #[must_use]
    pub fn failing() -> Self {
        Self {
            tokens: Vec::new(),
            delay: Duration::ZERO,
            fail: true,
            received_prompt: Arc::new(Mutex::new(None)),
            received_stable_prefix: Arc::new(AtomicU32::new(0)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Handler for MockInference {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        let Body::GenerateRequest(g) = req.body else {
            return Err(HandlerError::unsupported());
        };
        if let Ok(mut lock) = self.received_prompt.lock() {
            *lock = Some(g.prompt);
        }
        self.received_stable_prefix
            .store(g.stable_prefix_chars, Ordering::SeqCst);

        if self.fail {
            return Err(HandlerError::new(
                ErrorCode::Unavailable,
                "inference unavailable",
                true,
            ));
        }

        struct DropGuard {
            cancelled: Arc<AtomicBool>,
            done: bool,
        }
        impl Drop for DropGuard {
            fn drop(&mut self) {
                if !self.done {
                    self.cancelled.store(true, Ordering::SeqCst);
                }
            }
        }
        let mut guard = DropGuard {
            cancelled: self.cancelled.clone(),
            done: false,
        };

        for token in &self.tokens {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if let Err(e) = sink
                .send(Body::TokenChunk(TokenChunk {
                    text: token.clone(),
                    tokens: 1,
                }))
                .await
            {
                return Err(HandlerError::from(e));
            }
        }

        guard.done = true;
        Ok(Body::GenerateDone(GenerateDone {
            reason: StopReason::EndOfText as i32,
            prompt_tokens: 10,
            cached_prompt_tokens: 5,
            generated_tokens: self.tokens.len() as u32,
            prefill_ns: 1_000_000,
            decode_ns: 5_000_000,
            path: ComputePath::Cpu as i32,
        }))
    }
}

/// Echo service handler; see the crate docs for the request language.
#[derive(Debug, Clone, Copy, Default)]
pub struct EchoHandler;

impl Handler for EchoHandler {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        let Body::Ping(p) = req.body else {
            return Err(HandlerError::unsupported());
        };
        let cmd = String::from_utf8_lossy(&p.payload).into_owned();
        let (verb, arg) = cmd.split_once(':').unwrap_or((cmd.as_str(), "0"));
        let n: u64 = arg.parse().unwrap_or(0);
        match verb {
            "echo" => Ok(Body::Pong(Pong { payload: p.payload })),
            "stream" => {
                for i in 0..n {
                    sink.send(Body::Pong(Pong {
                        payload: i.to_le_bytes().to_vec(),
                    }))
                    .await
                    .map_err(HandlerError::from)?;
                }
                Ok(Body::Ack(Ack {}))
            }
            "slowstream" => loop {
                sink.send(Body::Pong(Pong { payload: vec![] }))
                    .await
                    .map_err(HandlerError::from)?;
                tokio::time::sleep(Duration::from_millis(n)).await;
            },
            "sleep" => {
                tokio::time::sleep(Duration::from_millis(n)).await;
                Ok(Body::Pong(Pong { payload: vec![] }))
            }
            "fail" => Err(HandlerError::new(
                ErrorCode::NotFound,
                "no such thing",
                false,
            )),
            _ => Err(HandlerError::unsupported()),
        }
    }
}
