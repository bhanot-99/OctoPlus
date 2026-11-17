//! Turning generated text into speakable sentences (FR-KNO-08, design.md §9.2). Pure.
//!
//! C5 strips markdown, lists, URLs and symbols and forwards the answer to C2 sentence by
//! sentence, so the first sentence can be spoken while the rest is still being generated.

/// Longest piece sent to C2 in one frame (C2 may split further for the speech engine).
pub const MAX_PIECE_CHARS: usize = 220;

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
        let (done, rest) = split_complete(&work);
        self.buf = format!("{rest}{pending}");
        done.into_iter()
            .flat_map(|p| cap(p, MAX_PIECE_CHARS))
            .collect()
    }

    /// The final unfinished piece, if any, and empties the stream.
    pub fn flush(&mut self) -> Vec<String> {
        // An unclosed code fence is dropped with everything after it.
        let raw = std::mem::take(&mut self.buf);
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
    fn nothing_buffered_means_nothing_flushed() {
        let mut s = SentenceStream::new();
        assert!(s.feed("   ").is_empty());
        assert!(s.flush().is_empty());
    }
}
