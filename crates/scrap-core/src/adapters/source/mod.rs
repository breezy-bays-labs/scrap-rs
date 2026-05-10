//! `SourcePort` adapter implementations.
//!
//! Module roster:
//! - `fs` — disk walker (`FsWalker`) backed by `ignore::WalkBuilder` +
//!   `globset` overrides; honours `.gitignore` per
//!   [`crate::domain::config::AnalysisConfig::respect_gitignore`].
//! - `memory` — in-memory test fixture (`MemorySource`); no I/O.

pub mod fs;
pub mod memory;
