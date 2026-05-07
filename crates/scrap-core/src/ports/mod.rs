//! Ports — trait definitions that bridge `domain/` to `adapters/`.
//!
//! Ports may depend only on `domain/` types. No external crates here.
//! Adapters implement these traits; `core/` consumes them.
//!
//! Module skeleton:
//! - `source.rs` — `SourcePort` (file enumeration) (v0.1 P7)
//! - `parser.rs` — `TestParserPort` (returns `ParsedTest`) (v0.1 P7)
//! - `output.rs` — `OutputPort` (rendering) (v0.1 P7)
