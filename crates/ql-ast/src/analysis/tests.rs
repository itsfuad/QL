use std::time::{Duration, Instant};

use crate::rows::{CommentRow, FunctionRow, StructRow, TableBatch};

use super::second_pass;

#[test]
fn resolves_has_test_comments_and_implements() {
    let mut batch = TableBatch::new("");
    batch.functions.push(FunctionRow {
        file: "src/lib.rs".to_string(),
        line: 5,
        name: "add".to_string(),
        visibility: "private".to_string(),
        param_count: 0,
        return_type: String::new(),
        complexity: 1,
        has_test: false,
    });
    batch.functions.push(FunctionRow {
        file: "src/lib.rs".to_string(),
        line: 20,
        name: "test_add".to_string(),
        visibility: "private".to_string(),
        param_count: 0,
        return_type: String::new(),
        complexity: 1,
        has_test: false,
    });
    batch.structs.push(StructRow {
        file: "src/lib.rs".to_string(),
        line: 10,
        name: "User".to_string(),
        field_count: 2,
        visibility: "public".to_string(),
        implements: "Display, Display".to_string(),
    });
    batch.comments.push(CommentRow {
        file: "src/lib.rs".to_string(),
        line: 1,
        text: "// docs".to_string(),
        attached_to: String::new(),
        is_doc: true,
    });

    second_pass(&mut batch);

    assert!(batch.functions[0].has_test);
    assert_eq!(batch.structs[0].implements, "Display");
    assert_eq!(batch.comments[0].attached_to, "add");
}

// -- resolve_comment_attachments correctness --
//
// `resolve_comment_attachments` was rewritten from an O(C * (F + S)) full-batch
// scan to a sorted, per-file lookup. These tests pin down the original behavior
// so the optimization can't silently change results: a comment attaches to the
// *nearest following* function or struct in the *same file*, with ties between a
// function and a struct on the same line resolved in favor of the function.

fn function(file: &str, line: usize, name: &str) -> FunctionRow {
    FunctionRow {
        file: file.to_string(),
        line,
        name: name.to_string(),
        ..FunctionRow::default()
    }
}

fn struct_row(file: &str, line: usize, name: &str) -> StructRow {
    StructRow {
        file: file.to_string(),
        line,
        name: name.to_string(),
        ..StructRow::default()
    }
}

fn comment(file: &str, line: usize) -> CommentRow {
    CommentRow {
        file: file.to_string(),
        line,
        text: "// comment".to_string(),
        attached_to: String::new(),
        is_doc: true,
    }
}

#[test]
fn attaches_to_nearest_following_function() {
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 10, "first"));
    batch.functions.push(function("src/lib.rs", 20, "second"));
    batch.comments.push(comment("src/lib.rs", 5));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "first");
}

#[test]
fn attaches_to_nearest_following_struct() {
    let mut batch = TableBatch::new("");
    batch.structs.push(struct_row("src/lib.rs", 30, "Config"));
    batch.comments.push(comment("src/lib.rs", 25));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "Config");
}

#[test]
fn picks_whichever_definition_is_closer() {
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 50, "far_fn"));
    batch.structs.push(struct_row("src/lib.rs", 12, "Near"));
    batch.comments.push(comment("src/lib.rs", 5));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "Near");
}

#[test]
fn ties_favor_the_function() {
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 10, "tied_fn"));
    batch
        .structs
        .push(struct_row("src/lib.rs", 10, "TiedStruct"));
    batch.comments.push(comment("src/lib.rs", 5));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "tied_fn");
}

#[test]
fn comment_with_no_following_definition_is_unattached() {
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 1, "before"));
    batch.comments.push(comment("src/lib.rs", 10));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "");
}

#[test]
fn definitions_in_other_files_are_ignored() {
    let mut batch = TableBatch::new("");
    batch
        .functions
        .push(function("src/other.rs", 100, "other_fn"));
    batch.comments.push(comment("src/lib.rs", 1));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "");
}

#[test]
fn each_comment_attaches_independently_within_a_file() {
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 5, "alpha"));
    batch.functions.push(function("src/lib.rs", 15, "beta"));
    batch.structs.push(struct_row("src/lib.rs", 25, "Gamma"));
    batch.comments.push(comment("src/lib.rs", 1)); // -> alpha
    batch.comments.push(comment("src/lib.rs", 10)); // -> beta
    batch.comments.push(comment("src/lib.rs", 20)); // -> Gamma
    batch.comments.push(comment("src/lib.rs", 30)); // -> nothing

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "alpha");
    assert_eq!(batch.comments[1].attached_to, "beta");
    assert_eq!(batch.comments[2].attached_to, "Gamma");
    assert_eq!(batch.comments[3].attached_to, "");
}

#[test]
fn comment_on_same_line_as_definition_does_not_attach_to_it() {
    // Original logic required `row.line > comment.line`, so a comment that shares
    // a definition's line attaches to whatever comes *after* that definition
    // instead.
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 5, "same_line"));
    batch.functions.push(function("src/lib.rs", 9, "next"));
    batch.comments.push(comment("src/lib.rs", 5));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "next");
}

// -- resolve_comment_attachments performance regression --
//
// The original implementation scanned the *entire* batch's functions and structs
// for every comment (O(C * (F + S))), which was the dominant cost of `second_pass`
// on large repositories. This test builds a batch large enough (tens of thousands
// of rows spread across thousands of files) that the quadratic version would take
// a very long time, while the sorted/binary-search version finishes in well under
// a second. A generous wall-clock bound turns a reintroduced quadratic scan into a
// failing test rather than a silent slowdown.

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
