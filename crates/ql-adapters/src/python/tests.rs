use ql_ast::walk_source;

use super::PythonAdapter;

#[test]
fn maps_python_items() {
    let source = r#"
import os

class User(BaseUser, Serializable):
def greet(self, message):
    return message

def add(a, b):
return a + b

x = 1
"#;

    let batch =
        walk_source(&PythonAdapter, "main.py", source).expect("python grammar should parse");

    assert_eq!(batch.functions.len(), 2);
    assert_eq!(batch.functions[0].name, "greet");
    assert_eq!(batch.functions[1].name, "add");
    assert_eq!(batch.imports.len(), 1);
    assert_eq!(batch.structs.len(), 1);
    assert_eq!(batch.structs[0].implements, "BaseUser,Serializable");
    assert_eq!(batch.variables.len(), 1);
}
