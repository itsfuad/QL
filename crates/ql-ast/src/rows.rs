use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct FunctionRow {
    pub file: String,
    pub line: usize,
    pub name: String,
    pub visibility: String,
    pub param_count: usize,
    pub return_type: String,
    pub complexity: usize,
    pub has_test: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct CallRow {
    pub file: String,
    pub line: usize,
    pub caller: String,
    pub callee: String,
    pub is_external: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct ImportRow {
    pub file: String,
    pub line: usize,
    pub module: String,
    pub alias: String,
    pub is_std: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct StructRow {
    pub file: String,
    pub line: usize,
    pub name: String,
    pub field_count: usize,
    pub visibility: String,
    pub implements: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct VariableRow {
    pub file: String,
    pub line: usize,
    pub name: String,
    pub type_hint: String,
    pub scope: String,
    pub is_mutated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct CommentRow {
    pub file: String,
    pub line: usize,
    pub text: String,
    pub attached_to: String,
    pub is_doc: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableBatch {
    pub current_file: String,
    pub functions: Vec<FunctionRow>,
    pub calls: Vec<CallRow>,
    pub imports: Vec<ImportRow>,
    pub structs: Vec<StructRow>,
    pub variables: Vec<VariableRow>,
    pub comments: Vec<CommentRow>,
}

impl TableBatch {
    pub fn new(file: impl Into<String>) -> Self {
        Self {
            current_file: file.into(),
            ..Self::default()
        }
    }

    pub fn extend(&mut self, mut other: TableBatch) {
        self.functions.append(&mut other.functions);
        self.calls.append(&mut other.calls);
        self.imports.append(&mut other.imports);
        self.structs.append(&mut other.structs);
        self.variables.append(&mut other.variables);
        self.comments.append(&mut other.comments);
    }
}
