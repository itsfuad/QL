use ql_ast::{
    CallRow, CommentRow, FunctionRow, ImportRow, LanguageAdapter, StructRow, TableBatch,
    VariableRow,
};
use tree_sitter::Node;

pub struct TypeScriptAdapter;

impl TypeScriptAdapter {
    fn is_public(name: &str, node: Node<'_>, source: &str) -> String {
        let mut visibility = "public";
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "accessibility_modifier" {
                continue;
            }
            let Ok(text) = child.utf8_text(source.as_bytes()) else {
                continue;
            };
            match text.trim() {
                "private" => return "private".to_string(),
                "protected" => visibility = "internal",
                _ => {}
            }
        }

        if name.starts_with('_') {
            return "private".to_string();
        }

        visibility.to_string()
    }

    fn count_params(parameters_node: Node<'_>) -> usize {
        let mut count = 0;
        let mut cursor = parameters_node.walk();
        for child in parameters_node.children(&mut cursor) {
            match child.kind() {
                "required_parameter" | "optional_parameter" | "rest_parameter" => count += 1,
                _ => {}
            }
        }
        count
    }

    fn count_complexity(node: Node<'_>, source: &str) -> usize {
        let mut score = 1;
        let mut stack = vec![node];

        while let Some(current) = stack.pop() {
            match current.kind() {
                "if_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "switch_case"
                | "catch_clause"
                | "conditional_expression" => score += 1,
                "binary_expression" => {
                    let op = current
                        .child_by_field_name("operator")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .unwrap_or("");
                    if op.trim() == "&&" || op.trim() == "||" {
                        score += 1;
                    }
                }
                _ => {}
            }

            let mut cursor = current.walk();
            if cursor.goto_first_child() {
                loop {
                    stack.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }

        score
    }

    fn map_function(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };

        let params = node
            .child_by_field_name("parameters")
            .map(Self::count_params)
            .unwrap_or(0);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|ret| ret.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .trim()
            .trim_start_matches(':')
            .trim()
            .to_string();

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            visibility: Self::is_public(name, node, source),
            param_count: params,
            return_type,
            complexity: Self::count_complexity(node, source),
            has_test: false,
        });
    }

    fn map_method(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };

        let params = node
            .child_by_field_name("parameters")
            .map(Self::count_params)
            .unwrap_or(0);
        let return_type = node
            .child_by_field_name("result")
            .and_then(|ret| ret.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .trim()
            .trim_start_matches(':')
            .trim()
            .to_string();

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            visibility: Self::is_public(name, node, source),
            param_count: params,
            return_type,
            complexity: Self::count_complexity(node, source),
            has_test: false,
        });
    }

    fn map_call(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(func_node) = node.child_by_field_name("function") else {
            return;
        };
        let Ok(callee) = func_node.utf8_text(source.as_bytes()) else {
            return;
        };
        let caller = find_enclosing_function(node, source).unwrap_or("");

        rows.calls.push(CallRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            caller: caller.to_string(),
            callee: callee.to_string(),
            is_external: callee.contains('.'),
        });
    }

    fn map_import(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let text = node.utf8_text(source.as_bytes()).unwrap_or("").trim();
        let module = node
            .child_by_field_name("source")
            .and_then(|s| s.utf8_text(source.as_bytes()).ok())
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_else(|| text.trim_start_matches("import ").to_string());
        let alias = if let Some(pos) = text.rfind(" as ") {
            text[pos + 4..].trim_end_matches(';').to_string()
        } else {
            String::new()
        };
        let is_std = matches!(
            module.as_str(),
            "fs" | "path"
                | "url"
                | "util"
                | "os"
                | "crypto"
                | "events"
                | "stream"
                | "buffer"
                | "assert"
                | "http"
                | "https"
                | "tty"
                | "net"
        ) || module.starts_with("node:");

        rows.imports.push(ImportRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            module,
            alias,
            is_std,
        });
    }

    fn map_class(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };

        let mut field_count = 0;
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "method_definition"
                | "public_field_definition"
                | "property_signature"
                | "public_method_definition"
                | "index_signature" => field_count += 1,
                _ => {}
            }
        }

        rows.structs.push(StructRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            field_count,
            visibility: Self::is_public(name, node, source),
            implements: Self::implements(node, source),
        });
    }

    fn implements(node: Node<'_>, source: &str) -> String {
        let mut cursor = node.walk();
        let Some(heritage) = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "class_heritage")
        else {
            return String::new();
        };

        let mut cursor = heritage.walk();
        let Some(implements_clause) = heritage
            .named_children(&mut cursor)
            .find(|child| child.kind() == "implements_clause")
        else {
            return String::new();
        };

        let mut names = Vec::new();
        let mut cursor = implements_clause.walk();
        for child in implements_clause.named_children(&mut cursor) {
            if let Ok(name) = child.utf8_text(source.as_bytes()) {
                names.push(name.to_string());
            }
        }
        names.join(",")
    }

    fn map_variable(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let kind = node.kind();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "variable_declarator" {
                continue;
            }

            let Some(name_node) = child.child_by_field_name("name") else {
                continue;
            };
            let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
                continue;
            };
            let type_hint = child
                .child_by_field_name("type")
                .and_then(|ty| ty.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .to_string();

            let scope = if node.parent().is_some_and(|p| p.kind() == "source_file") {
                "module"
            } else {
                "function"
            }
            .to_string();

            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            let is_mutated = match kind {
                "lexical_declaration" => !text.trim_start().starts_with("const "),
                "variable_declaration" => true,
                _ => false,
            };

            rows.variables.push(VariableRow {
                file: rows.current_file.clone(),
                line: child.start_position().row + 1,
                name: name.to_string(),
                type_hint,
                scope,
                is_mutated,
            });
        }
    }

    fn map_comment(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Ok(text) = node.utf8_text(source.as_bytes()) else {
            return;
        };
        let trimmed = text.trim();
        let is_doc =
            trimmed.starts_with("/**") || trimmed.starts_with("///") || trimmed.starts_with("/*!");

        rows.comments.push(CommentRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            text: trimmed.to_string(),
            attached_to: String::new(),
            is_doc,
        });
    }
}

impl LanguageAdapter for TypeScriptAdapter {
    fn language_name(&self) -> &str {
        "typescript"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    }

    fn extensions(&self) -> &[&str] {
        &[".ts", ".tsx"]
    }

    fn map_node(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        match node.kind() {
            "function_declaration" => self.map_function(node, source, rows),
            "method_definition" => self.map_method(node, source, rows),
            "call_expression" => self.map_call(node, source, rows),
            "import_statement" => self.map_import(node, source, rows),
            "class_declaration" | "abstract_class_declaration" => {
                self.map_class(node, source, rows)
            }
            "lexical_declaration" | "variable_declaration" => self.map_variable(node, source, rows),
            "comment" => self.map_comment(node, source, rows),
            _ => {}
        }
    }
}

fn find_enclosing_function<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            "function_declaration" | "method_definition" => {
                return current
                    .child_by_field_name("name")
                    .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            }
            "source_file" => return None,
            _ => current = current.parent()?,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
