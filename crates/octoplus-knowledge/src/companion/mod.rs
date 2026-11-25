//! Companion persona, mode machine, RNG core, and fixed safety behaviors (Phase 12).
//!
//! Provides pure components for Laila:
//! - [`persona`]: configuration file structures, validation, and defaults.
//! - [`rng`]: deterministic splitmix64 PRNG for controlled switching and chance rules.
//! - [`mode`]: two-mode switching state machine (logic and heart).
//! - [`guards`]: crisis detection, reality inquiries, and forbidden phrase scrubbing.
//! - [`kabir`]: backstory gate with cooldown and chance roll for past relationship mentions.
//! - [`dependence`]: session duration and frequency monitoring to prevent unhealthy reliance.
//! - [`repeat`]: 64-bit FNV-1a ring buffer preventing repeated conversational lines.
//! - [`prompt`]: companion ChatML prompt assembly and cacheable system block.
//! - [`session`]: conversational memory with rolling summary and turn retention.
//! - [`filter`]: streamed sentence validation and safety scrubbing before speech synthesis.
//! - [`engine`]: unified dialogue orchestrator managing per-conversation state.
//! - [`pulse`]: when she starts a chat herself (gates, caps, learned rhythm, spoken controls).
//! - [`humour`]: "funny / not funny" feedback, humour levels and the no-jokes-at-sensitive-moments rule.
//! - [`hub`]: the scheduler state shared by the runner and the chat handler.
//! - [`runner`]: the loop that starts a talk by herself and asks C2 to speak it.
//! - [`boot`]: preloading and fallback resolution for the persona configuration.

pub mod boot;
pub mod dependence;
pub mod directed;
pub mod engine;
pub mod filter;
pub mod guards;
pub mod hub;
pub mod humour;
pub mod kabir;
pub mod mode;
pub mod persona;
pub mod prompt;
pub mod pulse;
pub mod repeat;
pub mod rng;
pub mod runner;
pub mod session;
