//! Landlock self-restriction, proven in a child process (NFR-SEC-04, P0-S10 groundwork).
//!
//! Restriction is irreversible, so each case re-runs this test binary with
//! `NEUROOS_SANDBOX_CASE` set; the child restricts itself, tries what it should
//! and should not be able to do, and prints one line per check.
//!
//! The TCP checks connect to 127.0.0.1 only: nothing can leave the machine even
//! if the sandbox failed (rules.md §8.4).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use std::io::ErrorKind;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;

use neuroos_sandbox::{Policy, SandboxError, covered, restrict};

const CASE: &str = "NEUROOS_SANDBOX_CASE";

fn check(name: &str, ok: bool) {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn denied<T>(r: std::io::Result<T>) -> bool {
    matches!(r, Err(e) if e.kind() == ErrorKind::PermissionDenied)
}

fn run_child_case(case: &str) {
    let ro = PathBuf::from(std::env::var("RO").unwrap());
    let rw = PathBuf::from(std::env::var("RW").unwrap());
    match case {
        "scopes" => {
            // The abstract socket is reachable before the sandbox, so a later
            // failure means the scope rule, not a missing socket.
            let name = std::env::var("ABSTRACT").unwrap();
            let addr = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
            check(
                "abstract-connect-before",
                UnixStream::connect_addr(&addr).is_ok(),
            );
            let report = restrict(&Policy {
                read_only: vec![ro],
                read_write: vec![rw],
            })
            .unwrap();
            check("report-scoped", report.scoped);
            check(
                "abstract-connect-denied",
                denied(UnixStream::connect_addr(&addr)),
            );
        }
        "rules" => {
            let report = restrict(&Policy {
                read_only: vec![ro.clone()],
                read_write: vec![rw.clone()],
            })
            .unwrap();
            check("report-full", report.fs_and_tcp && report.no_new_privs);
            check("read-ro", std::fs::read(ro.join("data")).is_ok());
            check(
                "write-ro-denied",
                denied(std::fs::write(ro.join("new"), b"x")),
            );
            check("write-rw", std::fs::write(rw.join("new"), b"x").is_ok());
            check(
                "read-outside-denied",
                denied(std::fs::read("/etc/hostname")),
            );
            check("list-home-denied", denied(std::fs::read_dir("/home")));
            check(
                "tcp-connect-denied",
                denied(std::net::TcpStream::connect("127.0.0.1:9")),
            );
            check(
                "tcp-bind-denied",
                denied(std::net::TcpListener::bind("127.0.0.1:0")),
            );
            // Filesystem Unix sockets in our own folder still work (AB-6).
            let sock = rw.join("s.sock");
            let l = UnixListener::bind(&sock);
            check("unix-bind", l.is_ok());
            check("unix-connect", UnixStream::connect(&sock).is_ok());
        }
        "missing-path" => {
            let r = restrict(&Policy {
                read_only: vec![ro.join("does-not-exist")],
                read_write: vec![],
            });
            check(
                "missing-path-error",
                matches!(r, Err(SandboxError::Path(_))),
            );
            // Nothing was restricted.
            check("still-unrestricted", std::fs::read("/etc/hostname").is_ok());
        }
        "negative-control" => {
            let outside = PathBuf::from(std::env::var("OUTSIDE").unwrap());
            check("outside-read-before", std::fs::read(&outside).is_ok());
            let report = restrict(&Policy {
                read_only: vec![ro],
                read_write: vec![rw],
            })
            .unwrap();
            check("report-full", report.fs_and_tcp);
            check("outside-read-denied-after", denied(std::fs::read(&outside)));
        }
        other => panic!("unknown case {other}"),
    }
}

fn run_case(case: &str, test_name: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("ro");
    let rw = dir.path().join("rw");
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::create_dir_all(&rw).unwrap();
    std::fs::write(ro.join("data"), b"hello").unwrap();
    let name = format!("neuroos-sandbox-test-{}-{}", std::process::id(), test_name);
    let abs = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
    let _listener = UnixListener::bind_addr(&abs).unwrap();

    let out = Command::new(std::env::current_exe().unwrap())
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CASE, case)
        .env("RO", &ro)
        .env("RW", &rw)
        .env("ABSTRACT", &name)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "child failed: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn assert_all_pass(stdout: &str, expected: &[&str]) {
    for e in expected {
        assert!(
            stdout.contains(&format!("PASS {e}")),
            "{e} missing:\n{stdout}"
        );
    }
    assert!(!stdout.contains("FAIL "), "{stdout}");
}

#[test]
fn rules_are_enforced() {
    if let Ok(case) = std::env::var(CASE) {
        return run_child_case(&case);
    }
    let out = run_case("rules", "rules_are_enforced");
    assert_all_pass(
        &out,
        &[
            "report-full",
            "read-ro",
            "write-ro-denied",
            "write-rw",
            "read-outside-denied",
            "list-home-denied",
            "tcp-connect-denied",
            "tcp-bind-denied",
            "unix-bind",
            "unix-connect",
        ],
    );
}

/// Landlock ABI v6 scoping (abstract sockets, signals). Hosted runners have an
/// older kernel, so this runs on the reference machine only (PRD §4 requires v4).
#[test]
fn local_scopes_block_abstract_sockets() {
    if let Ok(case) = std::env::var(CASE) {
        return run_child_case(&case);
    }
    let out = run_case("scopes", "local_scopes_block_abstract_sockets");
    assert_all_pass(
        &out,
        &[
            "abstract-connect-before",
            "report-scoped",
            "abstract-connect-denied",
        ],
    );
}

#[test]
fn missing_path_fails_closed_before_restricting() {
    if let Ok(case) = std::env::var(CASE) {
        return run_child_case(&case);
    }
    let out = run_case(
        "missing-path",
        "missing_path_fails_closed_before_restricting",
    );
    assert_all_pass(&out, &["missing-path-error", "still-unrestricted"]);
}

#[test]
fn negative_control_proves_landlock_is_load_bearing() {
    if let Ok(case) = std::env::var(CASE) {
        return run_child_case(&case);
    }
    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("ro");
    let rw = dir.path().join("rw");
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::create_dir_all(&rw).unwrap();

    let outside_dir = tempfile::tempdir().unwrap();
    let outside_file = outside_dir.path().join("unrestricted.txt");
    std::fs::write(&outside_file, b"readable before restrict").unwrap();

    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "negative_control_proves_landlock_is_load_bearing",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE, "negative-control")
        .env("RO", &ro)
        .env("RW", &rw)
        .env("OUTSIDE", &outside_file)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "child failed: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_all_pass(
        &stdout,
        &[
            "outside-read-before",
            "report-full",
            "outside-read-denied-after",
        ],
    );
}

#[test]
fn coverage_helper() {
    let roots = vec![PathBuf::from("/a/b")];
    assert!(covered(&PathBuf::from("/a/b/c"), &roots));
    assert!(!covered(&PathBuf::from("/a/bc"), &roots));
}
