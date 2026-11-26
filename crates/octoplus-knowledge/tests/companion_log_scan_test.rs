#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Rule R0-10 for the companion (story C-S08): no personal content in logs at info or above.
//! Every `info!`, `warn!` and `error!` in the companion code may only carry fixed words and error
//! values, never the owner's words, her lines or a fact.

use std::path::Path;

const BANNED: [&str; 14] = [
    "text",
    "line",
    "fact",
    "question",
    "said",
    "reply",
    "answer",
    "opener",
    "sentence",
    "speech",
    "words",
    "transcript",
    "memory_text",
    "keep",
];

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for e in std::fs::read_dir(root.join("src/companion")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            out.push((
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
    for f in [
        "src/turn/companion_turn.rs",
        "src/turn/companion_admin.rs",
        "src/turn/chance_memory.rs",
    ] {
        out.push((
            f.to_string(),
            std::fs::read_to_string(root.join(f)).unwrap(),
        ));
    }
    out
}

/// What follows the format string of one macro call: the arguments, and the `{name}` placeholders.
fn macro_args(call: &str) -> (String, String) {
    // `call` starts right after "info!(" and runs to the closing ");".
    let start = call.find('"').unwrap_or(0);
    let rest = &call[start + 1..];
    let end = rest.find('"').unwrap_or(rest.len());
    let fmt = &rest[..end];
    let args = rest.get(end + 1..).unwrap_or("");
    let placeholders: String = fmt
        .split('{')
        .skip(1)
        .filter_map(|p| p.split('}').next())
        .collect::<Vec<_>>()
        .join(" ");
    (placeholders, args.to_string())
}

fn has_word(hay: &str, word: &str) -> bool {
    hay.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == word)
}

#[test]
fn companion_logs_carry_no_personal_content() {
    let mut calls = 0;
    for (name, src) in sources() {
        for level in ["info!(", "warn!(", "error!("] {
            let mut from = 0;
            while let Some(i) = src[from..].find(level) {
                let at = from + i + level.len();
                let end = src[at..].find(");").map_or(src.len(), |e| at + e);
                let (placeholders, args) = macro_args(&src[at..end]);
                for b in BANNED {
                    assert!(
                        !has_word(&placeholders, b) && !has_word(&args, b),
                        "{name}: a {level} call carries `{b}`: {}",
                        &src[at..end]
                    );
                }
                calls += 1;
                from = end;
            }
        }
    }
    println!("scanned {calls} log calls");
    assert!(calls >= 1, "the scan found the real log calls");
}

#[test]
fn the_scan_would_catch_a_leak() {
    let (p, a) = macro_args(r#""said {line} to the owner", text"#);
    assert!(has_word(&p, "line") && has_word(&a, "text"));
    let (p, a) = macro_args(r#""could not talk: {e}", e"#);
    assert!(!BANNED.iter().any(|b| has_word(&p, b) || has_word(&a, b)));
}
