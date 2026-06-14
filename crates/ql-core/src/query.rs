use ql_ast::{FunctionRow, TableBatch};
use serde_json::Value;

use crate::{protocol::QueryResult, storage::open_batch};

pub fn function_columns() -> Vec<String> {
    vec![
        "file".to_string(),
        "line".to_string(),
        "name".to_string(),
        "visibility".to_string(),
        "param_count".to_string(),
        "return_type".to_string(),
        "complexity".to_string(),
        "has_test".to_string(),
    ]
}

pub fn select_functions(batch: &TableBatch) -> Result<Vec<FunctionRow>, duckdb::Error> {
    let connection = open_batch(batch)?;
    let mut statement = connection.prepare(
        "SELECT file, line, name, visibility, param_count, return_type, complexity, has_test
         FROM functions
         ORDER BY file, line, name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(FunctionRow {
            file: row.get(0)?,
            line: row.get(1)?,
            name: row.get(2)?,
            visibility: row.get(3)?,
            param_count: row.get(4)?,
            return_type: row.get(5)?,
            complexity: row.get(6)?,
            has_test: row.get(7)?,
        })
    })?;

    rows.collect()
}

pub fn query_all_functions(batch: &TableBatch) -> Result<QueryResult, duckdb::Error> {
    let rows = select_functions(batch)?;

    Ok(QueryResult {
        columns: function_columns(),
        rows: rows
            .into_iter()
            .map(|row| {
                vec![
                    Value::String(row.file),
                    Value::from(row.line),
                    Value::String(row.name),
                    Value::String(row.visibility),
                    Value::from(row.param_count),
                    Value::String(row.return_type),
                    Value::from(row.complexity),
                    Value::Bool(row.has_test),
                ]
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests;
