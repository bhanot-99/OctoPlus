//! Implementation of `neuroosctl ask` (FR-CLI-01, FR-CLI-02, design.md §7).

use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::{Client, IpcError};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{AskRequest, ErrorCode, Origin};
use serde::Serialize;

use crate::style::Styler;

#[derive(Serialize)]
#[serde(tag = "type")]
enum JsonAskEvent<'a> {
    #[serde(rename = "chunk")]
    Chunk { text: &'a str },
    #[serde(rename = "done")]
    Done { route: i32, used_web: bool },
}

/// Runs the `ask` command, streaming response chunks to stdout.
pub async fn run_ask(runtime_root: &Path, question: String, json: bool, styler: Styler) -> i32 {
    let cancel = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    tokio::pin!(cancel);
    run_ask_internal(runtime_root, question, json, styler, cancel).await
}

/// Internal implementation of `ask` with generic cancellation signal.
pub async fn run_ask_internal<F>(
    runtime_root: &Path,
    question: String,
    json: bool,
    styler: Styler,
    mut cancel_signal: F,
) -> i32
where
    F: Future<Output = ()> + Unpin,
{
    let sock = paths::socket_path(runtime_root, Part::Knowledge, "knowledge");
    let client = match Client::connect(&sock, 1024 * 1024).await {
        Ok(c) => c,
        Err(_) => {
            eprintln!("{}", styler.error("could not connect to neuroos-knowledge"));
            eprintln!(
                "{}",
                styler.hint("start the service with 'systemctl --user start neuroos-knowledge'")
            );
            return 1;
        }
    };

    let req = AskRequest {
        text: question,
        origin: Origin::Cli as i32,
    };

    let deadline = Duration::from_secs(60);
    let mut stream = match client.stream(Body::AskRequest(req), deadline).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "{}",
                styler.error(&format!("failed to send ask request: {e}"))
            );
            return 1;
        }
    };

    loop {
        tokio::select! {
            () = &mut cancel_signal => {
                stream.cancel();
                // Bounded drain for end frame
                let _ = tokio::time::timeout(Duration::from_secs(2), async {
                    while let Some(item) = stream.next().await {
                        if item.is_err() {
                            break;
                        }
                    }
                }).await;
                if !json {
                    println!("\n{}", styler.dim("[cancelled]"));
                }
                return 130;
            }
            item = stream.next() => {
                match item {
                    Some(Ok(Body::AnswerChunk(chunk))) => {
                        if json {
                            let ev = JsonAskEvent::Chunk { text: &chunk.text };
                            if let Ok(line) = serde_json::to_string(&ev) {
                                println!("{line}");
                            }
                        } else {
                            print!("{}", chunk.text);
                            let _ = std::io::stdout().flush();
                        }
                    }
                    Some(Ok(Body::AnswerDone(done))) => {
                        if json {
                            let ev = JsonAskEvent::Done {
                                route: done.route,
                                used_web: done.used_web,
                            };
                            if let Ok(line) = serde_json::to_string(&ev) {
                                println!("{line}");
                            }
                        } else {
                            println!();
                        }
                    }
                    Some(Err(IpcError::Remote { code, message, .. })) => {
                        if code == ErrorCode::PermissionDenied {
                            eprintln!("{}", styler.error(&format!("permission denied: {message}")));
                            return 4;
                        }
                        eprintln!("{}", styler.error(&format!("service error: {message}")));
                        return 1;
                    }
                    Some(Err(e)) => {
                        eprintln!("{}", styler.error(&format!("IPC stream error: {e}")));
                        return 1;
                    }
                    None => {
                        // Stream ended
                        break;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    0
}
