//! Adapters — Rust-specific implementations of port traits.
//!
//! `syn` AST walking, `walkdir`/`ignore` test discovery, `serde`
//! reporters, `comfy-table` stdout formatter all live here. None of
//! these types may leak into `domain/` or `ports/`.
//!
//! Module skeleton:
//! - `source/` — walkdir/ignore-based test discovery (v0.1 P9)
//! - `parser/` — syn AST walker, attribute + assertion recognition (v0.1 P10–P11)
//! - `detectors/` — one detector module per smell (v0.1 P13–P17)
//! - `config.rs` — `scrap4rs.toml` schema (v0.1 P22)
//! - `reporters/` — JSON / Markdown / table / SARIF / scorecard-row (v0.1 P18–P21, v0.2 P29)
