//! Tests for `neuroosctl replay` (FR-MON-12, P3-S06).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::BufRead;
use std::path::Path;
use std::time::Duration;

use neuroos_ipc::Client;
use neuroos_proto::dump::DumpWriter;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{FocusChanged, IdleChanged, LockChanged, MonitorEvent, MonitorSubscribe};

fn make_test_events() -> Vec<MonitorEvent> {
    vec![
        MonitorEvent {
            observed_at_ns: 1000,
            event: Some(Event::Idle(IdleChanged { idle: true })),
        },
        MonitorEvent {
            observed_at_ns: 2000,
            event: Some(Event::Lock(LockChanged { locked: false })),
        },
        MonitorEvent {
            observed_at_ns: 3000,
            event: Some(Event::Focus(FocusChanged {
                app_id: "org.test.App".to_string(),
                title: "Test Window".to_string(),
                pid: 42,
                fullscreen: true,
                fullscreen_known: true,
            })),
        },
    ]
}

fn write_dump(path: &Path, events: &[MonitorEvent]) {
    let file = std::fs::File::create(path).expect("create dump file");
    let mut writer = DumpWriter::new(file).expect("create dump writer");
    for ev in events {
        writer.write(ev).expect("write event");
    }
    writer.finish().expect("finish dump");
}

#[test]
fn replay_print_exact_lines_at_speed_zero() {
    let dir = tempfile::tempdir().unwrap();
    let dump_path = dir.path().join("test.dump");
    let events = make_test_events();
    write_dump(&dump_path, &events);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args(["replay", dump_path.to_str().unwrap(), "--speed", "0"])
        .output()
        .expect("run neuroosctl");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "t=1000 idle IdleChanged { idle: true }");
    assert_eq!(lines[1], "t=2000 lock LockChanged { locked: false }");
    assert_eq!(
        lines[2],
        "t=3000 focus FocusChanged { app_id: \"org.test.App\", title: \"Test Window\", pid: 42, fullscreen: true, fullscreen_known: true }"
    );
}

#[test]
fn replay_json_lines_parse_with_right_fields() {
    let dir = tempfile::tempdir().unwrap();
    let dump_path = dir.path().join("test.dump");
    let events = make_test_events();
    write_dump(&dump_path, &events);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args([
            "replay",
            dump_path.to_str().unwrap(),
            "--speed",
            "0",
            "--json",
        ])
        .output()
        .expect("run neuroosctl");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);

    let j0: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(j0["t_ns"], 1000);
    assert_eq!(j0["kind"], "idle");
    assert_eq!(j0["event"], "IdleChanged { idle: true }");

    let j1: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(j1["t_ns"], 2000);
    assert_eq!(j1["kind"], "lock");
    assert_eq!(j1["event"], "LockChanged { locked: false }");

    let j2: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(j2["t_ns"], 3000);
    assert_eq!(j2["kind"], "focus");
    assert!(j2["event"].as_str().unwrap().contains("org.test.App"));
}

#[test]
fn replay_truncated_dump_fails_with_exit_code_1() {
    let dir = tempfile::tempdir().unwrap();
    let dump_path = dir.path().join("good.dump");
    let trunc_path = dir.path().join("trunc.dump");
    let events = make_test_events();
    write_dump(&dump_path, &events);

    let bytes = std::fs::read(&dump_path).unwrap();
    // Cut off the end record (the last 12 bytes: 4 bytes END_MARK + 8 bytes count)
    assert!(bytes.len() > 12);
    let truncated_bytes = &bytes[..bytes.len() - 12];
    std::fs::write(&trunc_path, truncated_bytes).unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args(["replay", trunc_path.to_str().unwrap(), "--speed", "0"])
        .output()
        .expect("run neuroosctl");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "stdout must be empty on error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("truncated"),
        "stderr should mention truncated: {stderr}"
    );
}

#[test]
fn replay_bad_magic_fails_with_exit_code_1() {
    let dir = tempfile::tempdir().unwrap();
    let bad_magic_path = dir.path().join("bad_magic.dump");
    std::fs::write(&bad_magic_path, b"NOT_DUMP_HEADER_DATA_12345").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args(["replay", bad_magic_path.to_str().unwrap(), "--speed", "0"])
        .output()
        .expect("run neuroosctl");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "stdout must be empty on error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("magic")
            || stderr.to_lowercase().contains("not a neuroos dump"),
        "stderr should mention bad magic: {stderr}"
    );
}

#[test]
fn replay_missing_file_fails_with_exit_code_1() {
    let dir = tempfile::tempdir().unwrap();
    let missing_path = dir.path().join("does_not_exist.dump");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args(["replay", missing_path.to_str().unwrap(), "--speed", "0"])
        .output()
        .expect("run neuroosctl");

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "stdout must be empty on error");
}

#[test]
fn replay_negative_speed_fails_with_non_zero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let dump_path = dir.path().join("test.dump");
    let events = make_test_events();
    write_dump(&dump_path, &events);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args(["replay", dump_path.to_str().unwrap(), "--speed", "-1.0"])
        .output()
        .expect("run neuroosctl");

    assert_ne!(output.status.code(), Some(0));
}

#[tokio::test]
async fn replay_serve_streams_events_to_multiple_subscribers() {
    let dir = tempfile::tempdir().unwrap();
    let dump_path = dir.path().join("test.dump");
    let events = make_test_events();
    write_dump(&dump_path, &events);

    let sock_dir = dir.path().join("replay");
    let sock_path = sock_dir.join("stream.sock");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_neuroosctl"))
        .args([
            "replay",
            dump_path.to_str().unwrap(),
            "--serve",
            sock_path.to_str().unwrap(),
            "--speed",
            "0",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn neuroosctl");

    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = std::io::BufReader::new(stdout);

    // Read the "serving ..." line from stdout
    let mut line = String::new();
    reader.read_line(&mut line).expect("read serving line");
    assert!(
        line.contains("serving"),
        "expected 'serving ...', got: {line}"
    );

    // Subscriber 1
    let client1 = Client::connect(&sock_path, 1024 * 1024).await.unwrap();
    let mut stream1 = client1
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    let mut received1 = Vec::new();
    while let Some(res) = stream1.next().await {
        let body = res.unwrap();
        match body {
            Body::MonitorEvent(ev) => received1.push(ev),
            Body::Ack(_) => break,
            other => panic!("unexpected body {other:?}"),
        }
    }
    assert_eq!(
        received1, events,
        "subscriber 1 received all events in order"
    );

    // Subscriber 2
    let client2 = Client::connect(&sock_path, 1024 * 1024).await.unwrap();
    let mut stream2 = client2
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    let mut received2 = Vec::new();
    while let Some(res) = stream2.next().await {
        let body = res.unwrap();
        match body {
            Body::MonitorEvent(ev) => received2.push(ev),
            Body::Ack(_) => break,
            other => panic!("unexpected body {other:?}"),
        }
    }
    assert_eq!(
        received2, events,
        "subscriber 2 received all events in order"
    );

    // Negative control: a reordered list of events must NOT equal the original events
    let mut reordered = events.clone();
    reordered.swap(0, 1);
    assert_ne!(
        received1, reordered,
        "negative control: reordered events must not match received stream"
    );

    // Kill child
    let _ = child.kill();
    let _ = child.wait();
}
