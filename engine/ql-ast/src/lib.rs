pub mod adapter;
pub mod rows;

pub use adapter::{LanguageAdapter, walk_source};
pub use rows::{CallRow, CommentRow, FunctionRow, ImportRow, StructRow, TableBatch, VariableRow};

pub fn second_pass(_batch: &mut TableBatch) {}
