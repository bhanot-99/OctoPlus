//! `kernel`-kind skills: one schema-validated message that C6 sends itself
//! on a control socket it already holds (ADR-0010 decision 2). No file access,
//! no child process; audited by the dispatch pipeline like every other skill.

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::ReconnectingClient;
use neuroos_proto::v1::PauseRequest;
use neuroos_proto::v1::envelope::Body;
use serde_json::{Value, json};

use crate::dispatch::KernelSkills;

/// Longest pause `privacy.pause` accepts (matches its manifest schema).
pub const MAX_PAUSE_MINUTES: u64 = 480;

/// Runs `privacy.pause` on C1. (C2's own pause arrives with voice in Phase 7.)
#[derive(Debug, Clone)]
pub struct ControlSkills {
    monitor: Arc<ReconnectingClient>,
    deadline: Duration,
}

impl ControlSkills {
    /// Uses the kernel's existing `monitor/control` client.
    #[must_use]
    pub fn new(monitor: Arc<ReconnectingClient>, deadline: Duration) -> Self {
        Self { monitor, deadline }
    }

    async fn privacy_pause(&self, args: &Value) -> Result<Value, String> {
        // Already schema-checked; checked again here so this function is safe on its own.
        let minutes = args["minutes"]
            .as_u64()
            .filter(|m| (1..=MAX_PAUSE_MINUTES).contains(m))
            .ok_or("bad_args")?;
        let duration_ns = minutes.saturating_mul(60 * 1_000_000_000);
        match self
            .monitor
            .call(
                Body::PauseRequest(PauseRequest { duration_ns }),
                self.deadline,
            )
            .await
        {
            Ok(Body::PrivacyState(s)) if s.paused => {
                Ok(json!({"paused": true, "minutes": minutes}))
            }
            Ok(_) => Err("monitor_refused".into()),
            Err(_) => Err("monitor_unavailable".into()),
        }
    }
}

impl KernelSkills for ControlSkills {
    async fn run(&self, capability: &str, args: &Value) -> Result<Value, String> {
        match capability {
            "privacy.pause" => self.privacy_pause(args).await,
            _ => Err("unknown_kernel_skill".into()),
        }
    }
}
