# ql

`ql` lets developers query codebases with SQL.

It parses source files with Tree-sitter, maps syntax into language-agnostic tables, loads rows into DuckDB, and runs SQL against those tables. Goal: deterministic code search and analysis without AI, embeddings, or fuzzy guesses.

## Example

```bash
ql "SELECT name, file, line
    FROM functions
    WHERE complexity > 10
      AND has_test = false" ./src
```

## What It Extracts

Language adapters populate the shared schema:

- `functions`
- `calls`
- `imports`
- `structs`
- `variables`
- `comments`

Schema source lives in [schema/tables.json](schema/tables.json).

## Architecture

```
ql/
├── crates/
│   ├── ql-ast/         AST bridge, Tree-sitter walk, schema mapper
│   ├── ql-adapters/    Tree-sitter adapter implementations per language
│   ├── ql-core/        Query engine, SQL parser, planner, DuckDB execution
│   └── ql-cli/         CLI binary — arg parsing, output formatting, watch mode
└── extension/          VS Code extension (TypeScript)
```

Single binary. No subprocess protocol. Rust CLI only.

## Current Status

- Shared row types and `TableBatch`
- Language adapter trait and Tree-sitter walk path
- Go, Rust, TypeScript, and Python adapter support
- DuckDB-backed in-memory schema with multi-table ingest
- Hand-written SQL parser (SELECT, FROM, JOIN, WHERE, ORDER BY, LIMIT, operators)
- Planner maps SQL AST to DuckDB execution
- CLI with table/JSON/CSV output and watch mode

## Development

```bash
cargo test
cargo build --bin ql
```

## Scope

v1 targets Linux and macOS. No AI, no remote repositories, no type-resolution-heavy semantic analysis, no plugin system.
