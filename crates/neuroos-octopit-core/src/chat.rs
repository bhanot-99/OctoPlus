//! Chat state and the plain-text rule (ADR-0017 item 3, `docs/design/octopit-ux.md` §4).
//!
//! Model text reaches the screen only through [`sanitize_model_text`]: no control characters,
//! no escape sequences, no direction overrides, no invisible characters, and a length cap. The
//! window draws the result in a label that never reads markup.

use neuroos_proto::v1::AnswerDone;

/// The longest answer shown, in characters. Longer text is cut with a marker.
pub const MAX_ANSWER_CHARS: usize = 8000;

/// The longest question sent, in characters.
pub const MAX_QUESTION_CHARS: usize = 2000;

const CUT_MARKER: &str = "\n\u{2026} (answer cut: too long to show)";

/// Characters that change how text is drawn or hide it: they never reach the screen.
fn is_hidden_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'                      // soft hyphen
        | '\u{061C}'                    // Arabic letter mark
        | '\u{180E}'                    // Mongolian vowel separator
        | '\u{200B}'..='\u{200F}'       // zero-width space/joiners, LRM, RLM
        | '\u{202A}'..='\u{202E}'       // embeddings and overrides
        | '\u{2060}'..='\u{2064}'       // word joiner, invisible operators
        | '\u{2066}'..='\u{2069}'       // isolates
        | '\u{FEFF}'                    // byte order mark
        | '\u{FFF9}'..='\u{FFFB}'       // interlinear annotation
        | '\u{E0000}'..='\u{E007F}' // tag characters
    )
}

/// Makes model text safe to show as plain text.
///
/// Keeps letters, digits, punctuation, emoji, new lines (as `\n`) and tabs. Drops every other
/// control character, whole terminal escape sequences, and the format characters that can reorder
/// or hide text. Cuts at [`MAX_ANSWER_CHARS`].
#[must_use]
pub fn sanitize_model_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_ANSWER_CHARS + 64));
    let mut count = 0usize;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for f in chars.by_ref() {
                        if ('@'..='~').contains(&f) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: ends at BEL or ESC \.
                    chars.next();
                    while let Some(f) = chars.next() {
                        if f == '\u{7}' {
                            break;
                        }
                        if f == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        let c = match c {
            '\r' => '\n',
            other => other,
        };
        if c == '\n' || c == '\t' {
            // Never more than one blank line in a row.
            if c == '\n' && out.ends_with("\n\n") {
                continue;
            }
        } else if c.is_control() || is_hidden_format_char(c) {
            continue;
        }
        if count >= MAX_ANSWER_CHARS {
            out.push_str(CUT_MARKER);
            return out;
        }
        out.push(c);
        count += 1;
    }
    out
}

/// Makes a typed question ready to send: control characters out, trimmed, capped.
/// Returns `None` when nothing is left.
#[must_use]
pub fn clean_question(raw: &str) -> Option<String> {
    let text: String = raw
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || (!c.is_control() && !is_hidden_format_char(*c)))
        .take(MAX_QUESTION_CHARS)
        .collect();
    let text = text.trim().to_owned();
    if text.is_empty() { None } else { Some(text) }
}

/// What C5 says about a finished answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerInfo {
    /// Plain name of the route (`neuroosctl ask --explain` wording).
    pub route: &'static str,
    /// The answer used text from outside this machine (taint bit 1).
    pub external: bool,
    /// A web lookup was part of the answer.
    pub used_web: bool,
    /// The text is a status sentence for a part that is down, not an answer.
    pub status_only: bool,
    /// Some data was missing for this turn.
    pub degraded: bool,
}

impl AnswerInfo {
    /// Reads C5's done message.
    #[must_use]
    pub fn from_done(done: &AnswerDone) -> Self {
        let flags = done.taint.as_ref().map_or(0, |t| t.flags);
        Self {
            route: match done.route {
                1 => "action",
                2 => "desktop answer",
                3 => "general chat",
                4 => "nudge reply",
                _ => "answer",
            },
            external: flags & 1 != 0,
            used_web: done.used_web,
            status_only: done.status_only,
            degraded: done.degraded,
        }
    }
}

/// Where one question and answer stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnState {
    /// Waiting for the first words or still streaming.
    Streaming,
    /// Finished.
    Done(AnswerInfo),
    /// Stopped by the user.
    Cancelled,
    /// Could not be answered; the text says why, in plain words.
    Failed(String),
}

/// One question and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The question as sent.
    pub question: String,
    raw_answer: String,
    /// Where the turn stands.
    pub state: TurnState,
}

impl Turn {
    /// The answer so far, safe to show.
    #[must_use]
    pub fn answer(&self) -> String {
        sanitize_model_text(&self.raw_answer)
    }

    /// Whether the turn is still receiving text.
    #[must_use]
    pub const fn is_streaming(&self) -> bool {
        matches!(self.state, TurnState::Streaming)
    }
}

/// Something that happened to the running turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// More answer text.
    Chunk(String),
    /// The answer is complete.
    Done(AnswerInfo),
    /// The user stopped it.
    Cancelled,
    /// It failed; the text is shown as is, so it must be our own wording.
    Failed(String),
}

/// The conversation of this window. Not stored anywhere: closing the window ends it.
#[derive(Debug, Default, Clone)]
pub struct Conversation {
    turns: Vec<Turn>,
}

/// The longest raw answer kept while streaming (a little over what is shown).
const MAX_RAW_CHARS: usize = MAX_ANSWER_CHARS * 4;

impl Conversation {
    /// An empty conversation.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// All turns, oldest first.
    #[must_use]
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// Whether an answer is being received now.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.turns.last().is_some_and(Turn::is_streaming)
    }

    /// Starts a turn. Returns the cleaned question to send, or `None` when there is nothing to
    /// send or an answer is still streaming (one question at a time).
    pub fn begin(&mut self, typed: &str) -> Option<String> {
        if self.busy() {
            return None;
        }
        let question = clean_question(typed)?;
        self.turns.push(Turn {
            question: question.clone(),
            raw_answer: String::new(),
            state: TurnState::Streaming,
        });
        Some(question)
    }

    /// Applies an event to the running turn. Events with no running turn are ignored.
    pub fn apply(&mut self, event: ChatEvent) {
        let Some(turn) = self.turns.last_mut() else {
            return;
        };
        if !turn.is_streaming() {
            return;
        }
        match event {
            ChatEvent::Chunk(text) => {
                if turn.raw_answer.chars().count() < MAX_RAW_CHARS {
                    turn.raw_answer.push_str(&text);
                }
            }
            ChatEvent::Done(info) => turn.state = TurnState::Done(info),
            ChatEvent::Cancelled => turn.state = TurnState::Cancelled,
            ChatEvent::Failed(why) => turn.state = TurnState::Failed(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neuroos_proto::v1::Taint;

    fn done(route: i32, flags: u32) -> AnswerDone {
        AnswerDone {
            route,
            taint: Some(Taint { flags }),
            ..AnswerDone::default()
        }
    }

    #[test]
    fn markup_and_links_stay_as_literal_text() {
        let s = sanitize_model_text("<b>hi</b> [link](http://x.y) <img src=x>");
        assert_eq!(s, "<b>hi</b> [link](http://x.y) <img src=x>");
    }

    #[test]
    fn escape_sequences_controls_and_overrides_are_removed() {
        assert_eq!(sanitize_model_text("a\u{1b}[31mred\u{1b}[0m b"), "ared b");
        assert_eq!(sanitize_model_text("x\u{1b}]0;title\u{7}y"), "xy");
        assert_eq!(sanitize_model_text("x\u{1b}]0;t\u{1b}\\y"), "xy");
        assert_eq!(sanitize_model_text("a\u{0}b\u{7}c\u{8}d"), "abcd");
        assert_eq!(sanitize_model_text("evil\u{202E}txt.exe"), "eviltxt.exe");
        assert_eq!(sanitize_model_text("z\u{200B}w\u{FEFF}"), "zw");
        assert_eq!(sanitize_model_text("a\u{E0041}b"), "ab");
        assert_eq!(sanitize_model_text("trailing\u{1b}"), "trailing");
    }

    #[test]
    fn real_text_survives() {
        let s = "Delhi \u{092E}\u{094C}\u{0938}\u{092E}: 21\u{00B0}C \u{1F31E}\n\tline two";
        assert_eq!(sanitize_model_text(s), s);
        assert_eq!(sanitize_model_text("a\r\nb"), "a\n\nb");
    }

    #[test]
    fn blank_lines_are_limited_to_one() {
        assert_eq!(sanitize_model_text("a\n\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn long_text_is_cut_with_a_marker() {
        let long = "x".repeat(MAX_ANSWER_CHARS + 500);
        let s = sanitize_model_text(&long);
        assert!(s.starts_with(&"x".repeat(MAX_ANSWER_CHARS)));
        assert!(s.ends_with("too long to show)"));
        assert!(s.chars().count() < MAX_ANSWER_CHARS + 60);
    }

    #[test]
    fn questions_are_cleaned_trimmed_and_capped() {
        assert_eq!(
            clean_question("  hi \u{1b}[A there \u{0}"),
            Some("hi [A there".to_owned())
        );
        assert_eq!(clean_question(" \n\t "), None);
        assert_eq!(clean_question(""), None);
        let long = "q".repeat(MAX_QUESTION_CHARS * 2);
        assert_eq!(
            clean_question(&long).map(|s| s.chars().count()),
            Some(MAX_QUESTION_CHARS)
        );
    }

    #[test]
    fn one_question_at_a_time_and_events_build_the_answer() {
        let mut c = Conversation::new();
        assert_eq!(c.begin("   "), None);
        assert_eq!(c.begin("What is 2+2?"), Some("What is 2+2?".to_owned()));
        assert!(c.busy());
        assert_eq!(c.begin("another"), None, "still streaming");
        c.apply(ChatEvent::Chunk("Four".into()));
        c.apply(ChatEvent::Chunk(".".into()));
        assert_eq!(c.turns()[0].answer(), "Four.");
        c.apply(ChatEvent::Done(AnswerInfo::from_done(&done(3, 0))));
        assert!(!c.busy());
        c.apply(ChatEvent::Chunk("late".into()));
        assert_eq!(
            c.turns()[0].answer(),
            "Four.",
            "a finished turn ignores late text"
        );
        assert!(c.begin("next").is_some());
        c.apply(ChatEvent::Cancelled);
        assert_eq!(c.turns()[1].state, TurnState::Cancelled);
        assert_eq!(c.turns().len(), 2);
    }

    #[test]
    fn events_without_a_turn_are_ignored() {
        let mut c = Conversation::new();
        c.apply(ChatEvent::Chunk("x".into()));
        c.apply(ChatEvent::Failed("no".into()));
        assert!(c.turns().is_empty());
    }

    #[test]
    fn answer_info_reads_route_and_external_text() {
        let i = AnswerInfo::from_done(&done(2, 0b1));
        assert_eq!(i.route, "desktop answer");
        assert!(i.external);
        let j = AnswerInfo::from_done(&done(9, 0));
        assert_eq!(j.route, "answer");
        assert!(!j.external);
    }

    #[test]
    fn an_endless_stream_cannot_grow_without_bound() {
        let mut c = Conversation::new();
        c.begin("go");
        for _ in 0..100 {
            c.apply(ChatEvent::Chunk("y".repeat(10_000)));
        }
        assert!(c.turns()[0].raw_answer.len() < MAX_RAW_CHARS + 10_000);
        assert!(c.turns()[0].answer().chars().count() < MAX_ANSWER_CHARS + 60);
    }
}
