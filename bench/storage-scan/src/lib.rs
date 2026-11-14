//! Spike S-01 (story P0-S07): SQLite database with FTS5 plus an exact in-memory vector scan
//! to evaluate NFR-PERF-04 (hybrid query <= 25 ms p50, <= 50 ms p99) and C3 weight.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::{Context, Result};
use clap::Parser;
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DIM: usize = 384;
const DEFAULT_RRF_K: f64 = 60.0;
const TOP_K_CANDIDATES: usize = 50;
const FINAL_TOP_N: usize = 5;

// Fixed vocabulary for realistic text generation (~150 words)
const VOCABULARY: &[&str] = &[
    "semantic",
    "neuroos",
    "kernel",
    "storage",
    "vector",
    "sqlite",
    "query",
    "monitor",
    "focus",
    "window",
    "application",
    "latency",
    "systemd",
    "pipewire",
    "process",
    "thread",
    "database",
    "runtime",
    "desktop",
    "battery",
    "hardware",
    "terminal",
    "session",
    "compiler",
    "network",
    "browser",
    "socket",
    "security",
    "taint",
    "filter",
    "prompt",
    "audio",
    "voice",
    "inference",
    "benchmark",
    "analysis",
    "performance",
    "cache",
    "buffer",
    "event",
    "engine",
    "memory",
    "stream",
    "frame",
    "timestamp",
    "channel",
    "driver",
    "device",
    "state",
    "context",
    "history",
    "profile",
    "entity",
    "domain",
    "sensor",
    "thermal",
    "power",
    "temperature",
    "package",
    "processor",
    "scheduler",
    "pipeline",
    "activity",
    "signal",
    "command",
    "response",
    "envelope",
    "protocol",
    "transport",
    "handshake",
    "deadline",
    "cancel",
    "backoff",
    "reconnect",
    "health",
    "metrics",
    "sample",
    "counter",
    "duration",
    "frequency",
    "allocation",
    "matrix",
    "product",
    "similarity",
    "distance",
    "dimension",
    "ranking",
    "fusion",
    "reciprocal",
    "normalized",
    "batch",
    "burst",
    "transaction",
    "concurrency",
    "isolation",
    "journal",
    "write",
    "read",
    "wal",
    "checkpoint",
    "commit",
    "vacuum",
    "index",
    "scan",
    "lookup",
    "retrieval",
    "search",
    "match",
    "token",
    "word",
    "sentence",
    "text",
    "document",
    "payload",
    "header",
    "policy",
    "permission",
    "sandbox",
    "landlock",
    "seccomp",
    "namespace",
    "cgroup",
    "privilege",
    "audit",
    "ledger",
    "egress",
    "ingress",
    "spool",
];

// -----------------------------------------------------------------------------
// Seeded PRNG: XorShift64Star (no rand crate per rules)
// -----------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x853c49e6748fea9b } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn next_f32(&mut self) -> f32 {
        let u = (self.next_u64() >> 40) as u32; // 24 bits
        (u as f32 / 8388608.0) - 1.0
    }

    pub fn next_range(&mut self, min: usize, max: usize) -> usize {
        if min >= max {
            return min;
        }
        let range = max - min + 1;
        min + (self.next_u64() as usize % range)
    }
}

// -----------------------------------------------------------------------------
// Vector Math: 384 f32 L2-Normalised
// -----------------------------------------------------------------------------
pub fn generate_normalized_vector(rng: &mut XorShift64) -> [f32; DIM] {
    let mut vec = [0.0f32; DIM];
    let mut norm_sq = 0.0f32;
    for x in &mut vec {
        *x = rng.next_f32();
        norm_sq += *x * *x;
    }
    let norm = norm_sq.sqrt();
    if norm > 1e-9 {
        for x in &mut vec {
            *x /= norm;
        }
    } else {
        vec[0] = 1.0;
    }
    vec
}

pub fn vector_to_bytes(v: &[f32; DIM]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DIM * 4);
    for &val in v {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

// -----------------------------------------------------------------------------
// Synthetic Text Generation
// -----------------------------------------------------------------------------
pub fn generate_text(rng: &mut XorShift64) -> String {
    let num_sentences = rng.next_range(2, 5);
    let mut doc = String::with_capacity(num_sentences * 120);

    for s_idx in 0..num_sentences {
        let words_in_sentence = rng.next_range(8, 15);
        for w_idx in 0..words_in_sentence {
            let word_idx = rng.next_range(0, VOCABULARY.len() - 1);
            let word = VOCABULARY[word_idx];
            if w_idx == 0 {
                // Capitalize first letter
                let mut chars = word.chars();
                if let Some(first) = chars.next() {
                    doc.extend(first.to_uppercase());
                    doc.push_str(chars.as_str());
                }
            } else {
                doc.push_str(word);
            }

            if w_idx + 1 < words_in_sentence {
                doc.push(' ');
            } else {
                doc.push('.');
            }
        }
        if s_idx + 1 < num_sentences {
            doc.push(' ');
        }
    }
    doc
}

pub fn generate_query_text(rng: &mut XorShift64) -> String {
    let num_words = rng.next_range(1, 3);
    let mut q = String::new();
    for i in 0..num_words {
        let idx = rng.next_range(0, VOCABULARY.len() - 1);
        if i > 0 {
            q.push(' ');
        }
        q.push_str(VOCABULARY[idx]);
    }
    q
}

// -----------------------------------------------------------------------------
// Vector Scanning Algorithms (Single-thread & 4-thread)
// -----------------------------------------------------------------------------
pub fn scan_single_thread(
    matrix: &[f32],
    ids: &[i64],
    query: &[f32; DIM],
    top_k: usize,
) -> Vec<(f32, i64)> {
    let n = ids.len();
    assert_eq!(matrix.len(), n * DIM);
    let mut scored: Vec<(f32, i64)> = Vec::with_capacity(n);

    for (row_idx, &id) in ids.iter().enumerate() {
        let row = &matrix[row_idx * DIM..(row_idx + 1) * DIM];
        let mut dot = 0.0f32;
        for i in 0..DIM {
            dot += row[i] * query[i];
        }
        scored.push((dot, id));
    }

    if scored.len() <= top_k {
        scored.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        return scored;
    }

    scored.select_nth_unstable_by(top_k, |a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut top = scored[..top_k].to_vec();
    top.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    top
}

pub fn scan_4_threads(
    matrix: &[f32],
    ids: &[i64],
    query: &[f32; DIM],
    top_k: usize,
) -> Vec<(f32, i64)> {
    let n = ids.len();
    let num_threads = 4;
    if n <= top_k {
        return scan_single_thread(matrix, ids, query, top_k);
    }

    let chunk_size = n.div_ceil(num_threads);
    let mut thread_results: Vec<Vec<(f32, i64)>> = Vec::with_capacity(num_threads);

    std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(num_threads);
        for t in 0..num_threads {
            let start = t * chunk_size;
            if start >= n {
                break;
            }
            let end = (start + chunk_size).min(n);
            let sub_ids = &ids[start..end];
            let sub_matrix = &matrix[start * DIM..end * DIM];

            handles.push(s.spawn(move || scan_single_thread(sub_matrix, sub_ids, query, top_k)));
        }

        for h in handles {
            if let Ok(res) = h.join() {
                thread_results.push(res);
            }
        }
    });

    let mut merged: Vec<(f32, i64)> = thread_results.into_iter().flatten().collect();
    if merged.len() <= top_k {
        merged.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        return merged;
    }

    merged.select_nth_unstable_by(top_k, |a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut top = merged[..top_k].to_vec();
    top.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    top
}

// -----------------------------------------------------------------------------
// FTS5 & Hybrid Query Helpers
// -----------------------------------------------------------------------------
pub fn query_fts5(conn: &Connection, query_text: &str, limit: usize) -> Result<Vec<(f64, i64)>> {
    // Note: FTS5 MATCH with parameterised query. bm25() returns lower=better.
    let mut stmt = conn.prepare_cached(
        "SELECT rowid, bm25(chunks_fts) as rank
         FROM chunks_fts
         WHERE chunks_fts MATCH ?
         ORDER BY rank ASC
         LIMIT ?",
    )?;
    let rows = stmt.query_map(params![query_text, limit as i64], |row| {
        let id: i64 = row.get(0)?;
        let rank: f64 = row.get(1)?;
        Ok((rank, id))
    })?;

    let mut results = Vec::new();
    for r in rows {
        results.push(r?);
    }
    Ok(results)
}

pub fn fuse_rankings(
    vector_results: &[(f32, i64)],
    fts_results: &[(f64, i64)],
    k: f64,
    top_n: usize,
) -> Vec<(f64, i64)> {
    let mut score_map: HashMap<i64, f64> = HashMap::new();

    for (rank, &(_score, id)) in vector_results.iter().enumerate() {
        let rrf = 1.0 / (k + (rank + 1) as f64);
        *score_map.entry(id).or_insert(0.0) += rrf;
    }

    for (rank, &(_bm25, id)) in fts_results.iter().enumerate() {
        let rrf = 1.0 / (k + (rank + 1) as f64);
        *score_map.entry(id).or_insert(0.0) += rrf;
    }

    let mut fused: Vec<(f64, i64)> = score_map.into_iter().map(|(id, s)| (s, id)).collect();
    if fused.len() <= top_n {
        fused.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        return fused;
    }

    fused.select_nth_unstable_by(top_n, |a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut top = fused[..top_n].to_vec();
    top.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    top
}

pub fn fetch_text_and_taint(conn: &Connection, ids: &[i64]) -> Result<Vec<(i64, String, u32)>> {
    let mut results = Vec::with_capacity(ids.len());
    let mut stmt = conn.prepare_cached("SELECT text, taint FROM chunks WHERE id = ?")?;
    for &id in ids {
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let text: String = row.get(0)?;
            let taint: u32 = row.get(1)?;
            results.push((id, text, taint));
        }
    }
    Ok(results)
}

// -----------------------------------------------------------------------------
// Database Setup & Population
// -----------------------------------------------------------------------------
pub fn open_db_connection(path: &Path, read_only: bool) -> Result<Connection> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI
    };

    let conn = Connection::open_with_flags(path, flags)
        .with_context(|| format!("Open SQLite DB at {}", path.display()))?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -64000;",
    )
    .context("Configure SQLite PRAGMAs")?;

    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY,
            entity_id TEXT,
            domain INTEGER NOT NULL,
            text TEXT NOT NULL,
            taint INTEGER NOT NULL,
            t_ns INTEGER NOT NULL,
            vector BLOB NOT NULL
        );

        -- FTS5 table over chunks(text) using an external content table:
        -- Content='chunks' avoids duplicating the chunk text on disk, keeping the database
        -- compact while providing full BM25 full-text search capability.
        CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            text,
            content='chunks',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
          INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
          INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES('delete', old.id, old.text);
        END;

        CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
          INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES('delete', old.id, old.text);
          INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
        END;",
    )
    .context("Initialize schema and triggers")?;

    // Fail loudly if FTS5 is not functional
    conn.execute_batch("SELECT 1 FROM chunks_fts LIMIT 0;")
        .context("Verify FTS5 table execution")?;

    Ok(())
}

pub fn populate_chunks(
    conn: &mut Connection,
    rng: &mut XorShift64,
    start_id: i64,
    count: usize,
    batch_size: usize,
) -> Result<Duration> {
    let t0 = Instant::now();
    let mut current_id = start_id;
    let mut remaining = count;

    while remaining > 0 {
        let this_batch = remaining.min(batch_size);
        let tx = conn.transaction().context("Begin transaction")?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO chunks (id, entity_id, domain, text, taint, t_ns, vector)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;

            for _ in 0..this_batch {
                let vec = generate_normalized_vector(rng);
                let blob = vector_to_bytes(&vec);
                let text = generate_text(rng);
                let entity_id = format!("ent_{}", current_id % 500);
                let domain = (current_id % 14) + 1;
                let taint: u32 = if (current_id % 7) == 0 { 1 } else { 0 };
                let t_ns = 1770000000000000000i64 + (current_id * 1_000_000);

                stmt.execute(params![
                    current_id, entity_id, domain, text, taint, t_ns, blob
                ])?;
                current_id += 1;
            }
        }
        tx.commit().context("Commit transaction")?;
        remaining -= this_batch;
    }

    Ok(t0.elapsed())
}

pub fn load_vector_matrix(conn: &Connection) -> Result<(Vec<f32>, Vec<i64>, Duration)> {
    let t0 = Instant::now();
    let count_i64: i64 = conn.query_row("SELECT count(*) FROM chunks", [], |row| row.get(0))?;
    let count = usize::try_from(count_i64).context("count as usize")?;

    let mut matrix = Vec::with_capacity(count * DIM);
    let mut ids = Vec::with_capacity(count);

    let mut stmt = conn.prepare("SELECT id, vector FROM chunks ORDER BY id ASC")?;
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        ids.push(id);
        for chunk in blob.as_chunks::<4>().0 {
            matrix.push(f32::from_le_bytes(*chunk));
        }
    }

    Ok((matrix, ids, t0.elapsed()))
}

// -----------------------------------------------------------------------------
// Memory (RSS) Measurement from /proc/self/status
// -----------------------------------------------------------------------------
pub fn read_memory_status() -> (u64, u64) {
    let mut rss_kb = 0;
    let mut hwm_kb = 0;
    if let Ok(content) = fs::read_to_string("/proc/self/status") {
        for line in content.lines() {
            if line.starts_with("VmRSS:")
                && let Some(val) = line.split_whitespace().nth(1)
            {
                rss_kb = val.parse::<u64>().unwrap_or(0);
            } else if line.starts_with("VmHWM:")
                && let Some(val) = line.split_whitespace().nth(1)
            {
                hwm_kb = val.parse::<u64>().unwrap_or(0);
            }
        }
    }
    (rss_kb, hwm_kb)
}

// -----------------------------------------------------------------------------
// Benchmark Percentile Statistics
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyStats {
    pub count: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub min_ms: f64,
    pub mean_ms: f64,
}

impl LatencyStats {
    pub fn compute(mut values_ms: Vec<f64>) -> Self {
        let count = values_ms.len();
        if count == 0 {
            return Self {
                count: 0,
                p50_ms: 0.0,
                p95_ms: 0.0,
                p99_ms: 0.0,
                max_ms: 0.0,
                min_ms: 0.0,
                mean_ms: 0.0,
            };
        }

        values_ms.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let sum: f64 = values_ms.iter().sum();
        let mean_ms = sum / (count as f64);
        let min_ms = values_ms[0];
        let max_ms = values_ms[count - 1];

        let p50_idx = ((count as f64) * 0.50).floor() as usize;
        let p95_idx = ((count as f64) * 0.95).floor() as usize;
        let p99_idx = ((count as f64) * 0.99).floor() as usize;

        Self {
            count,
            p50_ms: values_ms[p50_idx.min(count - 1)],
            p95_ms: values_ms[p95_idx.min(count - 1)],
            p99_ms: values_ms[p99_idx.min(count - 1)],
            max_ms,
            min_ms,
            mean_ms,
        }
    }
}

// -----------------------------------------------------------------------------
// Report Data Structures
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPartBreakdown {
    pub scan_1t: LatencyStats,
    pub scan_4t: LatencyStats,
    pub fts: LatencyStats,
    pub fuse: LatencyStats,
    pub fetch: LatencyStats,
    pub total: LatencyStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkScaleResult {
    pub n_chunks: usize,
    pub build_time_secs: f64,
    pub db_size_bytes: u64,
    pub db_wal_size_bytes: u64,
    pub load_time_ms: f64,
    pub matrix_memory_bytes: usize,
    pub cold_first_10_queries_ms: Vec<f64>,
    pub warm_query_stats: QueryPartBreakdown,
    pub post_burst_query_stats: Option<LatencyStats>,
    pub concurrency_query_stats: Option<LatencyStats>,
    pub memory_rss_after_load_mib: f64,
    pub memory_hwm_after_load_mib: f64,
    pub memory_rss_after_queries_mib: f64,
    pub memory_hwm_after_queries_mib: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub spike: String,
    pub story: String,
    pub fts_table_design: String,
    pub fts_table_rationale: String,
    pub inject_sleep_ms: u64,
    pub is_negative_control: bool,
    pub negative_control_passed: bool,
    pub target_nfr_perf_04_p50_ms: f64,
    pub target_nfr_perf_04_p99_ms: f64,
    pub c3_rss_budget_mib: f64,
    pub c3_hard_limit_mib: f64,
    pub results_by_scale: Vec<ChunkScaleResult>,
}

// -----------------------------------------------------------------------------
// CLI Options
// -----------------------------------------------------------------------------
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Semantic-NeuroOS Spike S-01 Storage Scan Benchmark"
)]
struct Cli {
    #[arg(long, default_value = "")]
    db_path: String,

    #[arg(long, default_value = "30000")]
    n_chunks: usize,

    #[arg(long, default_value = "false")]
    all_scales: bool,

    #[arg(long, default_value = "1000")]
    queries: usize,

    #[arg(long, default_value = "0")]
    inject_sleep_ms: u64,

    #[arg(long, default_value = "")]
    output_json: String,
}

// -----------------------------------------------------------------------------
// Query Runner
// -----------------------------------------------------------------------------
struct QueryBatch<'a> {
    vectors: &'a [[f32; DIM]],
    texts: &'a [String],
    inject_sleep_ms: u64,
}

fn execute_query_run(
    conn: &Connection,
    matrix: &[f32],
    ids: &[i64],
    batch: QueryBatch<'_>,
) -> Result<QueryPartBreakdown> {
    let n = batch.vectors.len();
    assert_eq!(n, batch.texts.len());

    let mut scan_1t_lats = Vec::with_capacity(n);
    let mut scan_4t_lats = Vec::with_capacity(n);
    let mut fts_lats = Vec::with_capacity(n);
    let mut fuse_lats = Vec::with_capacity(n);
    let mut fetch_lats = Vec::with_capacity(n);
    let mut total_lats = Vec::with_capacity(n);

    for i in 0..n {
        let q_vec = &batch.vectors[i];
        let q_text = &batch.texts[i];

        let t_total_start = Instant::now();

        if batch.inject_sleep_ms > 0 {
            std::thread::sleep(Duration::from_millis(batch.inject_sleep_ms));
        }

        // 1. Vector scan (1 thread)
        let t_scan_start = Instant::now();
        let vec_top50 = scan_single_thread(matrix, ids, q_vec, TOP_K_CANDIDATES);
        let scan_1t_ms = t_scan_start.elapsed().as_secs_f64() * 1000.0;
        scan_1t_lats.push(scan_1t_ms);

        // Vector scan (4 threads) variant for reporting
        let t_scan_4t = Instant::now();
        let _ = scan_4_threads(matrix, ids, q_vec, TOP_K_CANDIDATES);
        scan_4t_lats.push(t_scan_4t.elapsed().as_secs_f64() * 1000.0);

        // 2. FTS5 MATCH with bm25()
        let t_fts_start = Instant::now();
        let fts_top50 = query_fts5(conn, q_text, TOP_K_CANDIDATES)?;
        let fts_ms = t_fts_start.elapsed().as_secs_f64() * 1000.0;
        fts_lats.push(fts_ms);

        // 3. Reciprocal Rank Fusion (k=60) -> top-5
        let t_fuse_start = Instant::now();
        let fused_top5 = fuse_rankings(&vec_top50, &fts_top50, DEFAULT_RRF_K, FINAL_TOP_N);
        let fuse_ms = t_fuse_start.elapsed().as_secs_f64() * 1000.0;
        fuse_lats.push(fuse_ms);

        // 4. Fetch those 5 rows' text+taint from SQLite
        let t_fetch_start = Instant::now();
        let top_ids: Vec<i64> = fused_top5.iter().map(|&(_score, id)| id).collect();
        let _ = fetch_text_and_taint(conn, &top_ids)?;
        let fetch_ms = t_fetch_start.elapsed().as_secs_f64() * 1000.0;
        fetch_lats.push(fetch_ms);

        let total_ms = t_total_start.elapsed().as_secs_f64() * 1000.0;
        total_lats.push(total_ms);
    }

    Ok(QueryPartBreakdown {
        scan_1t: LatencyStats::compute(scan_1t_lats),
        scan_4t: LatencyStats::compute(scan_4t_lats),
        fts: LatencyStats::compute(fts_lats),
        fuse: LatencyStats::compute(fuse_lats),
        fetch: LatencyStats::compute(fetch_lats),
        total: LatencyStats::compute(total_lats),
    })
}

// -----------------------------------------------------------------------------
// Scale Benchmark Runner (Runs a specific N)
// -----------------------------------------------------------------------------
fn benchmark_scale(
    db_path: &Path,
    n_chunks: usize,
    num_queries: usize,
    inject_sleep_ms: u64,
    run_burst_and_concurrency: bool,
) -> Result<ChunkScaleResult> {
    if db_path.exists() {
        let _ = fs::remove_file(db_path);
        let _ = fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = fs::remove_file(format!("{}-shm", db_path.display()));
    }

    println!("\n=== Benchmarking Scale N = {} chunks ===", n_chunks);
    let mut writer_conn = open_db_connection(db_path, false)?;
    init_schema(&writer_conn)?;

    let mut rng = XorShift64::new(42 + n_chunks as u64);

    // 1. Build database
    print!("Building database with {} chunks... ", n_chunks);
    std::io::stdout().flush()?;
    let build_time = populate_chunks(&mut writer_conn, &mut rng, 1, n_chunks, 1000)?;
    println!("{:.2} s", build_time.as_secs_f64());

    let db_size_bytes = fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let db_wal_size_bytes = fs::metadata(format!("{}-wal", db_path.display()))
        .map(|m| m.len())
        .unwrap_or(0);

    // 2. Open reader connection and load vector matrix
    print!("Loading vectors into contiguous memory matrix... ");
    std::io::stdout().flush()?;
    let reader_conn = open_db_connection(db_path, true)?;
    let (mut matrix, mut ids, load_time) = load_vector_matrix(&reader_conn)?;
    let matrix_memory_bytes = matrix.len() * std::mem::size_of::<f32>();
    println!(
        "{:.2} ms ({:.2} MiB, {} rows)",
        load_time.as_secs_f64() * 1000.0,
        matrix_memory_bytes as f64 / (1024.0 * 1024.0),
        ids.len()
    );

    let (rss_load_kb, hwm_load_kb) = read_memory_status();

    // 3. Pre-generate seeded query set
    let mut query_rng = XorShift64::new(1337);
    let total_query_count = num_queries + 100 + 10; // warm + warmup + cold
    let mut all_q_vecs = Vec::with_capacity(total_query_count);
    let mut all_q_texts = Vec::with_capacity(total_query_count);
    for _ in 0..total_query_count {
        all_q_vecs.push(generate_normalized_vector(&mut query_rng));
        all_q_texts.push(generate_query_text(&mut query_rng));
    }

    // 4. Case: Cold queries (first 10 queries right after opening DB and loading)
    println!("Measuring first 10 cold queries...");
    let mut cold_first_10_queries_ms = Vec::with_capacity(10);
    for i in 0..10 {
        let q_vec = &all_q_vecs[i];
        let q_text = &all_q_texts[i];

        let t0 = Instant::now();
        let vec_top50 = scan_single_thread(&matrix, &ids, q_vec, TOP_K_CANDIDATES);
        let fts_top50 = query_fts5(&reader_conn, q_text, TOP_K_CANDIDATES)?;
        let fused_top5 = fuse_rankings(&vec_top50, &fts_top50, DEFAULT_RRF_K, FINAL_TOP_N);
        let top_ids: Vec<i64> = fused_top5.iter().map(|&(_s, id)| id).collect();
        let _ = fetch_text_and_taint(&reader_conn, &top_ids)?;
        let dur_ms = t0.elapsed().as_secs_f64() * 1000.0;
        cold_first_10_queries_ms.push(dur_ms);
        println!("  Cold query {:2}: {:.3} ms", i + 1, dur_ms);
    }

    // 5. Case: Warm-up (100 queries)
    print!("Executing 100 warm-up queries... ");
    std::io::stdout().flush()?;
    for i in 10..110 {
        let q_vec = &all_q_vecs[i];
        let q_text = &all_q_texts[i];
        let vec_top50 = scan_single_thread(&matrix, &ids, q_vec, TOP_K_CANDIDATES);
        let fts_top50 = query_fts5(&reader_conn, q_text, TOP_K_CANDIDATES)?;
        let fused_top5 = fuse_rankings(&vec_top50, &fts_top50, DEFAULT_RRF_K, FINAL_TOP_N);
        let top_ids: Vec<i64> = fused_top5.iter().map(|&(_s, id)| id).collect();
        let _ = fetch_text_and_taint(&reader_conn, &top_ids)?;
    }
    println!("done.");

    // 6. Case: Warm benchmark (1,000 queries)
    println!(
        "Executing {} warm queries (NFR-PERF-04 evaluation)...",
        num_queries
    );
    let warm_vecs = &all_q_vecs[110..110 + num_queries];
    let warm_texts = &all_q_texts[110..110 + num_queries];
    let warm_stats = execute_query_run(
        &reader_conn,
        &matrix,
        &ids,
        QueryBatch {
            vectors: warm_vecs,
            texts: warm_texts,
            inject_sleep_ms,
        },
    )?;

    println!(
        "  Warm Total Latency: p50 = {:.3} ms, p95 = {:.3} ms, p99 = {:.3} ms, max = {:.3} ms",
        warm_stats.total.p50_ms,
        warm_stats.total.p95_ms,
        warm_stats.total.p99_ms,
        warm_stats.total.max_ms
    );
    println!(
        "    Breakdown (p50): scan 1T = {:.3} ms (4T = {:.3} ms), fts = {:.3} ms, fuse = {:.3} ms, fetch = {:.3} ms",
        warm_stats.scan_1t.p50_ms,
        warm_stats.scan_4t.p50_ms,
        warm_stats.fts.p50_ms,
        warm_stats.fuse.p50_ms,
        warm_stats.fetch.p50_ms
    );
    println!(
        "    Breakdown (p99): scan 1T = {:.3} ms (4T = {:.3} ms), fts = {:.3} ms, fuse = {:.3} ms, fetch = {:.3} ms",
        warm_stats.scan_1t.p99_ms,
        warm_stats.scan_4t.p99_ms,
        warm_stats.fts.p99_ms,
        warm_stats.fuse.p99_ms,
        warm_stats.fetch.p99_ms
    );

    let mut post_burst_stats = None;
    let mut concurrency_stats = None;

    if run_burst_and_concurrency {
        // 7. Case: Post-Burst (Insert 2,000 chunks burst, then immediately query 100 times)
        println!("\nExecuting post-burst workload (insert 2,000 chunks, then 100 queries)...");
        let burst_count = 2000;
        let start_burst_id = (n_chunks + 1) as i64;
        let t_burst = populate_chunks(
            &mut writer_conn,
            &mut rng,
            start_burst_id,
            burst_count,
            50, // transactions of 50 chunks
        )?;
        println!(
            "  Inserted 2,000 burst chunks in {:.2} s",
            t_burst.as_secs_f64()
        );

        // Update in-memory matrix with burst chunks
        let (burst_matrix, burst_ids, _) = load_vector_matrix(&reader_conn)?;
        matrix = burst_matrix;
        ids = burst_ids;

        // Immediately run 100 queries from reader connection
        let mut burst_query_lats = Vec::with_capacity(100);
        for i in 0..100 {
            let q_vec = &all_q_vecs[i % all_q_vecs.len()];
            let q_text = &all_q_texts[i % all_q_texts.len()];
            let t0 = Instant::now();
            let vec_top50 = scan_single_thread(&matrix, &ids, q_vec, TOP_K_CANDIDATES);
            let fts_top50 = query_fts5(&reader_conn, q_text, TOP_K_CANDIDATES)?;
            let fused_top5 = fuse_rankings(&vec_top50, &fts_top50, DEFAULT_RRF_K, FINAL_TOP_N);
            let top_ids: Vec<i64> = fused_top5.iter().map(|&(_s, id)| id).collect();
            let _ = fetch_text_and_taint(&reader_conn, &top_ids)?;
            burst_query_lats.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let pb_stat = LatencyStats::compute(burst_query_lats);
        println!(
            "  Post-Burst Latency (100 queries): p50 = {:.3} ms, p95 = {:.3} ms, p99 = {:.3} ms, max = {:.3} ms",
            pb_stat.p50_ms, pb_stat.p95_ms, pb_stat.p99_ms, pb_stat.max_ms
        );
        post_burst_stats = Some(pb_stat);

        // 8. Case: Concurrency Check (Reader queries while writer continuously inserts 50-chunk txs)
        println!(
            "\nExecuting concurrency workload (1,000 queries while writer inserts concurrently)..."
        );
        let keep_writing = Arc::new(AtomicBool::new(true));
        let keep_writing_clone = keep_writing.clone();
        let db_path_clone = db_path.to_path_buf();

        let writer_handle = std::thread::spawn(move || {
            let mut w_conn = match open_db_connection(&db_path_clone, false) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Background writer failed to open DB: {}", e);
                    return;
                }
            };
            let mut w_rng = XorShift64::new(9999);
            let mut curr_id = 1_000_000i64;

            while keep_writing_clone.load(Ordering::Relaxed) {
                if let Err(e) = populate_chunks(&mut w_conn, &mut w_rng, curr_id, 50, 50) {
                    eprintln!("Background write error: {}", e);
                    break;
                }
                curr_id += 50;
                std::thread::sleep(Duration::from_millis(2));
            }
        });

        // Run 1,000 queries on reader connection while writer runs
        let mut conc_query_lats = Vec::with_capacity(num_queries);
        for i in 0..num_queries {
            let q_vec = &all_q_vecs[i % all_q_vecs.len()];
            let q_text = &all_q_texts[i % all_q_texts.len()];
            let t0 = Instant::now();
            let vec_top50 = scan_single_thread(&matrix, &ids, q_vec, TOP_K_CANDIDATES);
            let fts_top50 = query_fts5(&reader_conn, q_text, TOP_K_CANDIDATES)?;
            let fused_top5 = fuse_rankings(&vec_top50, &fts_top50, DEFAULT_RRF_K, FINAL_TOP_N);
            let top_ids: Vec<i64> = fused_top5.iter().map(|&(_s, id)| id).collect();
            let _ = fetch_text_and_taint(&reader_conn, &top_ids)?;
            conc_query_lats.push(t0.elapsed().as_secs_f64() * 1000.0);
        }

        keep_writing.store(false, Ordering::Relaxed);
        let _ = writer_handle.join();

        let conc_stat = LatencyStats::compute(conc_query_lats);
        println!(
            "  Concurrent Query Latency (1,000 queries): p50 = {:.3} ms, p95 = {:.3} ms, p99 = {:.3} ms, max = {:.3} ms",
            conc_stat.p50_ms, conc_stat.p95_ms, conc_stat.p99_ms, conc_stat.max_ms
        );
        concurrency_stats = Some(conc_stat);
    }

    let (rss_queries_kb, hwm_queries_kb) = read_memory_status();

    Ok(ChunkScaleResult {
        n_chunks,
        build_time_secs: build_time.as_secs_f64(),
        db_size_bytes,
        db_wal_size_bytes,
        load_time_ms: load_time.as_secs_f64() * 1000.0,
        matrix_memory_bytes,
        cold_first_10_queries_ms,
        warm_query_stats: warm_stats,
        post_burst_query_stats: post_burst_stats,
        concurrency_query_stats: concurrency_stats,
        memory_rss_after_load_mib: rss_load_kb as f64 / 1024.0,
        memory_hwm_after_load_mib: hwm_load_kb as f64 / 1024.0,
        memory_rss_after_queries_mib: rss_queries_kb as f64 / 1024.0,
        memory_hwm_after_queries_mib: hwm_queries_kb as f64 / 1024.0,
    })
}

// -----------------------------------------------------------------------------
// Runner Entry Point
// -----------------------------------------------------------------------------
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    println!("============================================================");
    println!("Semantic-NeuroOS Spike S-01: Storage Scan & Hybrid Query");
    println!("Story: P0-S07 | Target NFR-PERF-04 (p50 <= 25 ms, p99 <= 50 ms)");
    println!("============================================================");

    let temp_dir;
    let db_path = if !cli.db_path.is_empty() {
        PathBuf::from(&cli.db_path)
    } else {
        temp_dir = tempfile::Builder::new()
            .prefix("neuroos-s01-")
            .tempdir()
            .context("Create temp dir for benchmark database")?;
        temp_dir.path().join("bench_storage.db")
    };

    println!("Database path: {}", db_path.display());

    let scales = if cli.all_scales {
        vec![5000, 15000, 30000]
    } else {
        vec![cli.n_chunks]
    };

    let mut scale_results = Vec::new();
    let is_negative_control = cli.inject_sleep_ms > 0;

    for &n in &scales {
        let is_primary_cap = n == 30000;
        let res = benchmark_scale(
            &db_path,
            n,
            cli.queries,
            cli.inject_sleep_ms,
            is_primary_cap && !is_negative_control,
        )?;
        scale_results.push(res);
    }

    // Cleanup DB files
    if db_path.exists() {
        let _ = fs::remove_file(&db_path);
        let _ = fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = fs::remove_file(format!("{}-shm", db_path.display()));
    }

    let primary_30k = scale_results.iter().find(|r| r.n_chunks == 30000);
    let neg_control_passed = if is_negative_control {
        if let Some(r30) = primary_30k {
            r30.warm_query_stats.total.p50_ms > 25.0
        } else if let Some(r_first) = scale_results.first() {
            r_first.warm_query_stats.total.p50_ms > 25.0
        } else {
            false
        }
    } else {
        false
    };

    let report = BenchmarkReport {
        spike: "S-01".to_string(),
        story: "P0-S07".to_string(),
        fts_table_design: "FTS5 external content table (content='chunks', content_rowid='id')".to_string(),
        fts_table_rationale: "External content table references chunks(text) directly without duplicating text storage on disk, minimizing database size while enabling standard BM25 ranking.".to_string(),
        inject_sleep_ms: cli.inject_sleep_ms,
        is_negative_control,
        negative_control_passed: neg_control_passed,
        target_nfr_perf_04_p50_ms: 25.0,
        target_nfr_perf_04_p99_ms: 50.0,
        c3_rss_budget_mib: 240.0,
        c3_hard_limit_mib: 320.0,
        results_by_scale: scale_results,
    };

    let json_output = serde_json::to_string_pretty(&report).context("Serialize JSON report")?;

    if !cli.output_json.is_empty() {
        let p = Path::new(&cli.output_json);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = File::create(p)?;
        f.write_all(json_output.as_bytes())?;
        println!("\nSaved benchmark report JSON to {}", p.display());
    } else {
        println!("\n=== Benchmark JSON Output ===");
        println!("{}", json_output);
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Unit Tests (deciding correctness per prompt requirements)
// -----------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalisation() {
        let mut rng = XorShift64::new(12345);
        for _ in 0..50 {
            let v = generate_normalized_vector(&mut rng);
            let mut sum_sq = 0.0f32;
            for x in v {
                sum_sq += x * x;
            }
            assert!(
                (sum_sq - 1.0).abs() < 1e-5,
                "Vector L2 norm must be 1.0, got sqrt({}) = {}",
                sum_sq,
                sum_sq.sqrt()
            );
        }
    }

    #[test]
    fn test_top_k_selection() {
        let mut matrix = Vec::new();
        let ids: Vec<i64> = (1..=10).collect();

        // 10 dummy vectors of 384 dimensions
        for i in 1..=10 {
            let mut row = [0.0f32; DIM];
            row[0] = i as f32 * 0.1; // dot products with [1.0, 0...] will be 0.1, 0.2, ... 1.0
            matrix.extend_from_slice(&row);
        }

        let mut query = [0.0f32; DIM];
        query[0] = 1.0;

        let top3 = scan_single_thread(&matrix, &ids, &query, 3);
        assert_eq!(top3.len(), 3);
        assert_eq!(top3[0].1, 10); // highest dot product = 1.0
        assert_eq!(top3[1].1, 9); // second = 0.9
        assert_eq!(top3[2].1, 8); // third = 0.8
    }

    #[test]
    fn test_exact_nearest_neighbour_handcrafted() {
        let mut matrix = Vec::new();
        let ids = vec![101, 102, 103, 104, 105];

        let mut v1 = [0.0f32; DIM];
        v1[0] = 1.0;
        let mut v2 = [0.0f32; DIM];
        v2[1] = 1.0;
        let mut v3 = [0.0f32; DIM];
        v3[2] = 1.0; // target match
        let mut v4 = [0.0f32; DIM];
        v4[3] = 1.0;
        let mut v5 = [0.0f32; DIM];
        v5[4] = 1.0;

        matrix.extend_from_slice(&v1);
        matrix.extend_from_slice(&v2);
        matrix.extend_from_slice(&v3);
        matrix.extend_from_slice(&v4);
        matrix.extend_from_slice(&v5);

        let mut query = [0.0f32; DIM];
        query[2] = 0.99;
        query[0] = 0.05;

        // Normalize query
        let norm = (0.99f32 * 0.99 + 0.05 * 0.05).sqrt();
        query[2] /= norm;
        query[0] /= norm;

        let res = scan_single_thread(&matrix, &ids, &query, 1);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, 103, "Exact nearest neighbour must be ID 103");
        assert!(
            (res[0].0 - (0.99 / norm)).abs() < 1e-5,
            "Dot product must match expected cosine similarity"
        );
    }

    #[test]
    fn test_rrf_fusion_ordering() {
        // Doc 10: rank 1 in vector, rank 1 in FTS -> RRF = 1/61 + 1/61 = 2/61 ≈ 0.03278
        // Doc 20: rank 1 in vector only -> RRF = 1/61 ≈ 0.01639
        // Doc 30: rank 2 in vector, rank 2 in FTS -> RRF = 1/62 + 1/62 = 2/62 ≈ 0.03225
        // Expected ordering: Doc 10 > Doc 30 > Doc 20
        let vec_res = vec![(0.95, 10), (0.90, 30), (0.85, 20)];
        let fts_res = vec![(-1.2, 10), (-0.8, 30)];

        let fused = fuse_rankings(&vec_res, &fts_res, 60.0, 5);
        assert_eq!(fused.len(), 3);
        assert_eq!(fused[0].1, 10, "Doc 10 (top in both) must rank first");
        assert_eq!(fused[1].1, 30, "Doc 30 (second in both) must rank second");
        assert_eq!(fused[2].1, 20, "Doc 20 (single appearance) must rank third");

        let score10 = 1.0 / 61.0 + 1.0 / 61.0;
        let score30 = 1.0 / 62.0 + 1.0 / 62.0;
        let score20 = 1.0 / 63.0;
        assert!((fused[0].0 - score10).abs() < 1e-6);
        assert!((fused[1].0 - score30).abs() < 1e-6);
        assert!((fused[2].0 - score20).abs() < 1e-6);
    }

    #[test]
    fn test_fts5_bundled_support() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;

        conn.execute(
            "INSERT INTO chunks (id, entity_id, domain, text, taint, t_ns, vector)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                1i64,
                "ent_1",
                1,
                "Semantic neuroos sqlite benchmark",
                0,
                100i64,
                vec![0u8; DIM * 4]
            ],
        )?;

        let hits = query_fts5(&conn, "semantic sqlite", 10)?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, 1);
        Ok(())
    }
}
