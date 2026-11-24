//! Tests for `octoplus_storage::vectors::VectorIndex`.
//!
//! Satisfies FR-STO-05, ADR-0004, rules.md §6, §7, §8:
//! (1) Table cases for insert/replace/remove/get/contains/len including every error.
//! (2) Swap-remove keeps every other row retrievable and correct.
//! (3) Proptest: top_k with threads 1, 2, 4 equals naive reference.
//! (4) Proptest: random sequence of inserts/removes/replaces equals HashMap model.
//! (5) Wrong-length, NaN, inf, and k=0 queries return empty without panicking.
//! (6) Negative control: deliberately wrong scans fail reference equality.

use octoplus_storage::vectors::{VectorError, VectorIndex};
use proptest::prelude::*;
use std::collections::HashMap;

// Naive reference implementation of exact dot-product scan.
// Computes every dot product, sorts by score descending with tie-break
// by smaller id first, and takes k.
fn naive_reference(
    dim: usize,
    ids: &[u64],
    data: &[f32],
    query: &[f32],
    k: usize,
) -> Vec<(u64, f32)> {
    if query.len() != dim || k == 0 || ids.is_empty() || query.iter().any(|v| !v.is_finite()) {
        return Vec::new();
    }
    let mut scores = Vec::with_capacity(ids.len());
    for (idx, &id) in ids.iter().enumerate() {
        let row = &data[idx * dim..(idx + 1) * dim];
        let dot = row
            .iter()
            .zip(query.iter())
            .map(|(a, b)| a * b)
            .sum::<f32>();
        scores.push((id, dot));
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scores.truncate(k);
    scores
}

// -----------------------------------------------------------------------------
// (1) Table cases: insert/replace/remove/get/contains/len including every error
// -----------------------------------------------------------------------------

#[test]
fn test_dim_initialization() {
    let index_0 = VectorIndex::new(0);
    assert_eq!(index_0.dim(), 1);

    let index_4 = VectorIndex::new(4);
    assert_eq!(index_4.dim(), 4);
}

#[test]
fn test_empty_index_properties() {
    let index = VectorIndex::new(3);
    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
    assert!(!index.contains(1));
    assert_eq!(index.get(1), None);
    assert_eq!(index.memory_bytes(), 0);
    assert!(index.top_k(&[0.1, 0.2, 0.3], 5, 1).is_empty());
}

#[test]
fn test_insert_dimension_and_finite_errors() {
    let mut index = VectorIndex::new(3);

    // Wrong dimension: too short
    let err_short = index.insert(1, &[1.0, 2.0]);
    assert_eq!(
        err_short,
        Err(VectorError::WrongDimension {
            expected: 3,
            got: 2
        })
    );

    // Wrong dimension: too long
    let err_long = index.insert(2, &[1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        err_long,
        Err(VectorError::WrongDimension {
            expected: 3,
            got: 4
        })
    );

    // Non-finite: NaN
    let err_nan = index.insert(3, &[1.0, f32::NAN, 3.0]);
    assert_eq!(err_nan, Err(VectorError::NonFinite));

    // Non-finite: Infinity
    let err_inf = index.insert(4, &[1.0, f32::INFINITY, 3.0]);
    assert_eq!(err_inf, Err(VectorError::NonFinite));

    // Non-finite: Negative Infinity
    let err_neginf = index.insert(5, &[1.0, f32::NEG_INFINITY, 3.0]);
    assert_eq!(err_neginf, Err(VectorError::NonFinite));

    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn test_insert_success_and_duplicate_error() {
    let mut index = VectorIndex::new(2);

    assert_eq!(index.insert(100, &[0.5, 0.5]), Ok(()));
    assert_eq!(index.len(), 1);
    assert!(!index.is_empty());
    assert!(index.contains(100));
    assert_eq!(index.get(100), Some(&[0.5, 0.5][..]));

    // Duplicate ID error
    assert_eq!(
        index.insert(100, &[0.1, 0.9]),
        Err(VectorError::DuplicateId(100))
    );
    // Value remains unchanged
    assert_eq!(index.get(100), Some(&[0.5, 0.5][..]));
    assert_eq!(index.len(), 1);
}

#[test]
fn test_replace_behavior() {
    let mut index = VectorIndex::new(2);

    // Replace rejects wrong dimension
    assert_eq!(
        index.replace(1, &[1.0]),
        Err(VectorError::WrongDimension {
            expected: 2,
            got: 1
        })
    );

    // Replace rejects non-finite
    assert_eq!(
        index.replace(1, &[f32::NAN, 1.0]),
        Err(VectorError::NonFinite)
    );

    // Replace absent row: inserts and returns false
    assert_eq!(index.replace(10, &[0.2, 0.8]), Ok(false));
    assert_eq!(index.len(), 1);
    assert!(index.contains(10));
    assert_eq!(index.get(10), Some(&[0.2, 0.8][..]));

    // Replace existing row: overwrites in place and returns true
    assert_eq!(index.replace(10, &[0.6, 0.4]), Ok(true));
    assert_eq!(index.len(), 1);
    assert!(index.contains(10));
    assert_eq!(index.get(10), Some(&[0.6, 0.4][..]));
}

#[test]
fn test_remove_non_existent() {
    let mut index = VectorIndex::new(2);
    assert!(!index.remove(42));
    assert_eq!(index.len(), 0);
}

#[test]
fn test_memory_bytes_reporting() {
    let mut index = VectorIndex::new(3);
    assert_eq!(index.memory_bytes(), 0);

    index.insert(1, &[0.1, 0.2, 0.3]).unwrap();
    // 1 row * 3 dim * 4 bytes + 1 row * 8 bytes = 20 bytes
    assert_eq!(index.memory_bytes(), 20);

    index.insert(2, &[0.4, 0.5, 0.6]).unwrap();
    // 2 rows * 3 dim * 4 bytes + 2 rows * 8 bytes = 40 bytes
    assert_eq!(index.memory_bytes(), 40);

    index.remove(1);
    assert_eq!(index.memory_bytes(), 20);
}

// -----------------------------------------------------------------------------
// (2) Remove keeps every other row retrievable and correct (first, middle, last, only)
// -----------------------------------------------------------------------------

#[test]
fn test_remove_only_row() {
    let mut index = VectorIndex::new(2);
    index.insert(42, &[1.0, 2.0]).unwrap();
    assert_eq!(index.len(), 1);

    assert!(index.remove(42));
    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
    assert!(!index.contains(42));
    assert_eq!(index.get(42), None);
}

#[test]
fn test_remove_first_row() {
    let mut index = VectorIndex::new(2);
    index.insert(1, &[1.0, 1.1]).unwrap();
    index.insert(2, &[2.0, 2.1]).unwrap();
    index.insert(3, &[3.0, 3.1]).unwrap();

    assert!(index.remove(1));
    assert_eq!(index.len(), 2);
    assert!(!index.contains(1));
    assert_eq!(index.get(1), None);

    assert!(index.contains(2));
    assert_eq!(index.get(2), Some(&[2.0, 2.1][..]));

    assert!(index.contains(3));
    assert_eq!(index.get(3), Some(&[3.0, 3.1][..]));
}

#[test]
fn test_remove_middle_row() {
    let mut index = VectorIndex::new(2);
    index.insert(1, &[1.0, 1.1]).unwrap();
    index.insert(2, &[2.0, 2.1]).unwrap();
    index.insert(3, &[3.0, 3.1]).unwrap();
    index.insert(4, &[4.0, 4.1]).unwrap();
    index.insert(5, &[5.0, 5.1]).unwrap();

    assert!(index.remove(3));
    assert_eq!(index.len(), 4);
    assert!(!index.contains(3));
    assert_eq!(index.get(3), None);

    assert_eq!(index.get(1), Some(&[1.0, 1.1][..]));
    assert_eq!(index.get(2), Some(&[2.0, 2.1][..]));
    assert_eq!(index.get(4), Some(&[4.0, 4.1][..]));
    assert_eq!(index.get(5), Some(&[5.0, 5.1][..]));
}

#[test]
fn test_remove_last_row() {
    let mut index = VectorIndex::new(2);
    index.insert(1, &[1.0, 1.1]).unwrap();
    index.insert(2, &[2.0, 2.1]).unwrap();
    index.insert(3, &[3.0, 3.1]).unwrap();

    assert!(index.remove(3));
    assert_eq!(index.len(), 2);
    assert!(!index.contains(3));
    assert_eq!(index.get(3), None);

    assert_eq!(index.get(1), Some(&[1.0, 1.1][..]));
    assert_eq!(index.get(2), Some(&[2.0, 2.1][..]));
}

// -----------------------------------------------------------------------------
// (3) Proptest: top_k threads 1, 2, 4 equal naive reference
// -----------------------------------------------------------------------------

prop_compose! {
    fn arb_vector_scenario()(dim in 1usize..=16)
        (
            dim in Just(dim),
            rows in proptest::collection::vec(
                (any::<u64>(), proptest::collection::vec(-1.0f32..=1.0f32, dim..=dim)),
                0..=200
            ),
            query in proptest::collection::vec(-1.0f32..=1.0f32, dim..=dim),
            k in 0usize..=250,
        ) -> (usize, Vec<(u64, Vec<f32>)>, Vec<f32>, usize) {
        (dim, rows, query, k)
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]
    #[test]
    fn proptest_top_k_matches_reference((dim, raw_rows, query, k) in arb_vector_scenario()) {
        let mut index = VectorIndex::new(dim);
        let mut seen = std::collections::HashSet::new();
        let mut unique_ids = Vec::new();
        let mut unique_data = Vec::new();

        for (id, vec) in raw_rows {
            if seen.insert(id) {
                index.insert(id, &vec).unwrap();
                unique_ids.push(id);
                unique_data.extend_from_slice(&vec);
            }
        }

        let ref_result = naive_reference(dim, &unique_ids, &unique_data, &query, k);
        let t1 = index.top_k(&query, k, 1);
        let t2 = index.top_k(&query, k, 2);
        let t4 = index.top_k(&query, k, 4);

        prop_assert_eq!(t1.len(), ref_result.len());
        prop_assert_eq!(t2.len(), ref_result.len());
        prop_assert_eq!(t4.len(), ref_result.len());

        for i in 0..ref_result.len() {
            prop_assert_eq!(t1[i].0, ref_result[i].0);
            prop_assert_eq!(t2[i].0, ref_result[i].0);
            prop_assert_eq!(t4[i].0, ref_result[i].0);

            prop_assert!((t1[i].1 - ref_result[i].1).abs() <= 1e-5);
            prop_assert!((t2[i].1 - ref_result[i].1).abs() <= 1e-5);
            prop_assert!((t4[i].1 - ref_result[i].1).abs() <= 1e-5);
        }
    }
}

// -----------------------------------------------------------------------------
// (4) Proptest: sequence of inserts/removes/replaces equals HashMap model
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ModelOp {
    Insert(u64, Vec<f32>),
    Replace(u64, Vec<f32>),
    Remove(u64),
}

fn arb_model_ops(dim: usize) -> impl Strategy<Value = Vec<ModelOp>> {
    let op_strategy = prop_oneof![
        (
            0u64..30,
            proptest::collection::vec(-1.0f32..=1.0f32, dim..=dim)
        )
            .prop_map(|(id, v)| ModelOp::Insert(id, v)),
        (
            0u64..30,
            proptest::collection::vec(-1.0f32..=1.0f32, dim..=dim)
        )
            .prop_map(|(id, v)| ModelOp::Replace(id, v)),
        (0u64..30).prop_map(ModelOp::Remove),
    ];
    proptest::collection::vec(op_strategy, 1..=80)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]
    #[test]
    fn proptest_sequence_equals_hashmap(ops in arb_model_ops(4)) {
        let dim = 4;
        let mut index = VectorIndex::new(dim);
        let mut map: HashMap<u64, Vec<f32>> = HashMap::new();

        for op in ops {
            match op {
                ModelOp::Insert(id, v) => {
                    let map_has = map.contains_key(&id);
                    let res = index.insert(id, &v);
                    if map_has {
                        prop_assert_eq!(res, Err(VectorError::DuplicateId(id)));
                    } else {
                        prop_assert_eq!(res, Ok(()));
                        map.insert(id, v);
                    }
                }
                ModelOp::Replace(id, v) => {
                    let map_has = map.contains_key(&id);
                    let res = index.replace(id, &v);
                    prop_assert_eq!(res, Ok(map_has));
                    map.insert(id, v);
                }
                ModelOp::Remove(id) => {
                    let map_has = map.remove(&id).is_some();
                    let res = index.remove(id);
                    prop_assert_eq!(res, map_has);
                }
            }
        }

        prop_assert_eq!(index.len(), map.len());
        prop_assert_eq!(index.is_empty(), map.is_empty());

        for id in 0u64..35 {
            prop_assert_eq!(index.contains(id), map.contains_key(&id));
            prop_assert_eq!(index.get(id), map.get(&id).map(|v| v.as_slice()));
        }
    }
}

// -----------------------------------------------------------------------------
// (5) Wrong-length and NaN queries return empty
// -----------------------------------------------------------------------------

#[test]
fn test_invalid_queries_return_empty() {
    let mut index = VectorIndex::new(3);
    index.insert(1, &[0.1, 0.2, 0.3]).unwrap();
    index.insert(2, &[0.4, 0.5, 0.6]).unwrap();

    // Wrong length: short
    assert!(index.top_k(&[0.1, 0.2], 5, 2).is_empty());

    // Wrong length: long
    assert!(index.top_k(&[0.1, 0.2, 0.3, 0.4], 5, 2).is_empty());

    // NaN in query
    assert!(index.top_k(&[0.1, f32::NAN, 0.3], 5, 2).is_empty());

    // Infinity in query
    assert!(index.top_k(&[0.1, f32::INFINITY, 0.3], 5, 2).is_empty());

    // Negative infinity in query
    assert!(index.top_k(&[0.1, f32::NEG_INFINITY, 0.3], 5, 2).is_empty());

    // k == 0
    assert!(index.top_k(&[0.1, 0.2, 0.3], 0, 2).is_empty());

    // k larger than index returns all rows
    let res = index.top_k(&[0.1, 0.2, 0.3], 100, 2);
    assert_eq!(res.len(), 2);
}

// -----------------------------------------------------------------------------
// (6) NEGATIVE CONTROL: Deliberately wrong scans fail equality assertion
// -----------------------------------------------------------------------------

// A deliberately wrong scan that breaks ties by larger id first instead of smaller id.
fn wrong_tie_break_scan(
    dim: usize,
    ids: &[u64],
    data: &[f32],
    query: &[f32],
    k: usize,
) -> Vec<(u64, f32)> {
    let mut scores = Vec::with_capacity(ids.len());
    for (idx, &id) in ids.iter().enumerate() {
        let row = &data[idx * dim..(idx + 1) * dim];
        let dot = row
            .iter()
            .zip(query.iter())
            .map(|(a, b)| a * b)
            .sum::<f32>();
        scores.push((id, dot));
    }
    // Deliberately incorrect: b.0.cmp(&a.0) (larger id first)
    scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    scores.truncate(k);
    scores
}

// A deliberately wrong scan that returns results in ascending order.
fn wrong_ascending_scan(
    dim: usize,
    ids: &[u64],
    data: &[f32],
    query: &[f32],
    k: usize,
) -> Vec<(u64, f32)> {
    let mut res = naive_reference(dim, ids, data, query, k);
    res.reverse();
    res
}

#[test]
fn test_negative_control_wrong_tie_break_fails() {
    // Crafted input: two items with IDENTICAL scores but different IDs.
    let dim = 2;
    let ids = [1u64, 2u64];
    let data = [1.0f32, 0.0f32, 1.0f32, 0.0f32];
    let query = [1.0f32, 0.0f32];

    let ref_res = naive_reference(dim, &ids, &data, &query, 2);
    let wrong_res = wrong_tie_break_scan(dim, &ids, &data, &query, 2);

    // Naive reference requires smaller id first: [(1, 1.0), (2, 1.0)]
    assert_eq!(ref_res, vec![(1, 1.0), (2, 1.0)]);
    // Wrong scan gives larger id first: [(2, 1.0), (1, 1.0)]
    assert_eq!(wrong_res, vec![(2, 1.0), (1, 1.0)]);

    // Proves the equality check would fail on incorrect tie-break implementation
    assert_ne!(
        ref_res, wrong_res,
        "Negative control: wrong tie-break must not match reference"
    );
}

#[test]
fn test_negative_control_wrong_order_fails() {
    let dim = 2;
    let ids = [1u64, 2u64];
    let data = [0.2f32, 0.0f32, 0.9f32, 0.0f32];
    let query = [1.0f32, 0.0f32];

    let ref_res = naive_reference(dim, &ids, &data, &query, 2);
    let wrong_res = wrong_ascending_scan(dim, &ids, &data, &query, 2);

    // Reference has best match first: [(2, 0.9), (1, 0.2)]
    assert_eq!(ref_res, vec![(2, 0.9), (1, 0.2)]);
    // Wrong ascending scan has worst match first: [(1, 0.2), (2, 0.9)]
    assert_eq!(wrong_res, vec![(1, 0.2), (2, 0.9)]);

    assert_ne!(
        ref_res, wrong_res,
        "Negative control: ascending order must not match reference"
    );
}
