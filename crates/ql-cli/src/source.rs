use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use ql_adapters::GoAdapter;
use ql_ast::{TableBatch, walk_source};

pub fn is_source_file(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("go" | "rs" | "ts" | "py") => true,
        _ => false,
    }
}

pub fn scan_snapshot(root: &Path) -> Result<HashMap<String, SystemTime>, String> {
    let mut snapshot = HashMap::new();
    collect_entries(root, root, &mut snapshot)?;
    Ok(snapshot)
}

fn collect_entries(
    root: &Path,
    dir: &Path,
    snapshot: &mut HashMap<String, SystemTime>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("error: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("error: {e}"))?;
        let path = entry.path();

        if path.is_dir() {
            collect_entries(root, &path, snapshot)?;
            continue;
        }

        if !is_source_file(&path) {
            continue;
        }

        let metadata = entry.metadata().map_err(|e| format!("error: {e}"))?;
        let modified = metadata.modified().map_err(|e| format!("error: {e}"))?;

        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();

        snapshot.insert(relative, modified);
    }

    Ok(())
}

pub fn snapshots_equal(
    left: &HashMap<String, SystemTime>,
    right: &HashMap<String, SystemTime>,
) -> bool {
    if left.len() != right.len() {
        return false;
    }
    for (path, left_time) in left {
        match right.get(path) {
            Some(right_time) => {
                if left_time != right_time {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

pub fn collect_source_batch(root: &Path) -> Result<TableBatch, String> {
    let mut batch = TableBatch::new("");
    collect_go_files(root, root, &mut batch)?;
    Ok(batch)
}

fn collect_go_files(root: &Path, path: &Path, batch: &mut TableBatch) -> Result<(), String> {
    let entries = std::fs::read_dir(path).map_err(|e| format!("error: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("error: {e}"))?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            collect_go_files(root, &entry_path, batch)?;
            continue;
        }

        if entry_path.extension().and_then(|ext| ext.to_str()) != Some("go") {
            continue;
        }

        let source = std::fs::read_to_string(&entry_path)
            .map_err(|e| format!("error: failed to read {}: {e}", entry_path.display()))?;
        let relative = entry_path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let file_batch = walk_source(&GoAdapter, relative, &source)
            .map_err(|e| format!("error: failed to parse {}: {e}", entry_path.display()))?;

        batch.extend(file_batch);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn detects_source_extensions() {
        assert!(is_source_file(Path::new("main.go")));
        assert!(is_source_file(Path::new("lib.rs")));
        assert!(is_source_file(Path::new("app.ts")));
        assert!(is_source_file(Path::new("test.py")));
        assert!(!is_source_file(Path::new("notes.txt")));
        assert!(!is_source_file(Path::new("data.json")));
    }

    #[test]
    fn scans_source_files_only() {
        let root = std::env::temp_dir().join("ql_test_scan");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp dir");
        fs::write(root.join("main.go"), "package main\n").expect("write");
        fs::write(root.join("notes.txt"), "ignore").expect("write");

        let snapshot = scan_snapshot(&root).expect("scan should succeed");

        assert_eq!(snapshot.len(), 1);
        assert!(snapshot.contains_key("main.go"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_snapshot_changes() {
        let now = SystemTime::now();
        let later = now.checked_add(Duration::from_secs(1)).unwrap();

        let mut left = HashMap::new();
        left.insert("main.go".to_string(), now);

        let mut right_same = HashMap::new();
        right_same.insert("main.go".to_string(), now);

        let mut right_diff = HashMap::new();
        right_diff.insert("main.go".to_string(), later);

        assert!(snapshots_equal(&left, &right_same));
        assert!(!snapshots_equal(&left, &right_diff));
    }

    #[test]
    fn detects_different_file_count() {
        let now = SystemTime::now();

        let mut left = HashMap::new();
        left.insert("main.go".to_string(), now);

        let mut right = HashMap::new();
        right.insert("main.go".to_string(), now);
        right.insert("lib.go".to_string(), now);

        assert!(!snapshots_equal(&left, &right));
    }
}
