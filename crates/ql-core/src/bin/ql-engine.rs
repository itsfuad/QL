use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use ql_adapters::GoAdapter;
use ql_ast::{TableBatch, walk_source};
use ql_core::{EngineRequest, EngineResponse, execute_query, parse_query};

fn main() {
    if let Err(error) = run() {
        let response = EngineResponse::from_error(error);
        let _ = writeln!(
            io::stdout(),
            "{}",
            serde_json::to_string(&response).unwrap_or_else(|_| {
                "{\"error\":\"internal error: failed to serialize response\"}".to_string()
            })
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut stdin = io::stdin().lock();
    let mut line = String::new();
    if stdin
        .read_line(&mut line)
        .map_err(|error| error.to_string())?
        == 0
    {
        return Err("error: expected JSON request on stdin".to_string());
    }

    let request: EngineRequest =
        serde_json::from_str(line.trim_end()).map_err(|error| format!("error: {error}"))?;
    let response = match execute_request(request) {
        Ok(response) => response,
        Err(error) => EngineResponse::from_error(error),
    };
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    writeln!(
        stdout,
        "{}",
        serde_json::to_string(&response).map_err(|error| error.to_string())?
    )
    .map_err(|error| error.to_string())
}

fn execute_request(request: EngineRequest) -> Result<EngineResponse, String> {
    let root = PathBuf::from(&request.root);
    if !root.exists() {
        return Err(format!("error: path \"{}\" does not exist", request.root));
    }

    let statement = parse_query(&request.query)
        .map_err(|error| format!("error: {} at position {}", error.message, error.position))?;
    let mut batch = TableBatch::new("");
    collect_go_files(&root, &root, &mut batch)?;
    let result = execute_query(&batch, &statement).map_err(|error| error.to_string())?;

    Ok(EngineResponse::from_result(result))
}

fn collect_go_files(root: &Path, path: &Path, batch: &mut TableBatch) -> Result<(), String> {
    let entries = fs::read_dir(path).map_err(|error| format!("error: {error}"))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("error: {error}"))?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            collect_go_files(root, &entry_path, batch)?;
            continue;
        }

        if entry_path.extension().and_then(|ext| ext.to_str()) != Some("go") {
            continue;
        }

        // Engine owns file IO. Adapters stay pure and only map AST nodes to rows.
        let source = fs::read_to_string(&entry_path)
            .map_err(|error| format!("error: failed to read {}: {error}", entry_path.display()))?;
        let relative = entry_path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned();
        let file_batch = walk_source(&GoAdapter, relative, &source)
            .map_err(|error| format!("error: failed to parse {}: {error}", entry_path.display()))?;

        batch.extend(file_batch);
    }

    Ok(())
}
