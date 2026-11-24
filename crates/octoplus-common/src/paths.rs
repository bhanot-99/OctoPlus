//! Filesystem layout (Architecture §7.1) and socket locations (§5.2).
//!
//! Every function takes the environment it needs as an argument, so callers
//! decide where it comes from and tests never touch process-global state.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Errors resolving a path.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    /// A required environment variable is unset or empty.
    #[error("environment variable {0} is not set")]
    Unset(&'static str),
    /// The variable is set but not an absolute path.
    #[error("environment variable {0} must be an absolute path")]
    NotAbsolute(&'static str),
}

/// The parts that own a runtime folder or talk over IPC (Architecture §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// C1 `octoplus-monitor`.
    Monitor,
    /// C2 `octoplus-voice`.
    Voice,
    /// C3 `octoplus-storage`.
    Storage,
    /// C4 `octoplus-inference`.
    Inference,
    /// C5 `octoplus-knowledge`.
    Knowledge,
    /// C6 `octoplus-kernel`.
    Kernel,
    /// C7 `octoplus-fetcher` (system scope, own user).
    Fetcher,
    /// D `octoplus-healthd`.
    Healthd,
}

impl Part {
    /// Every part, in start order (Architecture §13).
    pub const ALL: [Part; 8] = [
        Part::Healthd,
        Part::Storage,
        Part::Inference,
        Part::Monitor,
        Part::Kernel,
        Part::Knowledge,
        Part::Voice,
        Part::Fetcher,
    ];

    /// Short name used for the runtime sub-folder.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Part::Monitor => "monitor",
            Part::Voice => "voice",
            Part::Storage => "storage",
            Part::Inference => "inference",
            Part::Knowledge => "knowledge",
            Part::Kernel => "kernel",
            Part::Fetcher => "fetcher",
            Part::Healthd => "healthd",
        }
    }

    /// systemd unit name.
    #[must_use]
    pub fn unit(self) -> String {
        format!("octoplus-{}.service", self.name())
    }

    /// PRD §6.2 RSS budget in MiB.
    #[must_use]
    pub const fn rss_budget_mib(self) -> u64 {
        match self {
            Part::Monitor => 30,
            Part::Voice => 800,
            Part::Storage => 320,
            Part::Inference => 2800,
            Part::Knowledge => 60,
            Part::Kernel => 15,
            Part::Fetcher => 15,
            Part::Healthd => 15,
        }
    }
}

/// Total system RSS budget in MiB (PRD §6.2: sum of budgets, <= 4000 MiB cap).
pub const TOTAL_RSS_BUDGET_MIB: u64 = 3875;

/// Fetcher sockets live outside the user runtime root (Architecture §5.2).
pub const FETCHER_RUNTIME_DIR: &str = "/run/octoplus-fetcher";

fn absolute(var: &'static str, value: Option<&OsStr>) -> Result<PathBuf, PathError> {
    let v = value
        .filter(|v| !v.is_empty())
        .ok_or(PathError::Unset(var))?;
    let p = PathBuf::from(v);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(PathError::NotAbsolute(var))
    }
}

/// `$XDG_RUNTIME_DIR/octoplus`, the runtime root (mode 0700).
///
/// # Errors
/// `XDG_RUNTIME_DIR` unset, empty or relative.
pub fn runtime_root(xdg_runtime_dir: Option<&OsStr>) -> Result<PathBuf, PathError> {
    Ok(absolute("XDG_RUNTIME_DIR", xdg_runtime_dir)?.join("octoplus"))
}

/// The folder that holds `part`'s sockets.
#[must_use]
pub fn part_dir(runtime_root: &Path, part: Part) -> PathBuf {
    match part {
        Part::Fetcher => PathBuf::from(FETCHER_RUNTIME_DIR),
        _ => runtime_root.join(part.name()),
    }
}

/// Path to a specific socket named `socket_name` served by `part` (Architecture §5.2).
///
/// In the Phase 1 layout, each socket lives in its own dedicated directory:
/// `<runtime_root>/<part>/<socket>/<socket>.sock`, or for fetcher
/// `/run/octoplus-fetcher/<socket>/<socket>.sock`.
#[must_use]
pub fn socket_path(runtime_root: &Path, part: Part, socket_name: &str) -> PathBuf {
    part_dir(runtime_root, part)
        .join(socket_name)
        .join(format!("{socket_name}.sock"))
}

/// `part`'s health socket (served to healthd).
///
/// In the Phase 1 layout: `<part>/health/health.sock`, or `/run/octoplus-fetcher/health/health.sock`.
#[must_use]
pub fn health_socket(runtime_root: &Path, part: Part) -> PathBuf {
    socket_path(runtime_root, part, "health")
}

/// `~/.config/octoplus` (or `$XDG_CONFIG_HOME/octoplus`).
///
/// # Errors
/// Neither variable gives an absolute path.
pub fn config_dir(
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf, PathError> {
    match absolute("XDG_CONFIG_HOME", xdg_config_home) {
        Ok(p) => Ok(p.join("octoplus")),
        Err(_) => Ok(absolute("HOME", home)?.join(".config").join("octoplus")),
    }
}

/// `~/.local/share/octoplus` (or `$XDG_DATA_HOME/octoplus`): all persistent user data.
///
/// # Errors
/// Neither variable gives an absolute path.
pub fn data_dir(xdg_data_home: Option<&OsStr>, home: Option<&OsStr>) -> Result<PathBuf, PathError> {
    match absolute("XDG_DATA_HOME", xdg_data_home) {
        Ok(p) => Ok(p.join("octoplus")),
        Err(_) => Ok(absolute("HOME", home)?
            .join(".local")
            .join("share")
            .join("octoplus")),
    }
}

/// The config file: `$OCTOPLUS_CONFIG` if set, else `config.toml` in [`config_dir`].
///
/// # Errors
/// As [`config_dir`], or `OCTOPLUS_CONFIG` set to a relative path.
pub fn config_file(
    octoplus_config: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf, PathError> {
    match octoplus_config.filter(|v| !v.is_empty()) {
        Some(v) => absolute(crate::config::CONFIG_ENV, Some(v)),
        None => Ok(config_dir(xdg_config_home, home)?.join("config.toml")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> Option<&OsStr> {
        Some(OsStr::new(s))
    }

    #[test]
    fn runtime_root_requires_absolute_xdg() {
        assert_eq!(
            runtime_root(os("/run/user/1000")),
            Ok(PathBuf::from("/run/user/1000/octoplus"))
        );
        assert_eq!(runtime_root(None), Err(PathError::Unset("XDG_RUNTIME_DIR")));
        assert_eq!(
            runtime_root(os("")),
            Err(PathError::Unset("XDG_RUNTIME_DIR"))
        );
        assert_eq!(
            runtime_root(os("run/user")),
            Err(PathError::NotAbsolute("XDG_RUNTIME_DIR"))
        );
    }

    #[test]
    fn part_dirs_follow_socket_map() {
        let root = PathBuf::from("/run/user/1000/octoplus");
        assert_eq!(
            health_socket(&root, Part::Storage),
            PathBuf::from("/run/user/1000/octoplus/storage/health/health.sock")
        );
        assert_eq!(
            health_socket(&root, Part::Fetcher),
            PathBuf::from("/run/octoplus-fetcher/health/health.sock")
        );
        assert_eq!(
            socket_path(&root, Part::Monitor, "stream"),
            PathBuf::from("/run/user/1000/octoplus/monitor/stream/stream.sock")
        );
        assert_eq!(
            socket_path(&root, Part::Fetcher, "fetch"),
            PathBuf::from("/run/octoplus-fetcher/fetch/fetch.sock")
        );
        for p in Part::ALL {
            assert!(p.unit().starts_with("octoplus-") && p.unit().ends_with(".service"));
        }
    }

    #[test]
    fn xdg_overrides_home() {
        assert_eq!(
            config_dir(os("/x/cfg"), os("/home/u")),
            Ok(PathBuf::from("/x/cfg/octoplus"))
        );
        assert_eq!(
            config_dir(None, os("/home/u")),
            Ok(PathBuf::from("/home/u/.config/octoplus"))
        );
        assert_eq!(
            config_dir(os("relative"), os("/home/u")),
            Ok(PathBuf::from("/home/u/.config/octoplus"))
        );
        assert_eq!(
            data_dir(None, os("/home/u")),
            Ok(PathBuf::from("/home/u/.local/share/octoplus"))
        );
        assert_eq!(data_dir(os("/d"), None), Ok(PathBuf::from("/d/octoplus")));
        assert_eq!(data_dir(None, None), Err(PathError::Unset("HOME")));
    }

    #[test]
    fn config_file_env_override() {
        assert_eq!(
            config_file(os("/etc/x.toml"), None, os("/home/u")),
            Ok(PathBuf::from("/etc/x.toml"))
        );
        assert_eq!(
            config_file(os("x.toml"), None, os("/home/u")),
            Err(PathError::NotAbsolute("OCTOPLUS_CONFIG"))
        );
        assert_eq!(
            config_file(None, None, os("/home/u")),
            Ok(PathBuf::from("/home/u/.config/octoplus/config.toml"))
        );
    }
}
