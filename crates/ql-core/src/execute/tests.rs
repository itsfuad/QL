use ql_ast::{CallRow, FunctionRow, TableBatch};
use serde_json::Value;

use super::execute_query;
use crate::sql::parse_query;

#[test]
fn selects_requested_columns() {
    let mut batch = sample_batch();
    batch.functions.push(function_row("main.rs", 4, "main", 3));

    let result = execute(
        "SELECT name, complexity FROM functions ORDER BY line",
        &batch,
    );

    assert_eq!(result.columns, vec!["name", "complexity"]);
    assert_eq!(
        result.rows,
        vec![vec![Value::String("main".to_string()), Value::from(3)]]
    );
}

#[test]
fn filters_orders_and_limits() {
    let mut batch = sample_batch();
    batch.functions.push(function_row("main.rs", 4, "main", 3));
    batch.functions.push(function_row("math.rs", 8, "Add", 9));
    batch.functions.push(function_row("math.rs", 12, "Sub", 5));

    let result = execute(
        "SELECT name, complexity FROM functions WHERE complexity > 4 ORDER BY complexity DESC LIMIT 2",
        &batch,
    );

    assert_eq!(
        result.rows,
        vec![
            vec![Value::String("Add".to_string()), Value::from(9)],
            vec![Value::String("Sub".to_string()), Value::from(5)],
        ]
    );
}

#[test]
fn joins_related_tables() {
    let mut batch = sample_batch();
    batch.functions.push(function_row("main.rs", 4, "main", 3));
    batch.calls.push(CallRow {
        file: "main.rs".to_string(),
        line: 5,
        caller: "main".to_string(),
        callee: "fmt.Println".to_string(),
        is_external: true,
    });

    let result = execute(
        "SELECT functions.name, calls.callee FROM functions JOIN calls ON functions.name = calls.caller",
        &batch,
    );

    assert_eq!(
        result.rows,
        vec![vec![
            Value::String("main".to_string()),
            Value::String("fmt.Println".to_string()),
        ]]
    );
}

#[test]
fn supports_string_predicates() {
    let mut batch = sample_batch();
    batch.functions.push(function_row("main.rs", 4, "main", 3));
    batch.functions.push(function_row("math.rs", 8, "Add", 2));
    batch.functions.push(function_row("math.rs", 12, "Sub", 2));

    let result = execute(
        "SELECT name FROM functions WHERE name IN ('main', 'Sub') ORDER BY name",
        &batch,
    );

    assert_eq!(
        result.rows,
        vec![
            vec![Value::String("Sub".to_string())],
            vec![Value::String("main".to_string())],
        ]
    );
}

fn execute(query: &str, batch: &TableBatch) -> crate::QueryResult {
    let statement = parse_query(query).expect("query should parse");
    execute_query(batch, &statement).expect("query should execute")
}

fn function_row(file: &str, line: usize, name: &str, complexity: usize) -> FunctionRow {
    FunctionRow {
        file: file.to_string(),
        line,
        name: name.to_string(),
        visibility: "private".to_string(),
        param_count: 0,
        return_type: String::new(),
        complexity,
        has_test: false,
    }
}

fn sample_batch() -> TableBatch {
    TableBatch::new("ignored.rs")
}
