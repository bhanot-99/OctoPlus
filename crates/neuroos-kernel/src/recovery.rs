//! Start-up recovery (phases.md §5.3 FI "kernel restart mid-dialog → expired").
//!
//! If C6 stopped between an action's `ATTEMPT` and its `RESULT` (crash, kill,
//! restart while a dialog was open), the dialog and its token died with it:
//! nonces live only in memory, so that approval can never be used. On start,
//! C6 finds every `ATTEMPT` in the current log without a matching `RESULT` and
//! records `RESULT EXPIRED` for it, so every action keeps its pair (FR-KER-06).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

/// An action left without a `RESULT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dangling {
    /// Action id.
    pub action_id: String,
    /// Skill.
    pub skill: String,
    /// Tier spelling as written (`SAFE`, `REVIEW`, `DANGEROUS`, `NONE`).
    pub tier: String,
    /// Taint bits.
    pub taint: u32,
    /// Origin spelling as written.
    pub origin: String,
    /// Argument hash, hex.
    pub args_hash: String,
}

/// Scans `actions.jsonl` (the current file only; rotated files are older than
/// any approval window) and returns the dangling actions in log order.
/// A missing file means nothing to recover. Unparsable lines are skipped here;
/// chain verification reports them.
#[must_use]
pub fn dangling(actions_log: &Path) -> Vec<Dangling> {
    let Ok(file) = std::fs::File::open(actions_log) else {
        return Vec::new();
    };
    let mut open: BTreeMap<u64, Dangling> = BTreeMap::new();
    let mut by_id: BTreeMap<String, u64> = BTreeMap::new();
    for (n, line) in BufReader::new(file).lines().map_while(Result::ok).enumerate() {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let id = v["action_id"].as_str().unwrap_or_default().to_owned();
        match v["kind"].as_str() {
            Some("ATTEMPT") => {
                let order = u64::try_from(n).unwrap_or(u64::MAX);
                by_id.insert(id.clone(), order);
                open.insert(
                    order,
                    Dangling {
                        action_id: id,
                        skill: v["skill"].as_str().unwrap_or_default().to_owned(),
                        tier: v["tier"].as_str().unwrap_or("NONE").to_owned(),
                        taint: v["taint"].as_u64().and_then(|t| u32::try_from(t).ok()).unwrap_or(0),
                        origin: v["origin"].as_str().unwrap_or("UNSPECIFIED").to_owned(),
                        args_hash: v["args_hash"].as_str().unwrap_or_default().to_owned(),
                    },
                );
            }
            Some("RESULT") => {
                if let Some(order) = by_id.remove(&id) {
                    open.remove(&order);
                }
            }
            _ => {}
        }
    }
    open.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str, id: &str) -> String {
        format!(
            r#"{{"seq":1,"at_ns":1,"kind":"{kind}","action_id":"{id}","skill":"system.cleanup","tier":"REVIEW","taint":2,"origin":"CLI","args_hash":"ab","status":null,"prev_hash":"00"}}"#
        )
    }

    #[test]
    fn finds_attempts_without_results() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("actions.jsonl");
        let text = [
            line("ATTEMPT", "a"),
            line("RESULT", "a"),
            line("ATTEMPT", "b"),
            "not json".to_owned(),
            line("ATTEMPT", "c"),
            line("RESULT", "c"),
            line("ATTEMPT", "d"),
        ]
        .join("\n");
        std::fs::write(&p, text).unwrap();
        let found = dangling(&p);
        let ids: Vec<_> = found.iter().map(|x| x.action_id.as_str()).collect();
        assert_eq!(ids, ["b", "d"]);
        assert_eq!(found[0].tier, "REVIEW");
        assert_eq!(found[0].taint, 2);
        assert_eq!(found[0].origin, "CLI");
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(dangling(Path::new("/nonexistent/actions.jsonl")).is_empty());
    }
}
