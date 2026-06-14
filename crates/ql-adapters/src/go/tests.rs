use super::GoAdapter;
use ql_ast::walk_source;

#[test]
fn maps_go_function_declarations() {
    let source = r#"
package main

func main() {}

func add(a int, b int) int {
return a + b
}
"#;

    let batch = walk_source(&GoAdapter, "main.go", source).expect("go grammar should parse");

    assert_eq!(batch.functions.len(), 2);
    assert_eq!(batch.functions[0].name, "main");
    assert_eq!(batch.functions[0].file, "main.go");
    assert_eq!(batch.functions[0].line, 4);
    assert_eq!(batch.functions[1].name, "add");
    assert_eq!(batch.functions[1].line, 6);
}
