//! Implementation of `octoplusctl ask` (FR-CLI-01, FR-CLI-02, design.md §7).

use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::{Client, IpcError};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{AskRequest, ErrorCode, Origin, Route};
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
    /// Whether to enter continuous interactive conversation mode after the question.
    pub interactive: bool,
}

/// Plain name of an answer route (`octoplusctl ask --explain`, P5-S03).
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

/// Words that end an interactive session.
const SESSION_END_WORDS: [&str; 5] = ["bye", "quit", "exit", "/bye", "/exit"];

/// Runs the `ask` command, streaming response chunks to stdout.
/// If running in an interactive terminal without `--once` or `--json`, continues into
/// a multi-turn conversation session until the user types 'bye', 'exit', or 'quit'.
pub async fn run_ask(
    runtime_root: &Path,
    question: String,
    opts: AskOptions,
    styler: Styler,
) -> i32 {
    if !question.trim().is_empty() {
        let cancel = std::pin::pin!(ctrl_c());
        let code = run_ask_internal(
            runtime_root,
            question.trim().to_string(),
            opts,
            styler,
            cancel,
        )
        .await;
        if code != 0 || !opts.interactive {
            return code;
        }
    } else if !opts.interactive {
        eprintln!(
            "{}",
            styler.error("question text required in non-interactive mode")
        );
        return 1;
    } else {
        println!(
            "{}",
            styler.bold("=== OctoPlus Conversation Session ===")
        );
        println!(
            "{}",
            styler.dim("Type your message and press Enter. Say 'bye' to finish.\n")
        );
    }

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    conversation_loop(runtime_root, stdin, opts, styler, ctrl_c()).await
}

/// Resolves on Ctrl+C. If the handler cannot be installed it never resolves, so the
/// session still ends on "bye" or end of input.
async fn ctrl_c() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// `line` without terminal escape codes (arrow keys, Home, function keys) and other control
/// characters.
#[must_use]
pub fn typed_text(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                // CSI: ESC [ parameters... final byte in '@'..='~'.
                Some('[') => {
                    for f in chars.by_ref() {
                        if ('@'..='~').contains(&f) {
                            break;
                        }
                    }
                }
                // SS3 (ESC O A) and two-character sequences.
                Some('O') => {
                    chars.next();
                }
                _ => {}
            }
        } else if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    out
}

/// The interactive session: reads one line at a time from `input` and asks it, until an end
/// word, end of input (exit 0) or `quit` firing while waiting at the prompt (exit 130).
/// Ctrl+C during an answer cancels only that answer (FR-CLI-02).
pub async fn conversation_loop<R, Q>(
    runtime_root: &Path,
    input: R,
    opts: AskOptions,
    styler: Styler,
    quit: Q,
) -> i32
where
    R: tokio::io::AsyncBufRead + Unpin,
    Q: Future<Output = ()>,
{
    use tokio::io::AsyncBufReadExt;

    let mut lines = input.lines();
    let mut quit = std::pin::pin!(quit);
    loop {
        print!("{}", styler.bold("> "));
        let _ = std::io::stdout().flush();
        let line = tokio::select! {
            () = &mut quit => {
                println!();
                return 130;
            }
            line = lines.next_line() => line,
        };
        let line = match line {
            Ok(Some(line)) => line,
            // End of input (Ctrl+D) or an unreadable terminal ends the session.
            Ok(None) | Err(_) => {
                println!();
                return 0;
            }
        };
        // Arrow and other editing keys arrive as escape codes ("^[[A"); they are not words
        // and must not reach the model (owner, 2026-11-22).
        let typed = typed_text(&line);
        let trimmed = typed.trim();
        if trimmed.is_empty() {
            if line.contains('\u{1b}') {
                println!("(Arrow keys don't work here yet: please type your message.)");
            }
            continue;
        }
        if SESSION_END_WORDS
            .iter()
            .any(|w| trimmed.eq_ignore_ascii_case(w))
        {
            println!("Goodbye!");
            return 0;
        }
        let cancel = std::pin::pin!(ctrl_c());
        run_ask_internal(runtime_root, trimmed.to_string(), opts, styler, cancel).await;
    }
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
        ..
    } = opts;
    let sock = paths::socket_path(runtime_root, Part::Knowledge, "knowledge");
    let client = match Client::connect(&sock, 1024 * 1024).await {
        Ok(c) => c,
        Err(_) => {
            eprintln!("{}", styler.error("could not connect to octoplus-knowledge"));
            eprintln!(
                "{}",
                styler.hint("start the service with 'systemctl --user start octoplus-knowledge'")
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

#[cfg(test)]
mod typed_text_tests {
    use super::typed_text;

    #[test]
    fn escape_codes_from_editing_keys_are_not_text() {
        assert_eq!(typed_text("\u{1b}[A"), "");
        assert_eq!(typed_text("\u{1b}[A\u{1b}[B\u{1b}OC"), "");
        assert_eq!(typed_text("\u{1b}[1;5Dhello"), "hello");
        assert_eq!(typed_text("what is 2+2?"), "what is 2+2?");
        assert_eq!(typed_text("Delhi मौसम \u{1b}[3~"), "Delhi मौसम ");
    }
}
