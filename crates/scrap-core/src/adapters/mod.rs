//! Adapters — language- or backend-specific implementations of port
//! traits.
//!
//! `syn` AST walking, `ignore`-based test discovery, `serde`
//! reporters, `comfy-table` stdout formatter all live here. None of
//! these types may leak into `domain/` or `ports/`.
//!
//! Module skeleton:
//! - `source/` — `SourcePort` impls (`fs::FsWalker`, `memory::MemorySource`)
//! - `reporters/` — free-function reporters (`json` + `table` live;
//!   markdown / sarif / scorecard-row planned)
//! - `parser/` — adapter-specific (lives in `crates/scrap4rs/src/parser/`, NOT here)
//! - `detectors/` — one detector module per smell (lives in `crates/scrap-core/src/detectors/`)
//! - `config.rs` — `scrap4rs.toml` schema (lives in `crates/scrap-core/src/cli/config.rs`)

pub mod reporters;
pub mod source;
