//! `neuroosctl voice status` and `voice watch`: what the voice service is doing (DP-7).
//!
//! They ask C2's control socket (`voice/control`) the same question C5 asks before it speaks a
//! nudge, and print the answer; `watch` asks again and again and prints each change as a
//! clear signal. They change nothing.

use std::path::Path;
use std::time::{Duration, Instant};

use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{TurnPhase, VoiceStatus, VoiceStatusRequest};
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
    match command {
        VoiceCommands::Status { json } => run_status(runtime_root, json, styler).await,
        VoiceCommands::Watch {
            notify,
            interval_ms,
        } => run_watch(runtime_root, notify, interval_ms, styler).await,
    }
}

async fn run_status(runtime_root: &Path, json: bool, styler: Styler) -> i32 {
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
                        "phase": stage(&s).word(),
                        "wake_words": s.wake_words,
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

/// What the person should see, from the exact stage of the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// No microphone is being captured.
    Off,
    /// Another application has the microphone.
    MicBusy,
    /// Waiting for the wake phrase.
    Listening,
    /// The wake phrase was heard; the question is being recorded.
    Recording,
    /// Speech is being turned into text.
    Understanding,
    /// The answer is being worked out.
    Thinking,
    /// The answer is being spoken.
    Speaking,
    /// The follow-up window is open: no wake phrase needed.
    FollowUp,
}

impl Stage {
    /// One word, for scripts.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::MicBusy => "mic_busy",
            Self::Listening => "listening",
            Self::Recording => "recording",
            Self::Understanding => "understanding",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
            Self::FollowUp => "follow_up",
        }
    }

    /// The signal line shown to the person.
    #[must_use]
    pub fn line(self) -> &'static str {
        match self {
            Self::Off => "OFF        the microphone is not being captured",
            Self::MicBusy => "PAUSED     another application is using the microphone",
            Self::Listening => "LISTENING  say \"Hey Jarvis\"",
            Self::Recording => "HEARD YOU  speak now, I am recording",
            Self::Understanding => "UNDERSTANDING  turning your speech into text",
            Self::Thinking => "THINKING   working on the answer",
            Self::Speaking => "SPEAKING   here is the answer",
            Self::FollowUp => "FOLLOW-UP  ask again, no wake phrase needed",
        }
    }
}

/// The stage a status means. A service older than the `phase` field only sends the yes/no
/// flags, so those still give a sensible answer.
#[must_use]
pub fn stage(s: &VoiceStatus) -> Stage {
    if !s.capture_running {
        return Stage::Off;
    }
    match TurnPhase::try_from(s.phase).unwrap_or(TurnPhase::Idle) {
        TurnPhase::Capturing => Stage::Recording,
        TurnPhase::Transcribing => Stage::Understanding,
        TurnPhase::AwaitingAnswer => Stage::Thinking,
        TurnPhase::Speaking => Stage::Speaking,
        TurnPhase::FollowUp => Stage::FollowUp,
        TurnPhase::Idle => {
            if s.speaking {
                Stage::Speaking
            } else if s.turn_in_progress {
                Stage::Thinking
            } else if s.follow_up_open {
                Stage::FollowUp
            } else if s.mic_in_use_by_other {
                Stage::MicBusy
            } else {
                Stage::Listening
            }
        }
    }
}

fn colored(styler: Styler, stage: Stage) -> String {
    let line = stage.line();
    match stage {
        Stage::Listening | Stage::FollowUp => styler.ok(line),
        Stage::Off | Stage::MicBusy => styler.down(line),
        Stage::Recording | Stage::Understanding | Stage::Thinking => styler.degraded(line),
        Stage::Speaking => styler.bold(line),
    }
}

/// A desktop notification for a stage; fixed words only, nothing from the conversation.
fn notify(stage: Stage) {
    if matches!(stage, Stage::Listening | Stage::Off) {
        return;
    }
    let child = std::process::Command::new("notify-send")
        .args(["-a", "Semantic NeuroOS", "-t", "2000", "-u", "low", "--"])
        .arg(stage.line())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut c) = child {
        // Reaped on its own thread so no zombie is left behind.
        std::thread::spawn(move || {
            let _ = c.wait();
        });
    }
}

/// Shows every change of stage as it happens, until Ctrl-C.
async fn run_watch(runtime_root: &Path, notify_on: bool, interval_ms: u64, styler: Styler) -> i32 {
    let sock = paths::socket_path(runtime_root, Part::Voice, "control");
    let started = Instant::now();
    let stamp = || format!("{:7.1}s", started.elapsed().as_secs_f64());
    println!(
        "{}",
        styler.dim("Watching the voice service. Press Ctrl-C to stop.")
    );
    let mut last: Option<Stage> = None;
    let mut last_wakes: Option<u64> = None;
    let mut unreachable = false;
    loop {
        let reply = match Client::connect(&sock, 1024 * 1024).await {
            Ok(c) => {
                c.call(Body::VoiceStatusRequest(VoiceStatusRequest {}), DEADLINE)
                    .await
            }
            Err(e) => Err(e),
        };
        match reply {
            Ok(Body::VoiceStatus(s)) => {
                unreachable = false;
                let now = stage(&s);
                if last_wakes.is_some_and(|w| s.wake_words > w) && last == Some(now) {
                    // A wake phrase that changed nothing we could see in between two polls.
                    println!(
                        "{}  {}",
                        styler.dim(&stamp()),
                        styler.degraded("wake phrase heard")
                    );
                }
                last_wakes = Some(s.wake_words);
                if last != Some(now) {
                    println!("{}  {}", styler.dim(&stamp()), colored(styler, now));
                    if notify_on {
                        notify(now);
                    }
                    last = Some(now);
                }
            }
            _ => {
                if !unreachable {
                    println!(
                        "{}  {}",
                        styler.dim(&stamp()),
                        styler.down("the voice service is not reachable (waiting for it)")
                    );
                    unreachable = true;
                    last = None;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(interval_ms)).await;
    }
}
