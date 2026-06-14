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
mod tests;
