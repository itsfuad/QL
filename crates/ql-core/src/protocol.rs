use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct EngineRequest {
    pub query: String,
    pub root: String,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<Value>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl EngineResponse {
    pub fn from_result(result: QueryResult) -> Self {
        Self {
            columns: Some(result.columns),
            rows: Some(result.rows),
            error: None,
        }
    }

    pub fn from_error(error: impl Into<String>) -> Self {
        Self {
            columns: None,
            rows: None,
            error: Some(error.into()),
        }
    }
}
