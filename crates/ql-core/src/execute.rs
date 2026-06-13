use std::fmt;

use duckdb::types::Value as DuckValue;
use ql_ast::TableBatch;
use serde_json::Value;

use crate::{
    plan::{PlanError, plan_select},
    protocol::QueryResult,
    sql::SelectStatement,
    storage::open_batch,
};

#[derive(Debug)]
pub enum ExecuteError {
    Plan(PlanError),
    DuckDb(duckdb::Error),
}

impl fmt::Display for ExecuteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(formatter, "error: {}", error.message),
            Self::DuckDb(error) => write!(formatter, "internal error: {error}"),
        }
    }
}

impl From<PlanError> for ExecuteError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<duckdb::Error> for ExecuteError {
    fn from(error: duckdb::Error) -> Self {
        Self::DuckDb(error)
    }
}

pub fn execute_query(
    batch: &TableBatch,
    statement: &SelectStatement,
) -> Result<QueryResult, ExecuteError> {
    let plan = plan_select(statement)?;
    let connection = open_batch(batch)?;
    let mut query = connection.prepare(&plan.sql)?;
    let mut rows = query.query([])?;
    let mut values = Vec::new();
    let column_count = rows
        .as_ref()
        .map_or(0, |statement| statement.column_count());
    let columns = match rows.as_ref() {
        Some(statement) => {
            let mut columns = Vec::with_capacity(column_count);
            for index in 0..column_count {
                columns.push(statement.column_name(index)?.clone());
            }
            columns
        }
        None => Vec::new(),
    };

    while let Some(row) = rows.next()? {
        let mut record = Vec::with_capacity(column_count);
        for index in 0..column_count {
            record.push(to_json_value(row.get_ref_unwrap(index).to_owned()));
        }
        values.push(record);
    }

    Ok(QueryResult {
        columns,
        rows: values,
    })
}

fn to_json_value(value: DuckValue) -> Value {
    match value {
        DuckValue::Null => Value::Null,
        DuckValue::Boolean(value) => Value::Bool(value),
        DuckValue::TinyInt(value) => Value::from(value),
        DuckValue::SmallInt(value) => Value::from(value),
        DuckValue::Int(value) => Value::from(value),
        DuckValue::BigInt(value) => Value::from(value),
        DuckValue::HugeInt(value) => Value::String(value.to_string()),
        DuckValue::UTinyInt(value) => Value::from(value),
        DuckValue::USmallInt(value) => Value::from(value),
        DuckValue::UInt(value) => Value::from(value),
        DuckValue::UBigInt(value) => Value::from(value),
        DuckValue::Float(value) => Value::from(value),
        DuckValue::Double(value) => Value::from(value),
        DuckValue::Decimal(value) => Value::String(value.to_string()),
        DuckValue::Timestamp(_, value) => Value::from(value),
        DuckValue::Text(value) => Value::String(value),
        DuckValue::Blob(value) => Value::String(format!("{value:?}")),
        DuckValue::Date32(value) => Value::from(value),
        DuckValue::Time64(_, value) => Value::from(value),
        DuckValue::Interval {
            months,
            days,
            nanos,
        } => Value::String(format!("{months}:{days}:{nanos}")),
        DuckValue::List(value) => Value::Array(value.into_iter().map(to_json_value).collect()),
        DuckValue::Enum(value) => Value::String(value),
        DuckValue::Struct(value) => Value::Object(
            value
                .iter()
                .map(|(key, value)| (key.clone(), to_json_value(value.clone())))
                .collect(),
        ),
        DuckValue::Array(value) => Value::Array(value.into_iter().map(to_json_value).collect()),
        DuckValue::Map(value) => Value::Array(
            value
                .iter()
                .map(|(key, value)| {
                    Value::Array(vec![
                        to_json_value(key.clone()),
                        to_json_value(value.clone()),
                    ])
                })
                .collect(),
        ),
        DuckValue::Union(value) => to_json_value(*value),
    }
}

#[cfg(test)]
mod tests {
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
}
