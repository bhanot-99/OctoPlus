#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Skill-child locks on a real process (security.md §8 "Skill sandbox").
//! One test per binary: the limits and the filter stay on this process, so no
//! other test may share it (nextest runs each test in its own process anyway).

use std::io::ErrorKind;

use neuroos_sandbox::child::{ChildLimits, SyscallProfile, apply_limits, apply_seccomp};

#[test]
fn seccomp_and_limits_block_escape_paths() {
    // Before the filter: a socket works, so the probes below are meaningful (negative control).
    assert!(std::net::UdpSocket::bind("127.0.0.1:0").is_ok());

    apply_limits(&ChildLimits {
        address_space_bytes: 1 << 30,
        cpu_seconds: 5,
        open_files: 32,
        file_size_bytes: 1 << 20,
    })
    .unwrap();
    apply_seccomp(SyscallProfile::ReadOnly).unwrap();

    // Network: socket() is denied.
    let e = std::net::UdpSocket::bind("127.0.0.1:0").unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied, "{e}");
    let e = std::os::unix::net::UnixStream::connect("/run/user/0/bus").unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied, "{e}");
    // Programs: exec/fork are denied.
    assert!(std::process::Command::new("/bin/true").status().is_err());
    // Mutation is not in the read-only profile.
    let e = std::fs::remove_file("/nonexistent-neuroos-probe").unwrap_err();
    assert_eq!(e.kind(), ErrorKind::PermissionDenied, "{e}");
    // Reading still works.
    assert!(
        std::fs::read_to_string("/proc/thread-self/status")
            .unwrap()
            .contains("Seccomp:\t2")
    );
}
