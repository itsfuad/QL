use duckdb::{Connection, params};
use ql_ast::TableBatch;

pub fn open_batch(batch: &TableBatch) -> Result<Connection, duckdb::Error> {
    let connection = Connection::open_in_memory()?;
    create_schema(&connection)?;
    insert_batch(&connection, batch)?;
    Ok(connection)
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
    // We insert table-by-table, using DuckDB's Appender API for bulk loading. Appenders
    // buffer rows columnar-side and avoid the per-row prepared-statement/transaction
    // overhead of `INSERT` (each `execute()` would otherwise auto-commit on its own).
    let mut functions = connection.appender("functions")?;
    for row in &batch.functions {
        functions.append_row(params![
            &row.file,
            row.line as i64,
            &row.name,
            &row.visibility,
            row.param_count as i64,
            &row.return_type,
            row.complexity as i64,
            row.has_test,
        ])?;
    }
    functions.flush()?;

    let mut calls = connection.appender("calls")?;
    for row in &batch.calls {
        calls.append_row(params![
            &row.file,
            row.line as i64,
            &row.caller,
            &row.callee,
            row.is_external,
        ])?;
    }
    calls.flush()?;

    let mut imports = connection.appender("imports")?;
    for row in &batch.imports {
        imports.append_row(params![
            &row.file,
            row.line as i64,
            &row.module,
            &row.alias,
            row.is_std
        ])?;
    }
    imports.flush()?;

    let mut structs = connection.appender("structs")?;
    for row in &batch.structs {
        structs.append_row(params![
            &row.file,
            row.line as i64,
            &row.name,
            row.field_count as i64,
            &row.visibility,
            &row.implements,
        ])?;
    }
    structs.flush()?;

    let mut variables = connection.appender("variables")?;
    for row in &batch.variables {
        variables.append_row(params![
            &row.file,
            row.line as i64,
            &row.name,
            &row.type_hint,
            &row.scope,
            row.is_mutated,
        ])?;
    }
    variables.flush()?;

    let mut comments = connection.appender("comments")?;
    for row in &batch.comments {
        comments.append_row(params![
            &row.file,
            row.line as i64,
            &row.text,
            &row.attached_to,
            row.is_doc,
        ])?;
    }
    comments.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use ql_ast::{CallRow, CommentRow, FunctionRow, ImportRow, StructRow, TableBatch, VariableRow};
    use serde_json::Value;

    use crate::QueryResult;
    use crate::execute::execute_query;
    use crate::sql::parse_query;

    // Regression tests for the DuckDB Appender-based ingestion in `insert_batch`.
    //
    // These confirm that switching row-by-row `INSERT` statements to the Appender
    // API (for bulk-load performance) preserves exact query results: same row
    // counts, same column values (including booleans, zeros, and empty strings),
    // and same ordering behavior under `ORDER BY`/`LIMIT`.

    fn run(batch: &TableBatch, sql: &str) -> QueryResult {
        let statement = parse_query(sql).expect("query should parse");
        execute_query(batch, &statement).expect("query should execute")
    }

    #[test]
    fn inserts_and_queries_rows_in_every_table() {
        let mut batch = TableBatch::new("");

        batch.functions.push(FunctionRow {
            file: "src/lib.rs".to_string(),
            line: 10,
            name: "add".to_string(),
            visibility: "public".to_string(),
            param_count: 2,
            return_type: "i32".to_string(),
            complexity: 1,
            has_test: true,
        });
        batch.calls.push(CallRow {
            file: "src/lib.rs".to_string(),
            line: 11,
            caller: "add".to_string(),
            callee: "checked_add".to_string(),
            is_external: false,
        });
        batch.imports.push(ImportRow {
            file: "src/lib.rs".to_string(),
            line: 1,
            module: "std::fmt".to_string(),
            alias: String::new(),
            is_std: true,
        });
        batch.structs.push(StructRow {
            file: "src/lib.rs".to_string(),
            line: 20,
            name: "Point".to_string(),
            field_count: 2,
            visibility: "public".to_string(),
            implements: "Display".to_string(),
        });
        batch.variables.push(VariableRow {
            file: "src/lib.rs".to_string(),
            line: 21,
            name: "origin".to_string(),
            type_hint: "Point".to_string(),
            scope: "module".to_string(),
            is_mutated: false,
        });
        batch.comments.push(CommentRow {
            file: "src/lib.rs".to_string(),
            line: 9,
            text: "/// Adds two numbers".to_string(),
            attached_to: "add".to_string(),
            is_doc: true,
        });

        let functions = run(&batch, "SELECT * FROM functions");
        assert_eq!(
            functions.columns,
            vec![
                "file",
                "line",
                "name",
                "visibility",
                "param_count",
                "return_type",
                "complexity",
                "has_test",
            ]
        );
        assert_eq!(
            functions.rows,
            vec![vec![
                Value::String("src/lib.rs".to_string()),
                Value::from(10),
                Value::String("add".to_string()),
                Value::String("public".to_string()),
                Value::from(2),
                Value::String("i32".to_string()),
                Value::from(1),
                Value::Bool(true),
            ]]
        );

        let calls = run(&batch, "SELECT * FROM calls");
        assert_eq!(
            calls.rows,
            vec![vec![
                Value::String("src/lib.rs".to_string()),
                Value::from(11),
                Value::String("add".to_string()),
                Value::String("checked_add".to_string()),
                Value::Bool(false),
            ]]
        );

        let imports = run(&batch, "SELECT * FROM imports");
        assert_eq!(
            imports.rows,
            vec![vec![
                Value::String("src/lib.rs".to_string()),
                Value::from(1),
                Value::String("std::fmt".to_string()),
                Value::String(String::new()),
                Value::Bool(true),
            ]]
        );

        let structs = run(&batch, "SELECT * FROM structs");
        assert_eq!(
            structs.rows,
            vec![vec![
                Value::String("src/lib.rs".to_string()),
                Value::from(20),
                Value::String("Point".to_string()),
                Value::from(2),
                Value::String("public".to_string()),
                Value::String("Display".to_string()),
            ]]
        );

        let variables = run(&batch, "SELECT * FROM variables");
        assert_eq!(
            variables.rows,
            vec![vec![
                Value::String("src/lib.rs".to_string()),
                Value::from(21),
                Value::String("origin".to_string()),
                Value::String("Point".to_string()),
                Value::String("module".to_string()),
                Value::Bool(false),
            ]]
        );

        let comments = run(&batch, "SELECT * FROM comments");
        assert_eq!(
            comments.rows,
            vec![vec![
                Value::String("src/lib.rs".to_string()),
                Value::from(9),
                Value::String("/// Adds two numbers".to_string()),
                Value::String("add".to_string()),
                Value::Bool(true),
            ]]
        );
    }

    #[test]
    fn empty_batch_produces_empty_tables() {
        let batch = TableBatch::new("");

        let result = run(&batch, "SELECT * FROM functions");
        assert!(result.rows.is_empty());

        let result = run(&batch, "SELECT * FROM comments");
        assert!(result.rows.is_empty());
    }

    #[test]
    fn preserves_ordering_and_filtering_over_a_larger_batch() {
        let mut batch = TableBatch::new("");

        for i in 0..500 {
            batch.functions.push(FunctionRow {
                file: format!("src/file_{i}.rs"),
                line: i + 1,
                name: format!("fn_{i}"),
                visibility: if i % 2 == 0 { "public" } else { "private" }.to_string(),
                param_count: i % 5,
                return_type: String::new(),
                complexity: i % 20,
                has_test: i % 3 == 0,
            });
        }

        let high_complexity = run(
            &batch,
            "SELECT name, complexity FROM functions WHERE complexity > 15 ORDER BY complexity DESC, name ASC",
        );
        assert!(!high_complexity.rows.is_empty());
        for row in &high_complexity.rows {
            let Value::Number(complexity) = &row[1] else {
                panic!("expected numeric complexity")
            };
            assert!(complexity.as_u64().unwrap() > 15);
        }

        // complexity is i % 20, so the maximum possible value is 19.
        assert_eq!(high_complexity.rows[0][1], Value::from(19));

        let public_with_tests = run(
            &batch,
            "SELECT name FROM functions WHERE visibility = 'public' AND has_test = true",
        );
        for row in &public_with_tests.rows {
            let Value::String(name) = &row[0] else {
                panic!("expected string name")
            };
            let i: usize = name.strip_prefix("fn_").unwrap().parse().unwrap();
            assert_eq!(i % 2, 0);
            assert_eq!(i % 3, 0);
        }
        // i in 0..500 with i % 2 == 0 and i % 3 == 0 => i % 6 == 0 => 0,6,..,498 => 84 values.
        assert_eq!(public_with_tests.rows.len(), 84);
    }
}
