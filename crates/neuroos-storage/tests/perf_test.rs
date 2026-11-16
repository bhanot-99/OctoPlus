//! W11 / NFR-PERF-04 measurement (not part of the gate: `#[ignore]`, run in release):
//! `cargo nextest run --release -p neuroos-storage --run-ignored only -E 'test(perf_)'`
//! Writes `reports/bench/p4-search-<UTC>.json` with the conditions (rules.md §9).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use neuroos_proto::v1::QueryHybrid;
use neuroos_storage::db::MatrixOp;
use neuroos_storage::embed::{DIM, Embedder, MODEL_ID, OnnxEmbedder};
use neuroos_storage::store::{Store, StorePaths, vector_to_blob};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn vocab(rng: &mut Rng) -> Vec<String> {
    (0..3000)
        .map(|i| format!("w{i}x{:x}", rng.next() % 4096))
        .collect()
}

fn random_unit_vector(rng: &mut Rng) -> Vec<f32> {
    let mut v: Vec<f32> = (0..DIM)
        .map(|_| (0..4).map(|_| rng.unit()).sum::<f32>() - 2.0)
        .collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= n);
    v
}

#[allow(clippy::too_many_arguments)]
async fn bulk(
    store: &Arc<Store>,
    rng: &mut Rng,
    words: &[String],
    from: i64,
    n: i64,
    at_base: i64,
) {
    for start in (0..n).step_by(500) {
        let end = (start + 500).min(n);
        let rows: Vec<(i64, String, Vec<f32>)> = (start..end)
            .map(|i| {
                let text: Vec<&str> = (0..40)
                    .map(|_| {
                        words[usize::try_from(rng.next() % words.len() as u64).unwrap()].as_str()
                    })
                    .collect();
                (from + i, text.join(" "), random_unit_vector(rng))
            })
            .collect();
        store
            .db()
            .writer()
            .exec(move |c| {
                let mut ops = Vec::new();
                c.execute("INSERT OR IGNORE INTO entities (id, domain, kind, label, taint, first_seen_ns, last_seen_ns) VALUES (1, 7, 'k', 'bulk', 0, 1, 1)", [])?;
                for (id, text, v) in rows {
                    c.execute(
                        "INSERT INTO chunks (id, entity_id, domain, text, text_hash, taint, at_ns, source, vector, embed_model) VALUES (?1, 1, 7, ?2, ?1, 0, ?3, 'bench', ?4, ?5)",
                        rusqlite::params![id, text, at_base + id, vector_to_blob(&v), MODEL_ID],
                    )?;
                    ops.push(MatrixOp::Upsert { id: u64::try_from(id).unwrap(), vector: v });
                }
                Ok(ops)
            })
            .await
            .unwrap();
    }
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 * p).ceil() as usize)
        .saturating_sub(1)
        .min(v.len() - 1)]
}

fn rss_mib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap();
    s.split_whitespace().nth(1).unwrap().parse::<f64>().unwrap() * 4096.0 / 1048576.0
}

async fn run_queries(store: &Arc<Store>, rng: &mut Rng, words: &[String], n: usize) -> Vec<f64> {
    let mut times = Vec::new();
    for _ in 0..n {
        let q: Vec<&str> = (0..6)
            .map(|_| words[usize::try_from(rng.next() % words.len() as u64).unwrap()].as_str())
            .collect();
        let t = Instant::now();
        let r = store
            .hybrid(QueryHybrid {
                text: q.join(" "),
                top_k: 5,
                min_score: 0.0,
                domains: vec![],
            })
            .await
            .unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(!r.evidence.is_empty());
    }
    times
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "measurement, run in release"]
async fn perf_hybrid_query_at_the_chunk_cap() {
    let m = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let emb: Arc<dyn Embedder> = Arc::new(
        OnnxEmbedder::load(
            &m.join("runtime/libonnxruntime.so.1.31.1"),
            &m.join("embedding/bge-small-en-v1.5.onnx"),
            &m.join("embedding/bge-small-en-v1.5.tokenizer.json"),
            2,
        )
        .expect("model files are installed"),
    );
    let dir = tempfile::tempdir().unwrap();
    let cfg = StorageConfig::default();
    let cap = i64::try_from(cfg.chunk_cap).unwrap();
    let store = Arc::new(
        Store::open(
            &StorePaths::under(dir.path()),
            cfg,
            Arc::clone(&emb),
            Health::new("c3", "bench"),
        )
        .await
        .unwrap(),
    );
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let words = vocab(&mut rng);
    let rss0 = rss_mib();
    bulk(&store, &mut rng, &words, 1, cap - 2000, 1_000).await;
    let _ = run_queries(&store, &mut rng, &words, 20).await; // warm up
    // Embedding alone (the model call for one short query).
    let mut emb_ms: Vec<f64> = (0..100)
        .map(|i| {
            let t = Instant::now();
            emb.embed(&[format!("how do I fix the build error number {i}")])
                .unwrap();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    let mut warm = run_queries(&store, &mut rng, &words, 300).await;
    // First queries after an ingest burst.
    bulk(&store, &mut rng, &words, cap - 1999, 2000, 2_000_000).await;
    let mut burst = run_queries(&store, &mut rng, &words, 30).await;
    // Concurrent with a writer that keeps committing.
    let w = Arc::clone(&store);
    let writer = tokio::spawn(async move {
        let mut r = Rng(77);
        let ws = vocab(&mut r);
        for k in 0..40_i64 {
            bulk(&w, &mut r, &ws, 10_000_000 + k * 200, 200, 3_000_000).await;
        }
    });
    let mut conc = run_queries(&store, &mut rng, &words, 200).await;
    writer.await.unwrap();
    let rss1 = rss_mib();
    let gov = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .unwrap_or_default();
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let stats = |v: &mut Vec<f64>| serde_json::json!({"n": v.len(), "p50_ms": pct(v, 0.5), "p99_ms": pct(v, 0.99), "max_ms": pct(v, 1.0)});
    let report = serde_json::json!({
        "what": "C3 hybrid query at the 30,000-chunk cap, real embedding model (bge-small-en-v1.5, 2 threads), 4-thread scan, release build",
        "conditions": {"governor": gov.trim(), "loadavg": load.split_whitespace().next(), "chunks": cap, "commit": option_env!("GIT_COMMIT").unwrap_or("see report"), "date_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()},
        "embedding_alone": stats(&mut emb_ms),
        "warm_total": stats(&mut warm),
        "first_after_burst_total": stats(&mut burst),
        "concurrent_with_writer_total": stats(&mut conc),
        "process_rss_mib": {"before_load": rss0, "after_load_and_queries": rss1},
        "targets": {"p50_ms": 25, "p99_ms": 50, "ref": "NFR-PERF-04"},
    });
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reports/bench");
    std::fs::create_dir_all(&out).unwrap();
    let file = out.join(format!(
        "p4-search-{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    ));
    std::fs::write(&file, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    let _ = Duration::ZERO;
}
