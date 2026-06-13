use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ql_adapters::{adapter_for_path, adapters};
use ql_ast::{TableBatch, walk_source};

pub fn is_source_file(path: &Path) -> bool {
    adapter_for_path(path).is_some()
}

fn walk_relative_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    visited.insert(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()));

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let canon = path.canonicalize().unwrap_or_else(|_| path);
                if visited.insert(canon.clone()) && dirs.len() < 1000 {
                    dirs.push(canon);
                }
                continue;
            }
            let relative = match path.strip_prefix(root) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => continue,
            };
            files.push((path, relative));
        }
    }
    files
}

pub fn scan_snapshot(root: &Path) -> Result<HashMap<String, SystemTime>, String> {
    let mut snapshot = HashMap::new();
    for (path, relative) in walk_relative_files(root) {
        if !is_source_file(&path) {
            continue;
        }
        let metadata = std::fs::metadata(&path).map_err(|e| format!("error: {e}"))?;
        let modified = metadata.modified().map_err(|e| format!("error: {e}"))?;
        snapshot.insert(relative, modified);
    }
    Ok(snapshot)
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
            Some(right_time) if left_time == right_time => {}
            _ => return false,
        }
    }
    true
}

pub fn detect_languages(root: &Path) -> Vec<String> {
    let mut langs = std::collections::BTreeSet::new();
    for (path, _) in walk_relative_files(root) {
        if let Some(adapter) = adapter_for_path(&path) {
            langs.insert(adapter.language_name().to_string());
        }
    }
    langs.into_iter().collect()
}

pub fn collect_source_batch(root: &Path) -> Result<TableBatch, String> {
    let mut batch = TableBatch::new("");
    let mut warned_exts = std::collections::HashSet::new();
    for (path, relative) in walk_relative_files(root) {
        let Some(adapter) = adapter_for_path(&path) else {
            let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            if warned_exts.insert(ext.to_string()) {
                eprintln!("warning: no adapter for .{ext}, skipping");
            }
            continue;
        };
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("error: failed to read {}: {e}", path.display()))?;
        let file_batch = walk_source(adapter, relative, &source)
            .map_err(|e| format!("error: failed to parse {}: {e}", path.display()))?;
        batch.extend(file_batch);
    }
    for adapter in adapters() {
        adapter.second_pass(&mut batch, root);
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn detects_languages_in_directory() {
        let root = std::env::temp_dir().join("ql_test_detect_langs");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp dir");
        fs::write(root.join("main.go"), "package main\n").expect("write");
        fs::write(root.join("lib.rs"), "fn main() {}\n").expect("write");
        fs::write(root.join("app.ts"), "export function run() {}\n").expect("write");
        fs::write(root.join("script.py"), "def run():\n    return 1\n").expect("write");
        fs::write(root.join("notes.txt"), "ignore").expect("write");

        let langs = detect_languages(&root);
        assert!(langs.contains(&"go".to_string()));
        assert!(langs.contains(&"rust".to_string()));
        assert!(langs.contains(&"typescript".to_string()));
        assert!(langs.contains(&"python".to_string()));
        assert_eq!(langs.len(), 4);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_no_languages_in_empty_dir() {
        let root = std::env::temp_dir().join("ql_test_empty_detect");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp dir");

        let langs = detect_languages(&root);
        assert!(langs.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_source_extensions() {
        assert!(is_source_file(Path::new("main.go")));
        assert!(is_source_file(Path::new("lib.rs")));
        assert!(is_source_file(Path::new("app.ts")));
        assert!(is_source_file(Path::new("app.tsx")));
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
        left.insert("lib.rs".to_string(), now);

        let mut right_same = HashMap::new();
        right_same.insert("lib.rs".to_string(), now);

        let mut right_diff = HashMap::new();
        right_diff.insert("lib.rs".to_string(), later);

        assert!(snapshots_equal(&left, &right_same));
        assert!(!snapshots_equal(&left, &right_diff));
    }

    #[test]
    fn detects_different_file_count() {
        let now = SystemTime::now();

        let mut left = HashMap::new();
        left.insert("lib.rs".to_string(), now);

        let mut right = HashMap::new();
        right.insert("lib.rs".to_string(), now);
        right.insert("mod.rs".to_string(), now);

        assert!(!snapshots_equal(&left, &right));
    }
}
