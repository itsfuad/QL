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
