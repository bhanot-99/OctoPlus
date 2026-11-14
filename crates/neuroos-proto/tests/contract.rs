//! Contract test (P0-S02, phases.md §3.3 CT): every v1 message round-trips
//! C++ → Rust → C++ → Rust byte-identically.
//!
//! The C++ tool fills every field of every Envelope body variant (and every
//! nested oneof choice) from the descriptors, so a new message is covered
//! automatically. Needs the C++ debug build (`just build`); a missing tool is a
//! failure, not a skip.

// Test helpers outside #[test] functions: panicking on a broken fixture is the point.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use neuroos_proto::Message;
use neuroos_proto::v1::Envelope;

fn tool() -> PathBuf {
    if let Some(p) = std::env::var_os("NEUROOS_CONTRACT_TOOL") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../cpp/build/debug/tools/neuroos_contract_tool")
}

fn frames(mut bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        assert!(bytes.len() >= 4, "truncated length prefix");
        let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert!(bytes.len() >= 4 + n, "truncated frame");
        out.push(bytes[4..4 + n].to_vec());
        bytes = &bytes[4 + n..];
    }
    out
}

fn framed(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in payloads {
        out.extend_from_slice(&u32::try_from(p.len()).unwrap().to_le_bytes());
        out.extend_from_slice(p);
    }
    out
}

fn run_tool(mode: &str, input: &[u8]) -> (Vec<u8>, String) {
    let path = tool();
    assert!(
        path.exists(),
        "contract tool not found at {} - run `just build` first",
        path.display()
    );
    let mut child = Command::new(&path)
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "tool {mode} failed: {stderr}");
    (out.stdout, stderr)
}

/// Field numbers of `Envelope.body`, read from the proto source.
fn body_field_numbers() -> Vec<u32> {
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto/neuroos/v1/envelope.proto"),
    )
    .unwrap();
    let start = src.find("oneof body").unwrap();
    let body = &src[start..];
    let end = body.find("\n  }").unwrap();
    body[..end]
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with("//") || !l.ends_with(';') {
                return None;
            }
            l.split('=')
                .nth(1)?
                .trim()
                .trim_end_matches(';')
                .parse()
                .ok()
        })
        .collect()
}

fn body_tag(env: &Envelope) -> u32 {
    // Encoding just the body and reading its first key gives the field number
    // without a hand-written list that could fall out of date.
    let body_only = Envelope {
        body: env.body.clone(),
        ..Envelope::default()
    };
    let bytes = body_only.encode_to_vec();
    let mut key: u64 = 0;
    for (i, b) in bytes.iter().enumerate() {
        key |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            break;
        }
    }
    u32::try_from(key >> 3).unwrap()
}

#[test]
fn every_message_round_trips_byte_identically() {
    let (generated, stderr) = run_tool("generate", &[]);
    let cpp = frames(&generated);
    assert!(!cpp.is_empty());
    assert!(stderr.contains("body_variants="), "{stderr}");

    // C++ -> Rust: decode, re-encode, identical bytes (nothing dropped or reordered).
    let mut decoded = Vec::new();
    let mut rust_bytes = Vec::new();
    for (i, bytes) in cpp.iter().enumerate() {
        let env = Envelope::decode(bytes.as_slice()).unwrap();
        assert!(env.body.is_some(), "frame {i}: body missing in Rust");
        let again = env.encode_to_vec();
        assert_eq!(
            &again, bytes,
            "frame {i}: Rust re-encoding differs from C++"
        );
        rust_bytes.push(again);
        decoded.push(env);
    }

    // Rust -> C++ -> Rust: C++ parses, drops unknown fields, re-serialises.
    let (echoed, _) = run_tool("echo", &framed(&rust_bytes));
    let back = frames(&echoed);
    assert_eq!(back.len(), rust_bytes.len());
    for (i, (b, r)) in back.iter().zip(&rust_bytes).enumerate() {
        assert_eq!(b, r, "frame {i}: C++ echo differs from Rust bytes");
        assert_eq!(Envelope::decode(b.as_slice()).unwrap(), decoded[i]);
    }

    // Every body variant in the proto was exercised.
    let mut want = body_field_numbers();
    want.sort_unstable();
    let mut got: Vec<u32> = decoded.iter().map(body_tag).collect();
    got.sort_unstable();
    got.dedup();
    assert_eq!(got, want, "body variants covered vs declared");
    assert!(
        stderr.contains(&format!("body_variants={}", want.len())),
        "{stderr}"
    );
}

#[test]
fn echo_rejects_garbage() {
    let path = tool();
    let mut child = Command::new(&path)
        .arg("echo")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Length 3, then bytes that are not a valid Envelope (field 1 with a bad wire type).
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&[3, 0, 0, 0, 0x0f, 0xff, 0xff])
        .unwrap();
    assert!(!child.wait().unwrap().success());
}
