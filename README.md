# ql

`ql` lets developers query codebases with SQL.

It parses source files with Tree-sitter, maps syntax into language-agnostic tables, loads rows into DuckDB, and runs SQL against those tables. Goal: deterministic code search and analysis without AI, embeddings, or fuzzy guesses.

## Install

From a checkout:

```bash
cargo build --release
./target/release/ql --langs
```

Or install the binary locally:

```bash
cargo install --path crates/ql-cli
ql --langs
```

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

## Example Queries

All examples assume you run them from a repository root or pass an explicit path as the final argument.

1. Find non-test functions with higher complexity.

```bash
ql "SELECT name, file, line, complexity FROM functions WHERE has_test = false AND complexity > 10 ORDER BY complexity DESC" .
```

2. List all external calls.

```bash
ql "SELECT caller, callee, file, line FROM calls WHERE is_external = true ORDER BY file, line" .
```

3. Inspect imports from the standard library.

```bash
ql "SELECT module, alias, file, line FROM imports WHERE is_std = true ORDER BY file, line" .
```

4. Find public structs and their implemented interfaces.

```bash
ql "SELECT name, visibility, implements, file, line FROM structs WHERE visibility = 'public' ORDER BY file, line" .
```

5. List mutable variables.

```bash
ql "SELECT name, type_hint, scope, file, line FROM variables WHERE is_mutated = true ORDER BY file, line" .
```

6. Show doc comments attached to code.

```bash
ql "SELECT text, attached_to, file, line FROM comments WHERE is_doc = true AND attached_to != '' ORDER BY file, line" .
```

7. Find functions that mention tests.

```bash
ql "SELECT name, file, line FROM functions WHERE has_test = true ORDER BY file, line" .
```

8. Show functions that return results.

```bash
ql "SELECT name, return_type, file, line FROM functions WHERE return_type LIKE '%Result%' ORDER BY file, line" .
```

9. Render the same data as JSON.

```bash
ql --format json "SELECT name, file, line FROM structs ORDER BY file, line LIMIT 20" .
```

10. Render a compact CSV export.

```bash
ql --format csv "SELECT name, file, line FROM functions ORDER BY file, line LIMIT 50" .
```

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
- VS Code extension sidebar for running queries and opening rows

## Development

```bash
cargo test
cargo build --bin ql
```

## Scope

v1 targets Linux and macOS. No AI, no remote repositories, no type-resolution-heavy semantic analysis, no plugin system.

## TODO
 - [ ] add *.ext search in query
 - [ ] add line:col in table in cmd
 - [ ] better UI
 - [ ] cross-platform