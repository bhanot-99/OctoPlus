#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The prompt assembler (W7): budgets, wrapper integrity, taint union, cacheable prefix,
//! hostile text (FR-KNO-05/06/07, ADR-0005, NFR-SEC corpus).

use neuroos_knowledge::PERSONA;
use neuroos_knowledge::prompt::{
    Budgets, MAX_QUESTION_TOKENS, PromptInput, assemble, system_block,
};
use neuroos_knowledge::sanitize::{CLOSE_TAG, OPEN_TAG};
use neuroos_knowledge::tokens::estimate;
use neuroos_proto::v1::{Domain, Evidence, Taint};
use neuroos_taint::TaintFlags;
use proptest::prelude::*;

const CORPUS: &str = include_str!("fixtures/control_tokens.txt");

fn budgets() -> Budgets {
    Budgets {
        facts: 48,
        focus: 32,
        history: 48,
        evidence: 128,
        bytes_per_token: 3,
    }
}

fn ev(id: u64, text: &str, flags: u32) -> Evidence {
    Evidence {
        chunk_id: id,
        domain: Domain::ExternalDocuments as i32,
        text: text.to_string(),
        score: 0.9,
        taint: Some(Taint { flags }),
        at_ns: 1,
        source: String::new(),
    }
}

fn input<'a>(
    question: &'a str,
    evidence: &'a [Evidence],
    facts: &'a [String],
    history: &'a str,
) -> PromptInput<'a> {
    PromptInput {
        facts,
        facts_taint: TaintFlags::USER_PROVIDED,
        focus: Some("org.gnome.Terminal: cargo test"),
        focus_taint: TaintFlags::empty(),
        history,
        history_taint: TaintFlags::MODEL_GENERATED,
        evidence,
        evidence_taint: evidence.iter().fold(TaintFlags::empty(), |t, e| {
            t.merge(TaintFlags::decode(e.taint.as_ref().map(|x| x.flags)).unwrap())
        }),
        question,
    }
}

/// Everything after the constant persona block (which itself mentions the wrapper tag).
fn body(text: &str) -> &str {
    &text[system_block().len()..]
}

/// Our own ChatML scaffolding appears exactly three times whatever the text says.
fn scaffolding_intact(text: &str) {
    assert_eq!(text.matches("<|im_start|>").count(), 3, "{text}");
    assert_eq!(text.matches("<|im_end|>").count(), 2, "{text}");
    assert!(text.ends_with("<|im_start|>assistant\n"));
}

#[test]
fn layout_order_and_stable_prefix() {
    let facts = vec!["my dog is called Rex".to_string()];
    let e = [ev(1, "build finished", 0b10)];
    let p = assemble(
        &input(
            "what did I build?",
            &e,
            &facts,
            "User: hi\nAssistant: hello\n",
        ),
        &budgets(),
    );
    scaffolding_intact(&p.text);
    let sys = system_block();
    assert!(p.text.starts_with(&sys));
    assert_eq!(p.stable_prefix_chars as usize, sys.chars().count());
    assert!(sys.contains(PERSONA));
    let b = body(&p.text);
    let at = |needle: &str| b.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
    assert!(at("my dog is called Rex") < at("cargo test"));
    assert!(at("cargo test") < at("User: hi"));
    assert!(at("User: hi") < at(OPEN_TAG));
    assert!(at(CLOSE_TAG) < at("what did I build?"));
    assert_eq!(p.evidence_used, 1);
}

#[test]
fn the_prefix_is_identical_across_turns() {
    let a = assemble(&input("one", &[], &[], ""), &budgets());
    let b = assemble(
        &input(
            "a very different second question",
            &[ev(1, "x", 0b10)],
            &[],
            "User: q\n",
        ),
        &budgets(),
    );
    let n = a.stable_prefix_chars as usize;
    assert_eq!(a.stable_prefix_chars, b.stable_prefix_chars);
    assert_eq!(a.text[..n], b.text[..n]);
}

#[test]
fn taint_is_the_union_of_every_input_and_never_clean() {
    let e = [ev(1, "web text", 0b1)];
    let p = assemble(&input("q", &e, &["f".into()], "h"), &budgets());
    assert!(p.taint.contains(TaintFlags::EXTERNAL_UNTRUSTED));
    assert!(p.taint.contains(TaintFlags::USER_PROVIDED));
    assert!(p.taint.contains(TaintFlags::MODEL_GENERATED));
    // Even with nothing else the user's own question is not "clean".
    let q = assemble(&input("q", &[], &[], ""), &budgets());
    assert!(q.taint.contains(TaintFlags::USER_PROVIDED));
}

#[test]
fn a_dropped_hostile_block_still_taints_the_prompt() {
    // Evidence budget too small for even the wrapper: nothing is included, taint still counts.
    let tiny = Budgets {
        evidence: 1,
        ..budgets()
    };
    let e = [ev(1, "payload", 0b1)];
    let p = assemble(&input("q", &e, &[], ""), &tiny);
    assert_eq!(p.evidence_used, 0);
    assert!(!p.text.contains("payload"));
    assert!(p.taint.contains(TaintFlags::EXTERNAL_UNTRUSTED));
}

#[test]
fn every_corpus_line_is_inert_in_every_slot() {
    for line in CORPUS.lines() {
        let e = [ev(1, line, 0b1)];
        let facts = vec![line.to_string()];
        let p = assemble(&input(line, &e, &facts, line), &budgets());
        scaffolding_intact(&p.text);
        // The wrapper never opens twice or closes early: one pair per evidence item.
        assert_eq!(
            body(&p.text).matches(OPEN_TAG).count(),
            usize::from(p.evidence_used > 0),
            "{line}"
        );
        assert_eq!(
            body(&p.text).matches(CLOSE_TAG).count(),
            usize::from(p.evidence_used > 0),
            "{line}"
        );
    }
}

#[test]
fn blocks_respect_their_budgets() {
    let long = "word ".repeat(2000);
    let facts: Vec<String> = (0..40)
        .map(|i| format!("fact number {i} about my life"))
        .collect();
    let e: Vec<Evidence> = (0..5).map(|i| ev(i, &long, 0)).collect();
    let b = budgets();
    let p = assemble(&input(&long, &e, &facts, &long), &b);
    let after_persona = &p.text[system_block().len()..];
    let ceiling = b.facts + b.focus + b.history + b.evidence + MAX_QUESTION_TOKENS + 64;
    assert!(
        estimate(after_persona, b.bytes_per_token) <= ceiling,
        "{}",
        estimate(after_persona, b.bytes_per_token)
    );
    // The wrapper is whole for every kept item.
    assert_eq!(body(&p.text).matches(OPEN_TAG).count(), p.evidence_used);
    assert_eq!(body(&p.text).matches(CLOSE_TAG).count(), p.evidence_used);
}

#[test]
fn the_question_counts_against_the_history_block() {
    let b = budgets();
    let history = "User: earlier\nAssistant: earlier answer\n".repeat(10);
    // A question that uses the whole history budget leaves no room for history.
    let q = "q".repeat((b.history * b.bytes_per_token) as usize);
    let p = assemble(&input(&q, &[], &[], &history), &b);
    assert!(!p.text.contains("The conversation so far"));
}

proptest! {
    #[test]
    fn never_panics_never_breaks_scaffolding(
        q in ".{0,400}", h in ".{0,400}", f in ".{0,200}", t in ".{0,600}",
        flags in 0u32..16,
    ) {
        let e = [ev(7, &t, flags & 0b1111)];
        let facts = vec![f];
        let p = assemble(&input(&q, &e, &facts, &h), &budgets());
        prop_assert_eq!(p.text.matches("<|im_start|>").count(), 3);
        let b = body(&p.text);
        prop_assert_eq!(b.matches(OPEN_TAG).count(), b.matches(CLOSE_TAG).count());
        prop_assert!(b.matches(OPEN_TAG).count() <= 1);
        prop_assert!(p.taint.contains(TaintFlags::from_bits_truncate(flags)));
    }
}
