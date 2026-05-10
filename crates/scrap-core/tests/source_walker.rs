//! Compile-time invariant smoke tests for the `SourcePort` trait + impls.
//!
//! Behavioral scenarios live in `tests/features/file_walker.feature`
//! (executed by `tests/cucumber.rs`); this file pins the *type-level*
//! contracts only — if it compiles + links, the assertions below hold.
//!
//! Pinned invariants:
//! - `SourcePort` is object-safe — `Box<dyn SourcePort>` works.
//! - `dyn SourcePort` is **deliberately NOT** `Send + Sync` —
//!   parallelism bounds belong at the `core::analyze<S, P>` call site
//!   (per ADR `adr-port-surface-and-domain-conventions` D11).
//! - Both shipped adapters (`FsWalker`, `MemorySource`) are
//!   `Send + Sync` as an emergent property — `Override` is `Arc`-backed
//!   internally and `AnalysisConfig` is POD; `MemorySource` is a pure
//!   POD with no interior mutability.

use scrap_core::adapters::source::{fs::FsWalker, memory::MemorySource};
use scrap_core::ports::source::SourcePort;

static_assertions::assert_obj_safe!(SourcePort);
static_assertions::assert_not_impl_any!(dyn SourcePort: Send, Sync);
static_assertions::assert_impl_all!(FsWalker: Send, Sync);
static_assertions::assert_impl_all!(MemorySource: Send, Sync);

#[test]
fn smoke_compiles() {
    // Existence test — if this file compiles + links, the
    // static_assertions above hold. The fn body is intentionally empty.
}
