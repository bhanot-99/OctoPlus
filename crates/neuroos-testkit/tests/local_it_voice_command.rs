//! A spoken command sent straight to the installed C5 (the IT of Phase 7 "real C5/C4 streaming"):
//! the stream starts with a preamble request, carries the answer as speakable sentences and
//! ends with the closing frame. No microphone is involved: this is the half of the voice path
//! after C2 has turned speech into text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::VoiceCommand;
use neuroos_proto::v1::envelope::Body;

fn runtime_root() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")))
        .join("neuroos")
}

#[tokio::test]
async fn local_a_voice_command_gets_a_preamble_then_speakable_sentences() {
    let sock = paths::socket_path(&runtime_root(), Part::Knowledge, "knowledge");
    let Ok(client) = Client::connect(&sock, 1 << 20).await else {
        eprintln!("skipping: the stack is not running");
        return;
    };
    let t0 = Instant::now();
    let mut stream = client
        .stream(
            Body::VoiceCommand(VoiceCommand {
                text: std::env::var("VOICE_TEST_TEXT")
                    .unwrap_or_else(|_| "What is two plus two?".into()),
                // As C2 sends it: the start of the speech, a few seconds ago, wall clock.
                t_speech_start_ns: neuroos_common::time::now_ns().unwrap() - 3_000_000_000,
                t_speech_end_ns: neuroos_common::time::now_ns().unwrap(),
                ..VoiceCommand::default()
            }),
            Duration::from_secs(60),
        )
        .await
        .unwrap();
    let mut kinds = Vec::new();
    let mut spoken = String::new();
    let mut first_chunk = None;
    while let Some(item) = stream.next().await {
        match item.unwrap() {
            Body::PreambleRequest(_) => kinds.push("preamble"),
            Body::SpeakChunk(c) => {
                first_chunk.get_or_insert_with(|| t0.elapsed());
                spoken.push_str(&c.text);
                spoken.push(' ');
                kinds.push("speak");
            }
            Body::AnswerDone(_) => {
                kinds.push("done");
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    println!("frames: {kinds:?}; first sentence after {first_chunk:?}; said: {spoken}");
    assert_eq!(kinds.first(), Some(&"preamble"), "{kinds:?}");
    assert!(
        kinds.contains(&"speak"),
        "no spoken text came back: {kinds:?}"
    );
    assert!(
        spoken.to_lowercase().contains("four"),
        "the answer was not spoken: {spoken}"
    );
    assert_eq!(kinds.last(), Some(&"done"));
}
