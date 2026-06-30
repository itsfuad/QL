# ql

`ql` is a CLI for querying codebases with a small SQL-like language.

It parses source files with Tree-sitter, maps syntax into language-agnostic tables, loads rows into DuckDB, and runs deterministic queries over that indexed data.

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

## Query Language

`ql` supports a focused SQL-like DSL for codebase data.

```sql
SELECT [DISTINCT] <columns | *>
FROM <table> [AS alias]
[JOIN <table> [AS alias] ON <condition>]
[WHERE <condition>]
[GROUP BY <column>, ...]
[HAVING <condition>]
[ORDER BY <column> [ASC|DESC], ...]
[LIMIT <n>]
```

Supported in the current parser:

- column selection or `*`
- `DISTINCT`
- `JOIN ... ON ...`
- `WHERE`, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`
- aliases with `AS`
- operators: `=`, `!=`, `>`, `<`, `>=`, `<=`, `AND`, `OR`, `NOT`, `IN (...)`, `NOT IN (...)`, `LIKE`, `BETWEEN`, `IS NULL`

Not supported:

- subqueries
- aggregate functions such as `COUNT(...)`, `SUM(...)`, `AVG(...)`
- arithmetic or computed expressions in `SELECT`
- join types such as `LEFT JOIN` or `FULL JOIN`

Full query documentation: [docs/queries.md](docs/queries.md)

## Start Here

```bash
ql "SELECT * FROM functions LIMIT 5" .
```

## Common Examples

```bash
ql "SELECT name, file, line FROM functions ORDER BY file, line LIMIT 20" .
ql "SELECT name, file, line, complexity FROM functions WHERE complexity > 10 ORDER BY complexity DESC LIMIT 20" .
ql "SELECT caller, callee, file, line FROM calls WHERE is_external = true ORDER BY file, line" .
ql "SELECT module, file, line FROM imports WHERE is_std = true ORDER BY file, line" .
ql "SELECT name_a, file_a, name_b, file_b, combined_score FROM similarities ORDER BY combined_score DESC LIMIT 20" .
```

## Schema

`ql` indexes these shared tables:

- `functions`
- `calls`
- `imports`
- `structs`
- `variables`
- `comments`
- `fn_fingerprints`
- `fn_callsets`
- `similarities`

Canonical schema source: [schema/tables.json](schema/tables.json)

## Output Formats

```bash
ql --format table "SELECT name, file, line FROM functions LIMIT 10" .
ql --format json "SELECT name, file, line FROM functions LIMIT 10" .
ql --format csv "SELECT name, file, line FROM functions LIMIT 10" .
```

## Architecture

```
ql/
├── crates/
│   ├── ql-ast/         AST bridge, Tree-sitter walk, schema mapper
│   ├── ql-adapters/    Tree-sitter adapter implementations per language
│   ├── ql-core/        Query engine, parser, planner, DuckDB execution
│   └── ql-cli/         CLI binary and watch mode
└── extension/          VS Code extension
```

Single binary. No subprocess protocol.

## Development

```bash
cargo test
cargo build --bin ql
```

## Scope

v1 targets Linux and macOS. No AI, no remote repositories, no type-resolution-heavy semantic analysis, no plugin system.
