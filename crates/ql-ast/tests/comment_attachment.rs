//! Correctness tests for `second_pass`'s comment-attachment pass.
//!
//! `resolve_comment_attachments` was rewritten from an O(C * (F + S)) full-batch scan
//! to a sorted, per-file lookup (see `crates/ql-ast/src/analysis.rs`). These tests pin
//! down the original behavior so the optimization can't silently change results:
//! a comment attaches to the *nearest following* function or struct in the *same
//! file*, with ties between a function and a struct on the same line resolved in
//! favor of the function.

use ql_ast::{CommentRow, FunctionRow, StructRow, TableBatch, second_pass};

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
    // Original logic required `row.line > comment.line`, so a comment that shares a
    // definition's line attaches to whatever comes *after* that definition instead.
    let mut batch = TableBatch::new("");
    batch.functions.push(function("src/lib.rs", 5, "same_line"));
    batch.functions.push(function("src/lib.rs", 9, "next"));
    batch.comments.push(comment("src/lib.rs", 5));

    second_pass(&mut batch);

    assert_eq!(batch.comments[0].attached_to, "next");
}
