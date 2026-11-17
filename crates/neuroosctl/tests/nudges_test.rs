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
                rule: "disk_critical".into()
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
