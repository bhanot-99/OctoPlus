//! Carrying out what the companion's memory rule decided (Phase 12, story C-S05).
//!
//! The decision itself is pure and lives in `companion/memory.rs`. Keeping a fact is an action
//! (rule R0-8), so it is done here, outside the companion code, as one ordinary audited call to
//! the safety kernel under the owner's own taint, exactly like a spoken "remember that ...".
//! Nothing the model wrote can reach this path: only the owner's own sentence is kept.

use octoplus_ipc::StreamSink;
use octoplus_proto::v1::{DispatchStatus, Origin};
use octoplus_taint::TaintFlags;
use tracing::debug;

use super::{Call, KnowledgeHandler};
use crate::companion::memory::Keep;

impl KnowledgeHandler {
    /// The companion turn decided to keep this fact; it is saved right after the reply.
    pub(super) fn queue_chance_memory(&self, keep: Keep) {
        *self
            .chance_fact
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(keep);
    }

    /// Saves the queued fact, if any, and says "got it" when the rule asked for it.
    pub(super) async fn flush_chance_memory(
        &self,
        question: &str,
        origin: Origin,
        now: u64,
        sink: &StreamSink,
    ) {
        let Some(keep) = self
            .chance_fact
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return;
        };
        // Web text in the session means the context is not purely the owner's: leave it to an
        // explicit "remember" (and its dialog).
        if self.base_taint().is_external() {
            return;
        }
        let args = serde_json::json!({ "fact": keep.fact }).to_string();
        let call = Call {
            skill: "profile.remember",
            args_json: &args,
            taint: TaintFlags::USER_PROVIDED,
            origin,
            question,
            at_ns: now,
        };
        match self.send_dispatch(&call).await {
            Ok((result, _))
                if DispatchStatus::try_from(result.status) == Ok(DispatchStatus::Executed) =>
            {
                debug!("kept a fact by chance ({:?})", keep.category);
                if keep.acknowledge {
                    let _ = Self::say(sink, " Got it.").await;
                }
            }
            _ => debug!("a fact decided by chance could not be saved"),
        }
    }
}
