//! Memory probe for the embedder: embeds many texts of varying length and prints the process
//! memory (resident + swapped) now and then. Run: `embed_mem <lean|default> <rounds>`.
#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)] // a dev tool: its output is its result
use std::path::PathBuf;

use octoplus_storage::embed::{Embedder, OnnxEmbedder};

fn mib(key: &str) -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lean = a.get(1).map(String::as_str) != Some("default");
    let rounds: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(3000);
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let opt = home.join(".local/opt/octoplus");
    let e = OnnxEmbedder::load_tuned(
        &opt.join("lib/libonnxruntime.so.1.31.1"),
        &opt.join("models/embedding/bge-small-en-v1.5.onnx"),
        &opt.join("models/embedding/bge-small-en-v1.5.tokenizer.json"),
        2,
        lean,
    )
    .expect("load");
    println!(
        "variant={} after load: rss {:.1} MiB",
        if lean { "lean" } else { "default" },
        mib("VmRSS")
    );
    if rounds == 0 {
        // Fingerprint mode: fixed texts, so two processes can be compared.
        let t: Vec<String> = [
            "What is two plus two?",
            "Summarize my recent git activity.",
            "rust",
        ]
        .iter()
        .map(|x| (*x).to_owned())
        .collect();
        for v in e.embed(&t).expect("embed") {
            println!("{:?}", &v[..6]);
        }
        return;
    }
    let words = [
        "memory", "kernel", "search", "terminal", "rust", "document", "approve", "network",
    ];
    let mut x: u64 = 12345;
    for r in 1..=rounds {
        // 1..=16 texts, each 1..=400 words, so shapes change every call.
        let short = std::env::var("SHORT").is_ok();
        let n = if short { 1 } else { 1 + (x % 16) as usize };
        let mut batch = Vec::new();
        for _ in 0..n {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = if short {
                8 + ((x >> 33) % 25) as usize
            } else {
                1 + ((x >> 33) % 120) as usize
            };
            let t: Vec<&str> = (0..len)
                .map(|i| words[(i + (x >> 20) as usize) % words.len()])
                .collect();
            batch.push(t.join(" "));
        }
        e.embed(&batch).expect("embed");
        if r % 100 == 0 {
            println!(
                "round {r}: rss {:.1} MiB  swap {:.1} MiB  hwm {:.1} MiB",
                mib("VmRSS"),
                mib("VmSwap"),
                mib("VmHWM")
            );
        }
    }
}
