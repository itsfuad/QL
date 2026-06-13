use std::path::Path;

use ql_ast::{
    CallRow, CommentRow, FunctionRow, ImportRow, LanguageAdapter, StructRow, TableBatch, VariableRow,
};
use tree_sitter::Node;

pub struct PythonAdapter;

impl PythonAdapter {
    fn is_private(name: &str) -> String {
        if name.starts_with('_') {
            "private".to_string()
        } else {
            "public".to_string()
        }
    }

    fn count_params(parameters_node: Node<'_>) -> usize {
        let mut count = 0;
        let mut cursor = parameters_node.walk();
        for child in parameters_node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "typed_parameter" | "default_parameter"
                | "list_splat_pattern" | "dictionary_splat_pattern" => count += 1,
                _ => {}
            }
        }
        count
    }

    fn count_complexity(node: Node<'_>) -> usize {
        let mut score = 1;
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            match current.kind() {
                "if_statement" | "for_statement" | "while_statement"
                | "match_case" | "except_clause" | "elif_clause" => score += 1,
                "boolean_operator" => score += 1,
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
        let Some(name_node) = node.child_by_field_name("name") else { return };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else { return };

        let params = node
            .child_by_field_name("parameters")
            .map(Self::count_params)
            .unwrap_or(0);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|ret| ret.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .to_string();

        rows.functions.push(FunctionRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            visibility: Self::is_private(name),
            param_count: params,
            return_type,
            complexity: Self::count_complexity(node),
            has_test: false,
        });
    }

    fn map_class(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(name_node) = node.child_by_field_name("name") else { return };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else { return };
        let Some(body) = node.child_by_field_name("body") else { return };

        let mut field_count = 0;
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "function_definition" | "assignment" | "class_definition" => field_count += 1,
                _ => {}
            }
        }

        rows.structs.push(StructRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            field_count,
            visibility: Self::is_private(name),
            implements: String::new(),
        });
    }

    fn map_call(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(function_node) = node.child_by_field_name("function") else { return };
        let Ok(callee) = function_node.utf8_text(source.as_bytes()) else { return };
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
        let module = if node.kind() == "import_from_statement" {
            node.child_by_field_name("module_name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("")
                .to_string()
        } else {
            text.trim_start_matches("import ").to_string()
        };
        let alias = if let Some(pos) = text.rfind(" as ") {
            text[pos + 4..].trim().to_string()
        } else {
            String::new()
        };
        let is_std = matches!(
            module.as_str(),
            "os" | "sys" | "re" | "math" | "json" | "pathlib" | "typing" | "collections"
                | "itertools" | "functools" | "datetime" | "subprocess" | "threading" | "asyncio"
        );

        rows.imports.push(ImportRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            module,
            alias,
            is_std,
        });
    }

    fn map_assignment(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Some(left) = node.child_by_field_name("left") else { return };
        let Ok(name) = left.utf8_text(source.as_bytes()) else { return };

        let scope = if node.parent().is_some_and(|p| p.kind() == "module") {
            "module"
        } else {
            "function"
        }
        .to_string();

        rows.variables.push(VariableRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            name: name.to_string(),
            type_hint: String::new(),
            scope,
            is_mutated: true,
        });
    }

    fn map_comment(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        let Ok(text) = node.utf8_text(source.as_bytes()) else { return };
        let trimmed = text.trim();
        let is_doc = trimmed.starts_with("#:") || trimmed.starts_with("#.");

        rows.comments.push(CommentRow {
            file: rows.current_file.clone(),
            line: node.start_position().row + 1,
            text: trimmed.to_string(),
            attached_to: String::new(),
            is_doc,
        });
    }

}

impl LanguageAdapter for PythonAdapter {
    fn language_name(&self) -> &str {
        "python"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn extensions(&self) -> &[&str] {
        &[".py"]
    }

    fn map_node(&self, node: Node<'_>, source: &str, rows: &mut TableBatch) {
        match node.kind() {
            "function_definition" => self.map_function(node, source, rows),
            "class_definition" => self.map_class(node, source, rows),
            "call" => self.map_call(node, source, rows),
            "import_statement" | "import_from_statement" => self.map_import(node, source, rows),
            "assignment" => self.map_assignment(node, source, rows),
            "comment" => self.map_comment(node, source, rows),
            _ => {}
        }
    }

    fn second_pass(&self, batch: &mut TableBatch, root: &Path) {
        let test_files = scan_test_files(root);

        for function in &mut batch.functions {
            let base = function
                .file
                .strip_suffix(".py")
                .unwrap_or(function.file.as_str());
            let file_name = Path::new(&function.file)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let candidates = [
                format!("{base}_test.py"),
                format!("test_{file_name}"),
                format!("tests/{file_name}"),
                format!("tests/test_{file_name}"),
            ];
            if candidates.iter().any(|candidate| test_files.contains(candidate)) {
                function.has_test = true;
            }
        }

        for comment in &mut batch.comments {
            let text = comment.text.trim();
            if text.starts_with("#:") {
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

fn find_enclosing_function<'a>(node: Node<'a>, source: &'a str) -> Option<&'a str> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            "function_definition" => {
                return current
                    .child_by_field_name("name")
                    .and_then(|name| name.utf8_text(source.as_bytes()).ok());
            }
            "source_file" => return None,
            _ => current = current.parent()?,
        }
    }
}

fn scan_test_files(root: &Path) -> std::collections::HashSet<String> {
    let mut test_files = std::collections::HashSet::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }

            let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
            if name.starts_with("test_") && name.ends_with(".py") || name.ends_with("_test.py") {
                test_files.insert(name.to_string());
            }
        }
    }

    test_files
}

#[cfg(test)]
mod tests {
    use ql_ast::walk_source;

    use super::PythonAdapter;

    #[test]
    fn maps_python_items() {
        let source = r#"
import os

class User:
    def greet(self, message):
        return message

def add(a, b):
    return a + b

x = 1
"#;

        let batch = walk_source(&PythonAdapter, "main.py", source).expect("python grammar should parse");

        assert_eq!(batch.functions.len(), 2);
        assert_eq!(batch.functions[0].name, "greet");
        assert_eq!(batch.functions[1].name, "add");
        assert_eq!(batch.imports.len(), 1);
        assert_eq!(batch.structs.len(), 1);
        assert_eq!(batch.variables.len(), 1);
    }
}
