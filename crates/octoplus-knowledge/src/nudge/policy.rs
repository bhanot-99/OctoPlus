//! Pure nudge policy gate (FR-NUD-05/06/07, design.md §8.3, KPI-10).
//!
//! Decides speak, notify, hold or drop for each candidate nudge, enforces rate limits,
//! and merges held nudges when the moment clears.
//! Pure code: no I/O, no clock. Time is passed in as epoch nanoseconds (AB-13).

use std::collections::HashMap;

use super::types::{
    Action, Candidate, Decision, Delivery, PolicyConfig, Priority, Reason, UserContext,
};

#[derive(Debug, Clone)]
pub struct Policy {
    cfg: PolicyConfig,
    held: Vec<Candidate>,
    held_reasons: Vec<Reason>,
    dropped: Vec<Decision>,
    spoken_timestamps: Vec<u64>,
    urgent_exceptions: HashMap<String, u64>,
}

impl Policy {
    pub fn new(cfg: PolicyConfig) -> Self {
        Self {
            cfg,
            held: Vec::new(),
            held_reasons: Vec::new(),
            dropped: Vec::new(),
            spoken_timestamps: Vec::new(),
            urgent_exceptions: HashMap::new(),
        }
    }

    pub fn held(&self) -> &[Candidate] {
        &self.held
    }

    pub fn take_dropped(&mut self) -> Vec<Decision> {
        std::mem::take(&mut self.dropped)
    }

    pub fn spoken_in_last_hour(&self, now_ns: u64) -> u32 {
        let hour_ns = 3600 * 1_000_000_000;
        self.spoken_timestamps
            .iter()
            .filter(|&&t| now_ns >= t && now_ns.saturating_sub(t) < hour_ns)
            .count() as u32
    }

    fn spoken_in_last_24h(&self, now_ns: u64) -> u32 {
        let day_ns = 24 * 3600 * 1_000_000_000;
        self.spoken_timestamps
            .iter()
            .filter(|&&t| now_ns >= t && now_ns.saturating_sub(t) < day_ns)
            .count() as u32
    }

    fn prune_history(&mut self, now_ns: u64) {
        let day_ns = 24 * 3600 * 1_000_000_000;
        self.spoken_timestamps
            .retain(|&t| now_ns < t || now_ns.saturating_sub(t) <= day_ns);
        self.urgent_exceptions.retain(|_, &mut exc_ns| {
            now_ns < exc_ns || now_ns.saturating_sub(exc_ns) <= 3600 * 1_000_000_000
        });
    }

    fn effective_delivery(&self, c: &Candidate) -> Delivery {
        if let Some(d) = c.deliver {
            d
        } else {
            match c.priority {
                Priority::Info => self.cfg.deliver_info,
                Priority::Important => self.cfg.deliver_important,
                Priority::Urgent => self.cfg.deliver_urgent,
            }
        }
    }

    fn evaluate_candidate(
        &self,
        c: &Candidate,
        ctx: &UserContext,
        now_ns: u64,
    ) -> (Action, Reason) {
        let mut delivery = self.effective_delivery(c);

        // 1. delivery == Off -> Action::Drop, Reason::DeliveryOff.
        if delivery == Delivery::Off {
            return (Action::Drop, Reason::DeliveryOff);
        }

        // 2. If cfg.quiet_mode and delivery == Voice: delivery = Notify
        let mut quiet_mode_lowered = false;
        if self.cfg.quiet_mode && delivery == Delivery::Voice {
            delivery = Delivery::Notify;
            quiet_mode_lowered = true;
        }

        // 3. Bad moments (check in this order):
        if ctx.paused {
            return match c.priority {
                Priority::Urgent => (Action::Notify { silent: true }, Reason::Paused),
                _ => (Action::Hold, Reason::Paused),
            };
        }
        if ctx.quiet_hours {
            return match c.priority {
                Priority::Urgent => (Action::Notify { silent: true }, Reason::QuietHours),
                _ => (Action::Hold, Reason::QuietHours),
            };
        }
        if ctx.in_turn {
            return (Action::Hold, Reason::InTurn);
        }
        if ctx.mic_busy {
            return match c.priority {
                Priority::Urgent => (Action::Notify { silent: true }, Reason::MicBusy),
                _ => (Action::Hold, Reason::MicBusy),
            };
        }
        if ctx.away {
            return (Action::Hold, Reason::Away);
        }

        // 4. delivery == Notify -> Notify{silent:false}, Reason::Clear (or QuietMode)
        if delivery == Delivery::Notify {
            let reason = if quiet_mode_lowered {
                Reason::QuietMode
            } else {
                Reason::Clear
            };
            return (Action::Notify { silent: false }, reason);
        }

        // 5. delivery == Voice:
        if !ctx.voice_available {
            return (Action::Notify { silent: false }, Reason::NoVoice);
        }
        if ctx.media_playing {
            return (Action::Notify { silent: false }, Reason::Media);
        }
        if ctx.fullscreen {
            return (Action::Notify { silent: false }, Reason::Fullscreen);
        }

        // Rate limit:
        let h = self.spoken_in_last_hour(now_ns);
        let d = self.spoken_in_last_24h(now_ns);
        if h < self.cfg.max_spoken_per_hour && d < self.cfg.max_spoken_per_day {
            return (Action::Speak, Reason::Clear);
        }

        if c.priority == Priority::Urgent {
            let exc_used = self
                .urgent_exceptions
                .get(&c.rule_id)
                .is_some_and(|&exc_t| {
                    now_ns >= exc_t && now_ns.saturating_sub(exc_t) < 3600 * 1_000_000_000
                });
            if !exc_used {
                return (Action::Speak, Reason::Clear);
            }
        }

        (Action::Hold, Reason::RateLimited)
    }

    fn store_held(&mut self, c: Candidate, reason: Reason) {
        // Replacement: A new held candidate with the same (rule_id, key) replaces the older one
        if let Some(pos) = self
            .held
            .iter()
            .position(|h| h.rule_id == c.rule_id && h.key == c.key)
        {
            let old_c = self.held.remove(pos);
            self.held_reasons.remove(pos);
            self.dropped.push(Decision {
                candidate: old_c,
                action: Action::Drop,
                reason: Reason::Replaced,
                merged_from: Vec::new(),
            });
        }

        self.held.push(c);
        self.held_reasons.push(reason);

        // Overflow: If the held list would exceed cfg.max_held, the oldest of the lowest priority is pushed out
        if self.held.len() > self.cfg.max_held {
            let min_p = self
                .held
                .iter()
                .map(|h| h.priority)
                .min()
                .unwrap_or(Priority::Info);
            if let Some(pos) = self.held.iter().position(|h| h.priority == min_p) {
                let evicted = self.held.remove(pos);
                self.held_reasons.remove(pos);
                self.dropped.push(Decision {
                    candidate: evicted,
                    action: Action::Drop,
                    reason: Reason::Overflow,
                    merged_from: Vec::new(),
                });
            }
        }
    }

    /// Decide one fresh candidate. May hold it (stored), drop it, notify or speak.
    pub fn decide(&mut self, c: Candidate, ctx: &UserContext, now_ns: u64) -> Decision {
        self.prune_history(now_ns);
        let (action, reason) = self.evaluate_candidate(&c, ctx, now_ns);

        match action {
            Action::Speak => {
                let h = self.spoken_in_last_hour(now_ns);
                let d = self.spoken_in_last_24h(now_ns);
                let limit_reached =
                    h >= self.cfg.max_spoken_per_hour || d >= self.cfg.max_spoken_per_day;
                if limit_reached && c.priority == Priority::Urgent {
                    self.urgent_exceptions.insert(c.rule_id.clone(), now_ns);
                }
                self.spoken_timestamps.push(now_ns);
                Decision {
                    candidate: c,
                    action,
                    reason,
                    merged_from: Vec::new(),
                }
            }
            Action::Hold => {
                let decision = Decision {
                    candidate: c.clone(),
                    action,
                    reason,
                    merged_from: Vec::new(),
                };
                self.store_held(c, reason);
                decision
            }
            _ => Decision {
                candidate: c,
                action,
                reason,
                merged_from: Vec::new(),
            },
        }
    }

    /// Call when the context may have changed: returns the held nudges that can go out now.
    /// Two or more at once become ONE merged notification. Nudges still blocked stay held.
    pub fn release(&mut self, ctx: &UserContext, now_ns: u64) -> Vec<Decision> {
        self.prune_history(now_ns);

        let mut still_held = Vec::new();
        let mut still_held_reasons = Vec::new();
        let mut released = Vec::new();

        let old_held = std::mem::take(&mut self.held);
        let old_reasons = std::mem::take(&mut self.held_reasons);

        for (c, orig_reason) in old_held.into_iter().zip(old_reasons) {
            let (action, reason) = self.evaluate_candidate(&c, ctx, now_ns);
            match action {
                Action::Hold => {
                    still_held.push(c);
                    // Keep original reason (e.g. Away) if it was held for that
                    still_held_reasons.push(orig_reason);
                }
                Action::Speak | Action::Notify { .. } => {
                    released.push((c, action, reason, orig_reason));
                }
                Action::Drop => {
                    // Dropped upon re-eval (e.g. config changed to DeliveryOff)
                }
            }
        }

        self.held = still_held;
        self.held_reasons = still_held_reasons;

        if released.is_empty() {
            Vec::new()
        } else if released.len() == 1 {
            if let Some((single_c, single_action, _reason, _orig_reason)) = released.pop() {
                if single_action == Action::Speak {
                    let h = self.spoken_in_last_hour(now_ns);
                    let d = self.spoken_in_last_24h(now_ns);
                    let limit_reached =
                        h >= self.cfg.max_spoken_per_hour || d >= self.cfg.max_spoken_per_day;
                    if limit_reached && single_c.priority == Priority::Urgent {
                        self.urgent_exceptions
                            .insert(single_c.rule_id.clone(), now_ns);
                    }
                    self.spoken_timestamps.push(now_ns);
                }
                vec![Decision {
                    candidate: single_c,
                    action: single_action,
                    reason: Reason::Clear,
                    merged_from: Vec::new(),
                }]
            } else {
                Vec::new()
            }
        } else {
            // Two or more at once become ONE merged notification
            let n = released.len();
            let highest_priority = released
                .iter()
                .map(|(c, ..)| c.priority)
                .max()
                .unwrap_or(Priority::Info);

            let all_silent = released
                .iter()
                .all(|(_, act, ..)| matches!(act, Action::Notify { silent: true }));

            let any_held_away = released
                .iter()
                .any(|(_, _, _, orig_reason)| *orig_reason == Reason::Away);

            let merged_reason = if any_held_away {
                Reason::Away
            } else {
                Reason::Clear
            };

            let merged_from = released
                .iter()
                .map(|(c, ..)| (c.rule_id.clone(), c.key.clone()))
                .collect();

            let merged_candidate = Candidate {
                rule_id: "merged".to_string(),
                key: "".to_string(),
                priority: highest_priority,
                deliver: None,
                template: "nudge.while_away".to_string(),
                args: vec![("count".to_string(), n as f64)],
                offer_skill: None,
                at_ns: now_ns,
            };

            // A merged line is never spoken
            vec![Decision {
                candidate: merged_candidate,
                action: Action::Notify { silent: all_silent },
                reason: merged_reason,
                merged_from,
            }]
        }
    }
}
