//! Skill registry compiled into the kernel binary.
//!
//! Satisfies FR-KER-01, FR-KER-02, and rules.md R0-4.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::schema::ArgSchema;

/// Allowed symbolic root names for sandboxed child skills (Architecture §8.3).
const ALLOWED_ROOTS: &[&str] = &[
    "metadata",
    "cleanup_roots",
    "trash",
    "notes_folders",
    "calendar_folders",
];

/// Total expected count of v1.0 skills (PRD §5.9).
pub const EXPECTED_SKILL_COUNT: usize = 13;

/// Safety tier for capability evaluation (PRD §5.9, Architecture §8.3).
///
/// Order is SAFE < REVIEW < DANGEROUS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Tier {
    /// Safe to execute without user dialog confirmation.
    Safe = 1,
    /// Requires user confirmation dialog before dispatch.
    Review = 2,
    /// High-risk action requiring confirmation dialog and typed confirmation phrase.
    Dangerous = 3,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Safe => write!(f, "SAFE"),
            Self::Review => write!(f, "REVIEW"),
            Self::Dangerous => write!(f, "DANGEROUS"),
        }
    }
}

impl From<Tier> for neuroos_proto::v1::Tier {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Safe => Self::Safe,
            Tier::Review => Self::Review,
            Tier::Dangerous => Self::Dangerous,
        }
    }
}

/// Execution kind of a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    /// Runs in a dedicated sandboxed child process.
    Child,
    /// Runs as a validated IPC message sent directly by the kernel.
    Kernel,
}

/// Sandbox resource limits and symbolic filesystem roots for child skills.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Read-only symbolic filesystem roots.
    pub read_only: Vec<String>,
    /// Read-write symbolic filesystem roots.
    pub read_write: Vec<String>,
    /// Execution timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum resident memory limit in MiB.
    pub memory_mib: u64,
    /// Maximum CPU time limit in seconds.
    pub cpu_s: u64,
    /// Largest file the child may write, in KiB (0 = it writes nothing but its output pipe).
    #[serde(default)]
    pub fsize_kib: u64,
}

/// Manifest specification for a single skill.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    /// Capability identifier, must equal the file stem.
    pub capability: String,
    /// Base safety tier.
    pub tier: Tier,
    /// Whether this skill is administratively disabled.
    pub disabled: bool,
    /// Whether this skill is implemented in v1.0.
    pub implemented: bool,
    /// Kind of skill execution.
    pub kind: SkillKind,
    /// Argument schema table.
    #[serde(default)]
    pub args: BTreeMap<String, ArgSchema>,
    /// Sandbox configuration, required for child skills, forbidden for kernel skills.
    pub sandbox: Option<SandboxConfig>,
}

/// Errors loading or validating the skill registry.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("failed to parse skill manifest for stem '{stem}': {details}")]
    Parse { stem: String, details: String },
    #[error("capability '{capability}' does not match file stem '{stem}'")]
    StemMismatch { stem: String, capability: String },
    #[error("duplicate capability in registry: {0}")]
    DuplicateCapability(String),
    #[error("child skill '{0}' missing sandbox configuration")]
    MissingSandbox(String),
    #[error("kernel skill '{0}' must not have sandbox configuration")]
    UnexpectedSandbox(String),
    #[error("unknown symbolic sandbox root: {0}")]
    UnknownSandboxRoot(String),
    #[error("expected exactly 13 skills in registry, found {0}")]
    InvalidSkillCount(usize),
}

/// Static embedded manifests for all v1.0 skills (compiled in; cannot be altered at runtime).
const MANIFESTS: &[(&str, &str)] = &[
    (
        "system.read_metadata",
        include_str!("../skills/system.read_metadata.toml"),
    ),
    (
        "system.disk_report",
        include_str!("../skills/system.disk_report.toml"),
    ),
    (
        "calendar.read",
        include_str!("../skills/calendar.read.toml"),
    ),
    ("notes.search", include_str!("../skills/notes.search.toml")),
    (
        "privacy.pause",
        include_str!("../skills/privacy.pause.toml"),
    ),
    (
        "profile.remember",
        include_str!("../skills/profile.remember.toml"),
    ),
    (
        "profile.forget",
        include_str!("../skills/profile.forget.toml"),
    ),
    (
        "memory.forget",
        include_str!("../skills/memory.forget.toml"),
    ),
    ("notes.append", include_str!("../skills/notes.append.toml")),
    (
        "system.cleanup",
        include_str!("../skills/system.cleanup.toml"),
    ),
    ("web.fetch", include_str!("../skills/web.fetch.toml")),
    ("web.search", include_str!("../skills/web.search.toml")),
    ("shell.exec", include_str!("../skills/shell.exec.toml")),
];

/// Immutable skill registry holding all registered capabilities.
#[derive(Debug, Clone)]
pub struct Registry {
    skills: BTreeMap<String, Skill>,
}

impl Registry {
    /// Loads the skill registry from the embedded compile-time manifests.
    ///
    /// # Errors
    /// Returns [`RegistryError`] if any manifest is invalid or missing required constraints.
    pub fn load() -> Result<Self, RegistryError> {
        let mut skills = BTreeMap::new();
        for &(stem, content) in MANIFESTS {
            let skill = Self::parse_manifest(stem, content)?;
            if skills.contains_key(&skill.capability) {
                return Err(RegistryError::DuplicateCapability(skill.capability));
            }
            skills.insert(skill.capability.clone(), skill);
        }

        if skills.len() != EXPECTED_SKILL_COUNT {
            return Err(RegistryError::InvalidSkillCount(skills.len()));
        }

        Ok(Self { skills })
    }

    /// Parses and validates a single skill manifest string.
    ///
    /// # Errors
    /// Returns [`RegistryError`] if parsing fails, stem mismatches, or sandbox rules are violated.
    pub fn parse_manifest(stem: &str, content: &str) -> Result<Skill, RegistryError> {
        let skill: Skill = toml::from_str(content).map_err(|e| RegistryError::Parse {
            stem: stem.to_string(),
            details: e.to_string(),
        })?;

        if skill.capability != stem {
            return Err(RegistryError::StemMismatch {
                stem: stem.to_string(),
                capability: skill.capability,
            });
        }

        match skill.kind {
            SkillKind::Child => {
                let sb = skill
                    .sandbox
                    .as_ref()
                    .ok_or_else(|| RegistryError::MissingSandbox(skill.capability.clone()))?;
                for root in sb.read_only.iter().chain(sb.read_write.iter()) {
                    if !ALLOWED_ROOTS.contains(&root.as_str()) {
                        return Err(RegistryError::UnknownSandboxRoot(root.clone()));
                    }
                }
            }
            SkillKind::Kernel => {
                if skill.sandbox.is_some() {
                    return Err(RegistryError::UnexpectedSandbox(skill.capability.clone()));
                }
            }
        }

        Ok(skill)
    }

    /// Returns a reference to the skill for the given capability name.
    #[must_use]
    pub fn get(&self, capability: &str) -> Option<&Skill> {
        self.skills.get(capability)
    }

    /// Returns the total number of registered skills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Returns true if the registry contains no skills.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Iterates over all capabilities and skills in the registry.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Skill)> {
        self.skills.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_embedded_registry_matching_prd_table() {
        let reg = Registry::load().unwrap();
        assert_eq!(reg.len(), 13);

        // Exact PRD §5.9 table
        let expected: &[(&str, Tier, bool, bool, SkillKind)] = &[
            (
                "system.read_metadata",
                Tier::Safe,
                false,
                true,
                SkillKind::Child,
            ),
            (
                "system.disk_report",
                Tier::Safe,
                false,
                true,
                SkillKind::Child,
            ),
            ("calendar.read", Tier::Safe, false, true, SkillKind::Child),
            ("notes.search", Tier::Safe, false, true, SkillKind::Child),
            ("privacy.pause", Tier::Safe, false, true, SkillKind::Kernel),
            (
                "profile.remember",
                Tier::Safe,
                false,
                true,
                SkillKind::Kernel,
            ),
            (
                "profile.forget",
                Tier::Review,
                false,
                true,
                SkillKind::Kernel,
            ),
            (
                "memory.forget",
                Tier::Review,
                false,
                true,
                SkillKind::Kernel,
            ),
            ("notes.append", Tier::Review, false, true, SkillKind::Child),
            (
                "system.cleanup",
                Tier::Review,
                false,
                true,
                SkillKind::Child,
            ),
            ("web.fetch", Tier::Review, false, false, SkillKind::Child),
            ("web.search", Tier::Review, false, false, SkillKind::Child),
            ("shell.exec", Tier::Dangerous, true, false, SkillKind::Child),
        ];

        for &(cap, tier, disabled, implemented, kind) in expected {
            let skill = reg.get(cap).unwrap_or_else(|| panic!("missing {cap}"));
            assert_eq!(skill.capability, cap);
            assert_eq!(skill.tier, tier, "tier mismatch for {cap}");
            assert_eq!(skill.disabled, disabled, "disabled mismatch for {cap}");
            assert_eq!(
                skill.implemented, implemented,
                "implemented mismatch for {cap}"
            );
            assert_eq!(skill.kind, kind, "kind mismatch for {cap}");
        }
    }

    #[test]
    fn malformed_manifests_are_rejected() {
        // Unknown key
        let unknown_key = r#"
capability = "test.skill"
tier = "SAFE"
disabled = false
implemented = false
kind = "child"
unknown_field = 123
[sandbox]
read_only = []
read_write = []
timeout_ms = 1000
memory_mib = 16
cpu_s = 1
"#;
        assert!(matches!(
            Registry::parse_manifest("test.skill", unknown_key),
            Err(RegistryError::Parse { .. })
        ));

        // Stem mismatch
        let stem_mismatch = r#"
capability = "different.name"
tier = "SAFE"
disabled = false
implemented = false
kind = "child"
[sandbox]
read_only = []
read_write = []
timeout_ms = 1000
memory_mib = 16
cpu_s = 1
"#;
        assert_eq!(
            Registry::parse_manifest("test.skill", stem_mismatch),
            Err(RegistryError::StemMismatch {
                stem: "test.skill".into(),
                capability: "different.name".into()
            })
        );

        // Child missing sandbox
        let missing_sandbox = r#"
capability = "test.skill"
tier = "SAFE"
disabled = false
implemented = false
kind = "child"
"#;
        assert_eq!(
            Registry::parse_manifest("test.skill", missing_sandbox),
            Err(RegistryError::MissingSandbox("test.skill".into()))
        );

        // Kernel having sandbox
        let unexpected_sandbox = r#"
capability = "test.kernel"
tier = "SAFE"
disabled = false
implemented = true
kind = "kernel"
[sandbox]
read_only = []
read_write = []
timeout_ms = 1000
memory_mib = 16
cpu_s = 1
"#;
        assert_eq!(
            Registry::parse_manifest("test.kernel", unexpected_sandbox),
            Err(RegistryError::UnexpectedSandbox("test.kernel".into()))
        );

        // Unknown sandbox root
        let bad_root = r#"
capability = "test.skill"
tier = "SAFE"
disabled = false
implemented = false
kind = "child"
[sandbox]
read_only = ["invalid_root"]
read_write = []
timeout_ms = 1000
memory_mib = 16
cpu_s = 1
"#;
        assert_eq!(
            Registry::parse_manifest("test.skill", bad_root),
            Err(RegistryError::UnknownSandboxRoot("invalid_root".into()))
        );
    }
}
