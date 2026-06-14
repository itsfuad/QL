use ql_ast::walk_source;

use super::RustAdapter;

#[test]
fn maps_rust_functions() {
    let source = r#"
fn main() {}

pub fn add(a: i32, b: i32) -> i32 {
a + b
}
"#;

    let batch = walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

    assert_eq!(batch.functions.len(), 2);
    assert_eq!(batch.functions[0].name, "main");
    assert_eq!(batch.functions[0].visibility, "private");
    assert_eq!(batch.functions[1].name, "add");
    assert_eq!(batch.functions[1].visibility, "public");
    assert_eq!(batch.functions[1].param_count, 2);
    assert_eq!(batch.functions[1].return_type, "i32");
}

#[test]
fn maps_calls_imports_structs_variables_and_comments() {
    let source = r#"
use std::fmt as fmt_alias;

/// User doc
pub struct User {
id: i32,
name: String,
}

const LIMIT: usize = 10;

fn run() {
let mut total: i32 = 0;
helper();
std::mem::drop(total);
}
"#;

    let batch = walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

    assert_eq!(batch.imports.len(), 1);
    assert_eq!(batch.imports[0].module, "std::fmt");
    assert_eq!(batch.imports[0].alias, "fmt_alias");
    assert!(batch.imports[0].is_std);

    assert_eq!(batch.structs.len(), 1);
    assert_eq!(batch.structs[0].name, "User");
    assert_eq!(batch.structs[0].field_count, 2);

    assert_eq!(batch.variables.len(), 2);
    assert_eq!(batch.variables[0].name, "LIMIT");
    assert_eq!(batch.variables[0].scope, "module");
    assert_eq!(batch.variables[1].name, "total");
    assert!(batch.variables[1].is_mutated);

    assert_eq!(batch.calls.len(), 2);
    assert_eq!(batch.calls[0].caller, "run");
    assert_eq!(batch.calls[0].callee, "helper");
    assert_eq!(batch.calls[1].callee, "std::mem::drop");
    assert!(batch.calls[1].is_external);

    assert_eq!(batch.comments.len(), 1);
    assert!(batch.comments[0].is_doc);
}

#[test]
fn maps_impl_traits_to_structs() {
    let source = r#"
trait Greeter {}

pub struct User {}

impl Greeter for User {}
"#;

    let batch = walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

    assert_eq!(batch.structs.len(), 1);
    assert_eq!(batch.structs[0].name, "User");
    assert_eq!(batch.structs[0].implements, "Greeter");
}

#[test]
fn counts_complexity() {
    let source = r#"
fn complex(n: i32) -> i32 {
if n > 0 {
    return 1;
}

for i in 0..n {
    if i % 2 == 0 {
        return i;
    }
}

0
}
"#;

    let batch = walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

    assert_eq!(batch.functions.len(), 1);
    assert_eq!(batch.functions[0].complexity, 4);
}
