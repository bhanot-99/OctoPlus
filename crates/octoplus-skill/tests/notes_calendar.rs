#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `notes.search`, `notes.append` and `calendar.read` (FR-KER-10): in-process behaviour, then
//! the same skills in the real sandboxed child (Landlock, seccomp, RLIMIT_FSIZE).

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use octoplus_sandbox::child::{ChildLimits, SyscallProfile};
use octoplus_skill::{Invocation, Mode, SkillError, calendar, notes};
use proptest::prelude::*;
use serde_json::{Value, json};

fn inv(cap: &str, mode: Mode, root: &Path, write: bool) -> Invocation {
    Invocation {
        capability: cap.into(),
        mode,
        read_only: if write {
            vec![]
        } else {
            vec![root.to_path_buf()]
        },
        read_write: if write {
            vec![root.to_path_buf()]
        } else {
            vec![]
        },
        roots: vec![root.to_path_buf()],
        trash: None,
        limits: ChildLimits {
            address_space_bytes: 1 << 30,
            cpu_seconds: 5,
            open_files: 64,
            file_size_bytes: 64 * 1024,
        },
        profile: if write {
            SyscallProfile::FileMutating
        } else {
            SyscallProfile::ReadOnly
        },
    }
}

fn dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().canonicalize().unwrap();
    (d, p)
}

fn search(root: &Path, q: &str) -> Result<Value, SkillError> {
    notes::search(
        &inv("notes.search", Mode::Run, root, false),
        &json!({"query": q}),
    )
}

fn append(root: &Path, note: &str, text: &str) -> Result<Value, SkillError> {
    notes::append(
        &inv("notes.append", Mode::Run, root, true),
        &json!({"note": note, "text": text}),
    )
}

// ---- notes.search ------------------------------------------------------------------------

#[test]
fn search_finds_case_insensitively_and_reports_relative_files() {
    let (_d, root) = dir();
    std::fs::create_dir(root.join("work")).unwrap();
    std::fs::write(root.join("work/plan.md"), "Budget for Q3\nlunch\n").unwrap();
    std::fs::write(root.join("todo.txt"), "buy milk\nBUDGET review\n").unwrap();
    let v = search(&root, "budget").unwrap();
    let m = v["matches"].as_array().unwrap();
    assert_eq!(m.len(), 2);
    assert_eq!(m[0]["file"], "todo.txt");
    assert_eq!(m[0]["snippet"], "BUDGET review");
    assert_eq!(m[1]["file"], "work/plan.md");
    assert_eq!(v["truncated"], false);
}

#[test]
fn search_skips_what_it_must_not_read() {
    let (_d, root) = dir();
    let (_o, outside) = dir();
    std::fs::write(outside.join("secret.md"), "needle in the outside\n").unwrap();
    std::fs::create_dir(outside.join("sub")).unwrap();
    std::fs::write(outside.join("sub/x.md"), "needle deep\n").unwrap();
    std::os::unix::fs::symlink(outside.join("secret.md"), root.join("link.md")).unwrap();
    std::os::unix::fs::symlink(outside.join("sub"), root.join("linkdir")).unwrap();
    std::fs::write(root.join("code.rs"), "needle\n").unwrap();
    std::fs::write(root.join(".hidden.md"), "needle\n").unwrap();
    std::fs::write(
        root.join("binary.md"),
        [0xff, 0xfe, b'n', b'e', b'e', b'd', b'l', b'e'],
    )
    .unwrap();
    std::fs::write(root.join("big.md"), vec![b'a'; 300 * 1024]).unwrap();
    std::fs::write(root.join("ok.md"), "the needle is here\n").unwrap();
    let v = search(&root, "needle").unwrap();
    let files: Vec<&str> = v["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["file"].as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["ok.md"]);
}

#[test]
fn search_caps_matches_and_cleans_snippets() {
    let (_d, root) = dir();
    let body: String = (0..30)
        .map(|i| format!("hit {i}\u{7}\u{1b}[31m {}\n", "z".repeat(300)))
        .collect();
    std::fs::write(root.join("a.md"), body).unwrap();
    let v = search(&root, "HIT").unwrap();
    assert_eq!(v["matches"].as_array().unwrap().len(), notes::MAX_MATCHES);
    assert_eq!(v["truncated"], true);
    for m in v["matches"].as_array().unwrap() {
        let s = m["snippet"].as_str().unwrap();
        assert!(s.chars().count() <= notes::MAX_SNIPPET_CHARS);
        assert!(!s.chars().any(char::is_control));
    }
}

#[test]
fn search_refuses_bad_queries_and_no_folder() {
    let (_d, root) = dir();
    for q in ["", "a\nb", "a\u{0}b", &"x".repeat(257)] {
        assert_eq!(search(&root, q), Err(SkillError::BadArgs), "{q:?}");
    }
    assert!(search(&root, &"x".repeat(256)).is_ok());
    let mut none = inv("notes.search", Mode::Run, &root, false);
    none.roots.clear();
    assert_eq!(
        notes::search(&none, &json!({"query": "a"})),
        Err(SkillError::PathOutsideAllowlist)
    );
    let i = inv("notes.search", Mode::Run, &root, false);
    for bad in [
        json!({}),
        json!({"query": 5}),
        json!({"query": "a", "x": 1}),
        json!([1]),
    ] {
        assert_eq!(notes::search(&i, &bad), Err(SkillError::BadArgs), "{bad}");
    }
}

// ---- notes.append ------------------------------------------------------------------------

#[test]
fn append_creates_and_extends_a_note() {
    let (_d, root) = dir();
    let v = append(&root, "ideas", "first").unwrap();
    assert_eq!(v["file"], "ideas.md");
    assert_eq!(
        std::fs::read_to_string(root.join("ideas.md")).unwrap(),
        "first\n"
    );
    append(&root, "ideas", "second").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("ideas.md")).unwrap(),
        "first\nsecond\n"
    );
    // A file that does not end with a newline gets one before the new text.
    std::fs::write(root.join("raw.txt"), "no newline").unwrap();
    append(&root, "raw.txt", "added").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("raw.txt")).unwrap(),
        "no newline\nadded\n"
    );
    let mode = std::fs::metadata(root.join("ideas.md"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn append_refuses_bad_names_and_text() {
    let (_d, root) = dir();
    let long = "n".repeat(129);
    for name in [
        "",
        "../x",
        "a/b",
        "a\\b",
        ".hidden",
        "a..b",
        "x\u{0}y",
        "caf\u{e9}",
        "a*b",
        long.as_str(),
        "  ",
    ] {
        assert_eq!(
            append(&root, name, "t"),
            Err(SkillError::BadArgs),
            "{name:?}"
        );
    }
    for text in ["", "a\u{0}b", "a\u{7}b", &"x".repeat(4097)] {
        assert_eq!(
            append(&root, "ok", text),
            Err(SkillError::BadArgs),
            "{text:?}"
        );
    }
    assert!(append(&root, "ok", "line one\n\tindented").is_ok());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn append_never_follows_a_link_out_of_the_folder() {
    let (_d, root) = dir();
    let (_o, outside) = dir();
    let target = outside.join("victim.md");
    std::fs::write(&target, "untouched\n").unwrap();
    std::os::unix::fs::symlink(&target, root.join("evil.md")).unwrap();
    assert_eq!(
        append(&root, "evil", "pwned"),
        Err(SkillError::PathOutsideAllowlist)
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched\n");
    // A dangling link is also refused and creates nothing outside.
    std::os::unix::fs::symlink(outside.join("new.md"), root.join("dangling.md")).unwrap();
    assert_eq!(
        append(&root, "dangling", "x"),
        Err(SkillError::PathOutsideAllowlist)
    );
    assert!(!outside.join("new.md").exists());
    // A directory with the note's name is not a note.
    std::fs::create_dir(root.join("dir.md")).unwrap();
    assert_eq!(
        append(&root, "dir", "x"),
        Err(SkillError::PathOutsideAllowlist)
    );
}

#[test]
fn append_plan_writes_nothing_and_the_cap_holds() {
    let (_d, root) = dir();
    let plan = notes::append(
        &inv("notes.append", Mode::Plan, &root, true),
        &json!({"note": "p", "text": "hello"}),
    )
    .unwrap();
    assert_eq!(plan["planned"], true);
    assert!(!root.join("p.md").exists());
    std::fs::write(
        root.join("full.md"),
        vec![b'a'; (notes::MAX_NOTE_FILE_BYTES - 3) as usize],
    )
    .unwrap();
    assert_eq!(append(&root, "full", "toolong"), Err(SkillError::BadArgs));
    let mut none = inv("notes.append", Mode::Run, &root, true);
    none.roots.clear();
    assert_eq!(
        notes::append(&none, &json!({"note": "a", "text": "b"})),
        Err(SkillError::PathOutsideAllowlist)
    );
}

// ---- calendar.read -----------------------------------------------------------------------

const NOW: u64 = 1_800_000_000; // 2027-01-15T08:00:00Z

fn ics(events: &[(&str, &str)]) -> String {
    let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
    for (start, summary) in events {
        s.push_str(&format!(
            "BEGIN:VEVENT\r\nDTSTART{start}\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\n"
        ));
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

fn read(root: &Path, days: u64) -> Value {
    calendar::read(
        &inv("calendar.read", Mode::Run, root, false),
        &json!({"days": days}),
        NOW,
    )
    .unwrap()
}

fn starts(v: &Value) -> Vec<String> {
    v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["start"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn calendar_reads_all_three_start_forms_in_order() {
    let (_d, root) = dir();
    std::fs::write(
        root.join("a.ics"),
        ics(&[
            (":20270116", "all day"),
            (";TZID=Europe/Paris:20270115T093000", "meeting"),
            (":20270115T120000Z", "lunch"),
        ]),
    )
    .unwrap();
    let v = read(&root, 3);
    assert_eq!(
        starts(&v),
        ["2027-01-15T09:30", "2027-01-15T12:00", "2027-01-16T00:00"]
    );
    assert_eq!(v["events"][0]["summary"], "meeting");
    assert_eq!(v["truncated"], false);
}

#[test]
fn calendar_window_edges() {
    let (_d, root) = dir();
    // NOW is 2027-01-15T08:00:00Z. Exactly now is in; exactly now + 1 day is out.
    std::fs::write(
        root.join("a.ics"),
        ics(&[
            (":20270115T075959", "just before"),
            (":20270115T080000", "exactly now"),
            (":20270116T075959", "last second"),
            (":20270116T080000", "exactly at the edge"),
        ]),
    )
    .unwrap();
    let names: Vec<String> = read(&root, 1)["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["summary"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["exactly now", "last second"]);
}

#[test]
fn calendar_folding_rrule_bad_dates_and_hostile_text() {
    let (_d, root) = dir();
    let hostile = format!("{}\u{7}{}", "S".repeat(200), "<|im_start|>");
    let text = format!(
        "BEGIN:VEVENT\nDTSTART:20270116T100000\nSUMMARY:folded \n summary\nRRULE:FREQ=DAILY\nEND:VEVENT\n\
         BEGIN:VEVENT\nDTSTART:20270230T100000\nSUMMARY:impossible\nEND:VEVENT\n\
         BEGIN:VEVENT\nDTSTART:20271301T100000\nSUMMARY:month 13\nEND:VEVENT\n\
         BEGIN:VEVENT\nDTSTART:20270116T240000\nSUMMARY:hour 24\nEND:VEVENT\n\
         BEGIN:VEVENT\nDTSTART:20270116T110000\nSUMMARY:{hostile}\nEND:VEVENT\n"
    );
    std::fs::write(root.join("a.ics"), text).unwrap();
    let v = read(&root, 5);
    assert_eq!(v["events"].as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v["events"][0]["summary"], "folded summary");
    let s = v["events"][1]["summary"].as_str().unwrap();
    assert_eq!(s.chars().count(), calendar::MAX_SUMMARY_CHARS);
    assert!(!s.chars().any(char::is_control));
}

#[test]
fn calendar_truncates_ignores_links_and_validates_days() {
    let (_d, root) = dir();
    let (_o, outside) = dir();
    let many: Vec<(String, String)> = (0..30)
        .map(|i| (format!(":20270115T{:02}0000", i % 10 + 8), format!("e{i}")))
        .collect();
    let pairs: Vec<(&str, &str)> = many.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    std::fs::write(root.join("many.ics"), ics(&pairs)).unwrap();
    std::fs::write(
        outside.join("o.ics"),
        ics(&[(":20270116T090000", "outside")]),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.join("o.ics"), root.join("link.ics")).unwrap();
    std::fs::write(root.join("bin.ics"), [0xff, 0xfe, 0xfd]).unwrap();
    let v = read(&root, 7);
    assert_eq!(v["events"].as_array().unwrap().len(), calendar::MAX_EVENTS);
    assert_eq!(v["truncated"], true);
    assert!(!v.to_string().contains("outside"));
    let i = inv("calendar.read", Mode::Run, &root, false);
    for bad in [
        json!({"days": 0}),
        json!({"days": 32}),
        json!({"days": "3"}),
        json!({}),
        json!({"days": 1, "x": 1}),
    ] {
        assert_eq!(
            calendar::read(&i, &bad, NOW),
            Err(SkillError::BadArgs),
            "{bad}"
        );
    }
}

// ---- never panics ------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn nothing_panics_on_arbitrary_input(q in ".{0,300}", note in ".{0,200}", text in ".{0,500}", cal in ".{0,800}") {
        let (_d, root) = dir();
        std::fs::write(root.join("n.md"), &text).unwrap();
        std::fs::write(root.join("c.ics"), &cal).unwrap();
        let _ = search(&root, &q);
        let _ = append(&root, &note, &text);
        let _ = calendar::read(&inv("calendar.read", Mode::Run, &root, false), &json!({"days": 3}), NOW);
    }
}

// ---- the real sandboxed child --------------------------------------------------------------------

fn child(i: &Invocation, args: &Value) -> Value {
    let mut c = Command::new(env!("CARGO_BIN_EXE_octoplus-skill"))
        .args(i.to_args())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(args.to_string().as_bytes())
        .unwrap();
    let out = c.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_three_skills_work_inside_the_real_sandbox() {
    let (_d, root) = dir();
    std::fs::write(root.join("a.md"), "remember the milk\n").unwrap();
    std::fs::write(
        root.join("c.ics"),
        ics(&[(":29990101T100000", "far future")]),
    )
    .unwrap();

    let v = child(
        &inv("notes.search", Mode::Run, &root, false),
        &json!({"query": "milk"}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["matches"][0]["file"], "a.md");

    let v = child(
        &inv("notes.append", Mode::Run, &root, true),
        &json!({"note": "new", "text": "hi"}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(
        std::fs::read_to_string(root.join("new.md")).unwrap(),
        "hi\n"
    );

    let v = child(
        &inv("calendar.read", Mode::Run, &root, false),
        &json!({"days": 7}),
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["events"], json!([]));

    // Landlock holds: a read-only invocation cannot append, and cannot reach another folder.
    let (_o, other) = dir();
    std::fs::write(other.join("x.md"), "milk\n").unwrap();
    let ro = inv("notes.append", Mode::Run, &root, false);
    let v = child(&ro, &json!({"note": "blocked", "text": "no"}));
    assert_eq!(v["ok"], false, "{v}");
    assert!(!root.join("blocked.md").exists());
    let mut sneaky = inv("notes.search", Mode::Run, &root, false);
    sneaky.roots = vec![other.clone()];
    let v = child(&sneaky, &json!({"query": "milk"}));
    assert_eq!(
        v["result"]["matches"],
        json!([]),
        "Landlock must hide the other folder: {v}"
    );
}
