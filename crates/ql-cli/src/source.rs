use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use ql_adapters::adapter_for_path;
use ql_ast::{TableBatch, second_pass, walk_source};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    modified_secs: u64,
    modified_nanos: u32,
    len: u64,
    batch: TableBatch,
}

pub fn is_source_file(path: &Path) -> bool {
    adapter_for_path(path).is_some()
}

/// Directories that are always skipped during traversal, even if a project has
/// no `.gitignore` (or doesn't list them in one). These are common build output
/// and dependency directories that should never be indexed as source.
const DEFAULT_IGNORED_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".venv",
    "venv",
    ".git",
    "vendor",
    "dist",
    "build",
    "__pycache__",
];

/// Recursively lists every file under `root`, paired with its path relative to
/// `root`.
///
/// Traversal honors `.gitignore`/`.ignore` files and `.qlignore` (a `ql`-specific
/// ignore file using the same syntax) found within `root`, and skips hidden files
/// and directories (including `.git`), via the `ignore` crate. On top of that,
/// [`DEFAULT_IGNORED_DIRS`] is always skipped regardless of ignore files. Symlinks
/// are not followed, so there is no need to track visited directories, and there
/// is no limit on the number of directories visited.
fn walk_relative_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();

    let mut builder = WalkBuilder::new(root);
    builder
        // Respect `.gitignore`/`.ignore` even outside of a git repository (e.g. a
        // plain directory extracted from an archive).
        .require_git(false)
        // Only consider ignore files within `root` itself, so analysis results
        // don't depend on `.gitignore` files elsewhere on disk (e.g. an ancestor
        // directory outside the analyzed project).
        .parents(false)
        .add_custom_ignore_filename(".qlignore")
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| DEFAULT_IGNORED_DIRS.contains(&name))
        });

    for entry in builder.build() {
        let Ok(entry) = entry else { continue };
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        let path = entry.into_path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().into_owned();
        files.push((path, relative));
    }

    files
}

pub fn scan_snapshot(root: &Path) -> Result<HashMap<String, SystemTime>, String> {
    let mut snapshot = HashMap::new();
    for (path, relative) in walk_relative_files(root) {
        if !is_source_file(&path) {
            continue;
        }
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("error: failed to stat {}: {e}", path.display()))?;
        let modified = metadata
            .modified()
            .map_err(|e| format!("error: failed to read mtime for {}: {e}", path.display()))?;
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
    let mut cache = read_cache(root);
    let mut next_cache = HashMap::new();
    let mut warned_exts = std::collections::HashSet::new();
    for (path, relative) in walk_relative_files(root) {
        let Some(adapter) = adapter_for_path(&path) else {
            let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            let warning_key = if ext.is_empty() {
                "<no-extension>".to_string()
            } else {
                format!(".{ext}")
            };
            if warned_exts.insert(warning_key.clone()) {
                if ext.is_empty() {
                    eprintln!("warning: no adapter for files without an extension, skipping");
                } else {
                    eprintln!("warning: no adapter for .{ext}, skipping");
                }
            }
            continue;
        };

        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("error: failed to stat {}: {e}", path.display()))?;
        if let Some(key) = cache_key(&metadata)
            && let Some(cached) = cache.remove(&relative)
            && cached.matches(key, metadata.len())
        {
            batch.extend(cached.batch.clone());
            next_cache.insert(relative, cached);
            continue;
        }

        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("error: failed to read {}: {e}", path.display()))?;
        let file_batch = walk_source(adapter, relative, &source)
            .map_err(|e| format!("error: failed to parse {}: {e}", path.display()))?;
        if let Some((modified_secs, modified_nanos)) = cache_key(&metadata) {
            next_cache.insert(
                file_batch.current_file.clone(),
                CachedFile {
                    modified_secs,
                    modified_nanos,
                    len: metadata.len(),
                    batch: file_batch.clone(),
                },
            );
        }
        batch.extend(file_batch);
    }
    write_cache(root, &next_cache);
    second_pass(&mut batch);
    Ok(batch)
}

impl CachedFile {
    fn matches(&self, modified: (u64, u32), len: u64) -> bool {
        self.modified_secs == modified.0 && self.modified_nanos == modified.1 && self.len == len
    }
}

fn cache_key(metadata: &std::fs::Metadata) -> Option<(u64, u32)> {
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((modified.as_secs(), modified.subsec_nanos()))
}

fn read_cache(root: &Path) -> HashMap<String, CachedFile> {
    cache_path(root)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn write_cache(root: &Path, cache: &HashMap<String, CachedFile>) {
    let Some(path) = cache_path(root) else { return };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(content) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, content);
    }
}

fn cache_path(root: &Path) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut hasher = DefaultHasher::new();
    root.to_string_lossy().hash(&mut hasher);
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("ql")
            .join(format!("{:x}.json", hasher.finish())),
    )
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

    // Regression test for the directory-walk traversal cap.
    //
    // `walk_relative_files` used to push newly discovered subdirectories onto a
    // pending-directory stack only while that stack had fewer than 1000 entries, so
    // a project with more than 1000 directories was silently under-indexed:
    // everything past the first 1000 pending directories was dropped without any
    // warning. This builds a project with more than 1000 sibling directories, each
    // containing one source file, and checks that every single one is visited.
    #[test]
    fn walks_more_than_1000_directories() {
        const DIR_COUNT: usize = 1100;

        let root = std::env::temp_dir().join("ql_test_large_tree");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create root");

        for i in 0..DIR_COUNT {
            let dir = root.join(format!("module_{i:04}"));
            fs::create_dir_all(&dir).expect("create module dir");
            fs::write(
                dir.join("lib.rs"),
                format!("pub fn function_{i:04}() {{}}\n"),
            )
            .expect("write source file");
        }

        let files = walk_relative_files(&root);

        assert_eq!(
            files.len(),
            DIR_COUNT,
            "expected one file per directory ({DIR_COUNT} directories), got {}",
            files.len()
        );

        let mut relatives: Vec<String> = files.into_iter().map(|(_, relative)| relative).collect();
        relatives.sort();
        for (i, relative) in relatives.iter().enumerate() {
            assert_eq!(*relative, format!("module_{i:04}/lib.rs"));
        }

        let _ = fs::remove_dir_all(&root);
    }

    // Tests for ignore-pattern support in `walk_relative_files`.
    //
    // The directory walker uses the `ignore` crate, so traversal should:
    // - respect `.gitignore` entries,
    // - respect a `ql`-specific `.qlignore` file (same syntax as `.gitignore`),
    // - skip hidden files/directories (e.g. `.git`, `.hidden`), and
    // - always skip a built-in set of common build/dependency directories
    //   (`target`, `node_modules`, `vendor`, `.venv`), even without any ignore file
    //   listing them.
    #[test]
    fn skips_gitignored_qlignored_hidden_and_default_ignored_dirs() {
        let root = std::env::temp_dir().join("ql_test_ignore_patterns");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create root");

        let write = |relative: &str, contents: &str| {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");
            fs::write(path, contents).expect("write file");
        };

        // The only file that should be indexed.
        write("src/lib.rs", "pub fn included_fn() {}\n");

        // Excluded via `.gitignore`.
        write(".gitignore", "ignored_by_gitignore/\n");
        write(
            "ignored_by_gitignore/skip.rs",
            "pub fn gitignored_fn() {}\n",
        );

        // Excluded via a `ql`-specific `.qlignore` file.
        write(".qlignore", "custom_ignored/\n");
        write("custom_ignored/skip.rs", "pub fn qlignore_fn() {}\n");

        // Excluded because they're hidden directories.
        write(".git/objects/abc.rs", "pub fn git_fn() {}\n");
        write(".hidden/secret.rs", "pub fn hidden_fn() {}\n");

        // Excluded because they're in the built-in default-ignored list, even though
        // nothing here mentions them in `.gitignore` or `.qlignore`.
        write("target/debug/build.rs", "pub fn target_fn() {}\n");
        write("node_modules/pkg/index.rs", "pub fn node_modules_fn() {}\n");
        write("vendor/dep/dep.rs", "pub fn vendor_fn() {}\n");
        write(".venv/lib/site.rs", "pub fn venv_fn() {}\n");

        let files = walk_relative_files(&root);
        let relatives: Vec<&str> = files
            .iter()
            .map(|(_, relative)| relative.as_str())
            .collect();

        assert_eq!(
            relatives,
            vec!["src/lib.rs"],
            "only src/lib.rs should be visited; ignored directories leaked into results"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
