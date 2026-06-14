use ql_ast::walk_source;

use super::TypeScriptAdapter;

#[test]
fn maps_typescript_items() {
    let source = r#"
import { readFileSync as readFile } from "fs";

class Person implements Greeter, Serializable {
  name: string;
  greet(message: string): string {
return message;
  }
}

function add(a: number, b: number): number {
  return a + b;
}

const answer: number = 42;
"#;

    let batch = walk_source(&TypeScriptAdapter, "main.ts", source)
        .expect("typescript grammar should parse");

    assert_eq!(batch.functions.len(), 2);
    assert_eq!(batch.functions[0].name, "greet");
    assert_eq!(batch.functions[1].name, "add");
    assert_eq!(batch.imports.len(), 1);
    assert_eq!(batch.imports[0].module, "fs");
    assert_eq!(batch.structs.len(), 1);
    assert_eq!(batch.structs[0].implements, "Greeter,Serializable");
    assert_eq!(batch.variables.len(), 1);
}
