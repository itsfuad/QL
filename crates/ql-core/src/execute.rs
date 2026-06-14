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
mod tests;
