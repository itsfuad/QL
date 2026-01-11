use std::io::Write;
use std::path::Path;
use std::time::Duration;

use ql_core::execute::execute_query;
use ql_core::sql::parse_query;

use crate::format::{clear_screen, format_response};
use crate::source::{collect_source_batch, detect_languages, scan_snapshot, snapshots_equal};

pub fn run_watch(query: &str, root: &Path, format: &str) -> Result<(), String> {
    let detected = detect_languages(root);
    if detected.is_empty() {
        eprintln!("warning: no supported source files found in {}", root.display());
    }

    let snapshot = scan_snapshot(root)?;
    render_query(query, root, format)?;

    let poll_interval = Duration::from_millis(500);
    let mut current_snapshot = snapshot;

    loop {
        std::thread::sleep(poll_interval);

        let next_snapshot = scan_snapshot(root)?;
        if snapshots_equal(&current_snapshot, &next_snapshot) {
            continue;
        }

        current_snapshot = next_snapshot;
        render_query(query, root, format)?;
    }
}

fn render_query(query: &str, root: &Path, format: &str) -> Result<(), String> {
    let statement = parse_query(query)
        .map_err(|e| format!("error: {} at position {}", e.message, e.position))?;
    let batch = collect_source_batch(root)?;
    let result = execute_query(&batch, &statement).map_err(|e| e.to_string())?;

    clear_screen();
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    format_response(&mut handle, format, &result)?;
    handle.flush().map_err(|e| e.to_string())
}
