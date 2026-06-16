use std::collections::{HashMap, HashSet};

use crate::rows::{CallSetRow, SimilarityRow, TableBatch};

pub fn extract_callsets(batch: &mut TableBatch) {
    let mut call_map: HashMap<(String, String), HashSet<String>> = HashMap::new();

    for call in &batch.calls {
        if call.caller.is_empty() {
            continue;
        }
        let key = (call.file.clone(), call.caller.clone());
        call_map.entry(key).or_default().insert(call.callee.clone());
    }

    let mut callee_counts: HashMap<(String, String), usize> = HashMap::new();

    for ((file, caller), callees) in &call_map {
        callee_counts.insert((file.clone(), caller.clone()), callees.len());

        for callee in callees {
            let line = batch
                .functions
                .iter()
                .find(|f| f.file == *file && f.name == *caller)
                .map(|f| f.line)
                .unwrap_or(0);

            batch.callsets.push(CallSetRow {
                file: file.clone(),
                line,
                name: caller.clone(),
                callee: callee.clone(),
            });
        }
    }

    for fp in &mut batch.fingerprints {
        if let Some(&count) = callee_counts.get(&(fp.file.clone(), fp.name.clone())) {
            fp.unique_callee_count = count;
        }
    }
}

type FnKey = (String, usize, String);

pub fn compute_similarities(batch: &mut TableBatch, top_k: usize, threshold: f64) {
    if batch.fingerprints.len() < 2 {
        return;
    }

    let fps = &batch.fingerprints;
    let n = fps.len();

    let feature_count = 10;
    let mut features: Vec<Vec<f64>> = Vec::with_capacity(n);
    for fp in fps.iter() {
        features.push(vec![
            fp.param_count as f64,
            fp.complexity as f64,
            fp.nesting_depth as f64,
            fp.branch_count as f64,
            fp.loop_count as f64,
            fp.call_count as f64,
            fp.unique_callee_count as f64,
            fp.return_count as f64,
            fp.stmt_count as f64,
            fp.has_error_handling as i64 as f64,
        ]);
    }

    let mut mins = vec![f64::MAX; feature_count];
    let mut maxs = vec![f64::MIN; feature_count];
    for f in &features {
        for (i, &val) in f.iter().enumerate() {
            if val < mins[i] {
                mins[i] = val;
            }
            if val > maxs[i] {
                maxs[i] = val;
            }
        }
    }

    let mut normalized: Vec<Vec<f64>> = Vec::with_capacity(n);
    for f in &features {
        let mut row = Vec::with_capacity(feature_count);
        for (i, &val) in f.iter().enumerate() {
            let range = maxs[i] - mins[i];
            if range > 0.0 {
                row.push((val - mins[i]) / range);
            } else {
                row.push(0.5);
            }
        }
        normalized.push(row);
    }

    let mut callset_index: HashMap<FnKey, HashSet<String>> = HashMap::new();
    for cs in &batch.callsets {
        let key = (cs.file.clone(), cs.line, cs.name.clone());
        callset_index.entry(key).or_default().insert(cs.callee.clone());
    }

    let empty_set: HashSet<String> = HashSet::new();

    let mut all_similarities: Vec<SimilarityRow> = Vec::new();

    for i in 0..n {
        let key_a: FnKey = (fps[i].file.clone(), fps[i].line, fps[i].name.clone());
        let calls_a = callset_index.get(&key_a).unwrap_or(&empty_set);

        let mut candidates: Vec<(usize, f64, f64, f64)> = Vec::new();

        for j in 0..n {
            if i == j {
                continue;
            }

            let struct_score = cosine_similarity(&normalized[i], &normalized[j]);

            let key_b: FnKey = (fps[j].file.clone(), fps[j].line, fps[j].name.clone());
            let calls_b = callset_index.get(&key_b).unwrap_or(&empty_set);
            let behav_score = jaccard_similarity(calls_a, calls_b);

            let combined = 0.6 * struct_score + 0.4 * behav_score;

            if combined >= threshold {
                candidates.push((j, struct_score, behav_score, combined));
            }
        }

        candidates.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(top_k);

        for (j, struct_score, behav_score, combined) in candidates {
            all_similarities.push(SimilarityRow {
                file_a: fps[i].file.clone(),
                line_a: fps[i].line,
                name_a: fps[i].name.clone(),
                file_b: fps[j].file.clone(),
                line_b: fps[j].line,
                name_b: fps[j].name.clone(),
                structural_score: struct_score,
                behavioral_score: behav_score,
                combined_score: combined,
            });
        }
    }

    batch.similarities = all_similarities;
}

pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (&va, &vb) in a.iter().zip(b.iter()) {
        dot += va * vb;
        norm_a += va * va;
        norm_b += vb * vb;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

pub fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&a, &b);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-9);
    }

    #[test]
    fn test_cosine_similarity_zero_vectors() {
        let a = vec![0.0, 0.0];
        let b = vec![0.0, 0.0];
        let score = cosine_similarity(&a, &b);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_jaccard_similarity_identical() {
        let a: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let score = jaccard_similarity(&a, &b);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_similarity_disjoint() {
        let a: HashSet<String> = ["foo"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["bar"].iter().map(|s| s.to_string()).collect();
        let score = jaccard_similarity(&a, &b);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_jaccard_similarity_partial() {
        let a: HashSet<String> = ["foo", "bar", "baz"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["foo", "bar", "qux"].iter().map(|s| s.to_string()).collect();
        let score = jaccard_similarity(&a, &b);
        assert!((score - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_similarity_both_empty() {
        let a: HashSet<String> = HashSet::new();
        let b: HashSet<String> = HashSet::new();
        let score = jaccard_similarity(&a, &b);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_compute_similarities_basic() {
        let mut batch = TableBatch::default();
        batch.fingerprints = vec![
            FingerprintRow {
                file: "a.rs".into(),
                line: 1,
                name: "foo".into(),
                param_count: 2,
                complexity: 3,
                nesting_depth: 2,
                branch_count: 2,
                loop_count: 1,
                call_count: 3,
                unique_callee_count: 2,
                return_count: 1,
                stmt_count: 5,
                has_error_handling: false,
            },
            FingerprintRow {
                file: "b.rs".into(),
                line: 1,
                name: "bar".into(),
                param_count: 2,
                complexity: 3,
                nesting_depth: 2,
                branch_count: 2,
                loop_count: 1,
                call_count: 3,
                unique_callee_count: 2,
                return_count: 1,
                stmt_count: 5,
                has_error_handling: false,
            },
        ];

        compute_similarities(&mut batch, 10, 0.5);

        assert!(!batch.similarities.is_empty());
        let top = &batch.similarities[0];
        assert!(top.combined_score > 0.9);
    }

    #[test]
    fn test_compute_similarities_dissimilar() {
        let mut batch = TableBatch::default();
        batch.fingerprints = vec![
            FingerprintRow {
                file: "a.rs".into(),
                line: 1,
                name: "simple".into(),
                param_count: 0,
                complexity: 1,
                nesting_depth: 0,
                branch_count: 0,
                loop_count: 0,
                call_count: 0,
                unique_callee_count: 0,
                return_count: 0,
                stmt_count: 1,
                has_error_handling: false,
            },
            FingerprintRow {
                file: "b.rs".into(),
                line: 1,
                name: "complex".into(),
                param_count: 5,
                complexity: 20,
                nesting_depth: 5,
                branch_count: 8,
                loop_count: 3,
                call_count: 15,
                unique_callee_count: 10,
                return_count: 4,
                stmt_count: 30,
                has_error_handling: true,
            },
        ];

        compute_similarities(&mut batch, 10, 0.0);

        let sim = batch
            .similarities
            .iter()
            .find(|s| s.name_a == "simple" && s.name_b == "complex")
            .expect("should have a pair");
        assert!(sim.structural_score < 0.5);
    }

    #[test]
    fn test_extract_callsets() {
        let mut batch = TableBatch::default();
        batch.functions = vec![FunctionRow {
            file: "a.rs".into(),
            line: 1,
            name: "main".into(),
            ..Default::default()
        }];
        batch.calls = vec![
            CallRow {
                file: "a.rs".into(),
                line: 5,
                caller: "main".into(),
                callee: "foo".into(),
                is_external: false,
            },
            CallRow {
                file: "a.rs".into(),
                line: 6,
                caller: "main".into(),
                callee: "bar".into(),
                is_external: false,
            },
            CallRow {
                file: "a.rs".into(),
                line: 7,
                caller: "main".into(),
                callee: "foo".into(),
                is_external: false,
            },
        ];
        batch.fingerprints = vec![FingerprintRow {
            file: "a.rs".into(),
            line: 1,
            name: "main".into(),
            ..Default::default()
        }];

        extract_callsets(&mut batch);

        assert_eq!(batch.callsets.len(), 2);
        assert_eq!(batch.fingerprints[0].unique_callee_count, 2);
    }
}
