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
mod tests {
    use ql_ast::{FunctionRow, TableBatch};
    use serde_json::Value;

    use super::{function_columns, query_all_functions, select_functions};
    use crate::QueryResult;

    #[test]
    fn reads_functions_from_duckdb() {
        let mut batch = TableBatch::new("ignored.rs");
        batch.functions.push(FunctionRow {
            file: "main.rs".to_string(),
            line: 4,
            name: "main".to_string(),
            visibility: "private".to_string(),
            param_count: 0,
            return_type: "".to_string(),
            complexity: 1,
            has_test: false,
        });
        batch.functions.push(FunctionRow {
            file: "math.rs".to_string(),
            line: 8,
            name: "Add".to_string(),
            visibility: "public".to_string(),
            param_count: 2,
            return_type: "int".to_string(),
            complexity: 1,
            has_test: true,
        });

        let rows = select_functions(&batch).expect("duckdb should load rows");

        assert_eq!(rows, batch.functions);
    }

    #[test]
    fn handles_empty_function_table() {
        let batch = TableBatch::new("empty.rs");

        let rows = select_functions(&batch).expect("duckdb should handle empty rows");

        assert!(rows.is_empty());
    }

    #[test]
    fn converts_functions_to_protocol_shape() {
        let mut batch = TableBatch::new("ignored.rs");
        batch.functions.push(FunctionRow {
            file: "main.rs".to_string(),
            line: 4,
            name: "main".to_string(),
            visibility: "private".to_string(),
            param_count: 0,
            return_type: "".to_string(),
            complexity: 1,
            has_test: false,
        });

        let result = query_all_functions(&batch).expect("query result should build");

        assert_eq!(
            result,
            QueryResult {
                columns: function_columns(),
                rows: vec![vec![
                    Value::String("main.rs".to_string()),
                    Value::from(4),
                    Value::String("main".to_string()),
                    Value::String("private".to_string()),
                    Value::from(0),
                    Value::String(String::new()),
                    Value::from(1),
                    Value::Bool(false),
                ]],
            }
        );
    }
}
