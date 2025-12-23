use ql_ast::{FunctionRow, LanguageAdapter, TableBatch};
use tree_sitter::Node;

pub struct GoAdapter;

impl LanguageAdapter for GoAdapter {
    fn language_name(&self) -> &str {
        "go"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn extensions(&self) -> &[&str] {
        &[".go"]
    }

    fn map_node(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        if node.kind() != "function_declaration" {
            return;
        }

        // We keep v1 mapping narrow on purpose: only top-level Go functions feed the
        // shared `functions` table until richer extraction lands.
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };

        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            // Tree-sitter rows are zero-based. Schema contract is one-based.
            line: node.start_position().row + 1,
            name: name.to_string(),
            ..FunctionRow::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use ql_ast::walk_source;

    use super::GoAdapter;

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
}
