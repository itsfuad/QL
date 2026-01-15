# ql — Milestones

## Phase 1 — Foundation
- [x] Schema + row types (`*Row` structs, `TableBatch`) with serde derives
- [x] AST bridge (`LanguageAdapter`, AST walk, batch accumulation)
- [x] DuckDB ingest + raw SELECT

## Phase 2 — Query engine
- [x] Hand-written SQL parser
- [x] Query planner + executor wired to DuckDB
- [x] File cache (mtime-based, `~/.cache/ql/`)

## Phase 3 — Adapters
- [x] Go adapter complete (all tables, complexity, second-pass)
- [x] Second-pass analysis (`has_test`, `implements`, `attached_to`)
- [x] Rust adapter
- [x] TypeScript adapter
- [x] Python adapter

## Phase 4 — Interface
- [x] CLI binary
- [x] Watch mode
- [x] VS Code extension

## Phase 5 — Polish
- [x] Output formats: table, JSON, CSV
- [x] Error messages reviewed and consistent
- [x] README with install instructions and 10 example queries
- [ ] Cross-platform test on macOS
