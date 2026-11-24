//! Test-only escape probe for the skill sandbox (security.md §8 "Skill sandbox":
//! each skill tries to read `hmac.key`, write outside its allowlist and open a
//! socket; all must fail). It locks itself down with the **same**
//! [`octoplus_skill::lock_down`] the real child uses, then tries each escape and
//! prints what happened. `--control` skips the lock-down so the test can prove
//! every probe would succeed without it (rules.md §8.9 negative control).
//! Never installed (not in `scripts/install-dev.sh`).

use std::io::Read;
use std::process::ExitCode;

use octoplus_skill::{Invocation, lock_down};
use serde_json::{Value, json};

fn read_ok(p: &str) -> bool {
    std::fs::read(p).is_ok()
}

fn write_ok(p: &str) -> bool {
    std::fs::write(p, b"probe").is_ok()
}

#[allow(clippy::print_stdout, clippy::print_stderr)] // test probe, output read by the test
fn main() -> ExitCode {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let control = args.first().is_some_and(|a| a == "--control");
    if control {
        args.remove(0);
    }
    let Ok(inv) = Invocation::parse(args) else {
        eprintln!("probe: bad invocation");
        return ExitCode::from(2);
    };
    if !control && lock_down(&inv).is_err() {
        eprintln!("probe: lock-down failed");
        return ExitCode::from(3);
    }
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::from(2);
    }
    let Ok(plan) = serde_json::from_str::<Value>(&buf) else {
        return ExitCode::from(2);
    };
    let list = |k: &str| -> Vec<String> {
        plan[k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let reads: Vec<Value> = list("read")
        .iter()
        .map(|p| json!({"path": p, "ok": read_ok(p)}))
        .collect();
    let writes: Vec<Value> = list("write")
        .iter()
        .map(|p| json!({"path": p, "ok": write_ok(p)}))
        .collect();
    let unix = plan["unix_socket"]
        .as_str()
        .map(|p| std::os::unix::net::UnixStream::connect(p).is_ok());
    let tcp = std::net::UdpSocket::bind("127.0.0.1:0").is_ok();
    let exec = std::process::Command::new("/bin/true").status().is_ok();
    println!(
        "{}",
        json!({"read": reads, "write": writes, "unix_connect": unix, "socket": tcp, "exec": exec})
    );
    ExitCode::SUCCESS
}
