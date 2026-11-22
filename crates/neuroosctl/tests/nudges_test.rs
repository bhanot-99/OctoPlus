//! `neuroosctl nudges` against a mock C5 (P6-S05).

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::server::{Server, ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, ErrorCode, NudgeList, NudgeRuleState};
use neuroosctl::NudgesCommands;
use neuroosctl::nudges::run_nudges;
use neuroosctl::style::{ColorChoice, Styler};

#[derive(Debug, Default)]
struct MockKnowledge {
    seen: Mutex<Vec<String>>,
}

impl Handler for MockKnowledge {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::NudgeListRequest(_) => Ok(Body::NudgeList(NudgeList {
                rules: vec![NudgeRuleState {
                    rule_id: "disk_nearly_full".into(),
                    enabled: true,
                    snoozed_until_ns: 0,
                    last_fired_ns: 0,
                }],
            })),
            Body::NudgeSetEnabled(s) if s.rule_id == "nope" => Err(HandlerError::new(
                ErrorCode::NotFound,
                "no such nudge rule",
                false,
            )),
            Body::NudgeSetEnabled(s) => {
                self.seen
                    .lock()
                    .unwrap()
                    .push(format!("{}:{}", s.rule_id, s.enabled));
                Ok(Body::Ack(Ack {}))
            }
            Body::NudgeTest(t) => {
                self.seen
                    .lock()
                    .unwrap()
                    .push(format!("test:{}", t.rule_id));
                Ok(Body::Ack(Ack {}))
            }
            _ => Err(HandlerError::unsupported()),
        }
    }
}

#[tokio::test]
async fn list_enable_disable_test_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let sock = paths::socket_path(&root, Part::Knowledge, "knowledge");
    ensure_private_dir(sock.parent().unwrap()).unwrap();
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(1)))
        .await
        .unwrap();
    let mock = Arc::new(MockKnowledge::default());
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    tokio::spawn(server.serve(Arc::clone(&mock), async move {
        let _ = rx.wait_for(|s| *s).await;
    }));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let styler = Styler::new(ColorChoice::Never);

    assert_eq!(
        run_nudges(&root, NudgesCommands::List { json: false }, styler).await,
        0
    );
    assert_eq!(
        run_nudges(&root, NudgesCommands::List { json: true }, styler).await,
        0
    );
    assert_eq!(
        run_nudges(
            &root,
            NudgesCommands::Disable {
                rule: "running_hot".into()
            },
            styler
        )
        .await,
        0
    );
    assert_eq!(
        run_nudges(
            &root,
            NudgesCommands::Enable {
                rule: "running_hot".into()
            },
            styler
        )
        .await,
        0
    );
    assert_eq!(
        run_nudges(
            &root,
            NudgesCommands::Test {
                rule: "disk_critical".into(),
                voice: false
            },
            styler
        )
        .await,
        0
    );
    assert_eq!(
        *mock.seen.lock().unwrap(),
        vec![
            "running_hot:false",
            "running_hot:true",
            "test:disk_critical"
        ]
    );
    // An unknown rule is a usage error (2), an absent C5 is a failure (1).
    assert_eq!(
        run_nudges(
            &root,
            NudgesCommands::Disable {
                rule: "nope".into()
            },
            styler
        )
        .await,
        2
    );
    let _ = tx.send(true);
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        run_nudges(empty.path(), NudgesCommands::List { json: false }, styler).await,
        1
    );
}

#[test]
fn the_voice_status_reads_as_plain_sentences() {
    use neuroos_proto::v1::VoiceStatus;
    use neuroosctl::voice::format_status;
    let idle = format_status(&VoiceStatus {
        capture_running: true,
        ..VoiceStatus::default()
    });
    assert!(idle.contains("listening"));
    assert!(idle.contains("idle (say the wake word"));
    assert!(idle.contains("no other application"));
    let busy = format_status(&VoiceStatus {
        capture_running: false,
        speaking: true,
        mic_in_use_by_other: true,
        ..VoiceStatus::default()
    });
    assert!(busy.contains("not listening"));
    assert!(busy.contains("speaking now"));
    assert!(busy.contains("another application is using"));
    let follow = format_status(&VoiceStatus {
        capture_running: true,
        follow_up_open: true,
        ..VoiceStatus::default()
    });
    assert!(follow.contains("follow-up"));
}

/// `neuroosctl voice watch` shows what the person needs to know at each moment.
#[test]
fn the_voice_signal_names_each_stage_of_a_turn() {
    use neuroos_proto::v1::{TurnPhase, VoiceStatus};
    use neuroosctl::voice::{Stage, stage};
    let on = |phase: TurnPhase| VoiceStatus {
        capture_running: true,
        phase: phase as i32,
        ..VoiceStatus::default()
    };
    assert_eq!(stage(&on(TurnPhase::Idle)), Stage::Listening);
    assert_eq!(stage(&on(TurnPhase::Capturing)), Stage::Recording);
    assert_eq!(stage(&on(TurnPhase::Transcribing)), Stage::Understanding);
    assert_eq!(stage(&on(TurnPhase::AwaitingAnswer)), Stage::Thinking);
    assert_eq!(stage(&on(TurnPhase::Speaking)), Stage::Speaking);
    assert_eq!(stage(&on(TurnPhase::FollowUp)), Stage::FollowUp);
    // No microphone at all is "off", whatever else is set.
    let off = VoiceStatus {
        capture_running: false,
        phase: TurnPhase::Speaking as i32,
        ..VoiceStatus::default()
    };
    assert_eq!(stage(&off), Stage::Off);
    // Another application on the microphone, while idle.
    let busy = VoiceStatus {
        capture_running: true,
        mic_in_use_by_other: true,
        ..VoiceStatus::default()
    };
    assert_eq!(stage(&busy), Stage::MicBusy);
    // A service that predates the phase field sends only the yes/no flags.
    let old = |f: fn(&mut VoiceStatus)| {
        let mut s = VoiceStatus {
            capture_running: true,
            ..VoiceStatus::default()
        };
        f(&mut s);
        stage(&s)
    };
    assert_eq!(old(|s| s.turn_in_progress = true), Stage::Thinking);
    assert_eq!(old(|s| s.speaking = true), Stage::Speaking);
    assert_eq!(old(|s| s.follow_up_open = true), Stage::FollowUp);
    // An unknown number from a newer service is treated as idle, not a crash.
    let unknown = VoiceStatus {
        capture_running: true,
        phase: 99,
        ..VoiceStatus::default()
    };
    assert_eq!(stage(&unknown), Stage::Listening);
}
