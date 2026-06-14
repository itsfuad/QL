use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::rows::TableBatch;

pub fn second_pass(batch: &mut TableBatch) {
    resolve_has_test(batch);
    resolve_implements(batch);
    resolve_comment_attachments(batch);
}

fn resolve_has_test(batch: &mut TableBatch) {
    let mut test_keys_by_package: HashMap<String, HashSet<String>> = HashMap::new();

    for function in &batch.functions {
        if let Some(key) = test_key(&function.name) {
            test_keys_by_package
                .entry(package_key(&function.file))
                .or_default()
                .insert(key);
        }
    }

    for function in &mut batch.functions {
        let package = package_key(&function.file);
        let Some(test_keys) = test_keys_by_package.get(&package) else {
            continue;
        };

        if test_keys.contains(&function_key(&function.name)) {
            function.has_test = true;
        }
    }
}

fn resolve_implements(batch: &mut TableBatch) {
    for row in &mut batch.structs {
        row.implements = normalize_csv_list(&row.implements);
    }
}

/// Attaches each comment to the nearest function or struct declared after it in the
/// same file (ties go to the function, matching the previous row-by-row scan).
///
/// Rather than scanning every function/struct for every comment (O(C * (F + S)),
/// which dominates `second_pass` on large batches), this groups declarations by file
/// once into a line-sorted list and binary-searches it per comment: O((C + F + S)
/// log(F + S)) overall.
fn resolve_comment_attachments(batch: &mut TableBatch) {
    // Tie-break tag: functions sort before structs on an equal line, so a
    // `partition_point` lookup reproduces the original "function wins on tie" rule.
    const FUNCTION_TAG: u8 = 0;
    const STRUCT_TAG: u8 = 1;

    let mut definitions_by_file: HashMap<&str, Vec<(usize, u8, &str)>> = HashMap::new();

    for function in &batch.functions {
        definitions_by_file
            .entry(function.file.as_str())
            .or_default()
            .push((function.line, FUNCTION_TAG, function.name.as_str()));
    }
    for struct_row in &batch.structs {
        definitions_by_file
            .entry(struct_row.file.as_str())
            .or_default()
            .push((struct_row.line, STRUCT_TAG, struct_row.name.as_str()));
    }

    for definitions in definitions_by_file.values_mut() {
        definitions.sort_unstable_by_key(|&(line, tag, _)| (line, tag));
    }

    for comment in &mut batch.comments {
        comment.attached_to = definitions_by_file
            .get(comment.file.as_str())
            .and_then(|definitions| {
                let index = definitions.partition_point(|&(line, _, _)| line <= comment.line);
                definitions.get(index)
            })
            .map(|&(_, _, name)| name.to_string())
            .unwrap_or_default();
    }
}

fn package_key(file: &str) -> String {
    Path::new(file)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn function_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn test_key(name: &str) -> Option<String> {
    if let Some(stripped) = name.strip_prefix("test_") {
        return Some(function_key(stripped));
    }
    if let Some(stripped) = name.strip_suffix("_test") {
        return Some(function_key(stripped));
    }
    if let Some(stripped) = name.strip_prefix("Test") {
        if stripped.is_empty() {
            return None;
        }
        return Some(function_key(stripped));
    }
    None
}

fn normalize_csv_list(value: &str) -> String {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if seen.insert(item.to_string()) {
            items.push(item.to_string());
        }
    }

    items.join(",")
}

#[cfg(test)]
mod tests;
