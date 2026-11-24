//! The pure heart of the engine: readings and the moment in, decisions out (AB-13).
//!
//! The live engine task and the replay harness both call [`Core::step`], so what a replay
//! proves (KPI-10: nothing spoken at a bad moment) is what runs. No I/O, no clock.

use super::eval::{EvalConfig, Evaluator};
use super::policy::Policy;
use super::types::{Decision, PolicyConfig, Reading, Rule, UserContext};

/// What one step produced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StepOut {
    /// Decisions for nudges that fired now plus held nudges released now (in that order).
    pub decisions: Vec<Decision>,
    /// Held nudges thrown away since the last step (replaced or pushed out): to be logged.
    pub dropped: Vec<Decision>,
}

/// The evaluator and the policy gate, wired in the order the engine uses them.
#[derive(Debug, Clone)]
pub struct Core {
    /// Turns readings into candidates.
    pub evaluator: Evaluator,
    /// Decides what to do with each candidate.
    pub policy: Policy,
}

impl Core {
    /// Builds the pair from rules and configuration.
    #[must_use]
    pub fn new(rules: Vec<Rule>, eval: EvalConfig, policy: PolicyConfig) -> Self {
        Self {
            evaluator: Evaluator::new(rules, eval),
            policy: Policy::new(policy),
        }
    }

    /// One evaluation at `now_ns`: fire, decide each candidate, then release what the moment
    /// now allows.
    pub fn step(&mut self, now_ns: u64, readings: &[Reading], ctx: &UserContext) -> StepOut {
        let candidates = self.evaluator.tick(now_ns, readings);
        let mut decisions: Vec<Decision> = candidates
            .into_iter()
            .map(|c| self.policy.decide(c, ctx, now_ns))
            .collect();
        decisions.extend(self.policy.release(ctx, now_ns));
        StepOut {
            decisions,
            dropped: self.policy.take_dropped(),
        }
    }
}
