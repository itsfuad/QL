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

All language adapters populate same shared schema:

- `functions`
- `calls`
- `imports`
- `structs`
- `variables`
- `comments`

Schema source lives in [schema/tables.json](schema/tables.json).

## Architecture

Project has four layers. They stay separate:

1. Interface layer
   Go CLI, watch mode, VS Code extension
2. Query engine
   Rust SQL parser, planner, DuckDB execution, output formatting
3. AST bridge
   Rust Tree-sitter walk, schema mapping, second-pass analysis
4. Language adapters
   One Rust adapter per language

## Current Status

Current repo contains:

- shared row types and `TableBatch`
- adapter trait and Tree-sitter walk path
- first Go adapter stub for function declarations
- workspace scaffolding for Rust, Go CLI, and VS Code extension

Planned next work:

- DuckDB ingest in `ql-core`
- hand-written SQL parser for v1 subset
- subprocess JSON protocol between CLI and engine
- file cache
- full language adapters

## Development

Current baseline commands:

```bash
cargo test
go test ./...
```

## Scope

v1 targets Linux and macOS. No AI, no remote repositories, no type-resolution-heavy semantic analysis, no plugin system.
