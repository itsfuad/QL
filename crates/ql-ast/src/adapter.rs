use tree_sitter::{Node, Parser, TreeCursor};

use crate::rows::TableBatch;

pub trait LanguageAdapter: Send + Sync {
    fn language_name(&self) -> &str;
    fn grammar(&self) -> tree_sitter::Language;
    fn extensions(&self) -> &[&str];
    fn map_node(&self, node: Node<'_>, source: &str, rows: &mut TableBatch);
}

pub fn walk_source(
    adapter: &dyn LanguageAdapter,
    file: impl Into<String>,
    source: &str,
) -> Result<TableBatch, tree_sitter::LanguageError> {
    let mut parser = Parser::new();
    parser.set_language(&adapter.grammar())?;

    let mut batch = TableBatch::new(file);
    let Some(tree) = parser.parse(source, None) else {
        return Ok(batch);
    };

    let mut cursor = tree.walk();
    walk_node(adapter, &mut cursor, source, &mut batch);
    Ok(batch)
}

fn walk_node(
    adapter: &dyn LanguageAdapter,
    cursor: &mut TreeCursor<'_>,
    source: &str,
    rows: &mut TableBatch,
) {
    loop {
        adapter.map_node(cursor.node(), source, rows);

        if cursor.goto_first_child() {
            walk_node(adapter, cursor, source, rows);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}
