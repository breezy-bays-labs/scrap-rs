//! Per-smell detector modules.
//!
//! Each detector is a free function that takes `&domain::ParsedTest`
//! and returns `Option<domain::Finding>` (or `Vec<Finding>` for
//! multi-finding detectors). Detectors are language-agnostic — they
//! operate on `domain::ParsedTest`, the language-agnostic projection
//! produced by the parser adapter, never on AST library types.
//!
//! Module skeleton (lands as detector PRs ship):
//! - `zero_assertion.rs` — body has no assert*!/`should_panic`/etc.
//!   and no implicit-assertion source (P13)
//! - `tautological_assertion.rs` — `assert!(true)`, `assert_eq!(x, x)`,
//!   literal-vs-literal compare (P14)
//! - `no_op_io.rs` — body is `let _ = ...;` with no follow-up check (P15)
//! - `surface_only_io.rs` — `*.exists()` post-create without read-back (P16)
//! - `large_example.rs` — body exceeds configured line threshold (P17)
//!
//! All detectors live in `scrap-core` so every adapter binary
//! inherits them via the linkage; only the parser adapter is
//! language-specific.

pub mod zero_assertion;
