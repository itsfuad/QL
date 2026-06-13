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
