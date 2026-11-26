#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Live check against the installed voice service (not part of the gate: `#[ignore]`).
//! `cargo test -p octoplus-knowledge --test live_companion_line -- --ignored --nocapture`
//! Proves the installed voice service refuses a companion line from anyone but C5.

use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::Client;
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{SpeakCompanion, VoiceStatusRequest};

async fn status(c: &Client) -> octoplus_proto::v1::VoiceStatus {
    match c
        .call(
            Body::VoiceStatusRequest(VoiceStatusRequest {}),
            Duration::from_secs(2),
        )
        .await
    {
        Ok(Body::VoiceStatus(s)) => s,
        other => panic!("voice status: {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs the installed stack (voice service running)"]
async fn only_c5_may_make_the_voice_part_speak_a_companion_line() {
    let root = paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref()).unwrap();
    let voice = Client::connect(&paths::socket_path(&root, Part::Voice, "voice"), 1 << 20)
        .await
        .unwrap();
    let r = voice
        .call(
            Body::SpeakCompanion(SpeakCompanion {
                text: "This line must never be spoken.".into(),
                listen_seconds: 10,
            }),
            Duration::from_secs(5),
        )
        .await;
    println!("speak_companion from a stranger: {r:?}");
    assert!(
        matches!(
            r,
            Err(octoplus_ipc::IpcError::Remote {
                code: octoplus_proto::v1::ErrorCode::PermissionDenied,
                ..
            })
        ),
        "{r:?}"
    );
    // And nothing happened: no session, nothing spoken.
    let control = Client::connect(&paths::socket_path(&root, Part::Voice, "control"), 1 << 20)
        .await
        .unwrap();
    let s = status(&control).await;
    assert!(!s.session_active && !s.speaking);
}
