use std::path::Path;

use ql_ast::{
    CallRow, CommentRow, FunctionRow, ImportRow, LanguageAdapter, StructRow, TableBatch, VariableRow,
};
use tree_sitter::Node;

pub struct GoAdapter;

impl GoAdapter {
    fn is_exported(name: &str) -> bool {
        name.starts_with(|c: char| c.is_uppercase())
    }

    fn count_params(parameters_node: Node, _source: &str) -> usize {
        let mut count = 0;
        let mut cursor = parameters_node.walk();
        for child in parameters_node.children(&mut cursor) {
            if child.kind() == "parameter_declaration" {
                count += 1;
            }
        }
        count
    }

    fn count_complexity(node: Node, _source: &str) -> usize {
        let mut score = 1;
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            match current.kind() {
                "if_statement" | "for_statement" | "range_clause"
                | "switch_statement" | "select_statement" | "case_clause"
                | "go_statement" => score += 1,
                _ => {}
            }
            let mut child_cursor = current.walk();
            if child_cursor.goto_first_child() {
                loop {
                    stack.push(child_cursor.node());
                    if !child_cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
        score
    }
}

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
        match node.kind() {
            "function_declaration" => self.map_function(node, source, rows),
            "method_declaration" => self.map_method(node, source, rows),
            "call_expression" => self.map_call(node, source, rows),
            "import_declaration" => self.map_imports(node, source, rows),
            "type_declaration" => self.map_type_decl(node, source, rows),
            "var_declaration" => self.map_var_decl(node, source, rows),
            "short_var_declaration" => self.map_short_var(node, source, rows),
            "comment" => self.map_comment(node, source, rows),
            _ => {}
        }
    }

    fn second_pass(&self, batch: &mut TableBatch, root: &Path) {
        let has_test_files = scan_test_files(root);

        for fn_row in &mut batch.functions {
            let test_file = fn_row.file.replace(".go", "_test.go");
            if has_test_files.contains(&test_file) {
                fn_row.has_test = true;
            }
        }

        for comment in &mut batch.comments {
            let text = comment.text.trim();
            if (text.starts_with("// ") && text.chars().nth(3).is_some_and(|c| c.is_uppercase()))
                || text.starts_with("/*")
            {
                comment.is_doc = true;
            }
            let nearest_fn = batch
                .functions
                .iter()
                .filter(|f| f.file == comment.file && f.line > comment.line)
                .min_by_key(|f| f.line);
            let nearest_struct = batch
                .structs
                .iter()
                .filter(|s| s.file == comment.file && s.line > comment.line)
                .min_by_key(|s| s.line);
            let nearest = match (nearest_fn, nearest_struct) {
                (Some(f), None) => Some(f.name.as_str()),
                (None, Some(s)) => Some(s.name.as_str()),
                (Some(f), Some(s)) if f.line <= s.line => Some(f.name.as_str()),
                (_, Some(s)) => Some(s.name.as_str()),
                _ => None,
            };
            if let Some(name) = nearest {
                comment.attached_to = name.to_string();
            }
        }
    }
}

impl GoAdapter {
    fn map_function(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else { return };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else { return };

        let params = node.child_by_field_name("parameters");
        let param_count = params.map(|p| Self::count_params(p, source)).unwrap_or(0);

        let return_type = node
            .child_by_field_name("result")
            .and_then(|r| r.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .to_string();

        let complexity = Self::count_complexity(node, source);

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            visibility: if Self::is_exported(name) {
                "public"
            } else {
                "private"
            }
            .to_string(),
            param_count,
            return_type,
            complexity,
            has_test: false,
        });
    }

    fn map_method(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else { return };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else { return };

        let params = node.child_by_field_name("parameters");
        let param_count = params.map(|p| Self::count_params(p, source)).unwrap_or(0);

        let return_type = node
            .child_by_field_name("result")
            .and_then(|r| r.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .to_string();

        let complexity = Self::count_complexity(node, source);

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            visibility: if Self::is_exported(name) {
                "public"
            } else {
                "private"
            }
            .to_string(),
            param_count,
            return_type,
            complexity,
            has_test: false,
        });
    }

    fn map_call(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(func_node) = node.child_by_field_name("function") else { return };
        let Ok(callee) = func_node.utf8_text(source.as_bytes()) else { return };

        let caller = find_enclosing_function(node, source).unwrap_or("");
        let is_external = callee.contains('.');

        rows.calls.push(CallRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            caller: caller.to_string(),
            callee: callee.to_string(),
            is_external,
        });
    }

    fn map_imports(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        use std::collections::VecDeque;
        let mut queue = VecDeque::from([node]);
        while let Some(current) = queue.pop_front() {
            if current.kind() == "import_spec" {
                let path = current
                    .child_by_field_name("path")
                    .and_then(|p| p.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("")
                    .to_string();
                let module = path.trim_matches('"').to_string();
                let alias = current
                    .child_by_field_name("name")
                    .and_then(|a| a.utf8_text(source.as_bytes()).ok())
                    .unwrap_or("")
                    .to_string();
                let is_std = !module.contains('.');

                rows.imports.push(ImportRow {
                    file: rows.current_file.clone(),
                    line: current.start_position().row + 1,
                    module,
                    alias,
                    is_std,
                });
            }
            let mut cursor = current.walk();
            if cursor.goto_first_child() {
                loop {
                    queue.push_back(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
    }

    fn map_type_decl(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "type_spec" {
                continue;
            }
            let Some(name_node) = child.child_by_field_name("name") else { continue };
            let Ok(name) = name_node.utf8_text(source.as_bytes()) else { continue };

            let type_node = child.child_by_field_name("type");
            let is_struct = type_node.is_some_and(|t| t.kind() == "struct_type");

            if !is_struct {
                continue;
            }

            let field_count = type_node
                .and_then(|t| {
                    let mut field_cursor = t.walk();
                    let field_list = t
                        .child_by_field_name("body")
                        .or_else(|| {
                            t.children(&mut field_cursor)
                                .find(|c| c.kind() == "field_declaration_list")
                        });
                    field_list.map(|fl| {
                        let mut fc = fl.walk();
                        fl.children(&mut fc)
                            .filter(|c| c.kind() == "field_declaration")
                            .count()
                    })
                })
                .unwrap_or(0);

            rows.structs.push(StructRow {
                file: rows.current_file.clone(),
                line: child.start_position().row + 1,
                name: name.to_string(),
                field_count,
                visibility: if Self::is_exported(name) {
                    "public"
                } else {
                    "private"
                }
                .to_string(),
                implements: String::new(),
            });
        }
    }

    fn map_var_decl(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "var_spec" {
                continue;
            }
            let Some(name_node) = child.child_by_field_name("name") else { continue };
            let Ok(name) = name_node.utf8_text(source.as_bytes()) else { continue };

            let type_hint = child
                .child_by_field_name("type")
                .and_then(|t| t.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .to_string();

            let scope = if node.parent().is_some_and(|p| p.kind() == "source_file") {
                "package"
            } else {
                "function"
            }
            .to_string();

            rows.variables.push(VariableRow {
                file: rows.current_file.clone(),
                line: child.start_position().row + 1,
                name: name.to_string(),
                type_hint,
                scope,
                is_mutated: false,
            });
        }
    }

    fn map_short_var(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(left_node) = node.child_by_field_name("left") else { return };
        let Ok(name) = left_node.utf8_text(source.as_bytes()) else { return };

        rows.variables.push(VariableRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            type_hint: String::new(),
            scope: "function".to_string(),
            is_mutated: true,
        });
    }

    fn map_comment(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Ok(text) = node.utf8_text(source.as_bytes()) else { return };
        let trimmed = text.trim();

        let is_doc = (trimmed.starts_with("// ")
            && trimmed.chars().nth(3).is_some_and(|c| c.is_uppercase()))
            || trimmed.starts_with("/*");

        rows.comments.push(CommentRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            text: trimmed.to_string(),
            attached_to: String::new(),
            is_doc,
        });
    }
}

fn find_enclosing_function<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            "function_declaration" | "method_declaration" => {
                return current
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok());
            }
            "source_file" => return None,
            _ => {
                current = current.parent()?;
            }
        }
    }
}

fn scan_test_files(root: &Path) -> std::collections::HashSet<String> {
    let mut test_files = std::collections::HashSet::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with("_test.go") {
                    test_files.insert(name.to_string());
                }
            }
        }
    }
    test_files
}

#[cfg(test)]
mod tests {
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
}
