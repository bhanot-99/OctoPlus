//! Safety tier evaluation with taint escalation.
//!
//! Satisfies FR-KER-01, FR-KER-02, FR-KER-03, FR-KER-08, Architecture §8.3, and rules.md R0-4.

use octoplus_taint::{TaintError, TaintFlags};
use serde_json::Value;

use crate::registry::{Registry, Tier};
use crate::schema::validate_args;

/// C5's check before any generation (Architecture §6.1). Not a skill: not in the registry.
pub const GENERATE: &str = "llm.generate";

/// Denial reasons for capability evaluation.
///
/// Contains no user content in error messages (rules.md R0-10).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Deny {
    #[error("unknown capability")]
    Unknown,
    #[error("missing context taint")]
    MissingTaint,
    #[error("invalid taint flags")]
    BadTaint,
    #[error("capability is disabled")]
    Disabled,
    #[error("invalid arguments")]
    BadArgs,
}

/// Evaluates the safety tier for a capability execution request.
///
/// Pure function implementing Architecture §8.3:
/// 1. Unknown capability -> Deny::Unknown
/// 2. Missing taint -> Deny::MissingTaint; unknown bits -> Deny::BadTaint
/// 3. Disabled skill -> Deny::Disabled
/// 4. If args provided and fail schema -> Deny::BadArgs
/// 5. If taint has EXTERNAL_UNTRUSTED and base tier is SAFE -> REVIEW
/// 6. Otherwise base tier (never lower than base).
///
/// # Errors
/// Returns [`Deny`] on any policy failure, failing closed.
pub fn evaluate(
    registry: &Registry,
    capability: &str,
    taint: Option<u32>,
    args: Option<&Value>,
) -> Result<Tier, Deny> {
    if capability == GENERATE {
        // Generating text runs nothing (Architecture §6.1, ADR-0010 decision 11):
        // it is answered SAFE with a valid taint and can never be dispatched.
        // Taint travels with the generated text and is checked at dispatch.
        TaintFlags::decode(taint).map_err(|e| match e {
            TaintError::Missing => Deny::MissingTaint,
            TaintError::UnknownBits(_) => Deny::BadTaint,
        })?;
        return Ok(Tier::Safe);
    }
    let skill = registry.get(capability).ok_or(Deny::Unknown)?;

    let flags = TaintFlags::decode(taint).map_err(|e| match e {
        TaintError::Missing => Deny::MissingTaint,
        TaintError::UnknownBits(_) => Deny::BadTaint,
    })?;

    if skill.disabled {
        return Err(Deny::Disabled);
    }

    if let Some(args_val) = args {
        validate_args(&skill.args, args_val).map_err(|_| Deny::BadArgs)?;
    }

    if flags.contains(TaintFlags::EXTERNAL_UNTRUSTED) && skill.tier == Tier::Safe {
        Ok(Tier::Review)
    } else {
        Ok(skill.tier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn exhaustive_skill_taint_table_test() {
        let registry = Registry::load().unwrap();

        let taint_cases: &[(Option<u32>, &str)] = &[
            (None, "None"),
            (Some(0), "clean_0"),
            (
                Some(TaintFlags::EXTERNAL_UNTRUSTED.bits()),
                "EXTERNAL_UNTRUSTED",
            ),
            (Some(TaintFlags::USER_PROVIDED.bits()), "USER_PROVIDED"),
            (Some(TaintFlags::MODEL_GENERATED.bits()), "MODEL_GENERATED"),
            (Some(TaintFlags::all().bits()), "all_known_bits"),
            (Some(1 << 31), "unknown_bit_31"),
        ];

        for (cap, skill) in registry.iter() {
            for &(taint, label) in taint_cases {
                let res = evaluate(&registry, cap, taint, None);

                let expected = match taint {
                    None => Err(Deny::MissingTaint),
                    Some(bits) if (bits & !TaintFlags::all().bits()) != 0 => Err(Deny::BadTaint),
                    Some(bits) => {
                        if skill.disabled {
                            Err(Deny::Disabled)
                        } else {
                            let flags = TaintFlags::from_bits_truncate(bits);
                            if flags.contains(TaintFlags::EXTERNAL_UNTRUSTED)
                                && skill.tier == Tier::Safe
                            {
                                Ok(Tier::Review)
                            } else {
                                Ok(skill.tier)
                            }
                        }
                    }
                };

                assert_eq!(
                    res, expected,
                    "Failed for skill '{cap}' with taint {label} ({taint:?})"
                );
            }
        }
    }

    #[test]
    fn hand_written_expectations() {
        let registry = Registry::load().unwrap();

        let clean = Some(0);
        let external = Some(TaintFlags::EXTERNAL_UNTRUSTED.bits());
        let user_model = Some((TaintFlags::USER_PROVIDED | TaintFlags::MODEL_GENERATED).bits());
        let bad_bits = Some(1 << 31);

        // 1. SAFE skills: clean -> Safe, external -> Review, user_model -> Safe
        let safe_skills = [
            "system.read_metadata",
            "system.disk_report",
            "privacy.pause",
            "profile.remember",
            "calendar.read",
            "notes.search",
        ];
        for skill in safe_skills {
            assert_eq!(
                evaluate(&registry, skill, clean, None),
                Ok(Tier::Safe),
                "skill {skill} clean must be Safe"
            );
            assert_eq!(
                evaluate(&registry, skill, external, None),
                Ok(Tier::Review),
                "skill {skill} external must escalate to Review"
            );
            assert_eq!(
                evaluate(&registry, skill, user_model, None),
                Ok(Tier::Safe),
                "skill {skill} user_model must be Safe"
            );
            assert_eq!(
                evaluate(&registry, skill, None, None),
                Err(Deny::MissingTaint),
                "skill {skill} None taint must be MissingTaint"
            );
            assert_eq!(
                evaluate(&registry, skill, bad_bits, None),
                Err(Deny::BadTaint),
                "skill {skill} bad bits must be BadTaint"
            );
        }

        // 2. REVIEW skills: clean -> Review, external -> Review
        let review_skills = [
            "profile.forget",
            "memory.forget",
            "notes.append",
            "system.cleanup",
            "web.fetch",
            "web.search",
        ];
        for skill in review_skills {
            assert_eq!(
                evaluate(&registry, skill, clean, None),
                Ok(Tier::Review),
                "skill {skill} clean must be Review"
            );
            assert_eq!(
                evaluate(&registry, skill, external, None),
                Ok(Tier::Review),
                "skill {skill} external must be Review"
            );
            assert_eq!(
                evaluate(&registry, skill, None, None),
                Err(Deny::MissingTaint),
                "skill {skill} None taint must be MissingTaint"
            );
            assert_eq!(
                evaluate(&registry, skill, bad_bits, None),
                Err(Deny::BadTaint),
                "skill {skill} bad bits must be BadTaint"
            );
        }

        // 3. shell.exec (DANGEROUS & disabled): clean and external -> Disabled
        assert_eq!(
            evaluate(&registry, "shell.exec", clean, None),
            Err(Deny::Disabled)
        );
        assert_eq!(
            evaluate(&registry, "shell.exec", external, None),
            Err(Deny::Disabled)
        );
        assert_eq!(
            evaluate(&registry, "shell.exec", None, None),
            Err(Deny::MissingTaint)
        );
        assert_eq!(
            evaluate(&registry, "shell.exec", bad_bits, None),
            Err(Deny::BadTaint)
        );
    }

    #[test]
    fn disabled_skill_evaluation_across_all_taints() {
        // Specifically tests shell.exec (disabled=true in registry)
        let registry = Registry::load().unwrap();
        assert_eq!(
            evaluate(&registry, "shell.exec", None, None),
            Err(Deny::MissingTaint)
        );
        assert_eq!(
            evaluate(&registry, "shell.exec", Some(1 << 31), None),
            Err(Deny::BadTaint)
        );
        assert_eq!(
            evaluate(&registry, "shell.exec", Some(0), None),
            Err(Deny::Disabled)
        );
        assert_eq!(
            evaluate(
                &registry,
                "shell.exec",
                Some(TaintFlags::EXTERNAL_UNTRUSTED.bits()),
                None
            ),
            Err(Deny::Disabled)
        );
    }

    #[test]
    fn args_validation_in_evaluate() {
        let registry = Registry::load().unwrap();

        // privacy.pause requires `minutes` int 1..=480
        let valid_args = serde_json::json!({"minutes": 30});
        assert_eq!(
            evaluate(&registry, "privacy.pause", Some(0), Some(&valid_args)),
            Ok(Tier::Safe)
        );

        let invalid_args = serde_json::json!({"minutes": 999});
        assert_eq!(
            evaluate(&registry, "privacy.pause", Some(0), Some(&invalid_args)),
            Err(Deny::BadArgs)
        );
    }

    // Proptest:
    // 1. For any registered skill and any known-bits taint, evaluate is Ok(t) with t >= base tier
    //    and Ok(REVIEW-or-higher) whenever EXTERNAL_UNTRUSTED is set (if not disabled).
    // 2. Unknown capability strings always Deny::Unknown.
    proptest! {
        #[test]
        fn prop_known_taint_tiers_never_decrease(
            taint_bits in 0u32..16u32,
            idx in 0usize..13usize,
        ) {
            let registry = Registry::load().unwrap();
            let skills: Vec<_> = registry.iter().collect();
            let (cap, skill) = skills[idx];

            let res = evaluate(&registry, cap, Some(taint_bits), None);
            if skill.disabled {
                prop_assert_eq!(res, Err(Deny::Disabled));
            } else {
                let tier = res.unwrap();
                prop_assert!(tier >= skill.tier);
                let flags = TaintFlags::from_bits_truncate(taint_bits);
                if flags.contains(TaintFlags::EXTERNAL_UNTRUSTED) {
                    prop_assert!(tier >= Tier::Review);
                }
            }
        }

        #[test]
        fn prop_unknown_capability_always_denies(
            cap in "[a-z]{3,10}\\.[a-z]{3,10}\\.unknown",
            taint in 0u32..16u32,
        ) {
            let registry = Registry::load().unwrap();
            prop_assert_eq!(
                evaluate(&registry, &cap, Some(taint), None),
                Err(Deny::Unknown)
            );
        }
    }

    #[test]
    fn generation_is_safe_under_any_valid_taint_but_needs_taint() {
        let registry = Registry::load().unwrap();
        for t in [0, 1, 2, 4, 7, 15] {
            assert_eq!(evaluate(&registry, GENERATE, Some(t), None), Ok(Tier::Safe));
        }
        assert_eq!(
            evaluate(&registry, GENERATE, None, None),
            Err(Deny::MissingTaint)
        );
        assert_eq!(
            evaluate(&registry, GENERATE, Some(1 << 31), None),
            Err(Deny::BadTaint)
        );
        // Not a skill: it can never be dispatched.
        assert!(registry.get(GENERATE).is_none());
    }
}
