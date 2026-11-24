//! In-memory vector matrix with parallel exact dot-product scan.
//!
//! Satisfies FR-STO-05 and ADR-0004:
//! - Contiguous `Vec<f32>` matrix (rows x dim) with parallel `Vec<u64>` row ids.
//! - Exact dot-product top-k scan (L2-normalised vectors, so dot product = cosine similarity).
//! - Deterministic score descending, tie-breaking by smaller id first.
//! - Parallelised across worker threads with `std::thread::scope`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Errors returned by [`VectorIndex`] operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VectorError {
    /// Vector length does not match the index dimension.
    #[error("wrong dimension: expected {expected}, got {got}")]
    WrongDimension { expected: usize, got: usize },

    /// Vector contains non-finite values (NaN or infinity).
    #[error("non-finite value in vector")]
    NonFinite,

    /// Vector with the specified id already exists.
    #[error("duplicate id: {0}")]
    DuplicateId(u64),
}

/// Entry used in the worker top-k binary heap.
///
/// Implements `Ord` such that the "worst" entry (lowest score, or on tie, largest id)
/// is considered `Greater`. In Rust's max-heap [`BinaryHeap`], this puts the worst
/// entry at `peek()`, enabling bounded top-k eviction in O(log k) per row.
#[derive(Clone, Copy, PartialEq)]
struct HeapEntry {
    id: u64,
    score: f32,
}

impl Eq for HeapEntry {}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // ADR-0004 deterministic tie-break: smaller id is better.
        // For heap eviction, worst entry (lower score or larger id) is Greater.
        match other.score.total_cmp(&self.score) {
            Ordering::Equal => self.id.cmp(&other.id),
            ord => ord,
        }
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Contiguous in-memory vector matrix for exact dot-product retrieval.
///
/// Implements FR-STO-05 and ADR-0004: contiguous storage avoids pointer indirection,
/// cache misses, and external vector database dependencies.
#[derive(Debug, Clone)]
pub struct VectorIndex {
    dim: usize,
    ids: Vec<u64>,
    data: Vec<f32>,
}

impl VectorIndex {
    /// Creates a new vector index with the given dimension.
    ///
    /// Dim >= 1; a dimension of 0 is treated as 1.
    #[must_use]
    pub fn new(dim: usize) -> Self {
        Self {
            dim: dim.max(1),
            ids: Vec::new(),
            data: Vec::new(),
        }
    }

    /// Returns the vector dimension.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Returns the number of vectors in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Returns `true` if the index contains no vectors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Returns `true` if the index contains the specified id.
    #[must_use]
    pub fn contains(&self, id: u64) -> bool {
        self.ids.contains(&id)
    }

    /// Inserts a vector with the given id.
    ///
    /// Rejects wrong length, non-finite values, and duplicate ids.
    pub fn insert(&mut self, id: u64, vector: &[f32]) -> Result<(), VectorError> {
        self.validate_vector(vector)?;
        if self.contains(id) {
            return Err(VectorError::DuplicateId(id));
        }
        self.ids.push(id);
        self.data.extend_from_slice(vector);
        Ok(())
    }

    /// Overwrites an existing row in place (returns `true`), or inserts it (returns `false`).
    ///
    /// Rejects wrong length and non-finite values.
    pub fn replace(&mut self, id: u64, vector: &[f32]) -> Result<bool, VectorError> {
        self.validate_vector(vector)?;
        if let Some(idx) = self.ids.iter().position(|&x| x == id) {
            let start = idx.saturating_mul(self.dim);
            let end = start.saturating_add(self.dim);
            if let Some(slice) = self.data.get_mut(start..end) {
                slice.copy_from_slice(vector);
            }
            Ok(true)
        } else {
            self.ids.push(id);
            self.data.extend_from_slice(vector);
            Ok(false)
        }
    }

    /// Removes a vector by id using swap-remove.
    ///
    /// Moves the last row into the hole to keep ids and data in step.
    /// Returns `false` if the id was not found.
    pub fn remove(&mut self, id: u64) -> bool {
        let Some(idx) = self.ids.iter().position(|&x| x == id) else {
            return false;
        };
        let last_idx = self.ids.len().saturating_sub(1);
        if idx < last_idx {
            let src_start = last_idx.saturating_mul(self.dim);
            let src_end = src_start.saturating_add(self.dim);
            let dst_start = idx.saturating_mul(self.dim);
            if src_end <= self.data.len() && dst_start.saturating_add(self.dim) <= self.data.len() {
                self.data.copy_within(src_start..src_end, dst_start);
            }
        }
        let new_data_len = last_idx.saturating_mul(self.dim);
        self.data.truncate(new_data_len);
        self.ids.swap_remove(idx);
        true
    }

    /// Returns a reference to the vector slice for the given id, if present.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&[f32]> {
        let idx = self.ids.iter().position(|&x| x == id)?;
        let start = idx.saturating_mul(self.dim);
        let end = start.saturating_add(self.dim);
        self.data.get(start..end)
    }

    /// Returns estimated memory bytes (`rows * dim * 4 + ids * 8`) for health reporting.
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        let matrix_bytes = self.data.len().saturating_mul(std::mem::size_of::<f32>());
        let id_bytes = self.ids.len().saturating_mul(std::mem::size_of::<u64>());
        matrix_bytes.saturating_add(id_bytes)
    }

    /// Exact top-k dot product search (FR-STO-05, ADR-0004).
    ///
    /// Best matches first (descending score, ties broken by smaller id).
    /// Uses `threads` worker threads via `std::thread::scope`.
    #[must_use]
    pub fn top_k(&self, query: &[f32], k: usize, threads: usize) -> Vec<(u64, f32)> {
        if query.len() != self.dim || k == 0 || self.ids.is_empty() {
            return Vec::new();
        }
        if query.iter().any(|v| !v.is_finite()) {
            return Vec::new();
        }

        let effective_threads = self.calculate_effective_threads(threads);
        if effective_threads <= 1 {
            return self.scan_single_worker(query, k);
        }

        self.scan_multi_worker(query, k, effective_threads)
    }

    fn validate_vector(&self, vector: &[f32]) -> Result<(), VectorError> {
        if vector.len() != self.dim {
            return Err(VectorError::WrongDimension {
                expected: self.dim,
                got: vector.len(),
            });
        }
        if vector.iter().any(|v| !v.is_finite()) {
            return Err(VectorError::NonFinite);
        }
        Ok(())
    }

    fn calculate_effective_threads(&self, threads: usize) -> usize {
        if threads <= 1 || self.ids.len() <= 1 {
            1
        } else if self.ids.len() >= 1024 {
            threads.min(self.ids.len().div_ceil(1024)).max(1)
        } else {
            threads.min(self.ids.len())
        }
    }

    fn scan_single_worker(&self, query: &[f32], k: usize) -> Vec<(u64, f32)> {
        let mut entries = scan_chunk(&self.ids, &self.data, query, self.dim, k);
        sort_and_truncate_entries(&mut entries, k);
        entries.into_iter().map(|e| (e.id, e.score)).collect()
    }

    fn scan_multi_worker(&self, query: &[f32], k: usize, threads: usize) -> Vec<(u64, f32)> {
        let chunk_size = self.ids.len().div_ceil(threads);
        let worker_results: Vec<Vec<HeapEntry>> = std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(threads);
            for t in 0..threads {
                let start = t.saturating_mul(chunk_size);
                if start >= self.ids.len() {
                    break;
                }
                let end = (start.saturating_add(chunk_size)).min(self.ids.len());
                let start_vec = start.saturating_mul(self.dim);
                let end_vec = end.saturating_mul(self.dim);
                let Some(thread_ids) = self.ids.get(start..end) else {
                    continue;
                };
                let Some(thread_vectors) = self.data.get(start_vec..end_vec) else {
                    continue;
                };
                let dim = self.dim;
                handles
                    .push(s.spawn(move || scan_chunk(thread_ids, thread_vectors, query, dim, k)));
            }
            let mut results = Vec::with_capacity(handles.len());
            for handle in handles {
                if let Ok(res) = handle.join() {
                    results.push(res);
                }
            }
            results
        });

        let mut merged: Vec<HeapEntry> = worker_results.into_iter().flatten().collect();
        sort_and_truncate_entries(&mut merged, k);
        merged.into_iter().map(|e| (e.id, e.score)).collect()
    }
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .fold(0.0f32, |acc, (&x, &y)| acc + x * y)
}

fn scan_chunk(ids: &[u64], vectors: &[f32], query: &[f32], dim: usize, k: usize) -> Vec<HeapEntry> {
    let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::with_capacity(k.min(ids.len()));
    for (idx, &id) in ids.iter().enumerate() {
        let start = idx.saturating_mul(dim);
        let end = start.saturating_add(dim);
        let Some(row) = vectors.get(start..end) else {
            continue;
        };
        let score = dot_product(row, query);
        let entry = HeapEntry { id, score };
        if heap.len() < k {
            heap.push(entry);
        } else if heap
            .peek()
            .is_some_and(|worst| entry.cmp(worst) == Ordering::Less)
        {
            heap.pop();
            heap.push(entry);
        }
    }
    heap.into_vec()
}

fn sort_and_truncate_entries(entries: &mut Vec<HeapEntry>, k: usize) {
    entries.sort_unstable_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    entries.truncate(k);
}
