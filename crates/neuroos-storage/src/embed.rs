//! The single embedding model (FR-STO-03, ADR-0012): `bge-small-en-v1.5` run through ONNX Runtime
//! loaded from a pinned file, with a BERT WordPiece tokenizer written here (no `tokenizers` crate,
//! no `fastembed`: R0-1, cargo-deny). Only C3 loads a model. Calls run on the blocking pool.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use sha2::{Digest, Sha256};

/// Vector length of `bge-small-en-v1.5`.
pub const DIM: usize = 384;
/// Model id recorded in `index_meta` (FR-STO-12).
pub const MODEL_ID: &str = "bge-small-en-v1.5";
/// Pinned SHA-256 of the ONNX file; equals `models/manifest.toml` (a test checks it).
pub const MODEL_SHA256: &str = "828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35";
/// Longest input in tokens, including `[CLS]` and `[SEP]`.
pub const MAX_TOKENS: usize = 512;
/// Id of the stand-in embedder, so rows it made are known to be stale once the real model loads.
pub const HASH_MODEL_ID: &str = "hash-bow-384";

/// Why embedding failed. Messages never contain the text (R0-10).
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The model, tokenizer or runtime could not be loaded.
    #[error("embedding model unavailable: {0}")]
    Load(&'static str),
    /// The model file is not the pinned one (R0-9).
    #[error("embedding model file does not match its pinned hash")]
    HashMismatch,
    /// The runtime failed on a request.
    #[error("embedding run failed")]
    Run,
}

/// Anything that turns text into unit-length vectors of one fixed size.
pub trait Embedder: Send + Sync {
    /// The model id, stored with every vector.
    fn id(&self) -> &str;
    /// Vector length.
    fn dim(&self) -> usize;
    /// Embeds a batch; one L2-normalised vector per text, in order.
    ///
    /// # Errors
    /// [`EmbedError`] if the runtime fails.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn normalise(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 && n.is_finite() {
        for x in v {
            *x /= n;
        }
    }
}

// ---- the stand-in -----------------------------------------------------------------------------

/// A deterministic bag-of-words hashing embedder. Not semantic: words that match score high,
/// synonyms do not. It exists for tests and for running without the model (DEGRADED).
#[derive(Debug, Default, Clone, Copy)]
pub struct HashEmbedder;

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl Embedder for HashEmbedder {
    fn id(&self) -> &str {
        HASH_MODEL_ID
    }

    fn dim(&self) -> usize {
        DIM
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0_f32; DIM];
                for w in t
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                {
                    let h = fnv(&w.to_lowercase());
                    let idx = usize::try_from(h % DIM as u64).unwrap_or(0);
                    v[idx] += if (h >> 40) & 1 == 0 { 1.0 } else { -1.0 };
                }
                normalise(&mut v);
                v
            })
            .collect())
    }
}

// ---- WordPiece ----------------------------------------------------------------------------------

/// BERT WordPiece, read from the model's `tokenizer.json`: lower-case, strip accents, split on
/// whitespace and punctuation, greedy longest-match with `##` continuations.
#[derive(Debug)]
pub struct WordPiece {
    vocab: HashMap<String, i64>,
    unk: i64,
    cls: i64,
    sep: i64,
    max_word_chars: usize,
}

fn strip_accent(c: char) -> char {
    const FROM: &str =
        "àáâãäåāăąçćĉċčďđèéêëēĕėęěĝğġģĥħìíîïĩīĭįıĵķĺļľłñńņňòóôõöøōŏőŕŗřśŝşšţťŧùúûüũūŭůűųŵýÿŷźżž";
    const TO: &str =
        "aaaaaaaaacccccddeeeeeeeeegggghhiiiiiiiiijklllllnnnnooooooooorrrssssstttuuuuuuuuuuwyyyzzz";
    FROM.chars()
        .position(|f| f == c)
        .and_then(|i| TO.chars().nth(i))
        .unwrap_or(c)
}

fn is_cjk(c: char) -> bool {
    matches!(u32::from(c), 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F | 0x2B740..=0x2B81F | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF | 0x2F800..=0x2FA1F)
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
}

impl WordPiece {
    /// Reads the vocabulary and special tokens from `tokenizer.json` text.
    ///
    /// # Errors
    /// [`EmbedError::Load`] if the JSON is not a BERT WordPiece tokenizer.
    pub fn from_json(text: &str) -> Result<Self, EmbedError> {
        let bad = |m| EmbedError::Load(m);
        let v: serde_json::Value =
            serde_json::from_str(text).map_err(|_| bad("tokenizer is not JSON"))?;
        let model = v.get("model").ok_or(bad("tokenizer has no model"))?;
        if model.get("type").and_then(|t| t.as_str()) != Some("WordPiece") {
            return Err(bad("tokenizer is not WordPiece"));
        }
        let vocab: HashMap<String, i64> = model
            .get("vocab")
            .and_then(|m| m.as_object())
            .ok_or(bad("tokenizer has no vocabulary"))?
            .iter()
            .filter_map(|(k, v)| v.as_i64().map(|i| (k.clone(), i)))
            .collect();
        let id = |t: &str| {
            vocab
                .get(t)
                .copied()
                .ok_or(bad("tokenizer lacks a special token"))
        };
        Ok(Self {
            unk: id("[UNK]")?,
            cls: id("[CLS]")?,
            sep: id("[SEP]")?,
            max_word_chars: usize::try_from(
                model
                    .get("max_input_chars_per_word")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(100),
            )
            .unwrap_or(100),
            vocab,
        })
    }

    fn normalise(text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 8);
        for c in text.chars() {
            if c == '\0' || c == '\u{fffd}' || (c.is_control() && !c.is_whitespace()) {
                continue;
            }
            let c = strip_accent(c.to_lowercase().next().unwrap_or(c));
            if is_cjk(c) {
                out.push(' ');
                out.push(c);
                out.push(' ');
            } else if c.is_whitespace() {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
        out
    }

    fn pieces(&self, word: &str, out: &mut Vec<i64>) {
        let chars: Vec<char> = word.chars().collect();
        if chars.len() > self.max_word_chars {
            out.push(self.unk);
            return;
        }
        let mut start = 0;
        let mut found = Vec::new();
        while start < chars.len() {
            let mut end = chars.len();
            let mut hit = None;
            while start < end {
                let mut piece: String = chars[start..end].iter().collect();
                if start > 0 {
                    piece.insert_str(0, "##");
                }
                if let Some(id) = self.vocab.get(&piece) {
                    hit = Some(*id);
                    break;
                }
                end -= 1;
            }
            let Some(id) = hit else {
                out.push(self.unk); // a word with any unknown piece is one [UNK], as in BERT
                return;
            };
            found.push(id);
            start = end;
        }
        out.extend(found);
    }

    /// Token ids for `text`, with `[CLS]` first and `[SEP]` last, at most [`MAX_TOKENS`] long.
    #[must_use]
    pub fn encode(&self, text: &str) -> Vec<i64> {
        let norm = Self::normalise(text);
        let mut ids = vec![self.cls];
        'words: for raw in norm.split_whitespace() {
            let mut word = String::new();
            for c in raw.chars() {
                if is_punct(c) {
                    if !word.is_empty() {
                        self.pieces(&std::mem::take(&mut word), &mut ids);
                    }
                    self.pieces(&c.to_string(), &mut ids);
                } else {
                    word.push(c);
                }
                if ids.len() >= MAX_TOKENS - 1 {
                    break 'words;
                }
            }
            if !word.is_empty() {
                self.pieces(&word, &mut ids);
            }
            if ids.len() >= MAX_TOKENS - 1 {
                break;
            }
        }
        ids.truncate(MAX_TOKENS - 1);
        ids.push(self.sep);
        ids
    }
}

// ---- the real model -------------------------------------------------------------------------------

/// Loads the ONNX Runtime shared library into the process. Call it **before** the Landlock
/// sandbox is applied: the library pulls in system libraries (`libstdc++`, …) that the sandbox
/// no longer lets the process open, so loading it afterwards fails. Calling it again later, from
/// [`OnnxEmbedder::load`], reuses the loaded library.
///
/// # Errors
/// [`EmbedError::Load`] if the file is missing or is not a usable ONNX Runtime.
pub fn preload_runtime(lib: &Path) -> Result<(), EmbedError> {
    ort::init_from(lib)
        .map(|_| ())
        .map_err(|_| EmbedError::Load("ONNX Runtime library cannot be loaded"))
}

/// `bge-small-en-v1.5` through ONNX Runtime (CPU).
pub struct OnnxEmbedder {
    session: Mutex<Session>,
    tokenizer: WordPiece,
}

impl std::fmt::Debug for OnnxEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnnxEmbedder").finish_non_exhaustive()
    }
}

impl OnnxEmbedder {
    /// Loads the runtime library, checks the model's SHA-256 against the pinned value (R0-9),
    /// and opens the session with `threads` intra-op threads.
    ///
    /// # Errors
    /// [`EmbedError`] if any file is missing, the model is not the pinned one, or the runtime
    /// cannot start.
    pub fn load(
        lib: &Path,
        model: &Path,
        tokenizer: &Path,
        threads: usize,
    ) -> Result<Self, EmbedError> {
        let bytes = std::fs::read(model).map_err(|_| EmbedError::Load("model file unreadable"))?;
        if hex(&Sha256::digest(&bytes)) != MODEL_SHA256 {
            return Err(EmbedError::HashMismatch);
        }
        drop(bytes);
        let text = std::fs::read_to_string(tokenizer)
            .map_err(|_| EmbedError::Load("tokenizer file unreadable"))?;
        let tokenizer = WordPiece::from_json(&text)?;
        let env = ort::init_from(lib)
            .map_err(|_| EmbedError::Load("ONNX Runtime library cannot be loaded"))?
            .build()
            .map_err(|_| EmbedError::Load("ONNX Runtime cannot start"))?;
        const OPEN: EmbedError = EmbedError::Load("the model cannot be opened");
        let builder = Session::builder(&env).map_err(|_| OPEN)?;
        let mut builder = builder
            .with_intra_threads(threads.max(1))
            .map_err(|_| OPEN)?;
        let session = builder.commit_from_file(model).map_err(|_| OPEN)?;
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    fn run_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let encoded: Vec<Vec<i64>> = texts.iter().map(|t| self.tokenizer.encode(t)).collect();
        let seq = encoded.iter().map(Vec::len).max().unwrap_or(2);
        let b = encoded.len();
        let mut ids = vec![0_i64; b * seq];
        let mut mask = vec![0_i64; b * seq];
        for (row, e) in encoded.iter().enumerate() {
            ids[row * seq..row * seq + e.len()].copy_from_slice(e);
            mask[row * seq..row * seq + e.len()].fill(1);
        }
        let types = vec![0_i64; b * seq];
        let shape = [b, seq];
        let run = |e: ort::Error| {
            let _ = e;
            EmbedError::Run
        };
        let ids_t = Tensor::from_array((shape, ids)).map_err(run)?;
        let mask_t = Tensor::from_array((shape, mask)).map_err(run)?;
        let types_t = Tensor::from_array((shape, types)).map_err(run)?;
        let mut session = self.session.lock().map_err(|_| EmbedError::Run)?;
        let outputs = session
            .run(ort::inputs!["input_ids" => ids_t, "attention_mask" => mask_t, "token_type_ids" => types_t])
            .map_err(run)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(run)?;
        let dims: Vec<usize> = shape
            .iter()
            .map(|d| usize::try_from(*d).unwrap_or(0))
            .collect();
        if dims.len() != 3 || dims[0] != b || dims[2] != DIM || data.len() != b * dims[1] * DIM {
            return Err(EmbedError::Run);
        }
        // CLS pooling: the first token's vector, then L2 normalisation (bge-small-en-v1.5).
        Ok((0..b)
            .map(|row| {
                let start = row * dims[1] * DIM;
                let mut v = data[start..start + DIM].to_vec();
                normalise(&mut v);
                v
            })
            .collect())
    }
}

impl Embedder for OnnxEmbedder {
    fn id(&self) -> &str {
        MODEL_ID
    }

    fn dim(&self) -> usize {
        DIM
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut out = Vec::with_capacity(texts.len());
        // Small batches keep memory flat and latency predictable.
        for chunk in texts.chunks(16) {
            out.extend(self.run_batch(chunk)?);
        }
        Ok(out)
    }
}
