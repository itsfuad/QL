# Query Language

`ql` queries indexed codebase tables with a small SQL-like DSL. It is intentionally narrower than general SQL: the goal is predictable code search and analysis over a fixed schema, not a full database language.

## Query Shape

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

Semicolons at the end of the query are accepted.

## Supported Syntax

`SELECT`

- `SELECT *`
- `SELECT col1, col2`
- `SELECT DISTINCT col`
- `SELECT functions.name, calls.callee`
- `SELECT name AS function_name`

`FROM` and `JOIN`

- one base table in `FROM`
- zero or more `JOIN ... ON ...` clauses
- table aliases with `AS`
- join conditions expressed as normal predicates, for example `f.name = c.caller`

`WHERE` and `HAVING`

- comparison operators: `=`, `!=`, `>`, `<`, `>=`, `<=`
- boolean operators: `AND`, `OR`, `NOT`
- membership: `IN (...)`, `NOT IN (...)`
- pattern matching: `LIKE`
- ranges: `BETWEEN`, `NOT BETWEEN`
- null checks: `IS NULL`, `IS NOT NULL`
- parentheses for grouping

`ORDER BY` and `LIMIT`

- one or more order keys
- `ASC` or `DESC`
- integer `LIMIT`

Literals

- strings: `'main'`
- integers: `10`
- booleans: `true`, `false`
- `NULL`

## Not Supported

The current parser and planner do not support:

- subqueries
- aggregate functions such as `COUNT(...)`, `SUM(...)`, `AVG(...)`, `MIN(...)`, `MAX(...)`
- computed `SELECT` expressions such as `complexity + 1`
- function calls in queries
- join variants such as `LEFT JOIN`, `RIGHT JOIN`, or `FULL JOIN`
- `UNION`, `INTERSECT`, or `EXCEPT`
- `INSERT`, `UPDATE`, `DELETE`, DDL, or multi-statement SQL

One important limitation: `GROUP BY` and `HAVING` parse and plan, but without aggregate functions they are only useful for a narrow set of distinct/grouped row queries.

## Tables Overview

These are the canonical tables defined in [schema/tables.json](../schema/tables.json):

| Table | Purpose | Common columns |
| --- | --- | --- |
| `functions` | Function and method definitions | `file`, `line`, `name`, `visibility`, `complexity`, `has_test` |
| `calls` | Call sites | `file`, `line`, `caller`, `callee`, `is_external` |
| `imports` | Import/use statements | `file`, `line`, `module`, `alias`, `is_std` |
| `structs` | Struct/class-like declarations | `file`, `line`, `name`, `field_count`, `visibility`, `implements` |
| `variables` | Variable bindings | `file`, `line`, `name`, `type_hint`, `scope`, `is_mutated` |
| `comments` | Comments and doc comments | `file`, `line`, `text`, `attached_to`, `is_doc` |
| `fn_fingerprints` | Structural per-function metrics | `file`, `line`, `name`, `complexity`, `nesting_depth`, `call_count` |
| `fn_callsets` | One row per function-to-callee relationship | `file`, `line`, `name`, `callee` |
| `similarities` | Pairwise function similarity scores | `file_a`, `line_a`, `name_a`, `file_b`, `line_b`, `name_b`, `combined_score` |

## Common Columns

These columns appear across most tables and are the best place to start:

- `file`: path relative to the query root
- `line`: 1-indexed source line
- `name`: function, struct, or variable name when the table has a named definition

Boolean columns are real booleans, so prefer `true` and `false` in queries:

```sql
SELECT name, file, line
FROM functions
WHERE has_test = false
ORDER BY file, line;
```

## Example Queries

Basic listing:

```bash
ql "SELECT name, file, line FROM functions ORDER BY file, line LIMIT 20" .
```

Filter by complexity:

```bash
ql "SELECT name, file, line, complexity FROM functions WHERE complexity > 10 ORDER BY complexity DESC LIMIT 20" .
```

Find external calls:

```bash
ql "SELECT caller, callee, file, line FROM calls WHERE is_external = true ORDER BY file, line" .
```

Filter with `LIKE`:

```bash
ql "SELECT name, return_type, file, line FROM functions WHERE return_type LIKE '%Result%' ORDER BY file, line" .
```

Join functions to their calls:

```bash
ql "SELECT f.name, c.callee, f.file, f.line FROM functions AS f JOIN calls AS c ON f.name = c.caller ORDER BY f.file, f.line LIMIT 20" .
```

Deduplicate files:

```bash
ql "SELECT DISTINCT file FROM functions ORDER BY file LIMIT 20" .
```

Range checks:

```bash
ql "SELECT name, file, complexity FROM functions WHERE complexity BETWEEN 5 AND 10 ORDER BY complexity DESC, file LIMIT 20" .
```

Similarity queries:

```bash
ql "SELECT name_a, file_a, name_b, file_b, combined_score FROM similarities ORDER BY combined_score DESC LIMIT 20" .
ql "SELECT name_b, file_b, combined_score FROM similarities WHERE name_a = 'parse_config' ORDER BY combined_score DESC LIMIT 20" .
```

## Output Formats

Table output:

```bash
ql --format table "SELECT name, file, line FROM functions LIMIT 10" .
```

JSON output:

```bash
ql --format json "SELECT name, file, line FROM functions LIMIT 10" .
```

CSV output:

```bash
ql --format csv "SELECT name, file, line FROM functions LIMIT 10" .
```

## Notes

- Query the indexed tables directly. Filtering and ranking should usually happen in your query, not by assuming the index already hid rows.
- Prefer `file` plus `line` when you need a stable way to identify a row.
- If you are unsure what a table contains, start with `SELECT * FROM <table> LIMIT 5`.
