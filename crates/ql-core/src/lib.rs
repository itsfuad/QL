use duckdb::{params, Connection};
use ql_ast::{FunctionRow, TableBatch};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ENGINE_NAME: &str = "ql-engine";

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

pub fn select_functions(batch: &TableBatch) -> Result<Vec<FunctionRow>, duckdb::Error> {
    let connection = Connection::open_in_memory()?;
    create_schema(&connection)?;
    insert_batch(&connection, batch)?;

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
        columns: vec![
            "file".to_string(),
            "line".to_string(),
            "name".to_string(),
            "visibility".to_string(),
            "param_count".to_string(),
            "return_type".to_string(),
            "complexity".to_string(),
            "has_test".to_string(),
        ],
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

fn create_schema(connection: &Connection) -> Result<(), duckdb::Error> {
    // Schema stays language-agnostic. Adapters normalize language-specific syntax
    // before rows reach DuckDB.
    connection.execute_batch(
        "CREATE TABLE functions (
            file TEXT NOT NULL,
            line BIGINT NOT NULL,
            name TEXT NOT NULL,
            visibility TEXT NOT NULL,
            param_count BIGINT NOT NULL,
            return_type TEXT NOT NULL,
            complexity BIGINT NOT NULL,
            has_test BOOLEAN NOT NULL
        );
        CREATE TABLE calls (
            file TEXT NOT NULL,
            line BIGINT NOT NULL,
            caller TEXT NOT NULL,
            callee TEXT NOT NULL,
            is_external BOOLEAN NOT NULL
        );
        CREATE TABLE imports (
            file TEXT NOT NULL,
            line BIGINT NOT NULL,
            module TEXT NOT NULL,
            alias TEXT NOT NULL,
            is_std BOOLEAN NOT NULL
        );
        CREATE TABLE structs (
            file TEXT NOT NULL,
            line BIGINT NOT NULL,
            name TEXT NOT NULL,
            field_count BIGINT NOT NULL,
            visibility TEXT NOT NULL,
            implements TEXT NOT NULL
        );
        CREATE TABLE variables (
            file TEXT NOT NULL,
            line BIGINT NOT NULL,
            name TEXT NOT NULL,
            type_hint TEXT NOT NULL,
            scope TEXT NOT NULL,
            is_mutated BOOLEAN NOT NULL
        );
        CREATE TABLE comments (
            file TEXT NOT NULL,
            line BIGINT NOT NULL,
            text TEXT NOT NULL,
            attached_to TEXT NOT NULL,
            is_doc BOOLEAN NOT NULL
        );",
    )
}

fn insert_batch(connection: &Connection, batch: &TableBatch) -> Result<(), duckdb::Error> {
    // We insert table-by-table so ingestion logic mirrors shared schema directly and
    // stays easy to extend when new adapters start filling more tables.
    let mut functions = connection.prepare(
        "INSERT INTO functions
         (file, line, name, visibility, param_count, return_type, complexity, has_test)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    for row in &batch.functions {
        functions.execute(params![
            &row.file,
            row.line,
            &row.name,
            &row.visibility,
            row.param_count,
            &row.return_type,
            row.complexity,
            row.has_test,
        ])?;
    }

    let mut calls = connection.prepare(
        "INSERT INTO calls (file, line, caller, callee, is_external)
         VALUES (?, ?, ?, ?, ?)",
    )?;
    for row in &batch.calls {
        calls.execute(params![
            &row.file,
            row.line,
            &row.caller,
            &row.callee,
            row.is_external,
        ])?;
    }

    let mut imports = connection.prepare(
        "INSERT INTO imports (file, line, module, alias, is_std)
         VALUES (?, ?, ?, ?, ?)",
    )?;
    for row in &batch.imports {
        imports.execute(params![
            &row.file,
            row.line,
            &row.module,
            &row.alias,
            row.is_std
        ])?;
    }

    let mut structs = connection.prepare(
        "INSERT INTO structs (file, line, name, field_count, visibility, implements)
         VALUES (?, ?, ?, ?, ?, ?)",
    )?;
    for row in &batch.structs {
        structs.execute(params![
            &row.file,
            row.line,
            &row.name,
            row.field_count,
            &row.visibility,
            &row.implements,
        ])?;
    }

    let mut variables = connection.prepare(
        "INSERT INTO variables (file, line, name, type_hint, scope, is_mutated)
         VALUES (?, ?, ?, ?, ?, ?)",
    )?;
    for row in &batch.variables {
        variables.execute(params![
            &row.file,
            row.line,
            &row.name,
            &row.type_hint,
            &row.scope,
            row.is_mutated,
        ])?;
    }

    let mut comments = connection.prepare(
        "INSERT INTO comments (file, line, text, attached_to, is_doc)
         VALUES (?, ?, ?, ?, ?)",
    )?;
    for row in &batch.comments {
        comments.execute(params![
            &row.file,
            row.line,
            &row.text,
            &row.attached_to,
            row.is_doc,
        ])?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use ql_ast::{FunctionRow, TableBatch};

    use super::{query_all_functions, select_functions, QueryResult};

    #[test]
    fn reads_functions_from_duckdb() {
        let mut batch = TableBatch::new("ignored.go");
        batch.functions.push(FunctionRow {
            file: "main.go".to_string(),
            line: 4,
            name: "main".to_string(),
            visibility: "private".to_string(),
            param_count: 0,
            return_type: "".to_string(),
            complexity: 1,
            has_test: false,
        });
        batch.functions.push(FunctionRow {
            file: "math.go".to_string(),
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
        let batch = TableBatch::new("empty.go");

        let rows = select_functions(&batch).expect("duckdb should handle empty rows");

        assert!(rows.is_empty());
    }

    #[test]
    fn converts_functions_to_protocol_shape() {
        let mut batch = TableBatch::new("ignored.go");
        batch.functions.push(FunctionRow {
            file: "main.go".to_string(),
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
                columns: vec![
                    "file".to_string(),
                    "line".to_string(),
                    "name".to_string(),
                    "visibility".to_string(),
                    "param_count".to_string(),
                    "return_type".to_string(),
                    "complexity".to_string(),
                    "has_test".to_string(),
                ],
                rows: vec![vec![
                    Value::String("main.go".to_string()),
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
