//! W5 / P4-S03: the tokenizer, the stand-in embedder and (when the model files are installed) the
//! real model. `local_*` tests are skipped, loudly, when the pinned files are not present.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use octoplus_storage::embed::{
    DIM, EmbedError, Embedder, HASH_MODEL_ID, HashEmbedder, MAX_TOKENS, MODEL_ID, MODEL_SHA256,
    OnnxEmbedder, WordPiece,
};
use proptest::prelude::*;

fn models() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models")
}

fn paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let m = models();
    let t = (
        m.join("runtime/libonnxruntime.so.1.31.1"),
        m.join("embedding/bge-small-en-v1.5.onnx"),
        m.join("embedding/bge-small-en-v1.5.tokenizer.json"),
    );
    (t.0.exists() && t.1.exists() && t.2.exists()).then_some(t)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f32]) -> f32 {
    dot(a, a).sqrt()
}

const TINY: &str = r###"{"model":{"type":"WordPiece","unk_token":"[UNK]","continuing_subword_prefix":"##","max_input_chars_per_word":10,
"vocab":{"[UNK]":0,"[CLS]":1,"[SEP]":2,"un":3,"##able":4,"##aff":5,"hello":6,",":7,"cafe":8,"a":9,"##b":10}}}"###;

#[test]
fn wordpiece_matches_longest_first_and_marks_unknown_words() {
    let t = WordPiece::from_json(TINY).unwrap();
    assert_eq!(t.encode("unable"), vec![1, 3, 4, 2]);
    assert_eq!(
        t.encode("unaffable"),
        vec![1, 3, 5, 4, 2],
        "un + ##aff + ##able"
    );
    assert_eq!(t.encode("zzz"), vec![1, 0, 2], "no piece fits: one [UNK]");
    assert_eq!(
        t.encode("unableX"),
        vec![1, 0, 2],
        "one unknown piece makes the whole word [UNK]"
    );
    assert_eq!(
        t.encode("Hello, hello"),
        vec![1, 6, 7, 6, 2],
        "lower-cased, punctuation split"
    );
    assert_eq!(t.encode("CAFÉ"), t.encode("cafe"), "accents are stripped");
    assert_eq!(t.encode(""), vec![1, 2]);
    assert_eq!(t.encode("  \t\n "), vec![1, 2]);
    assert_eq!(
        t.encode("abababababab"),
        vec![1, 0, 2],
        "a word over the length limit is [UNK]"
    );
}

#[test]
fn a_bad_tokenizer_file_is_an_error_not_a_panic() {
    for bad in [
        "",
        "{}",
        r#"{"model":{"type":"BPE"}}"#,
        r#"{"model":{"type":"WordPiece","vocab":{}}}"#,
        "not json",
    ] {
        assert!(
            matches!(WordPiece::from_json(bad), Err(EmbedError::Load(_))),
            "{bad}"
        );
    }
}

#[test]
fn long_input_is_cut_to_the_limit_and_still_ends_with_sep() {
    let t = WordPiece::from_json(TINY).unwrap();
    let ids = t.encode(&"hello ".repeat(5000));
    assert_eq!(ids.len(), MAX_TOKENS);
    assert_eq!((ids[0], *ids.last().unwrap()), (1, 2));
}

proptest! {
    #[test]
    fn encoding_never_panics_and_is_always_bounded_and_framed(text in ".{0,3000}") {
        let t = WordPiece::from_json(TINY).unwrap();
        let ids = t.encode(&text);
        prop_assert!(ids.len() >= 2 && ids.len() <= MAX_TOKENS);
        prop_assert_eq!((ids[0], *ids.last().unwrap()), (1, 2));
        prop_assert!(ids.iter().all(|i| (0..=10).contains(i)));
    }
}

#[test]
fn the_stand_in_is_deterministic_unit_length_and_rewards_shared_words() {
    let e = HashEmbedder;
    assert_eq!((e.id(), e.dim()), (HASH_MODEL_ID, DIM));
    let v = e
        .embed(&[
            "Cargo build failed with an error".into(),
            "cargo build failed with an error".into(),
            "my cargo build error".into(),
            "banana bread recipe".into(),
            String::new(),
        ])
        .unwrap();
    assert!((norm(&v[0]) - 1.0).abs() < 1e-5);
    assert_eq!(v[0], v[1], "case-insensitive and deterministic");
    assert!(dot(&v[0], &v[2]) > dot(&v[0], &v[3]) + 0.2);
    assert_eq!(norm(&v[4]), 0.0, "empty text is the zero vector, not NaN");
}

#[test]
fn the_pinned_hash_is_the_manifest_hash() {
    let manifest = std::fs::read_to_string(models().join("manifest.toml")).unwrap();
    assert!(
        manifest.contains(MODEL_SHA256),
        "ADR-0012: the code and the manifest must pin the same file"
    );
    assert!(manifest.contains("bge-small-en-v1.5"));
}

#[test]
fn local_the_real_tokenizer_agrees_with_known_bert_ids() {
    let Some((_, _, tok)) = paths() else {
        eprintln!("skipped: model files not installed");
        return;
    };
    let t = WordPiece::from_json(&std::fs::read_to_string(tok).unwrap()).unwrap();
    assert_eq!(t.encode("hello world"), vec![101, 7592, 2088, 102]);
    assert_eq!(
        t.encode("Hello, World!"),
        vec![101, 7592, 1010, 2088, 999, 102]
    );
    assert_eq!(t.encode("Café"), t.encode("cafe"));
}

#[test]
fn local_the_real_model_embeds_meaning_not_just_words() {
    let Some((lib, model, tok)) = paths() else {
        eprintln!("skipped: model files not installed");
        return;
    };
    let e = OnnxEmbedder::load(&lib, &model, &tok, 2).unwrap();
    assert_eq!((e.id(), e.dim()), (MODEL_ID, DIM));
    let texts: Vec<String> = [
        "how do I fix a rust compile error",
        "my cargo build is failing with errors",
        "recipe for banana bread with walnuts",
        "hello world",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let v = e.embed(&texts).unwrap();
    for x in &v {
        assert_eq!(x.len(), DIM);
        assert!((norm(x) - 1.0).abs() < 1e-4);
    }
    let (related, unrelated) = (dot(&v[0], &v[1]), dot(&v[0], &v[2]));
    assert!(
        related > unrelated + 0.15,
        "related {related} vs unrelated {unrelated}"
    );
    // Batch composition must not change a text's vector (padding is masked).
    let alone = e.embed(&texts[3..4]).unwrap();
    assert!(
        dot(&alone[0], &v[3]) > 0.9999,
        "batched and single embeddings differ"
    );
    assert_eq!(e.embed(&[]).unwrap().len(), 0);
}

#[test]
fn local_a_wrong_model_file_and_a_missing_library_are_refused_cleanly() {
    let Some((lib, model, tok)) = paths() else {
        eprintln!("skipped: model files not installed");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("fake.onnx");
    std::fs::write(&fake, b"not the pinned model").unwrap();
    assert!(matches!(
        OnnxEmbedder::load(&lib, &fake, &tok, 1),
        Err(EmbedError::HashMismatch)
    ));
    assert!(matches!(
        OnnxEmbedder::load(&dir.path().join("nope.so"), &model, &tok, 1),
        Err(EmbedError::Load(_))
    ));
    assert!(matches!(
        OnnxEmbedder::load(&lib, &dir.path().join("missing"), &tok, 1),
        Err(EmbedError::Load(_))
    ));
}
