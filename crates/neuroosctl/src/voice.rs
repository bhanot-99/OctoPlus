//! `neuroosctl voice status`: the text equivalent of what the voice service is doing (DP-7).
//!
//! It asks C2's control socket (`voice/control`) the same question C5 asks before it speaks a
//! nudge, and prints the answer. It changes nothing.

use std::path::Path;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{VoiceStatus, VoiceStatusRequest};
use serde_json::json;

use crate::VoiceCommands;
use crate::style::Styler;

const DEADLINE: Duration = Duration::from_secs(3);

/// The status as plain sentences, one per line.
#[must_use]
pub fn format_status(s: &VoiceStatus) -> String {
    let mic = if s.capture_running {
        "listening: the microphone is being captured"
    } else {
        "not listening: no microphone is being captured"
    };
    let turn = if s.speaking {
        "speaking now"
    } else if s.turn_in_progress {
        "in a turn"
    } else if s.follow_up_open {
        "waiting for a follow-up (no wake word needed)"
    } else {
        "idle (say the wake word to start)"
    };
    let other = if s.mic_in_use_by_other {
        "another application is using the microphone"
    } else {
        "no other application is using the microphone"
    };
    format!("{mic}\n{turn}\n{other}")
}

/// Runs `neuroosctl voice …` and returns the exit code.
pub async fn run_voice(runtime_root: &Path, command: VoiceCommands, styler: Styler) -> i32 {
    let VoiceCommands::Status { json } = command;
    let sock = paths::socket_path(runtime_root, Part::Voice, "control");
    let Ok(client) = Client::connect(&sock, 1024 * 1024).await else {
        eprintln!("{}", styler.error("could not connect to neuroos-voice"));
        return 1;
    };
    match client
        .call(Body::VoiceStatusRequest(VoiceStatusRequest {}), DEADLINE)
        .await
    {
        Ok(Body::VoiceStatus(s)) => {
            if json {
                println!(
                    "{}",
                    json!({
                        "capture_running": s.capture_running,
                        "turn_in_progress": s.turn_in_progress,
                        "speaking": s.speaking,
                        "follow_up_open": s.follow_up_open,
                        "mic_in_use_by_other": s.mic_in_use_by_other,
                    })
                );
            } else {
                println!("{}", format_status(&s));
            }
            if s.capture_running { 0 } else { 3 }
        }
        Ok(_) => {
            eprintln!("{}", styler.error("unexpected reply from neuroos-voice"));
            1
        }
        Err(e) => {
            eprintln!("{}", styler.error(&e.to_wire().message));
            1
        }
    }
}
