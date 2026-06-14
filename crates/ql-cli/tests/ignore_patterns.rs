//! Tests for ignore-pattern support in `walk_relative_files`.
//!
//! The directory walker now uses the `ignore` crate, so traversal should:
//! - respect `.gitignore` entries,
//! - respect a `ql`-specific `.qlignore` file (same syntax as `.gitignore`),
//! - skip hidden files/directories (e.g. `.git`, `.hidden`), and
//! - always skip a built-in set of common build/dependency directories
//!   (`target`, `node_modules`, `vendor`, `.venv`), even without any ignore
//!   file listing them.
//!
//! This test builds a small project covering each of those cases and checks
//! that only the one function outside of any ignored location is reported.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root_dir() -> PathBuf {
    std::env::temp_dir().join(format!("ql_test_ignore_patterns_{}", std::process::id()))
}

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).expect("create parent dir");
    fs::write(path, contents).expect("write file");
}

#[test]
fn skips_gitignored_qlignored_hidden_and_default_ignored_dirs() {
    let root = root_dir();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create root");

    // The only file that should be indexed.
    write_file(&root.join("src/lib.rs"), "pub fn included_fn() {}\n");

    // Excluded via `.gitignore`.
    write_file(&root.join(".gitignore"), "ignored_by_gitignore/\n");
    write_file(
        &root.join("ignored_by_gitignore/skip.rs"),
        "pub fn gitignored_fn() {}\n",
    );

    // Excluded via a `ql`-specific `.qlignore` file.
    write_file(&root.join(".qlignore"), "custom_ignored/\n");
    write_file(
        &root.join("custom_ignored/skip.rs"),
        "pub fn qlignore_fn() {}\n",
    );

    // Excluded because they're hidden directories.
    write_file(&root.join(".git/objects/abc.rs"), "pub fn git_fn() {}\n");
    write_file(&root.join(".hidden/secret.rs"), "pub fn hidden_fn() {}\n");

    // Excluded because they're in the built-in default-ignored list, even
    // though nothing here mentions them in `.gitignore` or `.qlignore`.
    write_file(
        &root.join("target/debug/build.rs"),
        "pub fn target_fn() {}\n",
    );
    write_file(
        &root.join("node_modules/pkg/index.rs"),
        "pub fn node_modules_fn() {}\n",
    );
    write_file(&root.join("vendor/dep/dep.rs"), "pub fn vendor_fn() {}\n");
    write_file(&root.join(".venv/lib/site.rs"), "pub fn venv_fn() {}\n");

    let output = Command::new(env!("CARGO_BIN_EXE_ql"))
        .arg("SELECT name FROM functions")
        .arg(&root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run ql binary");

    assert!(
        output.status.success(),
        "ql exited with {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&stdout).expect("valid json output");

    let names: HashSet<String> = rows
        .into_iter()
        .map(|row| {
            row.get("name")
                .and_then(|value| value.as_str())
                .expect("row has a string name column")
                .to_string()
        })
        .collect();

    assert_eq!(
        names,
        HashSet::from(["included_fn".to_string()]),
        "only included_fn should be indexed; ignored directories leaked into results"
    );

    let _ = fs::remove_dir_all(&root);
}
