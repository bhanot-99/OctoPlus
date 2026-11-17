//! Implementation of `neuroosctl ask` (FR-CLI-01, FR-CLI-02, design.md §7).

use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::{Client, IpcError};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{AskRequest, ErrorCode, Origin, Route};
use serde::Serialize;

use crate::style::Styler;

#[derive(Serialize)]
#[serde(tag = "type")]
enum JsonAskEvent<'a> {
    #[serde(rename = "chunk")]
    Chunk { text: &'a str },
    #[serde(rename = "done")]
    Done {
        route: i32,
        route_name: &'static str,
        taint: u32,
        used_web: bool,
        status_only: bool,
        degraded: bool,
    },
}

/// How `ask` prints its answer.
#[derive(Debug, Clone, Copy, Default)]
pub struct AskOptions {
    /// One JSON object per line instead of text.
    pub json: bool,
    /// After the answer, print the route and taint.
    pub explain: bool,
    /// When the question was asked, in epoch nanoseconds; 0 means now. The evaluation harness
    /// uses it to keep two questions in separate sessions without waiting for the follow-up window.
    pub at_ns: u64,
}

/// Plain name of an answer route (`neuroosctl ask --explain`, P5-S03).
#[must_use]
pub fn route_name(route: i32) -> &'static str {
    match Route::try_from(route) {
        Ok(Route::Action) => "action",
        Ok(Route::DesktopAnswer) => "desktop answer",
        Ok(Route::GeneralChat) => "general chat",
        Ok(Route::NudgeReply) => "nudge reply",
        _ => "unknown",
    }
}

/// Plain names of the taint bits (Architecture §7.4); `none` for a clean context.
#[must_use]
pub fn taint_names(flags: u32) -> String {
    let names: Vec<&str> = [
        (1_u32, "external_untrusted"),
        (1 << 1, "user_provided"),
        (1 << 2, "model_generated"),
        (1 << 3, "stale_index"),
    ]
    .iter()
    .filter(|(bit, _)| flags & bit != 0)
    .map(|(_, n)| *n)
    .collect();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(" | ")
    }
}

/// Runs the `ask` command, streaming response chunks to stdout.
pub async fn run_ask(
    runtime_root: &Path,
    question: String,
    opts: AskOptions,
    styler: Styler,
) -> i32 {
    let cancel = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    tokio::pin!(cancel);
    run_ask_internal(runtime_root, question, opts, styler, cancel).await
}

/// Internal implementation of `ask` with generic cancellation signal.
pub async fn run_ask_internal<F>(
    runtime_root: &Path,
    question: String,
    opts: AskOptions,
    styler: Styler,
    mut cancel_signal: F,
) -> i32
where
    F: Future<Output = ()> + Unpin,
{
    let AskOptions {
        json,
        explain,
        at_ns,
    } = opts;
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
        at_ns,
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

    // Exit 1 when the text was only a status sentence: scripts and tests must not take it for an answer.
    let mut status_only = false;
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
                        status_only = done.status_only;
                        if json {
                            let taint = done.taint.as_ref().map_or(0, |t| t.flags);
                            let ev = JsonAskEvent::Done {
                                route: done.route,
                                route_name: route_name(done.route),
                                taint,
                                used_web: done.used_web,
                                status_only: done.status_only,
                                degraded: done.degraded,
                            };
                            if let Ok(line) = serde_json::to_string(&ev) {
                                println!("{line}");
                            }
                        } else {
                            println!();
                            if done.status_only {
                                eprintln!(
                                    "{}",
                                    styler.error("a part is not running, so this is a status line, not an answer")
                                );
                            } else if done.degraded {
                                eprintln!(
                                    "{}",
                                    styler.hint("answered without all of its data (a part was slow or down)")
                                );
                            }
                            if explain {
                                let taint = done.taint.as_ref().map_or(0, |t| t.flags);
                                eprintln!(
                                    "{}",
                                    styler.dim(&format!(
                                        "[route: {} | taint: {}]",
                                        route_name(done.route),
                                        taint_names(taint)
                                    ))
                                );
                            }
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

    i32::from(status_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_and_taint_names() {
        assert_eq!(route_name(Route::Action as i32), "action");
        assert_eq!(route_name(Route::DesktopAnswer as i32), "desktop answer");
        assert_eq!(route_name(Route::GeneralChat as i32), "general chat");
        assert_eq!(route_name(99), "unknown");
        assert_eq!(taint_names(0), "none");
        assert_eq!(taint_names(0b0110), "user_provided | model_generated");
        assert_eq!(taint_names(0b1001), "external_untrusted | stale_index");
    }
}
