use std::env;
use std::path::PathBuf;
use std::process;

use ql_core::execute::execute_query;
use ql_core::sql::parse_query;

mod format;
mod source;
mod watch;

use format::{format_response, supported_languages, validate_format};
use source::{collect_source_batch, detect_languages};
use watch::run_watch;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut format = String::from("table");
    let mut show_langs = false;
    let mut watch = false;
    let mut positional = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                format = args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("error: --format requires a value");
                    process::exit(1);
                });
            }
            "--langs" => show_langs = true,
            "--watch" => watch = true,
            arg if arg.starts_with("--") => {
                eprintln!("error: unknown flag {arg}");
                process::exit(1);
            }
            arg => positional.push(arg.to_string()),
        }
        i += 1;
    }

    if show_langs {
        for lang in supported_languages() {
            println!("{lang}");
        }
        process::exit(0);
    }

    if positional.is_empty() {
        eprintln!("error: query is required");
        process::exit(1);
    }
    if positional.len() > 2 {
        eprintln!("error: expected ql <query> [path]");
        process::exit(1);
    }

    if let Err(e) = validate_format(&format) {
        eprintln!("error: {e}");
        process::exit(1);
    }

    let query = &positional[0];

    let root = if positional.len() == 2 {
        PathBuf::from(&positional[1])
    } else {
        PathBuf::from(".")
    };

    let root = root.canonicalize().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    if !root.exists() {
        eprintln!("error: path \"{}\" does not exist", root.display());
        process::exit(1);
    }

    if watch {
        if let Err(e) = run_watch(query, &root, &format) {
            eprintln!("{e}");
            process::exit(1);
        }
        return;
    }

    // Detect languages present before engine call
    let detected = detect_languages(&root);
    if detected.is_empty() {
        eprintln!("warning: no supported source files found in {}", root.display());
    }

    let statement = parse_query(query).unwrap_or_else(|e| {
        eprintln!("error: {} at position {}", e.message, e.position);
        process::exit(1);
    });
    let batch = collect_source_batch(&root).unwrap_or_else(|e| {
        eprintln!("{e}");
        process::exit(1);
    });
    let result = execute_query(&batch, &statement).unwrap_or_else(|e| {
        eprintln!("{e}");
        process::exit(1);
    });

    if let Err(e) = format_response(&mut std::io::stdout(), &format, &result) {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
