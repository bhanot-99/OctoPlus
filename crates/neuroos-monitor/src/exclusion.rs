//! The app exclusion list (FR-MON-10, FR-PRV-02). Matching is case-insensitive; an app
//! pattern is an exact id or `prefix*`; a title rule is a substring. Empty rules are
//! ignored so a blank config line can never exclude everything — or nothing by accident.

use neuroos_common::config::MonitorConfig;

/// App ids excluded by default: common password managers (FR-PRV-02).
pub const DEFAULT_APPS: &[&str] = &[
    "1password*",
    "com.1password*",
    "bitwarden*",
    "com.bitwarden*",
    "org.keepassxc*",
    "keepassxc*",
    "org.gnome.seahorse*",
    "com.proton.pass*",
    "proton-pass*",
    "enpass*",
    "dashlane*",
    "lastpass*",
];

/// Window-title markers of private browsing windows (FR-PRV-02).
pub const DEFAULT_TITLE_MARKERS: &[&str] = &[
    "private browsing",
    "private window",
    "incognito",
    "inprivate",
    "tor browser",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppRule {
    Exact(String),
    Prefix(String),
}

/// The compiled exclusion list.
#[derive(Debug, Clone, Default)]
pub struct Exclusion {
    apps: Vec<AppRule>,
    titles: Vec<String>,
}

fn compile_app(pattern: &str) -> Option<AppRule> {
    let p = pattern.trim().to_lowercase();
    match p.strip_suffix('*') {
        Some("") => None, // a bare `*` would exclude everything; refuse it
        Some(prefix) => Some(AppRule::Prefix(prefix.to_owned())),
        None if p.is_empty() => None,
        None => Some(AppRule::Exact(p)),
    }
}

impl Exclusion {
    /// Builds the list from the defaults plus the user's additions.
    #[must_use]
    pub fn from_config(cfg: &MonitorConfig) -> Self {
        let mut apps = Vec::new();
        let mut titles = Vec::new();
        if !cfg.drop_default_exclusions {
            apps.extend(DEFAULT_APPS.iter().filter_map(|p| compile_app(p)));
            titles.extend(DEFAULT_TITLE_MARKERS.iter().map(|t| (*t).to_owned()));
        }
        apps.extend(cfg.exclude_apps.iter().filter_map(|p| compile_app(p)));
        titles.extend(
            cfg.exclude_title_substrings
                .iter()
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty()),
        );
        Self { apps, titles }
    }

    /// Builds a list from explicit rules (tests, property tests).
    #[must_use]
    pub fn new(apps: &[&str], titles: &[&str]) -> Self {
        Self {
            apps: apps.iter().filter_map(|p| compile_app(p)).collect(),
            titles: titles
                .iter()
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect(),
        }
    }

    /// Whether an app id (or a process name) is on the list. An empty id never matches.
    #[must_use]
    pub fn matches_app(&self, app_id: &str) -> bool {
        if app_id.is_empty() {
            return false;
        }
        let id = app_id.to_lowercase();
        self.apps.iter().any(|r| match r {
            AppRule::Exact(e) => id == *e,
            AppRule::Prefix(p) => id.starts_with(p.as_str()),
        })
    }

    /// Whether a window title carries a private-window marker.
    #[must_use]
    pub fn matches_title(&self, title: &str) -> bool {
        if title.is_empty() {
            return false;
        }
        let t = title.to_lowercase();
        self.titles.iter().any(|m| t.contains(m.as_str()))
    }

    /// Whether a window is excluded.
    #[must_use]
    pub fn is_excluded(&self, app_id: &str, title: &str) -> bool {
        self.matches_app(app_id) || self.matches_title(title)
    }
}
