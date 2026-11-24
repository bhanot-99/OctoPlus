//! Every sentence the dialog shows (design.md §6). Wording lives here, in our
//! own code, keyed by capability — never in the request — so no other part
//! (and no model or web page) can choose what the dialog says (design.md §6.4 rule 7).

/// Fixed wording for one capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wording {
    /// Action title in plain words.
    pub title: &'static str,
    /// Exactly what will happen.
    pub lead: &'static str,
    /// The approve button says what it does (rule 3: never "OK" or "Yes").
    pub approve: &'static str,
}

/// Wording for `capability`; unknown capabilities get a neutral form that names the id.
#[must_use]
pub fn for_capability(capability: &str) -> Wording {
    match capability {
        "system.cleanup" => Wording {
            title: "Clean up disk space",
            lead: "OctoPlus will permanently delete these. Apps recreate caches and build files when needed:",
            approve: "Delete",
        },
        "web.search" => Wording {
            title: "Search the web",
            lead: "This exact text will be sent to",
            approve: "Send search",
        },
        "web.fetch" => Wording {
            title: "Open a web page",
            lead: "OctoPlus will fetch this page from",
            approve: "Fetch page",
        },
        "profile.remember" => Wording {
            title: "Remember a personal fact",
            lead: "OctoPlus will save this fact about you:",
            approve: "Remember it",
        },
        "profile.forget" => Wording {
            title: "Forget a personal fact",
            lead: "OctoPlus will delete this saved fact:",
            approve: "Forget it",
        },
        "memory.forget" => Wording {
            title: "Forget activity history",
            lead: "OctoPlus will delete the activity data matching:",
            approve: "Delete data",
        },
        "notes.append" => Wording {
            title: "Add to a note",
            lead: "OctoPlus will append this text:",
            approve: "Append text",
        },
        "privacy.pause" => Wording {
            title: "Pause sensing and listening",
            lead: "OctoPlus will stop watching and listening for:",
            approve: "Pause",
        },
        "system.read_metadata" => Wording {
            title: "Read machine facts",
            lead: "OctoPlus will read uptime and version numbers.",
            approve: "Allow once",
        },
        "system.disk_report" => Wording {
            title: "Show what uses disk space",
            lead: "OctoPlus will list sizes in the allowed folders.",
            approve: "Allow once",
        },
        _ => Wording {
            title: "Allow this action",
            lead: "OctoPlus wants to run:",
            approve: "Allow once",
        },
    }
}

/// Badge text per tier (design.md §3.2).
pub const BADGE_REVIEW: &str = "NEEDS YOUR OK";
/// Badge text for DANGEROUS.
pub const BADGE_DANGEROUS: &str = "DANGEROUS";
/// Egress badge.
pub const BADGE_EGRESS: &str = "LEAVES THIS DEVICE";
/// Taint panel badge.
pub const BADGE_TAINT: &str = "FROM THE WEB";
/// Deny button.
pub const DENY: &str = "Deny";
/// Taint panel text; `{hosts}` is replaced by the plain host list or "the web".
pub const TAINT_BODY: &str = "This request was influenced by content fetched from {hosts}. That content could be trying to trick the assistant.";
/// Under an editable query.
pub const EDIT_HINT: &str = "You can edit it. Remove anything you don't want to send.";
/// After the query; `{n}` is the page count.
pub const PAGES_LINE: &str = "OctoPlus will then read up to {n} result pages.";
/// Closing egress line.
pub const EGRESS_FOOTER: &str = "Nothing else is sent. This will be listed in your egress log.";
/// Prompt above the typed-phrase entry.
pub const DANGER_PROMPT: &str = "Type this phrase to enable the button:";
/// Expander label.
pub const DETAILS: &str = "Details";
/// Total line prefix.
pub const TOTAL: &str = "Total freed: about";
/// Label for permanent deletion rows.
pub const DELETE_FOREVER_SUFFIX: &str = "(delete permanently)";
/// Window title (also the accessible name's prefix).
pub const WINDOW_TITLE: &str = "OctoPlus confirmation";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_labels_never_ok_or_yes() {
        for cap in [
            "system.cleanup",
            "web.search",
            "web.fetch",
            "profile.remember",
            "profile.forget",
            "memory.forget",
            "notes.append",
            "privacy.pause",
            "system.read_metadata",
            "system.disk_report",
            "unknown.thing",
        ] {
            let w = for_capability(cap);
            let a = w.approve.to_ascii_lowercase();
            assert!(a != "ok" && a != "yes" && !a.is_empty(), "{cap}");
            assert!(!w.title.is_empty() && !w.lead.is_empty());
        }
    }

    #[test]
    fn specific_labels_match_design() {
        assert_eq!(for_capability("web.search").approve, "Send search");
        assert_eq!(for_capability("system.cleanup").approve, "Delete");
        assert_eq!(for_capability("x.y").approve, "Allow once");
    }
}
