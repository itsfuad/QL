use ql_ast::{
    CallRow, CommentRow, FunctionRow, ImportRow, LanguageAdapter, StructRow, TableBatch,
    VariableRow,
};
use tree_sitter::Node;

pub struct RustAdapter;

impl RustAdapter {
    fn is_public(node: Node<'_>, source: &str) -> bool {
        node.children(&mut node.walk()).any(|child| {
            child.kind() == "visibility_modifier"
                && child
                    .utf8_text(source.as_bytes())
                    .is_ok_and(|text| text.trim() == "pub")
        })
    }

    fn count_params(parameters_node: Node<'_>) -> usize {
        parameters_node
            .children(&mut parameters_node.walk())
            .filter(|child| child.kind() == "parameter")
            .count()
    }

    fn count_complexity(node: Node<'_>) -> usize {
        let mut score = 1;
        let mut stack = vec![node];

        while let Some(current) = stack.pop() {
            match current.kind() {
                "if_expression" | "for_expression" | "while_expression" | "loop_expression"
                | "match_expression" | "match_arm" => score += 1,
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

        let param_count = node
            .child_by_field_name("parameters")
            .map(Self::count_params)
            .unwrap_or(0);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|ret| ret.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .trim()
            .trim_start_matches("->")
            .trim()
            .to_string();

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            visibility: if Self::is_public(node, source) {
                "public"
            } else {
                "private"
            }
            .to_string(),
            param_count,
            return_type,
            complexity: Self::count_complexity(node),
            has_test: false,
        });
    }

    fn map_call(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(function_node) = node.child_by_field_name("function") else {
            return;
        };
        let Ok(callee) = function_node.utf8_text(source.as_bytes()) else {
            return;
        };
        let caller = find_enclosing_function(node, source).unwrap_or("");

        rows.calls.push(CallRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            caller: caller.to_string(),
            callee: callee.to_string(),
            is_external: callee.contains("::") || callee.contains('.'),
        });
    }

    fn map_import(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Ok(module) = node.utf8_text(source.as_bytes()) else {
            return;
        };
        let module = module
            .trim_start_matches("use")
            .trim()
            .trim_end_matches(';');
        if module.is_empty() {
            return;
        }

        let alias = module
            .split(" as ")
            .nth(1)
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let module_name = module
            .split(" as ")
            .next()
            .unwrap_or(module)
            .trim()
            .to_string();
        let is_std = matches!(
            module_name.split("::").next(),
            Some("std" | "core" | "alloc")
        );

        rows.imports.push(ImportRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            module: module_name,
            alias,
            is_std,
        });
    }

    fn map_struct(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };

        let field_count = node
            .child_by_field_name("body")
            .map(|body| {
                body.children(&mut body.walk())
                    .filter(|child| child.kind() == "field_declaration")
                    .count()
            })
            .unwrap_or(0);

        rows.structs.push(StructRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            field_count,
            visibility: if Self::is_public(node, source) {
                "public"
            } else {
                "private"
            }
            .to_string(),
            implements: String::new(),
        });
    }

    fn map_impl_item(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(target_node) = node.child_by_field_name("type") else {
            return;
        };
        let Ok(target_name) = target_node.utf8_text(source.as_bytes()) else {
            return;
        };
        let Some(trait_node) = node.child_by_field_name("trait") else {
            return;
        };
        let Ok(trait_name) = trait_node.utf8_text(source.as_bytes()) else {
            return;
        };

        let target_name = simplify_type_name(target_name);
        let trait_name = simplify_type_name(trait_name);

        if trait_name.is_empty() || target_name.is_empty() {
            return;
        }

        if let Some(struct_row) = rows
            .structs
            .iter_mut()
            .find(|row| row.file == rows.current_file && row.name == target_name)
        {
            struct_row.implements = merge_csv(&struct_row.implements, &trait_name);
        }
    }

    fn map_const_or_static(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };
        let type_hint = node
            .child_by_field_name("type")
            .and_then(|ty| ty.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .to_string();

        rows.variables.push(VariableRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            type_hint,
            scope: "module".to_string(),
            is_mutated: node.kind() == "static_item",
        });
    }

    fn map_let(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(pattern_node) = node.child_by_field_name("pattern") else {
            return;
        };
        let Ok(name) = pattern_node.utf8_text(source.as_bytes()) else {
            return;
        };
        let type_hint = node
            .child_by_field_name("type")
            .and_then(|ty| ty.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .to_string();
        let is_mutated = node
            .children(&mut node.walk())
            .any(|child| child.kind() == "mutable_specifier");

        rows.variables.push(VariableRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.trim().to_string(),
            type_hint,
            scope: "function".to_string(),
            is_mutated,
        });
    }

    fn map_comment(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Ok(text) = node.utf8_text(source.as_bytes()) else {
            return;
        };
        let trimmed = text.trim();
        let is_doc =
            trimmed.starts_with("///") || trimmed.starts_with("//!") || trimmed.starts_with("/**");

        rows.comments.push(CommentRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            text: trimmed.to_string(),
            attached_to: String::new(),
            is_doc,
        });
    }
}

impl LanguageAdapter for RustAdapter {
    fn language_name(&self) -> &str {
        "rust"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn extensions(&self) -> &[&str] {
        &[".rs"]
    }

    fn map_node(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        match node.kind() {
            "function_item" => self.map_function(node, source, rows),
            "call_expression" => self.map_call(node, source, rows),
            "use_declaration" => self.map_import(node, source, rows),
            "struct_item" => self.map_struct(node, source, rows),
            "impl_item" => self.map_impl_item(node, source, rows),
            "const_item" | "static_item" => self.map_const_or_static(node, source, rows),
            "let_declaration" => self.map_let(node, source, rows),
            "line_comment" | "block_comment" => self.map_comment(node, source, rows),
            _ => {}
        }
    }
}

fn find_enclosing_function<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            "function_item" => {
                return current
                    .child_by_field_name("name")
                    .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            }
            "source_file" => return None,
            _ => current = current.parent()?,
        }
    }
}

fn merge_csv(existing: &str, addition: &str) -> String {
    let mut items = existing
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if !addition.trim().is_empty() && !items.iter().any(|item| item == addition.trim()) {
        items.push(addition.trim().to_string());
    }

    items.join(",")
}

fn simplify_type_name(value: &str) -> String {
    value
        .split("::")
        .last()
        .unwrap_or(value)
        .split('<')
        .next()
        .unwrap_or(value)
        .trim()
        .trim_start_matches('&')
        .to_string()
}

#[cfg(test)]
mod tests {
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

        let batch =
            walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

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

        let batch =
            walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

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

        let batch =
            walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

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

        let batch =
            walk_source(&RustAdapter, "main.rs", source).expect("rust grammar should parse");

        assert_eq!(batch.functions.len(), 1);
        assert_eq!(batch.functions[0].complexity, 4);
    }
}
