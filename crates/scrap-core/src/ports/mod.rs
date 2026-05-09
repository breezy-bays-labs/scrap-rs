//! Ports — trait definitions that bridge `domain/` to `adapters/`.
//!
//! Ports may depend only on `domain/` types plus the dedicated error
//! crates (`thiserror`, `globset`). No AST library, no I/O type,
//! nothing inward. Adapters implement these traits; `core/` consumes
//! them.
//!
//! Reporter rendering is intentionally **not** a port. Reporters
//! (`format_json`, `format_table_with_explain`, `format_scorecard_row`,
//! ...) are free functions in `adapters/reporters/` (per crap4rs
//! precedent — reporters compose at the call site, not behind a
//! `dyn Trait` indirection).
//!
//! Module roster:
//! - `source` — `SourcePort` + `SourceError` (test-file discovery)
//! - `parser` — `TestParserPort` + `ParseError` (source → `ParsedTestFile`)

// Order matches data-flow (discovery → parse), mirroring the roster above.
pub mod parser;
pub mod source;
