# ql — Principal Engineer Technical Audit

**Scope:** `ql-main` workspace (4 crates, ~4,365 lines of Rust + ~550-line VS Code extension), commit `8686ebb`, 30 commits, single contributor, ~6 months old, no LICENSE/CI/CONTRIBUTING.

**Verdict in one sentence:** ql has the *right raw materials* (Tree-sitter → relational rows → DuckDB → SQL is a genuinely good idea, underused in the OSS tooling space), but the current implementation re-solves a problem DuckDB already solved (SQL parsing/planning), under-solves the problem that actually differentiates a code-intelligence tool (symbol resolution, cross-language schema consistency, scale), and is missing the OSS scaffolding (LICENSE, CI, `.gitignore` awareness) needed for anyone outside the author to trust or extend it.

---

## PHASE 1 — Repository Understanding

### 1. What problem does ql solve?

ql turns a source tree into a small relational database (6 tables: `functions`, `calls`, `imports`, `structs`, `variables`, `comments`) and lets you query it with a SQL-like language. The pitch — "grep is text, AST tools are syntax, ql is a database" — is correct and valuable: turning "find all public functions with complexity > 10 that have no tests" from a multi-tool pipeline (ripgrep + jq + manual review) into one SQL statement is a real productivity win, *if* the underlying facts are trustworthy.

Today it solves a narrow slice of that: **declaration-level inventory and simple cross-referencing** for Rust, Go, Python, and TypeScript. It does **not** yet solve semantic questions (does data flow from X to Y, is this symbol ever used, what does this interface implement).

### 2. Who are the target users?

- Individual developers auditing an unfamiliar codebase ("show me every exported function with no test sibling").
- Tech leads writing lightweight architecture/lint rules ("nothing in `src/ui` should import `src/db`" — this is *already* expressible via the `imports` table).
- CI pipelines enforcing complexity/visibility/documentation gates.

Not yet viable for: security teams (no taint tracking), large-org code search (no incremental index, no multi-repo), or anyone needing precise symbol references (renaming, call graphs).

### 3. What alternatives exist?

| Tool | Model | Closest analogue to ql |
| --- | --- | --- |
| CodeQL | Compiled semantic DB + Datalog-like QL | "SQL for codebases" but with real semantics |
| Semgrep | Tree-sitter pattern matching + metavariables | Same parser tech, different query model |
| Sourcegraph | Indexed code search + SCIP/LSIF | Cross-repo scale ql doesn't have |
| ast-grep | Tree-sitter structural search/rewrite, YAML rules | Closest sibling — same parser, no SQL |
| ripgrep | Text/regex search | Baseline speed and `.gitignore` behavior ql lacks |
| `tree-sitter query` / nvim-treesitter `tags.scm` | Declarative `.scm` queries per language | The reuse opportunity ql is missing (see Phase 3) |

### 4. Competitive advantages ql currently has

- **SQL is a near-universal query language.** Nobody has to learn a Datalog dialect (CodeQL) or a YAML rule grammar (Semgrep/ast-grep) to ask "top 10 most complex functions without tests."
- **Zero build step.** Tree-sitter parses source directly — no `cargo build`, no `mvn compile`, no language toolchain required. This is CodeQL's biggest adoption friction, and ql avoids it entirely.
- **Single static binary, no server/index daemon.** Trivial to drop into CI.
- **DuckDB as the backend is an excellent, underleveraged choice** — embedded, columnar, fast joins/aggregates, zero ops, and (critically) **already implements everything ql's hand-rolled SQL layer is reimplementing**.
- **Codebase is small enough to read in a day** (~4,400 LOC), which is a real onboarding advantage *today* — though it's fragile (see Phase 9).
- Production code is clean: no `unwrap()`/`expect()`/`panic!` outside test modules anywhere in the four crates — error handling is consistently `Result`/`Option` with `let-else`. This is genuinely good Rust hygiene.

### 5. Competitive disadvantages ql currently has

- **The "SQL" surface is ~10% of SQL-92 SELECT** — no `GROUP BY`, no aggregates, no `CASE`, no subqueries, no CTEs, no column aliases, no `DISTINCT`. The flagship example queries in the README don't even use `COUNT`.
- **No symbol resolution.** `calls.callee` and `functions.name` are raw source-text strings with no linkage — you cannot build a real call graph or do dead-code analysis.
- **No `.gitignore`/`.qlignore` awareness** — `node_modules`, `target/`, `vendor/`, `.venv` all get parsed as source if extensions match.
- **`complexity` and `visibility` mean different things per language** — directly undermines "SQL across your whole polyglot codebase."
- **No incremental/persistent index** — every invocation re-walks the tree and rebuilds an in-memory DuckDB from scratch.
- **No LICENSE, no CI, no CONTRIBUTING** — a legal and trust blocker for any team evaluating adoption.

---

## PHASE 2 — Architecture Review

### Workspace shape

```text
ql-ast      → tree-sitter walk + row schema + cross-file "second pass" analysis
ql-adapters → one file per language, implements LanguageAdapter
ql-core     → hand-written SQL lexer/parser/AST/planner + DuckDB storage/execute
ql-cli      → arg parsing, file walking/caching, output formatting, watch mode
```

Dependency graph (from `Cargo.toml` files): `ql-cli → {ql-core, ql-adapters, ql-ast}`, `ql-core → {ql-adapters, ql-ast}`, `ql-adapters → ql-ast`.

**Finding:** `ql-core`'s `Cargo.toml` declares `ql-adapters = { path = "../ql-adapters" }` (`crates/ql-core/Cargo.toml:8`), but `grep -rn "ql_adapters" crates/ql-core/src/` returns **nothing**. This is a dead dependency — harmless today, but it's a small canary for "nobody runs `cargo machete`/`cargo-udeps` in CI," which matters once 15 more adapters (each pulling a tree-sitter grammar) get added and nobody notices `ql-core` silently depending on all of them.

### `ql-ast`

**Responsibilities:** `adapter.rs` defines the `LanguageAdapter` trait and the generic Tree-sitter walk; `rows.rs` defines the row schema (`FunctionRow`, `CallRow`, etc.) and `TableBatch`; `analysis.rs` does cross-file "second pass" enrichment (`has_test`, `implements` dedup, `comments.attached_to`).

**Is the AST abstraction clean?**
Yes, at the *trait* level — `LanguageAdapter` (`crates/ql-ast/src/adapter.rs:7-15`) is a tight 4-method contract (`language_name`, `grammar`, `extensions`, `map_node`) plus an optional `second_pass` hook. That's a good extension point.

**Is Tree-sitter tightly coupled?**
Yes, and that's *appropriate* for this crate — `walk_source`/`walk_node` (`adapter.rs:17-55`) are thin, generic wrappers around `tree_sitter::{Parser, TreeCursor}`. The coupling is the crate's job.

**Where the abstraction leaks:**

1. **`rows.rs` has nothing to do with Tree-sitter** but lives in the Tree-sitter-coupled crate. `TableBatch`/`FunctionRow`/etc. are pure data schema that `ql-core` also needs. Today this is cosmetic (ql-core already depends on ql-ast), but it conflates two concerns: "the schema" (which should be the stable, shared contract between adapters and the query engine — arguably generated from `schema/tables.json`) and "the Tree-sitter walking machinery" (which is adapter-internal plumbing). A `ql-schema` crate containing just `rows.rs` (+ codegen from `schema/tables.json`, see Phase 5) would let `ql-core` depend on the schema without transitively pulling in `tree-sitter` at all.
2. **The recursive walk is not iterative.** `walk_node` (`adapter.rs:35-55`) recurses via `walk_node(adapter, cursor, source, rows)` on `goto_first_child()`. Recursion depth == AST nesting depth. For hand-written code this is fine (nesting rarely exceeds ~30), but **generated code, minified JS/TS, or deeply-nested JSON-like literals can produce ASTs hundreds of levels deep**, and this will blow the stack. The directory walker in `ql-cli/src/source.rs` *was* written iteratively with an explicit stack (`walk_relative_files`, `source.rs:22-49`) — the AST walker should get the same treatment, especially since "what breaks on generated code" is explicitly in scope for this audit (Phase 3).

**Is schema mapping maintainable?**
Mechanically yes — each `*Row` struct is a flat `Vec<String/usize/bool>`-friendly shape that maps 1:1 to a DuckDB table. But `TableBatch::extend` (`rows.rs:81-88`) and the whole row model assume **adding a table means touching five places**: `rows.rs` (struct + field in `TableBatch`), `lib.rs` exports, every adapter's `map_node`, `ql-core/src/storage.rs` (DDL + insert), and `schema/tables.json`. Nothing enforces these stay in sync (see Phase 5, Finding #14).

### `ql-adapters`

**How easy is adding a new language today?** Mechanically easy — copy `rust.rs` (463 lines) or `go.rs` (377 lines), swap the tree-sitter grammar, and remap node kinds. Each adapter is ~300-460 lines and self-contained. For someone fluent in a target language's Tree-sitter grammar, a new adapter is a 1-3 day task.

**Are adapters consistent? Is there duplicated logic?**
No, and yes — heavily. Every adapter independently re-implements:

- `find_enclosing_function` — **byte-for-byte structurally identical** in `rust.rs:302-315`, `go.rs:334-349`, `python.rs:260-273`, `typescript.rs:360-373`. Each walks `node.parent()` up to `source_file`, matching a per-language set of "function-like" node kinds.
- A `count_complexity` McCabe-style scorer — `rust.rs:26-49`, `go.rs:25-45`, `python.rs:34-56`, `typescript.rs:46-83` — same shape (iterative stack-based tree walk, `score += 1` on decision-node kinds), but with **different and inconsistent node-kind sets** (see Phase 5/6 for why this breaks cross-language queries).
- A `count_params` helper — `rust.rs:19-24`, `go.rs:14-23`, `python.rs:18-32`, `typescript.rs:34-44` — same idea, different node-kind lists.
- A visibility/exported-name classifier — `rust.rs:10-17` (`is_public`), `go.rs:10-12` (`is_exported`), `python.rs:10-16` (`is_private`), `typescript.rs:10-32` (`is_public`, returns a 3-valued string) — **four different return types/conventions for the same concept**.

At 4 languages this is "mildly annoying." At the 12-language scale this audit is asked to evaluate (Phase 7), it is **48+ near-identical copies of 4 helper functions**, each a candidate for subtle per-language drift (e.g., one adapter's complexity walk forgets a node kind the others have). There is currently **no shared trait or utility module** in `ql-ast` or a new `ql-adapters-common` for any of this — the `LanguageAdapter` trait only abstracts the *outermost* dispatch, not the *common sub-algorithms*.

**Does this architecture scale to 20+ languages?** Structurally yes (the trait + static registry pattern in `ql-adapters/src/lib.rs:15-23` scales trivially — add a `static` and push it into `ADAPTERS`). **Algorithmically/maintainability-wise, no** — without extracting the four duplicated helper families into shared, table-driven abstractions (a `language -> { complexity_node_kinds, visibility_rule, enclosing_fn_kinds }` config rather than imperative per-language Rust), the per-adapter cost grows linearly *and* the cross-language consistency (the entire point of a shared schema) degrades further with each addition.

### `ql-core`

**Is the planner architecture scalable?** This is the crate's central problem. `ql-core/src/sql/{lexer,ast,parser}.rs` (lexer 178 lines, ast 88 lines, parser 653 lines) implement a hand-written recursive-descent parser for a SQL subset, producing a `SelectStatement` AST (`sql/ast.rs:1-89`). `plan.rs` (248 lines) then **renders that AST back into a SQL string** (`render_select`, `plan.rs:30-57`), which `execute.rs:46-47` hands to `duckdb::Connection::prepare(&plan.sql)`.

So the pipeline is: **user SQL text → ql's lexer/parser → ql's AST → ql's planner stringifies it back to SQL text → DuckDB's own parser parses it again → DuckDB executes.**

This is ~1,167 lines (27% of the entire Rust codebase) spent re-implementing a strict *subset* of a language (SQL) whose full implementation is sitting one function-call away (`duckdb::Connection`). The only things this layer actually buys today:

1. **Identifier validation** (`is_valid_identifier`, `plan.rs:184-192`) — prevents injection via column/table names that flow from user input into a string-concatenated SQL statement. This is real and necessary *given the string-concatenation approach*.
2. **A restricted grammar** that can't express `INSERT`/`UPDATE`/`DROP`/`ATTACH`/`COPY`/`PRAGMA` — a sandboxing property.
3. Custom error messages with byte positions.

The cost: **every SQL feature DuckDB already has for free — `GROUP BY`, `COUNT`/`SUM`/`AVG`, window functions, `CASE`, subqueries, CTEs (including recursive CTEs, which DuckDB supports and which are *exactly* what call-graph traversal needs), `UNION`, column aliases — is unavailable until someone manually adds it to this hand-rolled grammar.** This is the single highest-leverage architectural decision in the project, and it currently points the wrong way (see Phase 4 for the alternative).

**Is the execution engine isolated?** Yes — `execute.rs` is a clean boundary (`execute_query(batch, statement) -> Result<QueryResult, ExecuteError>`), and `storage.rs::open_batch` cleanly separates "build an in-memory DuckDB from a `TableBatch`" from "run a query against it." The isolation is good; what's isolated is the problem.

**Is the SQL representation future-proof?** `sql/ast.rs` models `SelectStatement` with `select/from/joins/where_clause/order_by/limit` — a reasonable shape for *extending in place* (adding `group_by: Vec<Expr>` and `having: Option<Expr>` fields is mechanically easy), but every such addition requires lexer + parser + AST + planner + tests changes in lockstep, for a feature DuckDB's parser already has on day one.

### `ql-cli`

**Is the CLI architecture extensible?** Minimally. `main.rs` hand-rolls argument parsing in a `while i < args.len()` loop (`main.rs:24-43`) supporting exactly `--format`, `--langs`, `--watch`, and 1-2 positional args. There are no subcommands. Adding `ql index`, `ql explain`, `ql schema`, or `ql init` means extending this ad-hoc loop with more special cases and no `--help`/usage generation. A `clap`-based (derive) CLI would cost ~20 minutes to port today and pay for itself the moment a second subcommand is needed.

**Are command boundaries clean?** `source.rs` (file walking + caching), `format.rs` (output rendering), `watch.rs` (polling loop) are each focused single-purpose modules — this part is well-factored. The issue is *what's missing* from `source.rs`, not how it's organized: no `.gitignore` parsing (Phase 8, Finding #4) and a silent 1,000-directory cap (`source.rs:36`, Finding #3).

**Is the output system maintainable?** `format.rs` is small, well-tested (8 unit tests), and the table/JSON/CSV split is clean. `MAX_CELL_WIDTH = 60` (`format.rs:6`) is a reasonable default but is not configurable — fine for v1.

---

## PHASE 3 — Compiler Engineering Review

### Parsing pipeline

For each file: `Parser::set_language(grammar)` → `parser.parse(source, None)` → if `Some(tree)`, iterative directory-level walk but **recursive** AST-level walk (`walk_node`, `adapter.rs:35-55`) calling `adapter.map_node(node, source, rows)` on **every node in the tree**, regardless of whether that node kind is relevant.

This is a "visit everything, switch on kind" strategy (`match node.kind() { "function_item" => ..., _ => {} }` — e.g. `rust.rs:287-299`). It is correct and simple, but it means **total work is O(total AST nodes)** even though the adapters only care about ~10 node kinds per language. For a 500-line Rust file, that's plausibly 5,000-15,000 node visits to extract maybe 30-50 rows.

### AST traversal — the missed reuse opportunity

Tree-sitter's ecosystem already has a declarative answer to "extract definitions/calls/references from this language": **`.scm` tag queries** (the format used by `tree-sitter-tags`, GitHub's syntax highlighting, and `nvim-treesitter`'s `tags.scm`/`locals.scm`), e.g.:

```scheme
(function_item name: (identifier) @name) @definition.function
(call_expression function: (identifier) @name) @reference.call
```

These already exist, community-maintained, for **dozens of languages** — including several in Phase 7's wishlist (Java, C, C++, C#, Ruby, PHP). ql's current approach (hand-written Rust `match` arms per language, `rust.rs`/`go.rs`/`python.rs`/`typescript.rs`) reinvents what these queries already encode, in a less reusable form (Rust code vs. a portable `.scm` file).

**Concrete recommendation:** restructure `map_node` so each adapter's "what counts as a function/call/import/struct/comment" is expressed as a `tree_sitter::Query` (loaded from a per-language `.scm` file, possibly *adapted from* existing `tags.scm` files), executed via `QueryCursor`. The Rust code in each adapter then shrinks to "given these captures, build a `FunctionRow`" — the part that's genuinely language-specific (complexity scoring, visibility rules, param counting) stays in Rust, but the *node-kind enumeration*, which is 60-70% of each adapter file today, becomes a small declarative file that's easier to review, diff, and crowdsource from people who don't know Rust but know the target language's grammar. This single change is probably the highest-leverage thing for Phase 7 (12+ new languages).

### Error recovery

Tree-sitter's error recovery is a real strength ql gets "for free" — `parser.parse()` always returns `Some(tree)` for any byte string (barring a parse timeout/cancellation flag, which ql never sets), producing `ERROR`/`MISSING` nodes around malformed regions while parsing the rest of the file normally. Since `map_node`'s `_ => {}` fallthrough silently ignores `ERROR` nodes, **a syntax error in one function doesn't crash extraction for the rest of the file** — this is good, and better than a traditional compiler-frontend approach that might bail on the first error.

**But ql surfaces zero signal about this.** If tree-sitter's error-recovery resync skips or mis-attributes a region (which it does for sufficiently broken input, or for syntax newer than the pinned grammar version understands — see below), declarations in that region silently vanish from every table with **no warning**. For a tool whose entire value proposition is "trustworthy facts about your code," silent under-extraction is a correctness risk users can't detect. A `parse_health` table (file, error_node_count, error_byte_ranges) or a `--strict`/`--report-parse-errors` flag costs little and converts an invisible failure mode into a queryable one — and dogfoods nicely (`SELECT file FROM parse_health WHERE error_node_count > 0`).

### Grammar version skew

`Cargo.lock` shows: `tree-sitter = 0.26.9` (core), `tree-sitter-go = 0.25.0`, `tree-sitter-python = 0.25.0`, `tree-sitter-rust = 0.24.2`, `tree-sitter-typescript = 0.23.2`. Each grammar crate is pinned independently and is multiple minor versions behind `tree-sitter` core and behind each other. Practically:

- **Newer language syntax may not parse** — e.g., if Rust 2024/2025 syntax additions aren't in `tree-sitter-rust 0.24.2`'s grammar, those constructs become `ERROR` nodes (silently dropped per above). Given ql's own `Cargo.toml` declares `edition = "2024"`, there's a real chance ql can't fully self-host (parse its own newest-edition code) without hitting this.
- **TypeScript adapter uses the TSX grammar for plain `.ts` files** (`tree_sitter_typescript::LANGUAGE_TSX`, `typescript.rs:337`, applied to both `.ts` and `.tsx` via `extensions(&self) -> &[&str] { &[".ts", ".tsx"] }`, `typescript.rs:340-342`). TSX and TS grammars differ in how they disambiguate generics vs. JSX (`<T>` vs `<T>...</T>`); using TSX for `.ts` is a common pragmatic shortcut but is technically "the wrong grammar for the file type" and a candidate for subtle misparses on generic-heavy `.ts` code.

### What breaks on large repositories — concrete examples

1. **`walk_relative_files`'s 1,000-directory cap** (`crates/ql-cli/src/source.rs:36`):

   ```rust
   if visited.insert(canon.clone()) && dirs.len() < 1000 {
       dirs.push(canon);
   }
   ```
   Once the pending-directory stack reaches 1,000 entries, any *additional* subdirectory discovered is **silently dropped from the walk** — not deferred, not warned about, just never visited. A monorepo with >1,000 subdirectories (common for JS monorepos with per-package `node_modules`, or large Go/Java trees) will get a **silently incomplete index**, and every query result is quietly wrong with no indication. This is the single most dangerous correctness bug for "large repository" use, precisely because it fails silently.

2. **`resolve_comment_attachments`** (`crates/ql-ast/src/analysis.rs:42-65`):

   ```rust
   for comment in &mut batch.comments {
       let nearest_function = batch.functions.iter()
           .filter(|row| row.file == comment.file && row.line > comment.line)
           .min_by_key(|row| row.line);
       let nearest_struct = batch.structs.iter()
           .filter(|row| row.file == comment.file && row.line > comment.line)
           .min_by_key(|row| row.line);
       ...
   }
   ```
   This is **O(comments × (functions + structs))** over the *entire project's* batch (not per-file). For a 1M-LOC repo with, say, 150K comments and 80K functions+structs, that's **~1.2 × 10^10** filter/compare operations in a single pass of `second_pass`, which runs on *every CLI invocation* (`crates/ql-cli/src/source.rs:146`, including every watch-mode refresh). At 10K-100K LOC this is invisible; at 1M+ LOC this is the difference between sub-second and "didn't finish in the timeout."

3. **Row-by-row, non-transactional DuckDB inserts** (`crates/ql-core/src/storage.rs:65-159`): six `connection.prepare(...)` statements, each executed once per row in a loop, with **no surrounding transaction** (`grep -rn "transaction\|BEGIN\|Appender"` returns nothing in `ql-core/src/`). DuckDB (like SQLite) auto-commits each statement absent an explicit transaction — so a 100K-row batch is 100K+ implicit transactions. This dominates wall-clock time at scale and is pure waste, since DuckDB's `Appender` API (columnar bulk-load) exists for exactly this.

### What breaks on malformed code

Nothing crashes — that's the good news (Tree-sitter error recovery + the `let-else`/`Option` discipline in every `map_*` function means a missing `child_by_field_name` just `return`s early, e.g. `rust.rs:52-54`). The bad news is **silent data loss with no signal**, as discussed above — a half-written function during a live `--watch` session simply disappears from `functions` for that refresh, which could look like "the tool is unreliable" to a new user rather than "the file currently has a syntax error."

### What breaks on generated code

- **Deep nesting** (minified JS bundles, generated protobuf bindings, deeply nested match/switch trees): the **recursive** `walk_node` (`ql-ast/src/adapter.rs:47`) risks stack overflow proportional to AST depth — unlike the iterative directory walker.
- **Macro-generated code (Rust especially)**: `map_node` only matches concrete syntax node kinds like `function_item`. Code produced by `macro_rules!`/proc-macro expansion is **not visible to Tree-sitter at all** (Tree-sitter parses the macro *invocation*, not its expansion) — so `#[derive(Builder)]`-generated setter methods, `tokio::main`-wrapped `async fn main`, etc. are invisible to ql's tables. This is inherent to a Tree-sitter-only approach (CodeQL's compiler-integration approach sees post-expansion code; ql does not) and is worth stating explicitly as a known, structural limitation rather than a bug — but it should be **documented**, because "why doesn't `functions` include my derive-generated methods" will be a recurring user question.
- **`cfg`-gated code (Rust) / preprocessor-gated code (Go build tags, C/C++ `#ifdef`)**: Tree-sitter parses *all* branches textually present, including code that would never compile for the current target. ql will report functions/complexity for dead `#[cfg(test)]`-excluded or platform-specific code as if it were live — inflating complexity/function counts and producing false positives for "dead code" queries (Phase 6).

---

## PHASE 4 — Query Engine Review

### Current state, restated precisely

- **Lexer** (`sql/lexer.rs`, 178 lines): single-quoted strings only (no escapes beyond doubled `''`), unsigned integers only (no floats, no negative literals as tokens — `-5` would lex as... there's no `-` token at all, so negative numeric literals are **unsupported**), ASCII identifiers, a fixed keyword set (`SELECT FROM JOIN ON WHERE ORDER BY LIMIT ASC DESC AND OR NOT IN LIKE`).
- **AST** (`sql/ast.rs`, 88 lines): `SelectStatement { select, from, joins, where_clause, order_by, limit }`. `SelectItem` is `Wildcard | Column(String)` — **no expressions, no function calls, no aliases** in the select list.
- **Parser** (`sql/parser.rs`, 653 lines incl. 12 tests): recursive descent, standard precedence climbing for `OR > AND > NOT > comparison`, plus `IN`/`NOT IN`/`LIKE`. Single `FROM` table + `INNER JOIN ... ON` only (`JoinKind` has exactly one variant, `sql/ast.rs:30-32`).
- **Planner** (`plan.rs`, 248 lines): walks the AST and **re-serializes it to a SQL string** via `render_select`/`render_expr`/etc.
- **Execution** (`execute.rs`): opens a **fresh in-memory DuckDB connection per query** (`storage::open_batch`), creates 6 tables, inserts the batch, runs `plan.sql`, and converts every `duckdb::types::Value` to `serde_json::Value` (`to_json_value`, `execute.rs:78-126`).

### Can this architecture support these capabilities?

| Feature | Supported today? | What's actually needed |
| --- | --- | --- |
| Aggregations (`COUNT`, `SUM`, `AVG`, `GROUP BY`/`HAVING`) | **No** — `SelectItem` has no function-call/expression variant, no `GroupBy`/`Having` in `SelectStatement` | New AST nodes + lexer support for `(`-after-identifier function-call syntax + parser support for `GROUP BY`/`HAVING` + planner rendering — *all of which DuckDB already does* |
| Window functions (`ROW_NUMBER() OVER (...)`) | **No** | Same as above, plus `OVER`/`PARTITION BY`/frame syntax — a substantial parser addition |
| Recursive queries (`WITH RECURSIVE` — needed for call-graph traversal!) | **No** — no `WITH` at all | New top-level AST node (`CteList`), parser support, planner rendering. DuckDB supports `WITH RECURSIVE` natively today |
| CTEs (non-recursive) | **No** | Same |
| Materialized views | **No** — every invocation is a fresh in-memory DB; nothing persists between runs | Requires the persistence model change discussed in Phase 8 (file-backed DuckDB DB) before "materialized" means anything |
| Query caching | **No** — `execute_query` has no result cache, and the *input* (the batch) is rebuilt from a per-file cache but always re-ingested into a brand-new DuckDB instance | Needs a persistent DuckDB file + a cache-invalidation story keyed on the same per-file mtime/len signal `source.rs` already computes |

**Every single row in this table has the same root cause and the same fix.** ql is not missing these features because DuckDB can't do them — DuckDB supports `GROUP BY`, window functions, `WITH RECURSIVE`, persistent on-disk databases, and prepared-statement caching *today*, with no additional dependency. ql is missing them because **its hand-rolled SQL grammar is the bottleneck, not DuckDB.**

### The architectural fork in the road

**Option A (current path):** keep extending `sql/{lexer,ast,parser,plan}.rs` to cover more of SQL. Each feature (aggregates, CTEs, window functions, `CASE`, subqueries) is a multi-hundred-line addition across all four files plus tests — essentially a multi-year project to re-implement a worse copy of a parser DuckDB ships for free. This also means ql's SQL dialect will always lag and subtly diverge from real SQL/DuckDB SQL, producing a "SQL, but not quite" experience that erodes the core "SQL is universal" advantage from Phase 1.

**Option B (recommended): invert the relationship.** Treat DuckDB's parser as the real one. ql's job becomes:

1. Parse the user's query with a proper SQL AST (either via DuckDB's own `json_serialize_sql`/`PRAGMA` introspection, or via the `sqlparser` crate which already parses ANSI SQL + a `duckdb` dialect).
2. **Validate, don't translate**: walk that AST and reject anything that isn't a read-only `SELECT`/`WITH ... SELECT` over the known table names — no `ATTACH`, `COPY`, `PRAGMA`, `INSTALL`, `CREATE`, `INSERT`, `UPDATE`, `DELETE`, `CALL`, table-valued functions that touch the filesystem, etc. This is a **much smaller surface** than reimplementing SQL: a denylist/allowlist over statement and function types, not a grammar.
3. Pass the **original query text** to DuckDB unmodified.

This single change unlocks aggregates, `GROUP BY`, `CASE`, subqueries, window functions, and `WITH RECURSIVE` **immediately**, for free, with full DuckDB-dialect fidelity (DuckDB's excellent docs/error messages become ql's docs/error messages). The ~1,167 lines in `ql-core/src/sql/*` shrink to a validation pass that's an order of magnitude smaller and *more* defensible from a security standpoint (an allowlist of statement kinds is much easier to reason about than "did my hand-written grammar accidentally accept something dangerous"). The `is_valid_identifier`/sanitization logic in `plan.rs` becomes unnecessary because **no string concatenation of the query happens at all** — the user's text goes to DuckDB as-is, after AST-level validation.

This is, by a wide margin, the **highest-leverage architectural recommendation in this audit.** It simultaneously: (a) closes the aggregation/CTE/window-function gap that blocks nearly all of Phase 6's static-analysis use cases, (b) reduces code, (c) improves correctness (DuckDB's parser is more correct than a hand-rolled one), and (d) makes "SQL for codebases" *actually mean SQL*.

### Optimization opportunities (secondary, but real)

- **Connection-per-query** (`storage::open_batch`, called fresh inside `execute_query`, `execute.rs:46`) means schema creation + full re-insert happens every time, even in `--watch` mode where only one file changed. A persistent file-backed `Connection` + per-file `DELETE FROM ... WHERE file = ?` / re-`INSERT` on change would turn watch-mode refreshes from "rebuild everything" into "update the rows for the changed file."
- `execute_query` buffers the entire result into `Vec<Vec<serde_json::Value>>` (`execute.rs:49-70`) — fine for `LIMIT 100`, but `SELECT * FROM functions` on a large repo with no `LIMIT` (which the grammar allows — `limit` is `Option<u64>`) has no backpressure/streaming. For CLI table output this is unavoidable anyway (need full result to compute column widths), but for JSON/CSV it could stream.

---

## PHASE 5 — Schema Design Review

### `schema/tables.json` is documentation, not a source of truth

`schema/tables.json` declares 6 tables with column lists. Separately:

- `crates/ql-ast/src/rows.rs` defines `FunctionRow`/`CallRow`/etc. as Rust structs with the same fields.
- `crates/ql-core/src/storage.rs:11-63` hand-writes `CREATE TABLE` DDL with the same columns again.
- `crates/ql-core/src/storage.rs:65-159` hand-writes `INSERT` statements with the same columns a *third* time.
- `README.md` documents the same columns in prose a *fourth* time.

Today these four representations agree (I diffed them). **Nothing enforces that they continue to agree.** There is no test that says "every field in `FunctionRow` has a corresponding column in the `functions` DDL and in `schema/tables.json`." The moment someone adds a field to `FunctionRow` and forgets `storage.rs`, `cargo build` still succeeds (the `params![...]` macro just has the wrong arity → compile error, actually — so `storage.rs` insert *will* fail to compile if a field is added/removed from the row struct without updating the insert, which is a small safety net). But `schema/tables.json` and `README.md` have **zero compile-time or test-time connection** to the actual schema — they can silently drift into documentation that lies about the tool's behavior. **Recommendation:** generate `rows.rs` struct definitions *and* `storage.rs` DDL/inserts *and* `schema/tables.json` from one source (even a simple build-script/macro reading a single Rust `macro_rules!`-based table-definition list would collapse 4 sources of truth into 1).

### Normalization issues in the current 6 tables

1. **`structs.implements` is a denormalized CSV string** (`rows.rs:40`, populated by `merge_csv`/`normalize_csv_list` in `analysis.rs:94-109` and per-adapter `implements()` functions). This conflates two *semantically different* relationships that every OOP language distinguishes:
   - **Inheritance** (`extends` — single superclass in Java/C#/TS/Python(MRO), or trait `impl` blocks in Rust which are actually trait *implementations*, not inheritance at all).
   - **Interface implementation** (`implements` — multiple, in Java/C#/TS/Go-via-duck-typing).

   Python's adapter (`python.rs:118-131`) and TypeScript's (`typescript.rs:241-266`) both fold `extends`+`implements` into one CSV column. Rust's `impl Trait for Struct` (`rust.rs:177-205`) is *also* dumped into the same `implements` column — but a Rust `impl Display for User` is not "User implements Display" in the Java sense; it's closer to "User has an implementation of the Display interface," which happens to be the right English phrase but the wrong *queryable shape* once you add Java/C# where `extends` and `implements` are genuinely different keywords with different multiplicity rules (single extends, multiple implements). **A CSV string also can't be joined** — `WHERE implements LIKE '%Display%'` works for exact matches but breaks the moment two trait names share a substring (`Display` vs `DisplayExt`), and can never be the target of a `JOIN`.

   **Fix:** a normalized `type_relations` table: `(file, line, type_name, related_type, relation_kind)` where `relation_kind ∈ {extends, implements, impl_trait}`. One row per relationship. This is strictly more expressive, is joinable, and scales cleanly to languages with multiple interfaces.

2. **No methods-to-type linkage.** Go's `map_method` (`go.rs:113-147`) and TypeScript's `map_method` (`typescript.rs:118-149`) both push methods into the *same* `functions` table as free functions, with **no column recording which struct/class the method belongs to** (no `receiver`/`parent_type`/`owner` field). Today you cannot write `SELECT * FROM functions WHERE owner = 'User'` — methods and free functions are indistinguishable except by convention (and Python/Rust adapters don't even add class methods to `functions` consistently — Python's `map_class` (`python.rs:88-116`) counts methods toward `field_count` but the method *itself* is still picked up separately by `map_function` since `function_definition` nodes inside a class body are still visited by the generic walk). **This is the single most consequential missing column** for OOP-heavy languages (Java, C#, Kotlin, Swift, PHP, Ruby in Phase 7) — without it, "find all public methods of `UserService`" — one of the most natural queries for a SQL-over-code tool — is impossible.

3. **No qualified/stable identifiers.** Every cross-reference in the schema is by *raw name string*: `calls.caller`/`calls.callee` (`rows.rs:16-22`), `comments.attached_to` (`rows.rs:58`), `structs.implements`. Two files can both define a function `process`, and `calls.callee = "process"` cannot distinguish them. There is no `(file, line)`-based or synthetic-ID-based foreign key anywhere. This is the root blocker for Phase 6 (dead code/call graphs) — see below.

4. **`variables.scope`** (`rows.rs:49`) is a free-text string (`"module"`, `"function"`, `"package"` depending on language — Go uses `"package"`/`"function"` (`go.rs:278-283`), Rust/Python/TS use `"module"`/`"function"`) — another cross-language inconsistency in what should be a small closed enum.

5. **No end-line/end-column.** Every row has `line` (start line, 1-based) but no `end_line` — so you cannot compute "function length in lines" (a very common complexity proxy) or extract a function's full source text for display. Trivial to add (`node.end_position().row + 1`) and high value.

### What important code concepts are missing (prioritized)

| Concept | Priority | Why | Schema sketch |
| --- | --- | --- | --- |
| `end_line`/`end_column` on every row | **P0** | Enables function-length metrics, source extraction, "jump to range" in the VS Code extension | Add 2 columns to every existing table |
| Methods↔type linkage (`owner`/`receiver` on `functions`) | **P0** | Unlocks "methods of X" queries; required before Java/C#/Kotlin/Swift/Ruby/PHP adapters (Phase 7) can be useful | `functions.owner: TEXT` (nullable/empty for free functions) |
| `type_relations` (extends/implements/impl) | **P0** | Replaces the lossy `implements` CSV; required for inheritance-heavy languages | New table, see above |
| Stable symbol IDs + a `symbols`/`definitions` table | **P0** | Root fix for call-graph/dead-code analysis (Phase 6) | `(id, file, line, end_line, kind, name, qualified_name)` — `calls`/`comments.attached_to`/etc. reference `id` |
| `packages`/`modules` | **P1** | Go packages, Rust modules, Python packages, Java packages/namespaces — currently invisible; needed for "layering" architecture rules at the package (not just file-path) level | `(name, file, kind)` |
| `enums` | **P1** | Present in Rust, Go (via const groups/iota), TS, Python (Enum), Java, C#, Swift, Kotlin, C/C++ — currently produces *nothing* (an `enum` declaration in Rust doesn't match any `map_node` arm and is silently invisible) | New table: `(file, line, name, variant_count, visibility)` |
| `traits`/`interfaces` as first-class declarations (not just relations) | **P1** | Rust `trait`, Go interface `type X interface{}`, TS `interface`, Java/C#/PHP/Kotlin `interface` — currently a Rust `trait Greeter {}` (as in `rust.rs`'s own test fixture, `rust.rs:424`) produces **zero rows anywhere** | New table: `(file, line, name, method_count, visibility)` |
| `generics`/type parameters | **P2** | Needed for meaningful "this function is generic over T" queries; mostly metadata (count/names of type params) on existing tables | Add `type_params: TEXT` (CSV or normalized table) to `functions`/`structs` |
| `annotations`/`decorators`/`attributes` | **P2** | Python decorators, Java/C# annotations, Rust attributes (`#[tokio::main]`, `#[derive(...)]`) — currently invisible; high value for framework-aware queries ("find all `@Test` methods", "find all Axum route handlers via `#[get(...)]`") | New table: `(file, line, target_name, target_kind, name, args)` |
| `dependencies`/manifest parsing | **P2** | `Cargo.toml`/`package.json`/`go.mod`/`requirements.txt` — currently completely out of scope; needed for "what does this repo depend on" and supply-chain-adjacent queries | New table: `(file, manager, name, version_constraint)`, populated by manifest-specific parsers (not Tree-sitter — these are mostly TOML/JSON/text) |
| `macros` (Rust macro definitions/invocations as entities) | **P3** | Lower priority — fundamentally limited by Tree-sitter (Phase 3) | Best-effort: record macro *invocations* as a relation, not expansions |

### Evolution strategy

Given the current four-way-duplicated schema definition (Finding above), **the highest-priority schema change is not a new table — it's making the existing 6 tables have a single source of truth**, so that the P0/P1 additions above don't compound the drift problem 6x → 12x. Concretely: define tables once (e.g., a `define_table!` macro or a small build-time codegen reading `schema/tables.json` and emitting both the Rust row structs and the DuckDB DDL/insert SQL). Then add `end_line`/`end_column` (mechanical, every adapter, low risk) and `functions.owner` (touches Go/TS method mappers + Rust/Python class-body handling) as the first two schema PRs, before adding any new tables.

---

## PHASE 6 — Static Analysis Potential

### What ql can do *today*, with the current schema (an underrated strength)

Because `imports` records `(file, line, module, alias, is_std)` and is keyed by file path, **layering/architecture rules are expressible right now**:

```sql
SELECT file, module FROM imports
WHERE file LIKE 'src/ui/%' AND module LIKE '%db%'
```

This is a genuinely useful "Semgrep-lite" capability that requires *zero* new schema — it's a real, shippable feature today, and it's under-marketed (the README's 10 examples don't include a layering-rule example, which is one of the most "wow, I didn't expect SQL to do that" demos available).

Similarly, **basic security-pattern queries work today** via the `calls` table's free-text `callee`:

```sql
SELECT file, line, caller FROM calls WHERE callee LIKE '%unwrap%'
SELECT file, line, caller FROM calls WHERE callee LIKE '%eval%' OR callee LIKE '%exec%'
```

— a real, if shallow, "find risky calls" linting capability.

And **code-quality metrics work today**: `complexity`, `param_count`, `has_test`, and (via `comments.attached_to`) "undocumented public functions":

```sql
SELECT f.name, f.file, f.line FROM functions f
LEFT JOIN comments c ON c.attached_to = f.name AND c.is_doc = true
WHERE f.visibility = 'public' AND c.name IS NULL
```

(modulo the `complexity` cross-language inconsistency from Phase 2/5, and the name-collision risk in that `JOIN` from the lack of stable IDs).

### What's currently impossible, and why

| Capability | Blocked by | Schema/engine change required |
| --- | --- | --- |
| **Dead code detection** | `calls.callee` is a raw string; `JOIN functions ON functions.name = calls.callee` has massive false positives (name collisions across files) and false negatives (`self.foo()` → callee `"self.foo"`, never equals `functions.name = "foo"`) | Stable symbol IDs (Phase 5 P0) + resolved references; *also* needs to account for trait-dispatch/dynamic dispatch (a function only "called" via a trait object won't textually match) |
| **Call graph analysis** | Same root cause — `calls` has no foreign key to `functions` | Same fix; plus recursive CTEs (Phase 4) to traverse the graph (`WITH RECURSIVE reachable AS (...)`) |
| **Dependency analysis** (external packages) | No manifest parsing at all | New `dependencies` table (Phase 5 P2) |
| **Complexity analysis** | Exists, but **not comparable across languages** — see below | Unify the per-language `count_complexity` node-kind tables into one declarative, audited McCabe definition |
| **Architecture rules** | Mostly works today via `imports` (see above); deeper rules ("no `unsafe` outside `src/ffi`") need new node captures | Add targeted capture tables for language-specific risk constructs (`unsafe` blocks, `eval`, raw SQL string concatenation) |
| **Security rules** | Shallow text-matching on `calls.callee` works for "did you call `eval`"; **taint/data-flow** ("does this `eval` argument originate from `request.body`") is fundamentally out of reach without a data-flow/SSA layer | This is CodeQL's core differentiator and a multi-year investment — **not a near-term goal**, should be explicitly scoped out rather than implied |
| **Linting / code quality metrics** | Partially works (complexity, param_count, doc coverage); function length needs `end_line` (Phase 5 P0) | `end_line` + normalized complexity |

### The `complexity` cross-language inconsistency, concretely

- **Rust** (`rust.rs:26-49`): `+1` for each of `if_expression | for_expression | while_expression | loop_expression | match_expression | match_arm`. **A `match` with 5 arms scores `+1` (for `match_expression`) `+5` (one per `match_arm`) = +6** beyond the base score of 1.
- **Go** (`go.rs:25-45`): `+1` for each of `if_statement | for_statement | range_clause | switch_statement | select_statement | case_clause | go_statement`. **A `switch` with 5 cases scores `+1` (switch_statement) `+5` (case_clause) = +6`** — same double-counting pattern, different keywords.
- **TypeScript** (`typescript.rs:46-83`): `+1` for `if/for/for_in/while/switch_case/catch/conditional_expression`, **plus `+1` for each `&&`/`||` in a binary expression** — but **does NOT add for `switch_statement` itself** (only `switch_case`), and **does add for short-circuit boolean operators**, which neither Rust nor Go do.
- **Python** (`python.rs:34-56`): `+1` for `if/for/while/match_case/except_clause/elif_clause`, **plus `+1` for `boolean_operator`** (Python's `and`/`or`) — similar to TS's boolean handling, different from Rust/Go.

**Net effect:** a function with one `if` and one boolean `&&` scores complexity **2** in TS/Python but **1** in Rust/Go (no boolean-operator bonus). A function with a 5-arm `switch`/`match` scores **+6** in Rust/Go but a 5-case TS `switch` scores **+5** (no statement-level bonus) and Python has no `switch` equivalent at all. **`WHERE complexity > 10` selects a structurally different population of functions depending on `file`'s language** — this directly contradicts the "SQL across your whole polyglot codebase" pitch and should be considered a **correctness bug**, not a stylistic quirk. Fix: define McCabe complexity once (`+1` per decision point: `if`/`elif`/`for`/`while`/`case`/`catch`/`&&`/`||`/`?:`, with the *container* node — `match_expression`/`switch_statement` — contributing `0`, only its *arms/cases* contributing `+1` each, uniformly), then re-derive each adapter's node-kind table from that single definition.

### What schema additions are required, summary

For Phase 6 to mature from "declaration inventory + text-pattern matching" (today) to "CodeQL-lite" (the stated goal): **(1)** stable symbol IDs + resolved call/reference edges, **(2)** `type_relations` table, **(3)** `functions.owner`, **(4)** unified complexity definition, **(5)** `end_line`. These five changes — all schema/Phase-5 items — are the *engine* changes Phase 6 needs; no new DuckDB capability is required (Phase 4's recursive-CTE unlock handles the "analysis" part once the data is resolvable).

---

## PHASE 7 — Language Scalability

### Per-language effort estimate (current architecture)

Using `rust.rs`/`go.rs` (~400 lines each, including tests) as the template, a new adapter requires: tree-sitter grammar dependency, `map_node` dispatch, `map_function`/`map_call`/`map_import`/`map_class_or_struct`/`map_variable`/`map_comment`, plus the four duplicated helpers (`find_enclosing_function`, `count_complexity`, `count_params`, visibility classifier). For a contributor fluent in Rust + the target grammar:

| Language | Estimated effort | Why |
| --- | --- | --- |
| Java | 2-3 days | Similar shape to Go/TS (classes, interfaces, methods with receivers) but **needs `functions.owner`** (Phase 5 P0) to be useful — without it, every method/field collapses into the same flat tables as everything else, with annotations (`@Override`, `@Test`) invisible |
| C# | 2-3 days | Same as Java; additionally `partial class`, properties (getter/setter as pseudo-fields), and `using` aliases need mapping decisions |
| Kotlin | 3-4 days | Data classes, extension functions (a function "attached" to a type it doesn't own — doesn't fit `owner` cleanly), null-safety operators affecting complexity scoring |
| Swift | 3-4 days | Protocols (interfaces), extensions (similar issue to Kotlin), property wrappers/attributes |
| C | 1-2 days *for the happy path*, but... | No classes/structs-with-methods/visibility (file-`static` is the closest analogue to "private"); the dominant complexity is **preprocessor macros** (see below) |
| C++ | 4-6 days, possibly more | Namespaces, templates (generics), multiple inheritance, operator overloads, header/source split (declaration in `.h`, definition in `.cpp` — are these the "same" function?), and **heavy macro usage** |
| PHP | 2-3 days | Traits (PHP has its own `trait` keyword, distinct from Rust's), namespaces, visibility modifiers map cleanly |
| Ruby | 3-4 days, the hardest of the "easy" languages | **Visibility is not a node modifier** — `private`/`protected`/`public` are *method calls* that change the visibility of subsequently-defined methods (`private; def foo; end`). None of the existing adapters' `is_public`/`is_exported`/`is_private` pattern (a per-node-kind check, `rust.rs:10-17`, `go.rs:10-12`) can express "visibility is a stateful property of declaration order within a class body" — this requires a stateful traversal, a new pattern not present in any current adapter |

### Architecture changes that become necessary (not optional) past ~6 languages

1. **Extract the four duplicated helper families** (Phase 2 finding) into shared, table-driven abstractions *before* adding Java/C#/Kotlin — otherwise each new language adds another full copy of `find_enclosing_function`, and any future change to "how do we find the enclosing function" (e.g., to support closures/lambdas as scopes, which **none** of the current 4 adapters handle — a `callee` invoked inside a closure passed to `.map()` currently attributes to the *outer* function only) needs to land in 10+ files instead of 1.

2. **`functions.owner` (Phase 5 P0) becomes mandatory**, not nice-to-have, the moment Java/C#/Kotlin/Swift/PHP/Ruby arrive — these languages are *fundamentally* organized around methods-on-types in a way Go/Rust (free functions + occasional methods) and Python/TS (mixed) are not. Without it, half of each new adapter's extracted data (every method) is schema-orphaned.

3. **Stateful/scope-aware traversal** for Ruby-style visibility (and, more generally, for closures/lambdas as `find_enclosing_function` scopes) means the current "stateless `map_node(node, source, rows)` called independently per node" model (`adapter.rs:42`) needs an accumulator that can be threaded through the walk — e.g., `map_node` gaining a `&mut Vec<ScopeFrame>` "current scope stack" parameter that `walk_node` pushes/pops as it enters/exits scope-introducing node kinds. This is a **trait signature change** affecting every existing adapter, so it's better done *before* Ruby/Kotlin arrive than retrofitted after.

4. **The `tags.scm`-based extraction (Phase 3)** becomes the difference between "2-3 days per language" and "1 day per language" at scale — for Java/C#/PHP/Ruby, community `tags.scm` files already encode "what is a method/class/interface" and would let a new adapter start from a working capture set rather than reverse-engineering each grammar's node-kind names from scratch (which is currently how `rust.rs`/`go.rs`/etc. were clearly built — by inspecting `tree-sitter parse` output).

5. **C/C++ macro reality check**: unlike Rust's `macro_rules!` (invisible but at least *self-contained*), C/C++ `#define` macros are used for **conditional compilation that determines which `function_item`-equivalent nodes even exist** (`#ifdef _WIN32 ... #else ... #endif` defining two different versions of the same function). Tree-sitter parses **both branches as sibling text**, so ql would report duplicate-looking function definitions with no indication that they're mutually exclusive. This isn't a "fix the adapter" problem — it's a fundamental property of analyzing C/C++ without a preprocessor pass, and should be **documented as a known limitation** before investing in C/C++ adapters, or scoped as "parse macro-expanded/preprocessed output" (a much bigger undertaking, effectively requiring `clang -E` or similar).

### Bottom line for Phase 7

Adding languages 5-8 (Java, C#, PHP, one more) without first doing items 1-3 above will roughly **double the codebase's duplication ratio** (from "4x copies of 4 helpers" to "8x copies of 4 helpers, now also wrong for half of them because Java/C# need `owner` and the helpers don't have it"). The architecture *can* scale to 20+ languages, but **only if the abstraction work happens at language #5, not language #15.**

---

## PHASE 8 — Performance Review

### Parsing performance

Tree-sitter itself is fast (low-single-digit microseconds per KB for most grammars) and is **not** the bottleneck at any scale considered here. The per-file mtime/length cache (`crates/ql-cli/src/source.rs:150-159`, keyed on `(modified_secs, modified_nanos, len)`) means unchanged files skip re-parsing entirely on subsequent runs — good.

### Memory usage

Every row (`FunctionRow`, `CallRow`, etc., `rows.rs`) stores `file: String` as an **owned, independently-allocated copy** — `rows.current_file.clone()` is called once per row pushed (e.g. `rust.rs:73`, `:99`, `:137`, etc.). For a project with 50,000 functions across 2,000 files, that's 50,000 separate heap allocations of (on average) the same handful of thousand file-path strings. At 10M LOC scale (estimate: ~1-2M extracted rows across all tables), this is **single-digit GB of pure string duplication** before counting the actual content (names, types, comment text). `Rc<str>`/string interning for `file` (and likely `module`/`type_hint`/`return_type`, which also repeat heavily) would cut memory significantly with minimal API disruption (an `Rc<str>` clones cheaply and serializes the same via serde).

### DuckDB loading

As established in Phase 3/4: **row-by-row prepared-statement inserts with no transaction** (`storage.rs:65-159`). DuckDB's `Appender` API is the documented fast path for bulk programmatic loads (columnar batched writes, no per-row statement overhead/auto-commit). This is a mechanical rewrite of `insert_batch` with high payoff.

### Query execution

Once data is in DuckDB, query execution itself is **not** a concern — DuckDB is a columnar OLAP engine designed for far larger datasets than any codebase's fact tables will produce. The entire performance story is in the **ETL path** (parse → second-pass → insert), not the query path. This is good news: the fixes are concentrated and don't require touching the query engine.

### Estimated behavior by scale

| Scale | Parsing | `second_pass` (current O(C×(F+S))) | DuckDB insert (current row-by-row) | Overall, current architecture | After fixes (transactional/Appender insert + linear second_pass + persistent DB) |
| --- | --- | --- | --- | --- | --- |
| 10K LOC (~1-2K rows total) | <100ms | negligible | <100ms | **Sub-second**, no issues | Sub-second |
| 100K LOC (~10-20K rows) | ~1s (mostly cold-cache) | noticeable but tolerable (10K comments × 5K functions ≈ 5×10^7) | ~1-3s (thousands of implicit transactions) | **A few seconds per invocation** — annoying in `--watch` mode (Phase-8's 500ms poll loop, see below, makes this worse) | <1s |
| 1M LOC (~100-200K rows) | ~5-10s cold, fast warm (cache) | **10^9-10^10 ops — minutes**, dominates everything | tens of seconds | **Minutes per invocation** — the tool becomes "run it and go get coffee," and `--watch` mode is unusable (re-runs this every 500ms after any change) | Single-digit seconds |
| 10M LOC (~1-2M rows) | ~1min cold | **hours to never** (10^11-10^12 ops) | minutes | **Effectively broken** — `second_pass`'s quadratic comment-attachment alone makes this non-terminating in practice; the single monolithic JSON cache file (also discussed below) would itself be GB-scale | Tens of seconds, dominated by I/O |

The `second_pass` quadratic algorithm is the **dominant term at 1M+ LOC** by orders of magnitude — it should be the #1 performance fix (and is mechanically simple: sort `functions`/`structs` by `(file, line)` once, then for each file's comments do a sorted merge/binary-search instead of a full scan — turns O(C×(F+S)) into O((C+F+S) log(F+S))).

### The cache file itself becomes a bottleneck at scale

`crates/ql-cli/src/source.rs:161-189`: the **entire** per-file cache (every file's `TableBatch`, including all extracted rows) is serialized as **one JSON document** to `~/.cache/ql/<hash>.json` and **rewritten in full on every invocation** (`write_cache`, `:168-177`), even if only one file changed. At 1M+ LOC, this JSON file is plausibly hundreds of MB to low GB — `serde_json::to_string` on a struct that size allocates a same-sized `String` (potentially 2x momentarily during write), and `fs::write` is **not atomic** (no temp-file + rename), so a process killed mid-write (very plausible in `--watch` mode, which the user is likely to `Ctrl-C`) **corrupts the cache**, silently falling back to `unwrap_or_default()` (`read_cache`, `:161-166`) — i.e., a full cold rebuild next run, which itself could be interrupted again. Recommendation: per-file cache entries (one small file per source file, or a single embedded KV store / SQLite-as-cache) + atomic write (write to `.tmp`, `rename`).

### `--watch` mode at scale

`crates/ql-cli/src/watch.rs:23-36`: polls every 500ms via `scan_snapshot` (`source.rs:51-65`), which calls `walk_relative_files` (full directory tree walk + `std::fs::metadata` **stat() on every source file**) every 500ms, forever. At 100K+ files (node_modules-adjacent monorepos — compounded by the missing `.gitignore` support, Phase 9), this is 100K+ `stat()` syscalls twice a second, indefinitely, just to detect "did anything change" — before even considering the full re-parse+second_pass+reinsert that follows a detected change. A `notify`-crate-based (inotify/FSEvents/ReadDirectoryChangesW) watcher would turn this into "the OS tells us what changed," eliminating both the polling cost and the latency (up to 500ms today).

### Summary of optimizations, in priority order

1. Fix `second_pass`'s O(C×(F+S)) comment-attachment (`ql-ast/src/analysis.rs:42-65`) → linear via sort+sweep. **Single highest-impact fix in the entire codebase for large repos.**
2. Wrap `insert_batch` in a transaction or switch to DuckDB's `Appender` (`ql-core/src/storage.rs`).
3. Add `.gitignore`/`.qlignore` support to `walk_relative_files` (also a Phase 9 finding) — reduces the *input size* to everything above, often by 10-100x for JS/Python projects with `node_modules`/`.venv`.
4. Replace polling-based `--watch` with `notify`-based file watching.
5. Per-file (or embedded-DB) cache with atomic writes, instead of one monolithic JSON blob.
6. String interning for `file`/repeated strings in row structs.
7. Persistent file-backed DuckDB database with incremental per-file `DELETE`+`INSERT` (this is also Phase 4's prerequisite for materialized views/query caching).

---

## PHASE 9 — Open Source Readiness

### Crate organization

Good: 4 crates with clear, non-overlapping names, a sensible dependency direction (`cli → core → adapters → ast`), and each crate's `Cargo.toml` is minimal. The one hygiene gap found: `ql-core`'s unused `ql-adapters` dependency (Phase 2) — a 30-second fix that's worth doing precisely *because* it's the kind of thing a `cargo machete`/`cargo udeps` CI check would catch, and there is no CI.

### Contributor experience — what's missing

- **No LICENSE file.** `find . -iname "LICENSE*"` returns nothing. This is the single biggest blocker for *any* external adoption — companies' legal/OSS-compliance processes will not approve depending on (or even evaluating) a repository with no declared license, regardless of code quality. The VS Code extension's `package.json` declares `"license": "MIT"` (`extension/package.json:7`) but **no `LICENSE` file exists at any level** to back that claim, and the workspace root has none at all. This should be the literal first commit of any "OSS readiness" push.
- **No CI.** No `.github/workflows/`, no equivalent. `cargo test`, `cargo clippy`, `cargo fmt --check` are not automated. For a project this size, a single GitHub Actions workflow (`cargo build --workspace`, `cargo test --workspace`, `cargo clippy -- -D warnings`, `cargo fmt -- --check`) is a ~30-line YAML file and the single highest-leverage OSS-readiness change after the license.
- **No CONTRIBUTING.md.** No guidance on how to add a language adapter, what the testing convention is, or how `schema/tables.json` relates to `rows.rs`/`storage.rs` (Phase 5) — a newcomer has to *infer* the four-way schema duplication by reading all four places.
- **`MILESTONE.md` and `README.md`'s TODO section** are reasonably honest about current status (`MILESTONE.md` Phase 5 explicitly lists "Cross-platform test on macOS" as not done; README TODO lists "cross-platform," "better UI"). This honesty is good — but a TODO list is not a roadmap a contributor can pick a task from (no issue tracker usage visible, no "good first issue" labels referenced).
- **`.gitignore` excludes `milestone.md`/`project.md`/`dev.sh`** (`.gitignore:6-8`, lowercase — note `MILESTONE.md` the file that *is* tracked is uppercase, so this ignore entry is likely for a *different*, untracked lowercase-named planning file the author keeps locally) — i.e., the author has private planning notes not shared with contributors. Not wrong, but it means some project context lives outside the repo.

### Testing quality

Strong at the unit level: every adapter has 1-4 tests exercising `walk_source` end-to-end against small fixture strings (e.g., `rust.rs:345-463`, four tests covering functions, calls/imports/structs/variables/comments, impl-trait mapping, and complexity counting). `sql/parser.rs` has **12 tests** covering the supported grammar thoroughly (wildcard, column lists, comparisons, precedence, `NOT`, `IN`/`NOT IN`, `LIKE`, `ORDER BY`/`LIMIT`, trailing semicolons, joins, and two error cases) — this is genuinely solid coverage *of the subset that exists*. `execute.rs` has 4 tests covering select/filter/join/IN. `analysis.rs` has 1 test covering all three second-pass behaviors together. `format.rs` has 8 tests. `source.rs` has 6 tests.

**What's missing:**

- **No end-to-end test of the `ql` binary itself.** Every test exercises library functions (`walk_source`, `execute_query`, `format_response`) directly; nothing builds the binary and runs `ql "SELECT ..." <fixture-dir>` and asserts on stdout. For a CLI tool, this is the test that would have caught, e.g., the `--langs` exit-code-0 path or argument-parsing edge cases.
- **No multi-language fixture repository.** There's no `tests/fixtures/` with a small polyglot sample project used across adapters/integration tests — each adapter's tests use inline string literals, which is fine for unit tests but means there's no shared "does the whole pipeline work on a realistic small repo" check.
- **No fuzz/property testing of the hand-rolled SQL lexer/parser** (`sql/lexer.rs`/`sql/parser.rs`) — the most natural fuzz target in the codebase, since it parses arbitrary user-supplied text. I did not find any panics in manual review (the `'...'` string-literal handling at `lexer.rs:113-133` and the `.wrapping_sub(1)` bounds check at `:129` look correct), but "looks correct on manual review" is exactly the situation fuzzing exists for, and a hand-rolled parser is cheap to fuzz (`cargo-fuzz` + `parse_query` as the target, a few hours of setup).
- **No regression tests for the silent-failure modes identified in this audit** (1,000-directory cap, cache corruption on interrupted write, quadratic second-pass on large batches) — understandably, since these are *discovered* by this audit, but they're exactly the kind of thing that should get a test the moment they're fixed.

### Can a new contributor become productive in 1 hour / 1 day / 1 week?

- **1 hour:** Yes, for a Rust-fluent developer. `cargo build --release` (after a long first build due to DuckDB's bundled C++ amalgamation — `duckdb = { version = "1.10503.1", features = ["bundled"] }`, `ql-core/Cargo.toml:7` — **this first build will take several minutes**, worth calling out in a CONTRIBUTING doc so newcomers don't think something's broken), `./target/release/ql --langs`, run a couple of README example queries, skim `rows.rs` (89 lines) and `adapter.rs` (55 lines). The total "core mental model" surface is small enough to grasp in an hour.
- **1 day:** Yes, for a *templated* change — e.g., adding `.js`/`.jsx` to `TypeScriptAdapter`'s `extensions()` (`typescript.rs:340-342`) and verifying it still parses reasonably (JS is a subset of what the TSX grammar accepts), or adding a new boolean column to `FunctionRow` by following the existing pattern across `rows.rs`/`storage.rs`/one adapter/tests. The repetitive, templatable nature of the adapter pattern (Phase 2) is a *contributor-experience* asset even though it's a *maintenance* liability — the same duplication that will hurt at 20 languages makes "copy what `go.rs` does" a viable 1-day onboarding task today.
- **1 week:** Yes, for either a full new adapter (Java/PHP, following Phase 7's 2-3-day estimate plus review/iteration) or a moderate cross-cutting schema change (e.g., adding `end_line` to all 6 tables across all 4 adapters + `storage.rs` + `rows.rs` + `schema/tables.json` — mechanical but touches ~15 files). The **entire codebase (~4,400 lines) can be read in a single sitting**, which is rare and valuable — but it is a *temporary* asset that erodes with every un-refactored adapter addition (Phase 7).

---

## PHASE 10 — Product Strategy

Thinking as a startup CTO evaluating ql as a product bet, not just a codebase:

### Could ql become...

- **Open source project** — yes, and should be the *immediate* focus (add LICENSE+CI today; see Phase 13). The core idea is differentiated enough to attract contributors *if* the four-way schema duplication and per-language duplication (Phase 2/5/7) don't scare them off on first read, and *if* there's a license to contribute under.
- **Commercial product** — plausible, but not from the CLI alone. The defensible commercial wedge is **CI/CD policy enforcement** ("fail the build if any public function in `src/payments/` has complexity > 15 and no doc comment" — a SQL `WHERE` clause as a *policy*, version-controlled alongside the code) — this is a real budget line item (engineering-quality/compliance tooling) that Semgrep/CodeQL/SonarQube currently serve, and "write the rule in SQL" is a genuine differentiator for teams who already have SQL-fluent platform engineers but no Datalog/YAML-rule expertise.
- **Developer platform** — only after Phase 5's symbol-resolution work; "platform" implies other tools build on ql's data (e.g., an LSP server backed by the same DuckDB tables, a dependency-graph visualizer). The current schema (Phase 5) is too shallow (no stable IDs, no resolved references) to be a platform's foundation yet — building on it now would mean rebuilding it later.
- **VS Code-first product** — the extension (`extension/src/extension.ts`, ~550 lines) is a real asset: a webview with syntax-highlighted query input, results table, and **click-to-open-file-at-line** (`openRow`, `extension.ts:161-174`) — this is more polished than "scaffold." But it shells out to the `ql` binary per query (`spawnQuery`, `:111-140`) with **no persistent process/index** — every keystroke-triggered query re-runs the entire walk+parse+second_pass+DuckDB-rebuild pipeline (Phase 8's full cost, every time). For "VS Code-first," a long-lived `ql --serve` process (keeping the parsed batch + a persistent DuckDB connection warm, invalidating per-file on save) is a prerequisite — currently the extension is a thin, correct, but **performance-naive** wrapper around the CLI.
- **CI/CD analysis tool** — the *most immediately viable* of these, requiring the least new work: a single binary, JSON output (`--format json` already exists, `format.rs:31-46`), exit-code-based pass/fail (currently `process::exit(1)` only on errors, not on "query returned rows" — **a `ql check <query.sql> --fail-if-nonempty` mode would be a half-day feature** that turns ql into a CI gate today).

### Strongest opportunities

1. **"Architecture rules as SQL, checked in CI"** — uses only the *existing* `imports`/`functions` tables (Phase 6's "underrated strength"), needs only the `check`/exit-code feature above, and is a clear wedge against Semgrep/SonarQube for teams that find YAML rule DSLs unpleasant.
2. **The DuckDB-delegation rewrite (Phase 4)** — not a "feature" but a force-multiplier: every subsequent roadmap item (aggregation-based metrics dashboards, recursive call-graph CTEs, materialized historical trend tables) becomes *much* cheaper once the engine is "DuckDB + a validator" instead of "a SQL subset reimplementation."
3. **`tags.scm`-based adapter generation (Phase 3/7)** — turns "add a language" from a multi-day bespoke task into a templated one, which is the only realistic path to the 12+ languages this audit was asked to evaluate, and is also a *contribution magnet* (people who know a `.scm` file for their favorite language but not Rust can contribute).

### Biggest threats

1. **Sourcegraph, GitHub code search, and increasingly LLM-based "ask your codebase questions" tools** are converging on natural-language-over-code-search, which is a much lower-friction UX for *casual* questions than writing SQL — ql's differentiation must lean into what SQL is *uniquely good at* (aggregation, joins, repeatable/version-controlled policy checks), not "search," where it will lose to both ripgrep (speed) and LLM tools (ease).
2. **ast-grep** is the closest architectural sibling (same parser technology) and is more mature, multi-language already, and supports rewriting — if ast-grep added a SQL-like aggregation layer on top of its existing multi-language matches, it would directly threaten ql's niche with a head start on language coverage.
3. **The schema-inconsistency issues (Phase 5/6) are invisible until someone writes a cross-language query and gets nonsense results** — for an early-adopter audience (the kind that *would* try a novel SQL-for-code tool), a bad first impression from `WHERE complexity > 10` meaning different things per language could be reputationally costly in a way that's hard to walk back.

### Most valuable features to build next (product lens, cross-referencing the technical roadmap)

1. `ql check <file.sql>` with exit codes (CI gate) — cheap, high product value.
2. DuckDB-delegation rewrite (Phase 4) — expensive, but unlocks everything else cheaply afterward.
3. `.gitignore` support — cheap, removes the most likely "this tool is broken" first-run experience (running `ql` in a JS repo and getting thousands of `node_modules` rows).
4. `functions.owner` + unified complexity (Phase 5/6) — medium cost, fixes the cross-language correctness story before it becomes a reputational problem.
5. Persistent `ql --serve` for the VS Code extension — medium cost, transforms the IDE experience from "spinner every keystroke" to "instant."

---

## PHASE 11 — Competitive Analysis

### vs. CodeQL

CodeQL compiles source into a relational database too — conceptually the closest comparison. The difference is depth and resolution: CodeQL has full semantic models (data flow, type resolution, control-flow graphs) built on compiler-grade front-ends; ql has syntactic facts extracted via Tree-sitter (Phase 3). CodeQL's query language (QL/Datalog) is more expressive than ql's SQL subset (recursive predicates, the equivalent of recursive CTEs, are native to QL; ql's planner doesn't support CTEs at all — Phase 4). **ql's advantage**: SQL is a skill nearly every backend engineer already has; QL requires learning a bespoke language and the CodeQL CLI's compile/database-create workflow is heavyweight (minutes to hours for large repos). **ql's disadvantage**: CodeQL's semantic depth means it can answer "is this value tainted by user input by the time it reaches this sink" — a question ql's syntactic schema (Phase 5, no dataflow/no symbol resolution) cannot currently answer at all, and may never answer without a fundamentally different (type-checking) front-end per language.

### vs. Semgrep

Semgrep's core primitive is pattern-matching with metavariables (`$X.query($SQL)` style) plus a YAML rule format, increasingly with cross-file/cross-function taint tracking in the Pro tier. ql's primitive is "extract facts into tables, then SQL." **ql's advantage**: aggregation and joins are first-class in SQL but awkward-to-impossible to express as a single Semgrep pattern (e.g., "functions with complexity > 10 AND no test AND > 3 params" — Phase 6's example — is a 3-line `WHERE` clause in ql vs. requiring either three separate Semgrep rules or a post-processing script over Semgrep's JSON output). **ql's disadvantage**: Semgrep can *match and rewrite* code (autofix); ql is read-only by design — it answers "what" and "where," never "replace this with that." Semgrep also already supports ~30 languages with a registry of thousands of community rules; ql supports 4 languages with zero community rules.

### vs. ast-grep

The closest technical sibling — both are Tree-sitter-based, both are written in a systems language (ast-grep: Rust), both aim for multi-language structural analysis. ast-grep's primitive is structural pattern + rewrite (like Semgrep but faster/native, with a YAML rule format and a relevance "rule" object model). **ql's advantage**: the relational/SQL model supports aggregation queries ast-grep's pattern model fundamentally cannot ("average complexity per file," "top 10 files by import count" — Phase 1's competitive table) — ast-grep answers "where does pattern X occur," not "what's the distribution of Y across the codebase." **ql's disadvantage**: ast-grep already supports rewriting, has a much larger and more mature language-grammar integration layer (it's effectively *built* around managing many Tree-sitter grammars cleanly — exactly the abstraction ql's 4 duplicated adapters lack, Phase 7), and has an active multi-maintainer community. If ast-grep added a `--format duckdb`/aggregation mode, it would directly subsume ql's niche with a 2-3 year head start on language coverage and tooling maturity.

### vs. Sourcegraph

Sourcegraph is an indexed-search platform (often self-hosted/enterprise) with SCIP/LSIF-based precise code intelligence (go-to-definition, find-references across repos) plus a query language (`type:symbol`, `repo:`, regex). **ql's advantage**: SQL aggregation again — Sourcegraph's search query language is optimized for "find occurrences," not "compute statistics about occurrences." ql is also vastly simpler to run (single binary, no server/indexing infrastructure, no Postgres+blobstore deployment). **ql's disadvantage**: Sourcegraph's SCIP-based "find references" is *exactly* the symbol-resolution capability ql's schema lacks entirely (Phase 5's #1 gap — no `symbol_id`, no cross-file reference resolution); Sourcegraph also already operates at the multi-repo, enterprise scale ql hasn't been tested past tens of thousands of LOC (Phase 8).

### vs. ripgrep

ripgrep is the speed/simplicity baseline — instant, `.gitignore`-aware, regex-based text search, zero setup. **ql's advantage**: structure. `rg "fn \w+\("` cannot distinguish a function *definition* from a string containing that text, cannot tell you a function's complexity, and cannot join "functions with no test" against "functions called from main.rs." **ql's disadvantage**: ripgrep has zero startup cost and works on *any* text instantly; ql requires a full parse+second-pass+DB-load pipeline (Phase 8) before the first query, and (ironically, given ql's positioning) **ql doesn't respect `.gitignore`** (Phase 6/9) while ripgrep does by default — meaning today, `rg` gives a *cleaner* first-run experience on a typical JS/Python repo than `ql` does.

### Feature matrix

| Capability | ql | CodeQL | Semgrep | ast-grep | Sourcegraph | ripgrep |
| --- | --- | --- | --- | --- | --- | --- |
| Setup cost | Low (single binary) | High (DB build) | Low | Low | High (server) | None |
| Languages supported | 4 | 10+ | ~30 | ~20 | Many (via LSIF) | N/A (text) |
| Aggregation / GROUP BY | **No** (today) | Yes (Datalog) | No | No | No | No |
| Joins across fact types | **Yes** | Yes | No | No | Limited | No |
| Structural (AST-aware) match | Yes | Yes | Yes | Yes | Partial | No |
| Autofix / rewrite | No | No | Yes | Yes | No | No (`-r` is text-only) |
| Cross-file symbol resolution | **No** | Yes | Partial (Pro) | No | Yes (SCIP) | No |
| Dataflow / taint tracking | No | Yes | Partial (Pro) | No | No | No |
| `.gitignore`-aware | **No** | N/A | Yes | Yes | N/A | Yes |
| Recursive queries (call graphs) | **No** (no CTE) | Yes | No | No | Partial | No |
| Query language familiarity | High (SQL) | Low (QL) | Medium (YAML) | Medium (YAML) | Medium | High (regex) |
| Community rule registry | None | Large | Large | Growing | N/A | N/A |
| License declared | **No** | Yes | Yes | Yes | Yes (mixed) | Yes |

The bolded "Joins across fact types: Yes" row is ql's one *currently working* differentiator versus every peer in this table — `JOIN`s across `functions`/`calls`/`structs` etc. parse and execute today (`sql/parser.rs` tests cover joins; `sql/ast.rs:22-32` defines `Join`/`JoinKind`). But the bolded **"No" for aggregation** is the more important row: `SelectStatement` (`sql/ast.rs:1-9`) has no `GROUP BY`/`HAVING` field, and `Expr` (`sql/ast.rs:46-64`) has no aggregate-function or window-function variant — so `COUNT(*)`, `AVG(complexity)`, `GROUP BY file` etc. cannot even be *parsed*, let alone executed. This is the single most important finding in this audit (see Phase 12, #1): **the capability that most differentiates ql from every tool in this table — "SQL aggregation over code facts" — does not exist in the shipped grammar yet.** The README's 10 example queries (`README.md:49-107`) are all simple `SELECT...WHERE...ORDER BY` with no `GROUP BY`/aggregate, which is *honest* (nothing advertised is broken) but also means the aggregation pitch is currently aspirational. Combined with no `.gitignore` support and no declared license, these are the three highest-leverage fixes for first impressions — though the aggregation gap is architectural (Phase 4's Option A/B fork), not a quick patch.

---

## PHASE 12 — Top 50 Findings (Ranked)

Severity reflects technical/correctness impact. Priority reflects what to fix first given this is solo, pre-v1, zero-external-user software (P0 = blocks adoption or produces silently-wrong results; P1 = architectural, fix before it's expensive to undo; P2 = real but bounded; P3 = polish; P4 = informational/no action required).

### CRITICAL

**1. The SQL engine cannot express aggregation, `GROUP BY`, or window functions — ql's core "SQL for codebases" pitch doesn't work yet.**
Severity: Critical | Priority: P0 | Effort: Large (architectural)

- *Evidence*: `SelectStatement` (`crates/ql-core/src/sql/ast.rs:1-9`) has no `group_by`/`having` field; `Expr` (`ast.rs:46-64`) has no aggregate-function or window-function variant. `sql/parser.rs`'s 12 tests cover wildcard/columns/comparisons/`IN`/`LIKE`/`ORDER BY`/`LIMIT`/joins — none cover `COUNT`/`AVG`/`GROUP BY`.
- *Impact*: Queries like "average complexity per file" or "count of public functions per module" — the exact class of query that justifies "SQL" over grep/ast-grep (Phase 1, Phase 11) — cannot be parsed, let alone executed. The README's 10 examples avoid this honestly, but it means the differentiator is aspirational.
- *Fix*: This is Phase 4's central fork. Option A (extend the hand-rolled grammar with `GROUP BY`/`HAVING`/aggregate `Expr::FunctionCall` + planner support) is weeks of work to reach parity with a fraction of SQL. Option B (parse with DuckDB's own SQL parser via `duckdb`'s bound APIs, then walk *that* AST for an allowlist/validation pass before execution) gets `GROUP BY`, window functions, CTEs, and every join type for free, and is less code than Option A's partial reimplementation.

**2. No LICENSE file anywhere in the repository.**
Severity: Critical | Priority: P0 | Effort: Trivial

- *Evidence*: `find . -iname "LICENSE*"` returns nothing at the workspace root or in any crate. `extension/package.json:7` declares `"license": "MIT"` but no `LICENSE` file backs that claim anywhere in the repo.
- *Impact*: Any company's OSS-compliance process will reject evaluating, depending on, or contributing to a repository with no declared license — independent of code quality. This is the single highest-leverage non-code change available.
- *Fix*: Add a root `LICENSE` file (MIT, to match the extension's existing claim) and add `license = "MIT"` to each crate's `Cargo.toml`. Minutes of work.

**3. `resolve_comment_attachments` is O(C × (F+S)) — quadratic blowup at scale.**
Severity: Critical | Priority: P0 | Effort: Medium (1-2 days)

- *Evidence*: `crates/ql-ast/src/analysis.rs:42-65` — for every comment, scans every function and struct to find the nearest following definition.
- *Impact*: At ~1M LOC, C (comments) and F+S (functions+structs) are each plausibly in the hundreds of thousands, making this ~10^10 operations — the dominant cost of the entire second pass, likely turning "seconds" into "does not complete" at the upper end of Phase 8's scale estimates.
- *Fix*: Sort comments and definitions by `(file, line)` once, then do a single linear merge pass per file (or binary-search the sorted definitions for each comment). O(C log(F+S)) or O(C+F+S) instead of O(C×(F+S)).

**4. Silent 1,000-directory cap in the file walker — large repos get silently truncated.**
Severity: Critical | Priority: P0 | Effort: Trivial (fix) / needs regression test

- *Evidence*: `crates/ql-cli/src/source.rs:36` — `dirs.len() < 1000` guards whether a discovered subdirectory is pushed onto the traversal stack.
- *Impact*: On any repository with more than 1,000 subdirectories (not uncommon for a large monorepo with per-package/per-module directory structures), entire subtrees are dropped from the walk with **zero warning, log line, or error**. A user running `SELECT COUNT(*) FROM functions` gets a confidently-wrong number and has no way to know.
- *Fix*: Remove the cap (it has no documented purpose and silent data loss is strictly worse than a slow walk), or replace with a configurable limit that emits a visible warning to stderr when hit. Add a regression test with >1,000 synthetic directories.

**5. No `.gitignore`/ignore-file support — first run on a real repo walks `node_modules`/`.venv`/`target`/`build`.**
Severity: Critical | Priority: P0 | Effort: Small (half day)

- *Evidence*: `grep -ri gitignore crates/ql-cli/src` returns zero matches; `walk_relative_files` (`source.rs:22-49`) walks every directory unconditionally.
- *Impact*: Running `ql` in a typical JS repo parses every file in `node_modules` (often 10-100x the actual source code); in a Python repo, `.venv`. This multiplies parse time, pollutes every table with vendored/generated code, and is the most likely "this tool seems broken" first-run experience — directly undermining the "deterministic, no noise" pitch in `README.md:5`.
- *Fix*: Integrate the `ignore` crate (the same crate ripgrep uses) for `.gitignore`/`.ignore`/`.qlignore` handling. This is a drop-in replacement for the current walk and is a half-day change with outsized first-impression impact.

**6. The 6-table schema is defined independently in 4+ places with no single source of truth.**
Severity: Critical | Priority: P1 | Effort: Medium (2-3 days)

- *Evidence*: Column lists exist independently in `schema/tables.json` (e.g. `functions`: 8 columns), the `FunctionRow` struct (`crates/ql-ast/src/rows.rs`), the `CREATE TABLE functions` DDL (`crates/ql-core/src/storage.rs:11-63`), and implicitly in each of the 4 adapters' `map_function` functions which must populate every field in the right order.
- *Impact*: Any schema change (add a column, reorder, change type) requires manually updating 4+ independent files. A missed update produces either a DuckDB column-count/type error at `insert_batch` time (confusing, since the error appears unrelated to the user's actual query) or, in the worst case, silent column misalignment if types happen to coincide.
- *Fix*: Make `schema/tables.json` the single source of truth and generate the `Row` structs + DDL from it via a build script (`build.rs`) or a derive macro — or invert the direction and generate `schema/tables.json` from the `Row` struct definitions via a derive macro, whichever direction is more idiomatic for the Rust toolchain.

**7. `calls.is_external` misclassifies ordinary method calls as external in Rust and Go adapters.**
Severity: Critical | Priority: P1 | Effort: Medium (~1 day per language)

- *Evidence*: The `is_external` heuristic in `rust.rs`'s and `go.rs`'s call-mapping logic relies on whether the callee text contains a `.` — but `self.foo()` (Rust) and `t.Method()` (Go), the single most common call pattern in idiomatic object-oriented-style code in both languages, also contain a `.`.
- *Impact*: README's flagship example query #2 ("List all external calls", `README.md:55-59`) returns a result set dominated by false positives — ordinary internal method calls — on any real Rust or Go codebase, making this advertised use case effectively non-functional out of the box.
- *Fix*: Classify `is_external` based on whether the call's receiver/module prefix matches a name bound by an `imports` row (i.e., resolves to an external crate/package alias), not on the presence of a `.`. Requires the call-mapping logic to have access to the file's import list — a small but real coupling change.

**8. Cyclomatic complexity is computed with 4 different, mutually-inconsistent formulas — one per adapter.**
Severity: Critical | Priority: P1 | Effort: Medium (2-3 days incl. re-baselining tests)

- *Evidence*: `rust.rs:26-49` and `go.rs:25-45` both double-count `match`/`switch` (the `match_expression`/`switch_statement` node itself, *and* each `match_arm`/`case` adds another +1). `python.rs:34-56` adds an extra bonus for `boolean_operator` nodes. `typescript.rs:46-83` adds a `&&`/`||` bonus but no `switch_statement` bonus at all.
- *Impact*: README's flagship example query #1 (`WHERE complexity > 10`, `README.md:49-53`) means a *different threshold* per language. A 3-arm Rust `match` scores complexity contributions from both the match and each arm (effectively double what the McCabe definition would give), while an equivalent Go `switch` scores the same way, but an equivalent TypeScript chain of `if`/`else if` doesn't get the same treatment — and Python functions get an extra bump for boolean operators that Rust/Go/TS functions performing the same logic don't. Any cross-language "most complex functions in the monorepo" query is comparing apples to oranges.
- *Fix*: Define complexity once in `ql-ast` as a function over a normalized list of "decision point" node-kinds (one set per language, but the *counting logic* — i.e., what counts as +1 and what doesn't — lives in exactly one place). Each adapter supplies only its node-kind mapping table.

### HIGH

**9. No stable symbol identifiers — cross-file reference resolution is structurally impossible today.**
Severity: High | Priority: P1 | Effort: Large (1-2 weeks, touches schema + all adapters + second pass)

- *Evidence*: None of the 6 tables (`schema/tables.json`) have an ID column. `calls.callee`, `structs.implements`, `imports.module` are all free-text `VARCHAR`.
- *Impact*: "Who calls this function" can only be approximated by matching `calls.callee` against `functions.name` by string — which collides badly on common names (`new`, `run`, `get`, `init` exist in nearly every file). This blocks dead-code detection (Phase 6), call-graph queries, and any "find references" capability — exactly the gap that differentiates ql from Sourcegraph's SCIP-based resolution (Phase 11).
- *Fix*: Assign each definition a `symbol_id` (e.g., a hash of `file+name+line`, stable across runs as long as the definition doesn't move). Resolve `calls.callee` to a `symbol_id` where unambiguous, with a free-text fallback column for unresolved/external calls.

**10. `insert_batch` does row-by-row `prepare`+`execute` with no transaction — the dominant cost of loading any non-trivial codebase.**
Severity: High | Priority: P1 | Effort: Small (transaction wrap: ~1 day) to Medium (Appender: 2-3 days)

- *Evidence*: `crates/ql-core/src/storage.rs:65-159` — confirmed via grep, no `BEGIN`/`transaction`/`Appender` anywhere in `ql-core`. Each row is its own prepared statement + execute, each implicitly auto-committed.
- *Impact*: A ~50K-LOC codebase easily produces 50,000-100,000+ combined rows across the 6 tables. DuckDB's per-statement auto-commit overhead, multiplied by that row count, is plausibly the single largest contributor to "time from invocation to first result" at medium scale (Phase 8).
- *Fix*: Minimum: wrap the whole `insert_batch` call in a single `BEGIN`/`COMMIT`. Better: use DuckDB's Appender API for true columnar bulk-load, which is the documented fast path for exactly this use case.

**11. `execute_query` rebuilds the entire database from scratch on every single query — interactive use (VS Code extension) re-walks the whole repo per keystroke.**
Severity: High | Priority: P1 | Effort: Large (persistent server, 1-2 weeks)

- *Evidence*: `crates/ql-core/src/execute.rs:41-76` — `execute_query` calls `open_batch`, which runs `create_schema` (storage.rs:11-63) and `insert_batch` (storage.rs:65-159) fresh, every invocation.
- *Impact*: The VS Code extension (`extension/src/extension.ts`) spawns the `ql` binary per query (`spawnQuery`). Every query therefore pays the *entire* Phase 8 pipeline cost (walk + parse + second pass + schema + insert) before running the user's actual `SELECT`. At 10K LOC this is a multi-second tax per keystroke-triggered query; at larger scales it makes the "VS Code-first" product direction (Phase 10) infeasible without this fix.
- *Fix*: Separate "build the in-memory DB" from "query it" — either a long-lived `ql --serve` process holding a warm DuckDB connection + parsed batch, invalidated per-file on save, or at minimum an on-disk DuckDB file cached and reused across invocations when the source tree fingerprint is unchanged.

**12. Recursive AST walk has no depth guard — deeply nested/generated code can crash the entire invocation.**
Severity: High | Priority: P1 | Effort: Medium (1-2 days, ideally centralized in ql-ast)

- *Evidence*: `walk_source`/`walk_node` (`crates/ql-ast/src/adapter.rs:35-55`) is a plain recursive traversal — one Rust stack frame per AST nesting level.
- *Impact*: Minified/generated code (deeply nested JSX, generated parsers/protobuf bindings, deeply nested ternaries) can exceed the default 8MB thread stack, causing a hard stack-overflow abort. Because this happens inside the single-process `ql` walk, **one pathological file crashes the entire run, losing results for the whole repository**, not just that file.
- *Fix*: Convert to an iterative walk with an explicit stack — the same pattern already correctly used for directory traversal in `walk_relative_files` (`source.rs:22-49`), or drive the traversal directly off `TreeCursor`'s stack-based API.

**13. Tree-sitter grammar crates are version-skewed relative to tree-sitter core — a `cargo update` landmine.**
Severity: High | Priority: P2 | Effort: Small (a few hours now; ongoing via Dependabot)

- *Evidence*: `Cargo.lock` — `tree-sitter` core is `0.26.9`, but `tree-sitter-rust` is `0.24.2`, `tree-sitter-go`/`tree-sitter-python` are `0.25.0`, and `tree-sitter-typescript` is `0.23.2`.
- *Impact*: Grammar crates built against older `tree-sitter-language` ABI versions can behave subtly differently or fail to build entirely after a future `cargo update` bumps core further without corresponding grammar releases — a "works today, breaks on next dependency update" risk with no test currently guarding against it.
- *Fix*: Audit and align grammar versions against the core ABI version now; add Dependabot/Renovate + CI (#18) so version drift is caught incrementally rather than accumulating.

**14. `find_enclosing_function` is duplicated near-identically across all 4 adapters.**
Severity: High | Priority: P2 | Effort: Small (half day)

- *Evidence*: `rust.rs:302-315`, `go.rs:334-349`, `python.rs:260-273`, `typescript.rs:360-373` — same "walk up the parent chain until a function-like node is found" logic, four separate implementations differing only in the node-kind string(s) checked.
- *Impact*: A behavior change (e.g., "also treat `impl`/`class` bodies as a boundary so nested functions attribute correctly") must be applied 4 times; a missed adapter silently diverges, and there is no test that would catch the divergence since each adapter's tests are isolated.
- *Fix*: Extract `find_enclosing_function(node, function_kinds: &[&str]) -> Option<Node>` into `ql-ast`, parameterized by each adapter's node-kind list. This is the textbook use case for the trait-based extensibility (`LanguageAdapter`) the architecture already has but doesn't fully exploit.

**15. `is_public`/`is_exported`/`is_private`/`count_params` are also duplicated 4x, and `visibility`'s domain isn't even consistent in *type* across languages.**
Severity: High | Priority: P2 | Effort: Small-Medium (1 day)

- *Evidence*: `rust.rs:10-24`, `go.rs` (`is_exported`), `python.rs` (`is_private`), `typescript.rs:10-32` (`is_public`, **3-valued**: public/private/protected, unlike the other three adapters' boolean visibility).
- *Impact*: Same drift risk as #14, compounded: `functions.visibility` (`schema/tables.json`) is a single shared column whose *value domain* differs by language — `WHERE visibility = 'public'` (used in README example #4) means something subtly different for a Rust `pub fn` vs. a TypeScript class method with no modifier (implicitly public, 3rd value) vs. a Python function with no underscore prefix.
- *Fix*: Share what's truly identical (`count_params` — counting children of a `parameters`/`parameter_list` node minus `self`/`this`/`cls` is the same logic everywhere). For `visibility`, either normalize TypeScript's 3-valued result down to the shared 2-valued domain (documenting the lossy mapping) or widen the schema column intentionally and document the per-language domain explicitly in `schema/tables.json`.

**16. The file-content cache is a single monolithic JSON file written non-atomically — corruptible on interrupt, and an all-or-nothing (de)serialization at scale.**
Severity: High | Priority: P2 | Effort: Small (atomic write: hours) to Medium (sharded cache: 2-3 days)

- *Evidence*: `crates/ql-cli/src/source.rs:161-189` — `CachedFile`/`read_cache`/`write_cache` operate on one JSON file containing every cached file's content + mtime.
- *Impact*: A `Ctrl-C`, OOM, or crash mid-write leaves a truncated/corrupt JSON cache. Depending on how `read_cache`'s deserialization failure is handled, the next run either falls back to a full no-cache walk (slow but correct) or — if errors are too broadly swallowed — could load a partially-valid-but-wrong cache. Separately, at large-repo scale, every cache read/write is a multi-MB JSON (de)serialization in one shot (Phase 8 concern).
- *Fix*: Write to a temp file + atomic rename (cheap, fixes the corruption risk immediately). For the scale concern, consider a sharded/per-file cache (e.g., a small embedded KV store) as a follow-up.

**17. The file watcher polls every 500ms and fully re-walks the tree each time, instead of using OS-native file events.**
Severity: High | Priority: P2 | Effort: Medium (1-2 days, cross-platform testing needed)

- *Evidence*: `crates/ql-cli/src/watch.rs:23-36` — `std::thread::sleep` + `scan_snapshot` loop, full tree re-walk every iteration.
- *Impact*: Up to 500ms latency between a save and updated results; continuous I/O load proportional to repo size every 500ms regardless of whether anything changed (worse on network filesystems or CI runners); unnecessary CPU/battery drain for any long-running watch session (e.g., IDE-integrated).
- *Fix*: Use the `notify` crate (cross-platform inotify/FSEvents/ReadDirectoryChangesW) for event-driven invalidation, falling back to polling only where OS events are unavailable. Note this also touches the untested macOS path (`MILESTONE.md` Phase 5).

---

**18. No CI pipeline — nothing enforces tests, lints, or formatting on changes.**
Severity: High | Priority: P1 | Effort: Small (half day)

- *Evidence*: No `.github/workflows/` directory exists anywhere in the repo.
- *Impact*: `cargo test`/`cargo clippy`/`cargo fmt --check` aren't enforced; issues like #19 (unused dependency) can land and persist indefinitely. For prospective contributors, "no CI badge" is often the first signal used to judge whether a project is maintained — directly affects Phase 9/13 OSS-readiness.
- *Fix*: A single GitHub Actions workflow: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt -- --check`, on both `ubuntu-latest` and `macos-latest` (the latter also starts closing the untested-macOS gap in `MILESTONE.md`).

**19. `ql-core` depends on `ql-adapters` but never uses it.**
Severity: Medium | Priority: P3 | Effort: Trivial

- *Evidence*: `crates/ql-core/Cargo.toml` lists `ql-adapters = { path = ... }`; `grep -r ql_adapters crates/ql-core/src` returns zero matches.
- *Impact*: Minor build-time cost, and a misleading coupling signal — Phase 2 establishes that `ql-core` should have *no* knowledge of language adapters (clean layering: `cli → core → adapters → ast`), so this dependency either is a leftover from a refactor or hints at planned-but-unbuilt functionality with no documentation either way.
- *Fix*: Remove the dependency, or if there's a planned use, add a comment explaining it.

**20. `JoinKind` supports only `Inner` — no `LEFT JOIN`, so "find X with no matching Y" queries are impossible via joins.**
Severity: Medium | Priority: P2 | Effort: Medium standalone (2-3 days) / free under Option B

- *Evidence*: `crates/ql-core/src/sql/ast.rs:29-32` — `pub enum JoinKind { Inner }`.
- *Impact*: A natural query like "public functions with no attached doc comment" via `functions LEFT JOIN comments ON ... WHERE comments.text IS NULL` cannot be expressed. README example #6 works around this by relying on the second-pass-computed `attached_to` column instead — a workaround that only exists because the join-based formulation is unavailable.
- *Fix*: Add `Left`/`Right`/`Full` to `JoinKind` and implement NULL-padding in `plan.rs`/`execute.rs` — or, again, this is one of the things Option B (Phase 4/Finding #1) gets for free from DuckDB's native join support.

**21. The SQL lexer has no negative-number literal — `Literal::Integer(u64)` is unsigned-only.**
Severity: Low | Priority: P3 | Effort: Small standalone / free under Option B

- *Evidence*: `crates/ql-core/src/sql/ast.rs:85-88` — `Literal::Integer(u64)`; `sql/lexer.rs` has no `-` handling for numeric literals (only `UnaryOperator::Not` exists, `ast.rs:67-69`).
- *Impact*: Low practical impact for code-metric queries (most are non-negative), but is a symptom of the hand-rolled grammar's general incompleteness (#1) — any future column with signed semantics (e.g., a line-delta) would hit this immediately.
- *Fix*: Add unary-minus handling for numeric literals in the lexer/parser, or inherit this for free under Option B.

**22. No project-level configuration file — every run is parameterized only by CLI flags.**
Severity: Medium | Priority: P2 | Effort: Medium (2-3 days for basic ignore/config)

- *Evidence*: `crates/ql-cli/src/main.rs:24-43` — the only recognized flags are `--format`, `--langs`, `--watch`. No `.ql.toml`/config-file loading exists anywhere.
- *Impact*: There's no way to commit project-specific settings (ignore patterns beyond #5's fix, per-language complexity thresholds, saved/named queries) alongside the code — a prerequisite for the "architecture rules as SQL, checked in CI" product direction (Phase 10), which needs declarative, version-controlled configuration.
- *Fix*: Introduce `.ql.toml` (or similar) for ignore patterns and saved queries, loaded from the project root if present.

**23. Hand-rolled CLI argument parsing — no `--help`, no `--version`, no subcommands.**
Severity: Medium | Priority: P2 | Effort: Small (half day - 1 day)

- *Evidence*: `crates/ql-cli/src/main.rs:24-43` — a manual loop over `std::env::args()` recognizing exactly `--format`/`--langs`/`--watch`.
- *Impact*: No `--help` text, no `--version` (relevant given #26), inconsistent error messages for invalid flags, and every future flag (e.g., #22's config path, or a future `ql check` subcommand from Phase 10) means more hand-rolled branches in an already-ad-hoc parser.
- *Fix*: Adopt `clap` (derive API) — idiomatic for Rust CLIs, gives `--help`/`--version`/subcommands essentially for free, and is a near-zero-cost addition given DuckDB's bundled build already dominates compile time.

**24. Schema drift (finding #6) surfaces to end users as confusing, unrelated SQL errors.**
Severity: Medium | Priority: P2 | Effort: Same fix as #6

- *Evidence*: `TableBatch::extend` (`crates/ql-ast/src/rows.rs:81-88`) is a plain `Vec::extend` with no validation against `schema/tables.json` or the `storage.rs` DDL.
- *Impact*: If an adapter's row struct and the DDL ever drift (#6), the failure mode is a DuckDB column-count/type error raised during `insert_batch` — which, from the end user's perspective, looks like *their query* is broken, when the actual cause is an internal schema mismatch unrelated to anything they wrote.
- *Fix*: Same root fix as #6 (single source of truth + compile-time or build-time consistency check) — listed separately because the *symptom* (misleading error messages) is independently worth tracking even before the full fix lands.

### MEDIUM

**25. `MAX_CELL_WIDTH = 60` hardcoded — long values (file paths, generic types) are truncated with no override.**
Severity: Low | Priority: P3 | Effort: Small (a few hours)

- *Evidence*: `crates/ql-cli/src/format.rs:6`.
- *Impact*: README example #8 (`return_type LIKE '%Result%'`) on a type like `Result<HashMap<String, Vec<MyError>>, OtherError>` truncates to 60 characters in table output — exactly the information a user running that query wants to see.
- *Fix*: Add a `--width`/`--no-truncate` flag; ensure truncation is always visibly marked (e.g., trailing `…`).

**26. No `ql --version`.**
Severity: Low | Priority: P3 | Effort: Trivial (free with #23)

- *Evidence*: `crates/ql-cli/src/main.rs:24-43` recognizes only `--format`/`--langs`/`--watch`.
- *Impact*: Bug reports can't easily state which version is running; combined with no CI/releases (#18), versioning is entirely informal.
- *Fix*: Comes free once `clap` (#23) is adopted, sourced from `Cargo.toml`'s package version.

**27. No license/dependency audit (`cargo-deny`) for the bundled dependency tree.**
Severity: Low | Priority: P3 | Effort: Small

- *Evidence*: `crates/ql-core/Cargo.toml:7` — `duckdb = { version = "1.10503.1", features = ["bundled"] }` vendors DuckDB's C++ build, which itself has third-party dependencies that haven't been license-audited.
- *Impact*: Low immediate risk (DuckDB and tree-sitter grammars are permissively licensed to the best available knowledge) but currently unverified — relevant once #2 (LICENSE) is fixed and the project is held to OSS-compliance scrutiny.
- *Fix*: Add `cargo-deny` config (`licenses` check) as a CI step (#18).

**28. File walker has no explicit symlink-loop guard.**
Severity: Medium | Priority: P2 | Effort: Small

- *Evidence*: `walk_relative_files` (`crates/ql-cli/src/source.rs:22-49`) does not appear to track visited canonical paths or skip symlinks.
- *Impact*: A self-referential symlink (`ln -s . loop`) inside a scanned tree could cause unbounded traversal — though the #4 directory cap may currently mask this by silently truncating (itself a separate bug). Most relevant if `ql` is ever run on untrusted input (e.g., scanning a forked PR's checkout in CI).
- *Fix*: Track visited canonical paths, or skip symlinked directories entirely (the common default for code-search tools).

**29. `create_schema`'s full DDL is re-issued on every query (compounds #11).**
Severity: Low | Priority: P3 | Effort: Subsumed by #11

- *Evidence*: `crates/ql-core/src/storage.rs:11-63` — `create_schema` runs all 6 `CREATE TABLE` statements inside `open_batch`, called from `execute_query` (`execute.rs:41-76`) every invocation.
- *Impact*: Minor compared to #10/#11 individually, but compounds them — 6 DDL statements of pure overhead, repeated unnecessarily on every query.
- *Fix*: Same fix as #11 (persistent connection/server eliminates this entirely).

**30. `to_json_value` converts `HugeInt`/`Decimal` to JSON strings, not numbers — a landmine for future aggregate columns.**
Severity: Low | Priority: P3 | Effort: Small

- *Evidence*: `crates/ql-core/src/execute.rs:78-126`.
- *Impact*: Currently latent — no column in `schema/tables.json` is `HugeInt`/`Decimal` today. But once #1 (aggregation) is implemented, `COUNT(*)` over large tables can return `HugeInt`, which would then serialize as a JSON *string* — breaking numeric sort/comparison for any consumer (the VS Code extension's webview, or `--format json | jq`) that expects a number.
- *Fix*: Map `HugeInt`/`Decimal` to JSON numbers when within `f64`/`i64`-safe range, falling back to string only for genuine overflow. Track alongside #1.

**31. No integration tests for the `ql` binary's CLI entrypoint itself.**
Severity: Medium | Priority: P2 | Effort: Medium (1-2 days)

- *Evidence*: Tests in `source.rs`, `format.rs`, `watch.rs`, and the adapters all test library functions directly; none build and invoke the `ql` binary.
- *Impact*: Regressions in flag parsing (`--format json` vs `--format csv` selection), exit codes, or the interaction between `--watch` and the positional path argument would not be caught by `cargo test`.
- *Fix*: `assert_cmd`-based integration tests invoking the compiled binary against fixture directories.

**32. No polyglot fixture/integration test — cross-adapter consistency regressions have no test that would catch them.**
Severity: Medium | Priority: P2 | Effort: Medium (1-2 days)

- *Evidence*: Every adapter's tests (`rust.rs`, `go.rs`, `python.rs`, `typescript.rs`) use isolated inline string fixtures; there's no shared `tests/fixtures/` polyglot sample project.
- *Impact*: A shared-helper extraction (e.g., fixing #14/#15) that introduces a cross-adapter inconsistency has no single test exercising all adapters together against a known-good baseline.
- *Fix*: A `tests/fixtures/polyglot/` directory with a small sample project per language + an integration test that runs the full pipeline and snapshot-tests the resulting tables.

**33. The hand-rolled SQL lexer/parser — the most exposed input surface — has never been fuzzed.**
Severity: Medium | Priority: P2 | Effort: Small setup (a few hours) + ongoing

- *Evidence*: `sql/lexer.rs`/`sql/parser.rs` parse arbitrary user-supplied query strings; no `fuzz/` directory or `cargo-fuzz` target exists.
- *Impact*: Manual review found no obvious panics (string-literal handling at `lexer.rs:113-133` and the bounds check at `:129` look correct), but a hand-rolled parser over adversarial input is a classic source of `unwrap`-on-`None`/index-out-of-bounds panics — which would crash the CLI with a Rust backtrace instead of a clean "invalid query" error.
- *Fix*: `cargo-fuzz` target wrapping `parse_query`; run in CI or periodically.

**34. The VS Code extension assumes the `ql` binary is reachable, with no validated "binary not found" UX.**
Severity: Medium | Priority: P2 | Effort: Medium (per-platform binary packaging is real work)

- *Evidence*: `extension/src/extension.ts`'s `spawnQuery` (`:111-140`) shells out to `ql` via `cp.spawn`.
- *Impact*: A user installing the VS Code extension without separately building/installing the CLI gets an unclear failure — plausibly the most common first support issue for the extension.
- *Fix*: Either bundle per-platform binaries with the extension package, or detect-and-prompt-to-install on first activation with a clear error message pointing at the install instructions in `README.md`.

---

**35. TypeScript adapter uses the TSX grammar for plain `.ts` files — a known ambiguity with generic-arrow-function syntax.**
Severity: Medium | Priority: P2 | Effort: Small (a few hours + targeted tests)

- *Evidence*: `crates/ql-adapters/src/typescript.rs:337-342` — `extensions()` maps both `.ts` and `.tsx` to `LANGUAGE_TSX`.
- *Impact*: TSX grammar treats `<...>` preferentially as JSX, which is ambiguous with TypeScript's generic-arrow-function syntax (`<T,>(x: T) => x`) — valid and common in `.ts` files, problematic under TSX parsing rules. Plain `.ts` files using this pattern may be misparsed.
- *Fix*: Use `LANGUAGE_TYPESCRIPT` for `.ts`/`.mts`/`.cts` and reserve `LANGUAGE_TSX` for `.tsx` only.

**36. `comments.is_doc` likely has inconsistent semantics across languages (doc-comment conventions differ: `///`/`/** */` vs. `"""..."""` vs. JSDoc).**
Severity: Low | Priority: P3 | Effort: Small (docs) to Medium (new column)

- *Evidence*: Pattern inferred from the established cross-adapter-inconsistency theme (#8, #15) applied to `comments` (`schema/tables.json`); `is_doc` is consistent in *type* (boolean) but its matching convention is necessarily per-language.
- *Impact*: README example #6 ("Show doc comments attached to code") returns results whose *style* (line-doc vs. block vs. docstring) varies by language with no way to distinguish which convention matched.
- *Fix*: Document the per-language doc-comment convention recognized by each adapter; consider an optional `doc_style` column (`line`/`block`/`docstring`) for queries that need to discriminate.

**37. `variables.scope` is a free-text string whose value set isn't shared/enforced across adapters.**
Severity: Low | Priority: P3 | Effort: Small

- *Evidence*: Same root cause as #15 — `schema/tables.json`'s `variables.scope` column has no enum/constant definition shared across `rust.rs`/`go.rs`/`python.rs`/`typescript.rs`.
- *Impact*: `WHERE scope = 'global'` vs. `WHERE scope = 'module'` may both be "correct" depending on which adapter produced the row, with no documentation of the actual value domain.
- *Fix*: Define a shared set of scope-name constants in `ql-ast`, used by all adapters.

**38. No `rustfmt.toml`/`.editorconfig`, and no CI fmt-check (compounds #18).**
Severity: Low | Priority: P3 | Effort: Trivial

- *Evidence*: No `rustfmt.toml` at the workspace root.
- *Impact*: Formatting consistency depends entirely on contributor tooling defaults; without CI enforcement (#18), drift is possible.
- *Fix*: Add `rustfmt.toml` (even empty, to pin edition defaults) + `cargo fmt -- --check` in CI.

**39. README's "v1 targets Linux and macOS" is unverified — `MILESTONE.md` Phase 5 (macOS testing) is marked incomplete.**
Severity: Low | Priority: P3 | Effort: Small (once CI exists)

- *Evidence*: `MILESTONE.md` Phase 5 explicitly lists cross-platform macOS testing as not done; `README.md:143` states "v1 targets Linux and macOS."
- *Impact*: Any macOS-specific path-handling difference (case-insensitive filesystem, `std::fs` symlink behavior differences relevant to #28) is currently unvalidated against a stated platform-support claim.
- *Fix*: Add a `macos-latest` CI runner as part of #18.

**40. `.gitignore` excludes a lowercase `milestone.md`/`project.md`/`dev.sh` that differ from the tracked uppercase `MILESTONE.md`.**
Severity: Informational | Priority: P4 | Effort: N/A

- *Evidence*: `.gitignore:6-8` ignores lowercase filenames; the tracked file is `MILESTONE.md` (uppercase).
- *Impact*: Suggests the author maintains parallel personal planning notes not visible to contributors — some "current status" context may live outside the repo. Not a defect, but worth the author considering whether `MILESTONE.md` should absorb whatever the gitignored files track.
- *Fix*: None required; informational.

**41. `simplify_type_name` (Rust adapter) likely discards generic parameters from `return_type`, losing information needed for advanced queries.**
Severity: Low | Priority: P4 | Effort: Small

- *Evidence*: `crates/ql-adapters/src/rust.rs` — `simplify_type_name` helper used when populating `functions.return_type`.
- *Impact*: If `Result<T, E>` is simplified to `Result`, README example #8 (`return_type LIKE '%Result%'`) still works, but a user wanting to filter on the *error type* specifically (`WHERE return_type LIKE '%MyError%'`) loses that information.
- *Fix*: Store the full type signature in `return_type`; if a simplified form is independently useful, add it as a separate column rather than replacing the original.

**42. `implements`/multi-value columns are CSV-encoded inside a single VARCHAR — substring queries are unreliable.**
Severity: Medium | Priority: P3 | Effort: Large (rolled into #9's broader normalization)

- *Evidence*: `merge_csv` (`crates/ql-adapters/src/rust.rs`) encodes multiple trait implementations into one comma-joined string for `structs.implements` (`schema/tables.json`).
- *Impact*: `WHERE implements = 'Display'` fails to match a struct stored as `"Display,Debug,Clone"`; users must use `LIKE '%Display%'`, which has false-positive risk (`Display` also matches a hypothetical `DisplayName` trait). README example #4 surfaces `implements` as a raw column for exactly this reason — and inherits this fragility.
- *Fix*: Normalize into a join table (e.g., `struct_implements(struct_id, trait_name)`) — naturally falls out of the broader symbol-resolution work in #9.

**43. CSV-list handling is implemented twice — `normalize_csv_list` (analysis.rs) and `merge_csv` (rust.rs) — for the same conceptual problem.**
Severity: Low | Priority: P4 | Effort: Rolled into #42/#9

- *Evidence*: `crates/ql-ast/src/analysis.rs:94-109` (`normalize_csv_list`, used by `resolve_implements`, `:36-40`) and `crates/ql-adapters/src/rust.rs` (`merge_csv`) both implement "multi-value-in-one-column" joining/deduplication independently.
- *Impact*: Another instance of the duplication theme (#6/#14/#15) — two slightly-different implementations of the same semantic operation, with no shared definition of "what counts as a duplicate" or "what separator/escaping is used."
- *Fix*: Resolved entirely by #42's normalization — once multi-value fields become join tables, neither CSV-handling implementation is needed.

**44. The codebase mixes recursive (AST walk, #12) and iterative (directory walk) traversal styles with no documented convention.**
Severity: Informational | Priority: P4 | Effort: Trivial

- *Evidence*: `walk_relative_files` (`source.rs:22-49`) uses an explicit stack; `walk_node`/`walk_source` (`adapter.rs:35-55`) is recursive.
- *Impact*: The iterative choice for directory walking appears to be incidental rather than a deliberate project-wide convention — meaning future adapters could easily reintroduce #12's stack-overflow risk without realizing it's a known anti-pattern here.
- *Fix*: Document "AST and directory walks must be iterative with explicit stacks" as a project convention once #12 is fixed, so the fix doesn't silently regress.

**45. `format_csv` may not properly escape values containing commas — a likely correctness bug in a documented example query.**
Severity: Medium | Priority: P2 | Effort: Small

- *Evidence*: `crates/ql-cli/src/format.rs` — `format_csv`; `functions.return_type` (e.g., `HashMap<String, Vec<u8>>`) and `comments.text` can both contain literal commas.
- *Impact*: If `format_csv` joins fields with `,` without quoting/escaping, README example #10 (`--format csv ... FROM functions`) produces malformed CSV — extra columns — for any function whose `return_type` contains a generic type with multiple parameters. This is a *correctness* bug in a documented, user-facing example, not merely style.
- *Fix*: Use the `csv` crate for proper RFC 4180 quoting/escaping instead of hand-joining fields with commas.

**46. No `--explain` / query-plan visibility — slow queries are a black box.**
Severity: Low | Priority: P3 | Effort: Small (the SQL-rendering part is nearly free)

- *Evidence*: `render_select` (`crates/ql-core/src/plan.rs:30-57`) already stringifies the AST back to SQL for execution but isn't exposed to the user.
- *Impact*: Once joins/aggregation (#1/#20) make queries non-trivial, users debugging a slow query have no visibility into what was actually sent to DuckDB or how it was executed.
- *Fix*: A `--explain` flag that prints `render_select`'s output and/or pipes through DuckDB's own `EXPLAIN`.

**47. `is_valid_identifier`'s injection-prevention coverage is unverified against DuckDB's actual identifier grammar.**
Severity: Low | Priority: P3 | Effort: Small (review + adversarial tests)

- *Evidence*: `crates/ql-core/src/plan.rs:184-192` — `is_valid_identifier`, used because `render_select` interpolates table/column names directly into a SQL string rather than using parameterized identifiers (which SQL doesn't support for identifiers regardless).
- *Impact*: Low severity *today* (single-user CLI against the user's own code), but if this validation has any gap relative to DuckDB's quoting/escaping rules, it becomes a real injection vector the moment ql is used in a multi-tenant/server context (Phase 10's "platform" direction).
- *Fix*: Review against DuckDB's identifier grammar; add tests with adversarial identifiers (e.g. `"; DROP TABLE functions; --`).

**48. The `HugeInt`/`Decimal`→JSON-string path (#30) is currently dead code with no test coverage.**
Severity: Informational | Priority: P4 | Effort: Tracked with #1/#30

- *Evidence*: No column in `schema/tables.json` is `HugeInt`/`Decimal`, so `to_json_value`'s (`execute.rs:78-126`) handling of those variants is unreachable from any existing query.
- *Impact*: A second instance (with #19) of "code exists for a capability that isn't reachable yet" — becomes live the moment #1 (aggregation) ships, since `COUNT(*)` over large tables can return `HugeInt`.
- *Fix*: No action now; revisit as part of #1/#30.

**49. No `[workspace.package]` shared metadata — `edition`/`version`/etc. likely repeated per-crate `Cargo.toml`.**
Severity: Informational | Priority: P4 | Effort: Trivial

- *Evidence*: Workspace `Cargo.toml` structure (4 crates, each with independent `Cargo.toml`).
- *Impact*: Minor — another small "keep N copies in sync" surface, consistent with the duplication theme (#6/#14/#15/#42/#43), but at the build-metadata level rather than logic level.
- *Fix*: Hoist shared fields to `[workspace.package]`, reference via `.workspace = true`.

**50. This is a single-contributor, ~6-month-old, pre-v1 project — the Top 50 above is a list of what *will* matter, not what already has.**
Severity: N/A | Priority: Context for Phase 13 | Effort: N/A

- *Evidence*: `git log` shows 30 commits, single author ("itsfuad"), first commit 2025-12-22, most recent 2026-06-13.
- *Impact*: None of the findings above represent incidents that have occurred — they represent risk that activates the moment a second contributor, first external user, or first non-trivial-scale repo shows up. This reframes prioritization: "don't ship a misleading first impression" (#1, #2, #5, #45) and "OSS readiness" (#2, #18) outrank deep architectural rewrites (#9, #1's Option B) in immediate sequencing, even though the architectural items matter more long-term.
- *Fix*: See Phase 13 for sequencing.

---

## PHASE 13 — Future Roadmap

### Next 30 Days — stop the bleeding, fix first impressions

Cheap, high-leverage, no architectural risk. All independently shippable.

1. Add a root `LICENSE` file (MIT, matching `extension/package.json`) + `license` field in each crate's `Cargo.toml` (#2).
2. Integrate the `ignore` crate for `.gitignore`/`.qlignore` support in `walk_relative_files` (#5).
3. Remove the 1,000-directory cap or replace with a visible-warning configurable limit + regression test (#4).
4. Stand up a single GitHub Actions workflow: build, test, clippy, fmt, on Linux + macOS (#18, also starts addressing the untested-macOS gap in `MILESTONE.md`).
5. Fix `format_csv` to use the `csv` crate for proper escaping (#45) — currently a correctness bug in a documented example query.
6. Fix the `is_external` heuristic in `rust.rs`/`go.rs` to check against `imports`-resolved names instead of `.contains('.')` (#7) — restores README example #2.
7. Switch the cache write path (`source.rs:161-189`) to temp-file + atomic rename (#16).
8. Adopt `clap` for CLI parsing, gaining `--help`/`--version` immediately (#23, #26).

### Next 90 Days — fix the schema and the load path

Medium-effort, sets up everything after.

1. Make `schema/tables.json` the single source of truth; generate `Row` structs + DDL from it, or generate `tables.json` from the structs (#6, #24).
2. Wrap `insert_batch` in a transaction at minimum, or migrate to the Appender API (#10).
3. Extract shared adapter helpers — `find_enclosing_function`, `count_params`, visibility predicates — into `ql-ast`, parameterized per language (#14, #15).
4. Unify cyclomatic complexity into one shared formula with per-language node-kind tables (#8) — re-baseline existing tests against the corrected values.
5. Replace the 500ms polling watcher with `notify`-based event-driven invalidation (#17), tested on both Linux and macOS.
6. Split the TypeScript adapter's `.ts`/`.tsx` grammar selection (#35).
7. Add `assert_cmd` CLI integration tests (#31) and a `tests/fixtures/polyglot/` cross-adapter integration test (#32).
8. Stand up a `cargo-fuzz` target for `sql::parser::parse_query` (#33).
9. **Make the build-vs-buy decision on Phase 4's Option A vs Option B** (hand-rolled SQL extension vs. DuckDB-parser delegation) and land a prototype/spike. This decision gates the entire 6-month plan below — do not defer it past this window.

### Next 6 Months — deliver the core value proposition

This is where ql either becomes the tool the README describes, or doesn't.

1. **Complete the Option B rewrite** (or, if Option A was chosen, build out `GROUP BY`/`HAVING`/aggregate functions/window functions/all join types in the hand-rolled grammar — expect this to take materially longer and still land with narrower SQL coverage) (#1, #20, #21).
2. Introduce stable `symbol_id`s and resolve `calls.callee`/`structs.implements` against them where unambiguous (#9, #42, #43).
3. Normalize CSV-encoded multi-value columns into join tables once #9's symbol layer exists (#42).
4. Ship `ql check <query.sql>` with exit codes — the CI-gate feature identified in Phase 10 as the most immediately viable commercial wedge.
5. Introduce `.ql.toml` project configuration (ignore patterns, saved/named queries) (#22).
6. Add one new language adapter (Java or PHP, per Phase 7's effort estimates) using the now-shared helper layer from the 90-day work — this is the test of whether #11/#14/#15's refactor actually reduced per-language cost as intended.
7. Build the persistent `ql --serve` mode and update the VS Code extension to use it instead of per-query spawn (#11) — required before the extension is usable on anything beyond toy repos.

### Next 12 Months — breadth and ecosystem

1. Expand to 5-8 languages total, ideally bootstrapping new adapters from `tags.scm`-style queries (Phase 3/7) rather than hand-written Tree-sitter traversal code per language.
2. Build dead-code and call-graph analysis queries/templates on top of #9's symbol resolution (Phase 6).
3. Start a community rule/query registry — shareable `.sql` files for common architecture/security checks (the Semgrep-registry equivalent, but in SQL).
4. Add historical/trend tables (requires persistent, not purely in-memory, DuckDB storage) — "has average complexity in `src/payments/` increased over the last 90 days" becomes answerable.
5. Revisit `cargo-deny`/dependency licensing audit (#27) now that the dependency surface has grown with new language grammars.
6. Re-run this audit's Phase 8 performance benchmarks against real 100K/1M-LOC repositories now that #1/#10/#11/#3 are fixed — validate the projected gains against measured ones.

---

## PHASE 14 — Investor / Acquisition View

### What's technically impressive

- **Code hygiene is genuinely strong for a solo, 6-month, pre-v1 project**: zero `.unwrap()`/`.expect()`/`panic!` outside test modules across all 4 crates (confirmed via search) — most early-stage Rust projects don't reach this discipline even with a team.
- **The core architectural idea is clean and correctly layered**: `ql-cli → ql-core → ql-adapters → ql-ast` is a real dependency direction (modulo the one unused edge, #19), the `LanguageAdapter` trait (`ql-ast/src/adapter.rs`) is a sound extensibility point, and `TableBatch` (`rows.rs:81-88`) is a sensible unifying data structure.
- **Test coverage of what exists is real, not decorative**: 12 parser tests covering precedence/`IN`/`LIKE`/joins/error cases, 4 adapters each with multiple end-to-end `walk_source` tests, `execute.rs`/`format.rs`/`source.rs` all covered.
- **The "no AI, no embeddings, deterministic" positioning (`README.md:5`) is a real, defensible, currently-underserved niche** — most new code-intelligence tooling in 2025-2026 is chasing LLM integration; a deterministic SQL layer is a contrarian bet with a real audience (compliance, CI gates, anyone who needs *reproducible* answers).

### What's technically weak

- **The single capability that justifies "SQL" over every competing approach — aggregation — doesn't exist in the shipped grammar** (#1). This isn't a missing feature at the edges; it's the center of the value proposition.
- **The schema is one level of abstraction too shallow** (Phase 5/#9): no symbol IDs, no resolved references, free-text joins-by-name only. This is the difference between "a nicer grep" and "a code intelligence platform," and today ql is closer to the former despite aiming for the latter.
- **Duplication is systemic, not isolated** (#6, #14, #15, #42, #43): the same logic exists redundantly across schema definition, complexity counting, visibility checks, enclosing-function lookup, and CSV-list handling. This is the kind of debt that's cheap to fix at 4 languages and increasingly expensive at 8+ (Phase 7).
- **Performance has not been measured at the scales the product narrative targets** (Phase 8): the quadratic comment-resolution pass (#3), per-query full-rebuild (#11), and row-by-row inserts (#10) are each individually capable of making "query a 1M-LOC repo" impractical, and they compound.
- **Zero adoption infrastructure**: no license (#2), no CI (#18), no CONTRIBUTING, single contributor. An acquirer or investor cannot today point to "users," "contributors," or "a license that permits diligence."

### Moat — does one exist?

**Not yet, and not from the current codebase alone.** The core pipeline (Tree-sitter → relational rows → DuckDB → SQL) is conceptually simple enough that a competent team could prototype an equivalent MVP in 2-4 weeks — tree-sitter and DuckDB do the genuinely hard parts (parsing, query execution), and ql's current contribution on top of them is, candidly, thinner than it will need to be.

**What *could* become a moat, if built well:**

- A deep, broad, *correct* per-language adapter layer (Phase 7) — this is the multi-year grind that is ast-grep's and Semgrep's actual moat, not their initial architecture. Years of edge-case handling across 20+ languages is hard to copy quickly precisely because it's unglamorous.
- Symbol-resolution depth (#9) done well — once a tool has reliable cross-file references, switching costs appear (saved queries, CI gates, dashboards built on top of stable IDs).
- A community query/rule registry (Phase 13, item 27) — network effects, if it materializes, are durable; but this requires the product to be usable enough to attract contributors first.

### Easy vs. hard to copy

- **Easy to copy**: the v0 pipeline, the SQL-over-Tree-sitter-facts *idea*, the 6-table schema as currently defined.
- **Hard to copy**: a *correct*, *consistent*, *broad* implementation of that idea — which is precisely where this audit's findings show ql is currently weakest (#6, #8, #14, #15 — inconsistency across the 4 languages it *does* support). Ironically, fixing these findings is also what would make ql hard to copy.

### Must-fix before adoption (the short list)

`#2` (LICENSE), `#5` (`.gitignore`), `#1` (aggregation), `#6`/`#8` (schema/complexity consistency), `#18` (CI) — essentially the Phase 13 "Next 30/90 Days" list. None of these are individually hard; collectively they're the difference between "interesting solo project" and "thing a team could responsibly build on."

### Scores

| Dimension | Score | Rationale |
| --- | --- | --- |
| Technical | 6/10 | Clean architecture and unusually disciplined code (no panics, real tests) — but the core SQL engine is missing its headline capability, and systemic duplication will compound with every new language. |
| Product | 4/10 | The CI-policy-as-SQL wedge (Phase 10) is a genuinely good idea with real budget behind it in the market — but it's currently a CLI demo, not a product; no `check`/exit-code mode, no persistent server, no config file. |
| Open Source | 3/10 | No license, no CI, no CONTRIBUTING, single contributor — but the codebase is small (~4,400 lines) and readable enough that fixing these would rapidly improve this score; the *potential* is higher than the current state. |
| Commercial Potential | 5/10 | Real wedge (CI/CD architecture-policy enforcement) in a market with established budget (Semgrep/SonarQube/CodeQL all monetize here) — but credibility against those incumbents requires #1 (aggregation) and #9 (symbol resolution) to ship first; today it would lose a bake-off against any of them on capability. |

### Overall Verdict: **Promising OSS Project**

Not a **Hobby Project** — the code quality, test discipline, and architectural clarity are well above that bar, and the underlying idea (deterministic SQL over code facts, explicitly positioned against AI-driven tools) is a real, differentiated, currently-underserved angle.

Not yet a **Serious Developer Tool** — the headline capability (aggregation) is unimplemented, the schema is one layer too shallow for the use cases the positioning implies, and there is no evidence of use beyond the author's own development.

Far from a **Venture-Scale Opportunity** — no moat exists yet, the competitive field (CodeQL, Semgrep, ast-grep, Sourcegraph) is crowded with well-funded incumbents, and the path to differentiation (deep multi-language correctness + symbol resolution) is a multi-year investment that hasn't started.

**The honest framing**: this is a well-built foundation for a tool that doesn't fully exist yet. The gap between "what ql is" and "what `README.md` describes" is almost entirely closeable with the roadmap above — none of the required work is research-grade or speculative, it's execution. That combination (strong foundation + clear, executable gap-closing path + genuinely differentiated positioning) is exactly what "Promising" means: worth continued investment of the author's time, worth a contributor's first PR, not yet worth a term sheet.

---

*End of audit. See Phase 12 for the full ranked findings list (50 items) and Phase 13 for sequencing.*
