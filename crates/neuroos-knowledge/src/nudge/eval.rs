//! Pure nudge rule evaluator (FR-NUD-01/02/06, NFR-PERF-10).
//!
//! Evaluates sensor readings against declarative rules with hysteresis, hold timers,
//! cooldown, least-squares disk fill projection, and clock gap guards.
//! Pure code: no I/O, no clock. Time is passed in as epoch nanoseconds (AB-13).

use std::collections::{HashMap, VecDeque};

use super::types::{ArgSource, Candidate, Direction, Reading, Rule, RuleStatus, Signal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalConfig {
    /// A gap between two ticks above this, in seconds, or a clock that goes back,
    /// resets hold timers and disk history; nothing fires on that tick.
    pub max_gap_s: u64,
}

#[derive(Debug, Clone)]
struct InstanceState {
    armed: bool,
    timer_start_ns: Option<u64>,
    last_fired_ns: u64,
}

#[derive(Debug, Clone)]
struct PerRuleState {
    enabled: bool,
    snoozed_until_ns: u64,
    restored_last_fired_ns: u64,
}

#[derive(Debug, Clone)]
struct DiskHistoryEntry {
    at_ns: u64,
    free_gib: f64,
}

#[derive(Debug, Clone)]
pub struct Evaluator {
    rules: Vec<Rule>,
    rule_states: Vec<PerRuleState>,
    cfg: EvalConfig,
    prev_tick_ns: Option<u64>,
    instances: HashMap<(String, String), InstanceState>,
    disk_history: HashMap<String, VecDeque<DiskHistoryEntry>>,
}

impl Evaluator {
    pub fn new(rules: Vec<Rule>, cfg: EvalConfig) -> Self {
        let rule_states = rules
            .iter()
            .map(|r| PerRuleState {
                enabled: r.enabled,
                snoozed_until_ns: 0,
                restored_last_fired_ns: 0,
            })
            .collect();

        Self {
            rules,
            rule_states,
            cfg,
            prev_tick_ns: None,
            instances: HashMap::new(),
            disk_history: HashMap::new(),
        }
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn set_enabled(&mut self, rule_id: &str, on: bool) -> bool {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule_id) {
            self.rule_states[pos].enabled = on;
            true
        } else {
            false
        }
    }

    pub fn snooze(&mut self, rule_id: &str, until_ns: u64) -> bool {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule_id) {
            self.rule_states[pos].snoozed_until_ns = until_ns;
            true
        } else {
            false
        }
    }

    pub fn restore(
        &mut self,
        rule_id: &str,
        last_fired_ns: u64,
        snoozed_until_ns: u64,
        enabled: Option<bool>,
    ) -> bool {
        if let Some(pos) = self.rules.iter().position(|r| r.id == rule_id) {
            let state = &mut self.rule_states[pos];
            state.restored_last_fired_ns = last_fired_ns;
            state.snoozed_until_ns = snoozed_until_ns;
            if let Some(on) = enabled {
                state.enabled = on;
            }
            // Update any existing instances of this rule
            for ((r_id, _), inst) in &mut self.instances {
                if r_id == rule_id && last_fired_ns > inst.last_fired_ns {
                    inst.last_fired_ns = last_fired_ns;
                }
            }
            true
        } else {
            false
        }
    }

    pub fn status(&self) -> Vec<RuleStatus> {
        self.rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| {
                let state = &self.rule_states[idx];
                let mut latest_fired = state.restored_last_fired_ns;
                for ((r_id, _), inst) in &self.instances {
                    if r_id == &rule.id && inst.last_fired_ns > latest_fired {
                        latest_fired = inst.last_fired_ns;
                    }
                }
                RuleStatus {
                    rule_id: rule.id.clone(),
                    enabled: state.enabled,
                    snoozed_until_ns: state.snoozed_until_ns,
                    last_fired_ns: latest_fired,
                }
            })
            .collect()
    }

    pub fn tick(&mut self, now_ns: u64, readings: &[Reading]) -> Vec<Candidate> {
        let max_gap_ns = self.cfg.max_gap_s.saturating_mul(1_000_000_000);
        let is_gap = match self.prev_tick_ns {
            Some(prev) => now_ns < prev || now_ns.saturating_sub(prev) > max_gap_ns,
            None => false,
        };

        let Self {
            rules,
            rule_states,
            instances,
            disk_history,
            prev_tick_ns,
            ..
        } = self;

        if is_gap {
            // Gap guard: clear hold timers and disk history, apply re-arming, return empty
            for inst in instances.values_mut() {
                inst.timer_start_ns = None;
            }
            disk_history.clear();
            *prev_tick_ns = Some(now_ns);

            // Apply re-arming:
            // Group valid readings by (signal, key)
            let mut reading_map: HashMap<(Signal, String), &Reading> = HashMap::new();
            for r in readings {
                if r.signal != Signal::DiskFillHours && r.value.is_finite() {
                    reading_map.insert((r.signal, r.key.clone()), r);
                }
            }

            for (idx, rule) in rules.iter().enumerate() {
                let restored = rule_states[idx].restored_last_fired_ns;
                for ((sig, key), r) in &reading_map {
                    if *sig == rule.signal {
                        let inst = instances
                            .entry((rule.id.clone(), key.clone()))
                            .or_insert_with(|| InstanceState {
                                armed: true,
                                timer_start_ns: None,
                                last_fired_ns: restored,
                            });
                        let passed = match rule.direction {
                            Direction::AtLeast => r.value < rule.rearm,
                            Direction::AtMost => r.value > rule.rearm,
                        };
                        if !inst.armed && passed {
                            inst.armed = true;
                        }
                    }
                }
            }

            // Absent instances re-arm
            for ((r_id, key), inst) in instances.iter_mut() {
                if let Some(pos) = rules.iter().position(|r| r.id == *r_id) {
                    let sig = rules[pos].signal;
                    if !reading_map.contains_key(&(sig, key.clone())) {
                        inst.armed = true;
                    }
                }
            }

            return Vec::new();
        }

        *prev_tick_ns = Some(now_ns);

        // 2. Disk fill projection
        let mut derived_readings = Vec::new();
        for r in readings {
            if r.signal == Signal::DiskUsedPercent && r.value.is_finite() && r.extra.is_finite() {
                let history = disk_history.entry(r.key.clone()).or_default();

                // Drop entries older than 3600s
                while let Some(front) = history.front() {
                    if now_ns.saturating_sub(front.at_ns) > 3600 * 1_000_000_000 {
                        history.pop_front();
                    } else {
                        break;
                    }
                }

                history.push_back(DiskHistoryEntry {
                    at_ns: now_ns,
                    free_gib: r.extra,
                });

                if history.len() > 400 {
                    history.pop_front();
                }

                if history.len() >= 5 {
                    let oldest_ns = history.front().map(|e| e.at_ns).unwrap_or(now_ns);
                    let span_ns = now_ns.saturating_sub(oldest_ns);
                    if span_ns >= 600 * 1_000_000_000 {
                        let t0 = oldest_ns;
                        let n = history.len() as f64;
                        let mut sum_x = 0.0;
                        let mut sum_y = 0.0;

                        for e in history.iter() {
                            let x_hours = (e.at_ns - t0) as f64 / (3600.0 * 1e9);
                            sum_x += x_hours;
                            sum_y += e.free_gib;
                        }

                        let mean_x = sum_x / n;
                        let mean_y = sum_y / n;

                        let mut s_xx = 0.0;
                        let mut s_xy = 0.0;

                        for e in history.iter() {
                            let x_hours = (e.at_ns - t0) as f64 / (3600.0 * 1e9);
                            let dx = x_hours - mean_x;
                            let dy = e.free_gib - mean_y;
                            s_xx += dx * dx;
                            s_xy += dx * dy;
                        }

                        if s_xx > 1e-12 {
                            let slope = s_xy / s_xx;
                            let rate = -slope; // GiB per hour
                            if rate >= 0.05 {
                                let free_now = r.extra;
                                derived_readings.push(Reading {
                                    signal: Signal::DiskFillHours,
                                    key: r.key.clone(),
                                    value: free_now / rate,
                                    extra: free_now,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Collect all effective readings for this tick (ignoring external DiskFillHours)
        let mut reading_map: HashMap<(Signal, String), Reading> = HashMap::new();
        for r in readings {
            if r.signal != Signal::DiskFillHours && r.value.is_finite() {
                reading_map.insert((r.signal, r.key.clone()), r.clone());
            }
        }
        for dr in derived_readings {
            reading_map.insert((dr.signal, dr.key.clone()), dr);
        }

        // Track instances that matched a reading this tick
        let mut matched_instances = HashMap::new();
        let mut candidate_tuples = Vec::new();

        for (idx, rule) in rules.iter().enumerate() {
            let rule_enabled = rule_states[idx].enabled;
            let rule_snoozed_until = rule_states[idx].snoozed_until_ns;

            for ((sig, key), reading) in &reading_map {
                if *sig != rule.signal {
                    continue;
                }

                matched_instances.insert((rule.id.clone(), key.clone()), true);

                let restored_last_fired = rule_states[idx].restored_last_fired_ns;
                let inst = instances
                    .entry((rule.id.clone(), key.clone()))
                    .or_insert_with(|| InstanceState {
                        armed: true,
                        timer_start_ns: None,
                        last_fired_ns: restored_last_fired,
                    });

                let holds = match rule.direction {
                    Direction::AtLeast => reading.value >= rule.fire_at,
                    Direction::AtMost => reading.value <= rule.fire_at,
                };

                let passed = match rule.direction {
                    Direction::AtLeast => reading.value < rule.rearm,
                    Direction::AtMost => reading.value > rule.rearm,
                };

                if !inst.armed && passed {
                    inst.armed = true;
                }

                if holds {
                    if inst.timer_start_ns.is_none() {
                        inst.timer_start_ns = Some(now_ns);
                    }
                } else {
                    inst.timer_start_ns = None;
                }

                let hold_dur_ns = rule.hold_for_s.saturating_mul(1_000_000_000);
                let hold_ok = match inst.timer_start_ns {
                    Some(start) => now_ns.saturating_sub(start) >= hold_dur_ns,
                    None => false,
                };

                let cooldown_dur_ns = rule.cooldown_s.saturating_mul(1_000_000_000);
                let cooldown_ok = inst.last_fired_ns == 0
                    || now_ns.saturating_sub(inst.last_fired_ns) >= cooldown_dur_ns;

                let allowed = rule_enabled && now_ns >= rule_snoozed_until;

                if inst.armed && holds && hold_ok && cooldown_ok && allowed {
                    inst.armed = false;
                    inst.last_fired_ns = now_ns;

                    let args = rule
                        .args
                        .iter()
                        .map(|(name, src)| {
                            let val = match src {
                                ArgSource::Value => reading.value,
                                ArgSource::Extra => reading.extra,
                            };
                            (name.clone(), val)
                        })
                        .collect();

                    let cand = Candidate {
                        rule_id: rule.id.clone(),
                        key: key.clone(),
                        priority: rule.priority,
                        deliver: rule.deliver,
                        template: rule.template.clone(),
                        args,
                        offer_skill: rule.offer_skill.clone(),
                        at_ns: now_ns,
                    };

                    candidate_tuples.push((rule.priority, idx, key.clone(), cand));
                }
            }
        }

        // 4. An instance whose reading is absent this tick clears hold timer and re-arms
        for ((r_id, key), inst) in instances.iter_mut() {
            if !matched_instances.contains_key(&(r_id.clone(), key.clone())) {
                inst.timer_start_ns = None;
                inst.armed = true;
            }
        }

        // 5. Several candidates in one tick are returned highest priority first,
        // ties in rule order then key order
        candidate_tuples.sort_by(|a, b| {
            b.0.cmp(&a.0) // Urgent > Important > Info
                .then_with(|| a.1.cmp(&b.1)) // rule index
                .then_with(|| a.2.cmp(&b.2)) // key order
        });

        candidate_tuples.into_iter().map(|(_, _, _, c)| c).collect()
    }
}
