//! Regression test for the directory-walk traversal cap.
//!
//! `walk_relative_files` used to push newly discovered subdirectories onto a
//! pending-directory stack only while that stack had fewer than 1000 entries,
//! so a project with more than 1000 directories was silently under-indexed:
//! everything past the first 1000 pending directories was dropped without any
//! warning. This test builds a project with more than 1000 sibling
//! directories, each containing one source file with a uniquely named
//! function, and checks that the CLI finds a function in every single one of
//! them.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const DIR_COUNT: usize = 1100;

fn root_dir() -> PathBuf {
    std::env::temp_dir().join(format!("ql_test_large_tree_{}", std::process::id()))
}

#[test]
fn indexes_more_than_1000_directories() {
    let root = root_dir();
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

    assert_eq!(
        rows.len(),
        DIR_COUNT,
        "expected one function per directory ({DIR_COUNT} directories), got {} rows",
        rows.len()
    );

    let mut names: Vec<String> = rows
        .into_iter()
        .map(|row| {
            row.get("name")
                .and_then(|value| value.as_str())
                .expect("row has a string name column")
                .to_string()
        })
        .collect();
    names.sort();

    for (i, name) in names.iter().enumerate() {
        assert_eq!(*name, format!("function_{i:04}"));
    }

    let _ = fs::remove_dir_all(&root);
}
