#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Kernel restart mid-dialog → EXPIRED (phases.md §5.3 FI, FR-KER-06).

use neuroos_kernel::{ActionLog, ActionOrigin, ActionStatus, ActionTier, close_dangling_actions};

#[test]
fn open_attempts_are_closed_as_expired_and_the_chain_stays_valid() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("audit");
    std::fs::create_dir(&audit).unwrap();
    {
        let mut log = ActionLog::open(&audit).unwrap();
        let h = "ab".repeat(32);
        log.attempt(
            1,
            "done",
            "system.read_metadata",
            ActionTier::Safe,
            0,
            ActionOrigin::Cli,
            &h,
        )
        .unwrap();
        log.result(
            2,
            "done",
            "system.read_metadata",
            ActionTier::Safe,
            0,
            ActionOrigin::Cli,
            &h,
            ActionStatus::Executed,
        )
        .unwrap();
        // The kernel died while this one's dialog was open.
        log.attempt(
            3,
            "open",
            "system.cleanup",
            ActionTier::Review,
            2,
            ActionOrigin::Cli,
            &h,
        )
        .unwrap();
    }
    assert_eq!(close_dangling_actions(&audit).unwrap(), 1);
    // Idempotent: a second start finds nothing open.
    assert_eq!(close_dangling_actions(&audit).unwrap(), 0);

    let text = std::fs::read_to_string(audit.join("actions.jsonl")).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["kind"], "RESULT");
    assert_eq!(last["action_id"], "open");
    assert_eq!(last["status"], "EXPIRED");
    assert_eq!(last["tier"], "REVIEW");
    assert_eq!(last["taint"], 2);
    let report = neuroos_kernel::ChainedLog::verify(audit.join("actions.jsonl"), None);
    assert!(report.chain_ok, "{report:?}");
    assert_eq!(report.records_checked, 4);
}
