use std::io::Write;

use ql_adapters::supported_languages as adapter_supported_languages;
use ql_core::protocol::QueryResult;

const MAX_CELL_WIDTH: usize = 60;

pub fn validate_format(format: &str) -> Result<(), String> {
    match format {
        "table" | "json" | "csv" => Ok(()),
        _ => Err(format!("unsupported format {format:?}")),
    }
}

pub fn supported_languages() -> Vec<String> {
    adapter_supported_languages()
}

pub fn format_response(
    writer: &mut impl Write,
    format: &str,
    result: &QueryResult,
) -> Result<(), String> {
    match format {
        "json" => format_json(writer, result),
        "csv" => format_csv(writer, result),
        _ => format_table(writer, result),
    }
}

fn format_json(writer: &mut impl Write, result: &QueryResult) -> Result<(), String> {
    let rows: Vec<serde_json::Value> = result
        .rows
        .iter()
        .map(|row| {
            let mut item = serde_json::Map::new();
            for (i, column) in result.columns.iter().enumerate() {
                item.insert(column.clone(), row[i].clone());
            }
            serde_json::Value::Object(item)
        })
        .collect();

    let output = serde_json::to_string(&rows).map_err(|e| e.to_string())?;
    writeln!(writer, "{output}").map_err(|e| e.to_string())
}

fn format_csv(writer: &mut impl Write, result: &QueryResult) -> Result<(), String> {
    let mut csv = String::new();

    csv.push_str(&result.columns.join(","));
    csv.push('\n');

    for row in &result.rows {
        let record: Vec<String> = row
            .iter()
            .map(|value| match value {
                serde_json::Value::String(s) => {
                    if s.contains(',') || s.contains('"') || s.contains('\n') {
                        format!("\"{}\"", s.replace('"', "\"\""))
                    } else {
                        s.clone()
                    }
                }
                other => other.to_string(),
            })
            .collect();
        csv.push_str(&record.join(","));
        csv.push('\n');
    }

    write!(writer, "{csv}").map_err(|e| e.to_string())
}

fn display_cell(value: &serde_json::Value) -> String {
    let text = match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    };

    let mut chars = text.chars();
    let mut shortened: String = chars.by_ref().take(MAX_CELL_WIDTH).collect();
    if chars.next().is_some() {
        shortened.truncate(MAX_CELL_WIDTH.saturating_sub(3));
        shortened.push_str("...");
    }

    shortened
}

fn numeric_column(result: &QueryResult, index: usize) -> bool {
    let mut has_number = false;

    for row in &result.rows {
        match row.get(index) {
            Some(serde_json::Value::Null) => {}
            Some(serde_json::Value::Number(_)) => has_number = true,
            Some(_) | None => return false,
        }
    }

    has_number
}

fn format_table(writer: &mut impl Write, result: &QueryResult) -> Result<(), String> {
    if result.columns.is_empty() {
        return Ok(());
    }

    let mut widths: Vec<usize> = result.columns.iter().map(|column| column.len()).collect();
    let numeric = (0..result.columns.len())
        .map(|index| numeric_column(result, index))
        .collect::<Vec<_>>();

    for row in &result.rows {
        for (index, value) in row.iter().enumerate() {
            let cell = display_cell(value);
            widths[index] = widths[index].max(cell.len());
        }
    }

    for (i, column) in result.columns.iter().enumerate() {
        write!(writer, "{:<width$}", column, width = widths[i]).map_err(|e| e.to_string())?;
        if i < result.columns.len() - 1 {
            write!(writer, "  ").map_err(|e| e.to_string())?;
        }
    }
    writeln!(writer).map_err(|e| e.to_string())?;

    for (i, width) in widths.iter().enumerate() {
        write!(writer, "{}", "-".repeat(*width)).map_err(|e| e.to_string())?;
        if i < widths.len() - 1 {
            write!(writer, "  ").map_err(|e| e.to_string())?;
        }
    }
    writeln!(writer).map_err(|e| e.to_string())?;

    for row in &result.rows {
        for (i, value) in row.iter().enumerate() {
            let cell = display_cell(value);
            if numeric[i] {
                write!(writer, "{:>width$}", cell, width = widths[i])
            } else {
                write!(writer, "{:<width$}", cell, width = widths[i])
            }
            .map_err(|e| e.to_string())?;
            if i < row.len() - 1 {
                write!(writer, "  ").map_err(|e| e.to_string())?;
            }
        }
        writeln!(writer).map_err(|e| e.to_string())?;
    }

    Ok(())
}

pub fn clear_screen() {
    print!("\x1b[H\x1b[2J");
}

#[cfg(test)]
mod tests;
