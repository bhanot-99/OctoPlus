//! Status and switches for OctoPit (Phase 12, story C-S06). Status and switches only: no text
//! to speak, nothing that acts on the computer, no chat. The owner's own click is the request.

use octoplus_ipc::HandlerError;
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{
    Ack, CompanionAction, CompanionControl, CompanionMemory, CompanionStatus, CompanionTalk,
    DispatchStatus, ErrorCode, Origin, ProfileListRequest,
};
use octoplus_taint::TaintFlags;

use super::{Call, KnowledgeHandler};
use crate::companion::humour::{Feedback, HumourLevel};
use crate::companion::memory::{NOTICED_MARKER, plain_form};
use crate::companion::pulse::{PulseCommand, clock_label, response_label, topic_label};

fn humour_label(h: HumourLevel) -> &'static str {
    match h {
        HumourLevel::Off => "off",
        HumourLevel::Low => "low",
        HumourLevel::Normal => "normal",
        HumourLevel::High => "high",
    }
}

impl KnowledgeHandler {
    fn local_minute(now: u64) -> u16 {
        u16::try_from(crate::nudge::signals::local_minute(
            now,
            &crate::nudge::boot::time_zone(),
        ))
        .unwrap_or(0)
    }

    /// What OctoPit shows about her: rhythm, today's talks and what she remembers.
    pub(super) async fn companion_status(&self) -> Result<Body, HandlerError> {
        let now = octoplus_common::time::now_ns().unwrap_or(0);
        let enabled = self.persona.is_some() && self.companion_cfg.enabled;
        let mut out = CompanionStatus {
            enabled,
            ..CompanionStatus::default()
        };
        if let Some(hub) = &self.hub {
            let st = hub.status(now);
            out.paused = st.paused;
            out.quiet_minutes_left = st.quiet_minutes_left;
            out.talks_today = st.talks_today;
            out.talks_last_hour = st.talks_last_hour;
            out.per_day_cap = st.config.per_day_cap;
            out.per_hour_max = st.config.per_hour_max;
            out.quiet_start = clock_label(st.config.quiet_start_min);
            out.quiet_end = clock_label(st.config.quiet_end_min);
            out.humour = humour_label(st.humour).to_string();
            out.chattiness_percent = st.chattiness_percent;
            out.memories_by_chance = u32::try_from(st.memory.saved).unwrap_or(u32::MAX);
            out.talks = st
                .talks
                .iter()
                .map(|t| CompanionTalk {
                    at_ns: t.at_ns,
                    topic: topic_label(t.topic).to_string(),
                    reaction: response_label(t.response).to_string(),
                })
                .collect();
        }
        let listed = self
            .peers
            .storage
            .call(
                Body::ProfileListRequest(ProfileListRequest {
                    relevant_to: String::new(),
                }),
                self.timing.storage_query,
            )
            .await;
        if let Ok(Body::ProfileFacts(f)) = listed {
            out.memory_available = true;
            let mut facts = f.facts;
            facts.sort_by_key(|x| std::cmp::Reverse(x.id));
            out.memories = facts
                .into_iter()
                .map(|p| CompanionMemory {
                    id: p.id,
                    noticed: p.text.starts_with(NOTICED_MARKER),
                    text: plain_form(&p.text).to_string(),
                    saved_at_ns: p.saved_at_ns,
                })
                .collect();
        }
        Ok(Body::CompanionStatus(out))
    }

    /// One switch from OctoPit.
    pub(super) async fn companion_control(
        &self,
        c: CompanionControl,
    ) -> Result<Body, HandlerError> {
        let now = octoplus_common::time::now_ns().unwrap_or(0);
        let minute = Self::local_minute(now);
        let Some(hub) = &self.hub else {
            return Err(HandlerError::new(
                ErrorCode::Unavailable,
                "the companion is not running",
                false,
            ));
        };
        match CompanionAction::try_from(c.action).unwrap_or(CompanionAction::Unspecified) {
            CompanionAction::Pause => hub.control(now, minute, &PulseCommand::Pause),
            CompanionAction::Resume => hub.control(now, minute, &PulseCommand::Resume),
            CompanionAction::ChatMore => hub.control(now, minute, &PulseCommand::ChatMore),
            CompanionAction::ChatLess => hub.control(now, minute, &PulseCommand::ChatLess),
            CompanionAction::QuietFor => {
                let minutes = c.minutes.clamp(1, 600);
                hub.control(now, minute, &PulseCommand::QuietFor { minutes });
            }
            CompanionAction::Funny => hub.record_feedback(Feedback::Funny),
            CompanionAction::NotFunny => hub.record_feedback(Feedback::NotFunny),
            CompanionAction::Forget => {
                if c.id == 0 || !self.forget_remembered_fact(c.id, now).await {
                    return Err(HandlerError::new(
                        ErrorCode::InvalidRequest,
                        "that memory could not be forgotten",
                        false,
                    ));
                }
            }
            CompanionAction::Unspecified => {
                return Err(HandlerError::new(
                    ErrorCode::InvalidRequest,
                    "unknown companion control",
                    false,
                ));
            }
        }
        Ok(Body::Ack(Ack {}))
    }

    /// The owner clicked "forget" on a remembered fact: one audited call to the safety kernel,
    /// the same one a spoken "forget that" makes.
    async fn forget_remembered_fact(&self, id: u64, now: u64) -> bool {
        let args = serde_json::json!({ "fact_id": id }).to_string();
        let call = Call {
            skill: "profile.forget",
            args_json: &args,
            taint: TaintFlags::USER_PROVIDED,
            origin: Origin::Cli,
            question: "",
            at_ns: now,
        };
        matches!(
            self.send_dispatch(&call).await,
            Ok((r, _)) if DispatchStatus::try_from(r.status) == Ok(DispatchStatus::Executed)
        )
    }
}
