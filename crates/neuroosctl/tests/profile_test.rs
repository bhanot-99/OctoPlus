//! `neuroosctl profile` against mock C3 and C6 servers (P5-S07, FR-PRO-04).

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::server::{Server, ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    DispatchResult, DispatchStatus, ErrorCode, ProfileFact, ProfileFacts, Taint, Tier,
};
use neuroosctl::ProfileCommands;
use neuroosctl::profile::run_profile;
use neuroosctl::style::{ColorChoice, Styler};
use tempfile::tempdir;

#[derive(Debug, Default)]
struct MockKernel {
    seen: Mutex<Vec<(String, String, u32)>>,
    status: Mutex<i32>,
}

impl Handler for MockKernel {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::DispatchRequest(d) => {
                self.seen.lock().unwrap().push((
                    d.skill,
                    d.args_json,
                    d.context_taint.map_or(u32::MAX, |t| t.flags),
                ));
                Ok(Body::DispatchResult(DispatchResult {
                    status: *self.status.lock().unwrap(),
                    result_json: "{}".into(),
                    tier: Tier::Safe as i32,
                    result_taint: Some(Taint { flags: 0 }),
                }))
            }
            _ => Err(HandlerError::new(ErrorCode::InvalidRequest, "no", false)),
        }
    }
}

#[derive(Debug)]
struct MockStorage;

impl Handler for MockStorage {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::ProfileListRequest(_) => Ok(Body::ProfileFacts(ProfileFacts {
                facts: vec![ProfileFact {
                    id: 3,
                    text: "my dog is Rex".into(),
                    saved_at_ns: 1,
                    taint: Some(Taint { flags: 2 }),
                }],
            })),
            _ => Err(HandlerError::new(ErrorCode::InvalidRequest, "no", false)),
        }
    }
}

async fn serve<H: Handler>(
    root: &std::path::Path,
    part: Part,
    name: &str,
    h: Arc<H>,
) -> tokio::sync::watch::Sender<bool> {
    let sock = paths::socket_path(root, part, name);
    ensure_private_dir(sock.parent().unwrap()).unwrap();
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(1)))
        .await
        .unwrap();
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    tokio::spawn(server.serve(h, async move {
        let _ = rx.wait_for(|s| *s).await;
    }));
    tokio::time::sleep(Duration::from_millis(50)).await;
    tx
}

#[tokio::test]
async fn add_and_remove_go_through_the_kernel_and_list_reads_c3() {
    let dir = tempdir().unwrap();
    let root: PathBuf = dir.path().to_path_buf();
    let kernel = Arc::new(MockKernel::default());
    *kernel.status.lock().unwrap() = DispatchStatus::Executed as i32;
    let _k = serve(&root, Part::Kernel, "kernel", Arc::clone(&kernel)).await;
    let _s = serve(&root, Part::Storage, "storage", Arc::new(MockStorage)).await;
    let styler = Styler::new(ColorChoice::Never);

    assert_eq!(
        run_profile(
            &root,
            ProfileCommands::Add {
                text: "  I like tea ".into()
            },
            styler
        )
        .await,
        0
    );
    assert_eq!(
        run_profile(&root, ProfileCommands::Remove { id: 3 }, styler).await,
        0
    );
    assert_eq!(
        run_profile(
            &root,
            ProfileCommands::List {
                relevant_to: None,
                json: false
            },
            styler
        )
        .await,
        0
    );
    assert_eq!(
        run_profile(
            &root,
            ProfileCommands::List {
                relevant_to: Some("dog".into()),
                json: true
            },
            styler
        )
        .await,
        0
    );
    // Typed text is the user's own word: USER_PROVIDED (bit 1) only, never clean and never external.
    assert_eq!(
        *kernel.seen.lock().unwrap(),
        vec![
            (
                "profile.remember".to_string(),
                r#"{"fact":"I like tea"}"#.to_string(),
                2
            ),
            (
                "profile.forget".to_string(),
                r#"{"fact_id":3}"#.to_string(),
                2
            ),
        ]
    );

    // Bad input is refused before anything is sent.
    assert_eq!(
        run_profile(&root, ProfileCommands::Add { text: "   ".into() }, styler).await,
        2
    );
    assert_eq!(
        run_profile(&root, ProfileCommands::Remove { id: 0 }, styler).await,
        2
    );
    assert_eq!(kernel.seen.lock().unwrap().len(), 2);

    // A denied dialog is exit 4; an absent kernel or storage is exit 1.
    *kernel.status.lock().unwrap() = DispatchStatus::Rejected as i32;
    assert_eq!(
        run_profile(&root, ProfileCommands::Remove { id: 3 }, styler).await,
        4
    );
    let empty = tempdir().unwrap();
    assert_eq!(
        run_profile(
            empty.path(),
            ProfileCommands::Add { text: "x".into() },
            styler
        )
        .await,
        1
    );
    assert_eq!(
        run_profile(
            empty.path(),
            ProfileCommands::List {
                relevant_to: None,
                json: false
            },
            styler
        )
        .await,
        1
    );
}
