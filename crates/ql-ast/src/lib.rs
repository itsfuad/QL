pub mod adapter;
pub mod analysis;
pub mod rows;
pub mod similarity;

pub use adapter::{LanguageAdapter, walk_source};
pub use analysis::second_pass;
pub use rows::{
    CallRow, CallSetRow, CommentRow, FingerprintRow, FunctionRow, ImportRow, SimilarityRow,
    StructRow, TableBatch, VariableRow,
};
pub use similarity::{compute_similarities, cosine_similarity, extract_callsets, jaccard_similarity};
