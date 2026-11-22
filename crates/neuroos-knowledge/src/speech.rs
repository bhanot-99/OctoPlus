//! Turning generated text into speakable sentences (FR-KNO-08, design.md §9.2). Pure.
//!
//! C5 strips markdown, lists, URLs and symbols and forwards the answer to C2 sentence by
//! sentence, so the first sentence can be spoken while the rest is still being generated.

/// Longest piece sent to C2 in one frame (C2 may split further for the speech engine).
pub const MAX_PIECE_CHARS: usize = 220;

/// The first spoken piece of an answer may end at a clause mark (comma, semicolon, colon) once it
/// holds at least this many characters, so speech starts before the first whole sentence is
/// generated (NFR-PERF-02, owner decision D-47). Later pieces are whole sentences.
pub const FIRST_CLAUSE_MIN_CHARS: usize = 40;

/// Most text kept while waiting for a sentence to end.
const MAX_BUFFERED_BYTES: usize = 4096;

const ABBREVIATIONS: &[&str] = &[
    "dr", "mr", "mrs", "ms", "prof", "st", "vs", "e.g", "i.e", "etc",
];

/// Removes what must never be read aloud: code fences, URLs, markdown marks and list markers;
/// folds whitespace. Dashes become commas so the voice pauses.
#[must_use]
pub fn clean_for_speech(text: &str) -> String {
    // Drop fenced code blocks.
    let mut kept = String::with_capacity(text.len());
    let mut in_fence = false;
    for (i, part) in text.split("```").enumerate() {
        if i > 0 {
            in_fence = !in_fence;
        }
        if !in_fence {
            kept.push_str(part);
            kept.push(' ');
        }
    }
    let mut lines = Vec::new();
    for line in kept.lines() {
        let mut l = line.trim_start();
        // A list marker at the start of a line.
        if let Some(rest) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")) {
            l = rest;
        } else {
            let digits = l.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0
                && let Some(rest) = l[digits..].strip_prefix(". ")
            {
                l = rest;
            }
        }
        lines.push(l.trim_start_matches('#').trim_start());
    }
    let joined = lines.join(" ");
    let mut out = String::with_capacity(joined.len());
    for word in joined.split_whitespace() {
        if word.starts_with("http://") || word.starts_with("https://") {
            continue;
        }
        let word = word.replace(['*', '_', '`'], "");
        let word = word.replace(['\u{2014}', '\u{2013}'], ", ");
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&word);
    }
    out.replace(" ,", ",")
}

/// True when the `.` at byte `at` of `s` ends a sentence.
fn is_boundary(s: &str, at: usize) -> bool {
    let bytes = s.as_bytes();
    // A boundary needs whitespace right after (a '.' inside "main.rs" or "3.5" is not one).
    match bytes.get(at + 1) {
        Some(b) if b.is_ascii_whitespace() => {}
        None => {}
        Some(_) => return false,
    }
    if bytes[at] != b'.' {
        return true;
    }
    let before = &s[..at];
    let word = before
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if ABBREVIATIONS.contains(&word.as_str()) {
        // "etc." and the like may still end a sentence when a capital follows.
        let next = s[at + 1..].trim_start().chars().next();
        return word == "etc" && next.is_some_and(char::is_uppercase);
    }
    // A single capital letter is an initial: "J. R. Smith".
    let mut chars = word.chars();
    let single_letter = chars.next().is_some_and(char::is_alphabetic) && chars.next().is_none();
    !single_letter
}

/// Splits `text` at sentence ends; the last piece may be unfinished and is returned second.
fn split_complete(text: &str) -> (Vec<String>, String) {
    let mut done = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'.' | b'!' | b'?') {
            // A run of terminators counts once.
            let mut end = i;
            while end + 1 < bytes.len() && matches!(bytes[end + 1], b'.' | b'!' | b'?') {
                end += 1;
            }
            // Only a boundary when something follows to confirm it.
            if end + 1 < bytes.len() && is_boundary(text, end) {
                let piece = text[start..=end].trim();
                if !piece.is_empty() {
                    done.push(piece.to_string());
                }
                start = end + 1;
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    (done, text[start..].trim_start().to_string())
}

/// The first clause of `text` for an early start: ends at a `,` `;` or `:` that is followed by
/// whitespace (so "1,000" and "10:30" stay whole) and sits at least `FIRST_CLAUSE_MIN_CHARS`
/// in. Returns it and the remainder.
fn first_clause(text: &str) -> Option<(String, String)> {
    let mut chars = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        chars += 1;
        if chars >= FIRST_CLAUSE_MIN_CHARS
            && matches!(c, ',' | ';' | ':')
            && iter.peek().is_some_and(|(_, n)| n.is_whitespace())
        {
            let end = i + c.len_utf8();
            return Some((
                text[..end].trim().to_string(),
                text[end..].trim_start().to_string(),
            ));
        }
    }
    None
}

/// Splits an over-long piece at a comma, then a space, never inside a character.
fn cap(piece: String, max_chars: usize) -> Vec<String> {
    let max = max_chars.max(1);
    let mut out = Vec::new();
    let mut rest = piece;
    while rest.chars().count() > max {
        let limit = rest.char_indices().nth(max).map_or(rest.len(), |(i, _)| i);
        let head = &rest[..limit];
        let cut = head
            .rfind([',', ';', ':'])
            .map(|i| i + 1)
            .or_else(|| head.rfind(' '))
            .filter(|&c| c > 0)
            .unwrap_or(limit);
        let (a, b) = rest.split_at(cut);
        out.push(a.trim().to_string());
        rest = b.trim_start().to_string();
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out.retain(|p| !p.is_empty());
    out
}

/// Removes finished code fences from `raw`. Returns the text that can be worked on now and the
/// tail that must wait: an unclosed fence (and everything after it) or a partial "```".
fn split_fences(raw: &str) -> (String, String) {
    let mut work = String::new();
    let mut rest = raw;
    while let Some(open) = rest.find("```") {
        let after = &rest[open + 3..];
        match after.find("```") {
            Some(close) => {
                work.push_str(&rest[..open]);
                work.push(' ');
                rest = &after[close + 3..];
            }
            None => {
                work.push_str(&rest[..open]);
                return (work, rest[open..].to_string());
            }
        }
    }
    // One or two backticks at the very end may be the start of a fence.
    let trailing = rest.bytes().rev().take_while(|b| *b == b'`').count().min(2);
    let split = rest.len() - trailing;
    work.push_str(&rest[..split]);
    (work, rest[split..].to_string())
}

/// Feeds streamed text in; gets finished speakable sentences out.
#[derive(Debug, Default)]
pub struct SentenceStream {
    buf: String,
    /// True once this answer has produced a piece; the early clause cut applies only before.
    spoke: bool,
}

impl SentenceStream {
    /// An empty stream.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `piece` and returns the sentences that are now certainly complete.
    pub fn feed(&mut self, piece: &str) -> Vec<String> {
        self.buf.push_str(piece);
        let (workable, pending) = split_fences(&self.buf);
        let cleaned = clean_for_speech(&workable);
        // Keep a trailing space so "end. " still confirms the boundary after cleaning.
        let work = if workable.ends_with(char::is_whitespace) && !cleaned.is_empty() {
            format!("{cleaned} ")
        } else {
            cleaned
        };
        let (done, mut rest) = split_complete(&work);
        let mut out: Vec<String> = done
            .into_iter()
            .flat_map(|p| cap(p, MAX_PIECE_CHARS))
            .collect();
        // Start speaking early: no sentence is done yet, so give the first clause.
        if !self.spoke
            && out.is_empty()
            && let Some((clause, remainder)) = first_clause(&rest)
        {
            out.extend(cap(clause, MAX_PIECE_CHARS));
            rest = remainder;
        }
        // A model that never ends a sentence must not grow this buffer without bound: past the
        // cap, what is waiting is spoken as pieces (cut at a space) instead (review F5).
        if rest.len() > MAX_BUFFERED_BYTES {
            out.extend(cap(std::mem::take(&mut rest), MAX_PIECE_CHARS));
        }
        // The same for an unclosed code fence: the fence and what follows it are dropped once
        // they are longer than the cap, so the buffer stays small.
        let pending = if pending.len() > MAX_BUFFERED_BYTES {
            String::new()
        } else {
            pending
        };
        self.buf = format!("{rest}{pending}");
        self.spoke |= !out.is_empty();
        out
    }

    /// The final unfinished piece, if any, and empties the stream.
    pub fn flush(&mut self) -> Vec<String> {
        // An unclosed code fence is dropped with everything after it.
        let raw = std::mem::take(&mut self.buf);
        self.spoke = false;
        let (workable, _) = split_fences(&raw);
        let rest = clean_for_speech(&workable);
        cap(rest.trim().to_string(), MAX_PIECE_CHARS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str, chunk: usize) -> Vec<String> {
        let mut s = SentenceStream::new();
        let mut out = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        for c in chars.chunks(chunk) {
            out.extend(s.feed(&c.iter().collect::<String>()));
        }
        out.extend(s.flush());
        out
    }

    #[test]
    fn splits_plain_sentences_whatever_the_chunking() {
        for chunk in [1, 3, 1000] {
            assert_eq!(
                all("It is sunny. Take a coat! Why not?", chunk),
                vec!["It is sunny.", "Take a coat!", "Why not?"],
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn abbreviations_versions_files_and_initials_do_not_split() {
        assert_eq!(
            all("Dr. Smith went home. He slept.", 4),
            vec!["Dr. Smith went home.", "He slept."]
        );
        assert_eq!(
            all("Version 3.5 is out. See main.rs now.", 5),
            vec!["Version 3.5 is out.", "See main.rs now."]
        );
        assert_eq!(all("J. R. Smith arrived.", 2), vec!["J. R. Smith arrived."]);
    }

    #[test]
    fn markdown_urls_code_and_lists_are_never_spoken() {
        let got = all(
            "**Sure.** See https://example.com now.\n- first item\n1. second item\n```\nrm -rf /\n```\nDone.",
            7,
        );
        let joined = got.join(" ");
        assert!(!joined.contains('*') && !joined.contains("http") && !joined.contains("rm -rf"));
        assert!(joined.contains("first item") && joined.contains("second item"));
        assert!(joined.ends_with("Done."));
    }

    #[test]
    fn long_text_is_capped_and_utf8_is_safe() {
        let long = "word ".repeat(200);
        for p in all(&long, 50) {
            assert!(p.chars().count() <= MAX_PIECE_CHARS, "{}", p.len());
        }
        let got = all("Caf\u{e9} \u{65e5}\u{672c}\u{8a9e} is fine. Next.", 1);
        assert_eq!(
            got,
            vec!["Caf\u{e9} \u{65e5}\u{672c}\u{8a9e} is fine.", "Next."]
        );
    }

    #[test]
    fn a_stream_that_never_ends_a_sentence_has_a_bounded_buffer() {
        let mut s = SentenceStream::new();
        let mut spoken = 0;
        for _ in 0..2000 {
            spoken += s.feed("word word word word ").len();
        }
        assert!(s.buf.len() <= MAX_BUFFERED_BYTES + 64, "{}", s.buf.len());
        assert!(
            spoken > 100,
            "it was spoken in pieces, not held back: {spoken}"
        );
        // And an unclosed code fence does not hold the buffer either.
        let mut f = SentenceStream::new();
        f.feed("```");
        for _ in 0..2000 {
            f.feed("let x = 1; ");
        }
        assert!(f.buf.len() <= MAX_BUFFERED_BYTES + 64, "{}", f.buf.len());
    }

    #[test]
    fn nothing_buffered_means_nothing_flushed() {
        let mut s = SentenceStream::new();
        assert!(s.feed("   ").is_empty());
        assert!(s.flush().is_empty());
    }

    #[test]
    fn the_first_piece_may_end_at_a_clause_but_later_ones_are_whole_sentences() {
        let text = "The gross domestic product of India is about four trillion dollars, which makes it large, and it grows. Next sentence, with a comma, ends here. ";
        let mut s = SentenceStream::new();
        // Streamed word by word, as the model produces it.
        let got: Vec<String> = text.split_inclusive(' ').flat_map(|w| s.feed(w)).collect();
        assert_eq!(
            got[0],
            "The gross domestic product of India is about four trillion dollars,"
        );
        assert_eq!(got[1], "which makes it large, and it grows.");
        assert_eq!(got[2], "Next sentence, with a comma, ends here.");
    }

    #[test]
    fn a_short_clause_numbers_and_times_do_not_trigger_the_early_cut() {
        let mut s = SentenceStream::new();
        assert!(
            s.feed("Yes, it is 10:30 and costs 1,000 rupees today ")
                .is_empty()
        );
        assert_eq!(
            s.flush().concat(),
            "Yes, it is 10:30 and costs 1,000 rupees today"
        );
    }

    #[test]
    fn the_early_cut_applies_once_per_answer_and_resets_on_flush() {
        let long = "This is a fairly long opening clause for the answer, and then more words follow here, and more ";
        let mut s = SentenceStream::new();
        assert_eq!(s.feed(long).len(), 1);
        assert!(s.feed("again and again, ").is_empty());
        let _ = s.flush();
        assert_eq!(s.feed(long).len(), 1);
    }
}
