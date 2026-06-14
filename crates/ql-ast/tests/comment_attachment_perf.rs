//! Large synthetic regression test for `second_pass`'s comment-attachment pass.
//!
//! The original implementation scanned the *entire* batch's functions and structs
//! for every comment (O(C * (F + S))), which the audit identified as the dominant
//! cost of `second_pass` on large repositories. This test builds a batch large
//! enough (tens of thousands of rows spread across thousands of files) that the
//! quadratic version would take a very long time, while the sorted/binary-search
//! version finishes in well under a second. A generous wall-clock bound turns a
//! reintroduced quadratic scan into a failing test rather than a silent slowdown.

use std::time::{Duration, Instant};

use ql_ast::{CommentRow, FunctionRow, StructRow, TableBatch, second_pass};

const FILE_COUNT: usize = 3000;
const ITEMS_PER_FILE: usize = 5;

fn build_large_batch() -> TableBatch {
    let mut batch = TableBatch::new("");

    for file_index in 0..FILE_COUNT {
        let file = format!("src/module_{file_index}.rs");

        for item_index in 0..ITEMS_PER_FILE {
            // Interleave functions and structs at increasing line numbers, with a
            // comment immediately before each one.
            let base_line = item_index * 10;

            batch.comments.push(CommentRow {
                file: file.clone(),
                line: base_line + 1,
                text: "// doc".to_string(),
                attached_to: String::new(),
                is_doc: true,
            });
            batch.functions.push(FunctionRow {
                file: file.clone(),
                line: base_line + 2,
                name: format!("fn_{file_index}_{item_index}"),
                ..FunctionRow::default()
            });

            batch.comments.push(CommentRow {
                file: file.clone(),
                line: base_line + 5,
                text: "// doc".to_string(),
                attached_to: String::new(),
                is_doc: true,
            });
            batch.structs.push(StructRow {
                file: file.clone(),
                line: base_line + 6,
                name: format!("Struct_{file_index}_{item_index}"),
                ..StructRow::default()
            });
        }

        // A trailing comment with nothing after it in this file.
        batch.comments.push(CommentRow {
            file: file.clone(),
            line: base_line_after_last(),
            text: "// trailing".to_string(),
            attached_to: String::new(),
            is_doc: true,
        });
    }

    batch
}

fn base_line_after_last() -> usize {
    (ITEMS_PER_FILE - 1) * 10 + 100
}

#[test]
fn resolves_large_batch_quickly_and_correctly() {
    let mut batch = build_large_batch();

    let total_functions = batch.functions.len();
    let total_structs = batch.structs.len();
    let total_comments = batch.comments.len();
    assert_eq!(total_functions, FILE_COUNT * ITEMS_PER_FILE);
    assert_eq!(total_structs, FILE_COUNT * ITEMS_PER_FILE);
    assert_eq!(total_comments, FILE_COUNT * (ITEMS_PER_FILE * 2 + 1));

    let start = Instant::now();
    second_pass(&mut batch);
    let elapsed = start.elapsed();

    // The sorted/binary-search implementation resolves ~33k comments against ~30k
    // definitions in milliseconds. A reintroduced O(C * (F + S)) scan would mean
    // ~33_000 * 30_000 ≈ 1 billion comparisons here, which takes far longer than
    // this bound even in a debug build.
    assert!(
        elapsed < Duration::from_secs(3),
        "second_pass took {elapsed:?}, expected a near-linear pass to finish quickly"
    );

    // Spot-check correctness across the generated files.
    for file_index in [0usize, FILE_COUNT / 2, FILE_COUNT - 1] {
        let file = format!("src/module_{file_index}.rs");
        let comments_for_file: Vec<&str> = batch
            .comments
            .iter()
            .filter(|c| c.file == file)
            .map(|c| c.attached_to.as_str())
            .collect();

        // For each item: the doc comment right before a function attaches to that
        // function, and the doc comment right before a struct attaches to that
        // struct.
        for item_index in 0..ITEMS_PER_FILE {
            let function_comment = comments_for_file[item_index * 2];
            let struct_comment = comments_for_file[item_index * 2 + 1];
            assert_eq!(function_comment, format!("fn_{file_index}_{item_index}"));
            assert_eq!(struct_comment, format!("Struct_{file_index}_{item_index}"));
        }

        // The trailing comment has nothing after it in this file.
        let trailing = comments_for_file.last().unwrap();
        assert_eq!(*trailing, "");
    }
}
