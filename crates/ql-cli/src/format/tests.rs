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

    let expected = "name  line\n----  ----\nmain     4\nadd     12\n";
    assert_eq!(String::from_utf8(output).unwrap(), expected);
}

#[test]
fn truncates_long_cells() {
    let result = QueryResult {
        columns: vec!["description".to_string()],
        rows: vec![vec![Value::String("a".repeat(70))]],
    };

    let mut output = Vec::new();
    format_response(&mut output, "table", &result).expect("format should succeed");

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(&format!("{}...", "a".repeat(57))));
}

#[test]
fn formats_json_output() {
    let result = QueryResult {
        columns: vec!["name".to_string(), "line".to_string()],
        rows: vec![vec![Value::String("main".to_string()), Value::from(4)]],
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
        rows: vec![vec![Value::String("main".to_string()), Value::from(4)]],
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
