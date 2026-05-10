//! `SourcePort` adapter implementations.
//!
//! Module roster:
//! - `memory` — in-memory test fixture (`MemorySource`); no I/O.
//!
//! Module roster (planned, lands later in scrap-rs#13):
//! - `fs` — disk walker (`FsWalker`) backed by `ignore::WalkBuilder` +
//!   `globset` overrides; honours `.gitignore` per
//!   [`crate::domain::config::AnalysisConfig::respect_gitignore`].

pub mod memory;
