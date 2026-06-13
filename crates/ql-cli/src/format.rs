use std::io::Write;

use ql_adapters::supported_languages as adapter_supported_languages;
use ql_core::protocol::QueryResult;

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

fn cell_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn format_table(writer: &mut impl Write, result: &QueryResult) -> Result<(), String> {
    if result.columns.is_empty() {
        return Ok(());
    }

    let mut widths: Vec<usize> = result.columns.iter().map(|c| c.len()).collect();
    for row in &result.rows {
        for (i, value) in row.iter().enumerate() {
            let cell = cell_text(value);
            if cell.len() > widths[i] {
                widths[i] = cell.len();
            }
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
            write!(writer, "{:<width$}", cell_text(value), width = widths[i])
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
mod tests {
    use super::*;
    use ql_core::protocol::QueryResult;
    use serde_json::Value;

    #[test]
    fn validates_known_formats() {
        for fmt in &["table", "json", "csv"] {
            assert!(validate_format(fmt).is_ok());
        }
        assert!(validate_format("xml").is_err());
    }

    #[test]
    fn formats_table_output() {
        let result = QueryResult {
            columns: vec!["name".to_string(), "line".to_string()],
            rows: vec![
                vec![Value::String("main".to_string()), Value::from(4)],
                vec![Value::String("add".to_string()), Value::from(12)],
            ],
        };

        let mut output = Vec::new();
        format_response(&mut output, "table", &result).expect("format should succeed");

        let expected = "name  line\n----  ----\nmain  4   \nadd   12  \n";
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn formats_json_output() {
        let result = QueryResult {
            columns: vec!["name".to_string(), "line".to_string()],
            rows: vec![vec![
                Value::String("main".to_string()),
                Value::from(4),
            ]],
        };

        let mut output = Vec::new();
        format_response(&mut output, "json", &result).expect("format should succeed");

        let expected = "[{\"line\":4,\"name\":\"main\"}]\n";
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn formats_csv_output() {
        let result = QueryResult {
            columns: vec!["name".to_string(), "line".to_string()],
            rows: vec![vec![
                Value::String("main".to_string()),
                Value::from(4),
            ]],
        };

        let mut output = Vec::new();
        format_response(&mut output, "csv", &result).expect("format should succeed");

        let expected = "name,line\nmain,4\n";
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    #[test]
    fn handles_empty_columns() {
        let result = QueryResult {
            columns: vec![],
            rows: vec![],
        };

        let mut output = Vec::new();
        format_response(&mut output, "table", &result).expect("format should succeed");
        assert!(String::from_utf8(output).unwrap().is_empty());
    }
}
