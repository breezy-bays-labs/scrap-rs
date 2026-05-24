//! `TestVisitor<'ast>` — top-level walker using `syn::visit::Visit`.
//!
//! Discovers tests by overriding `visit_item_mod` (to maintain the
//! qualified-name path stack) and `visit_item_fn` (to project each
//! `#[test]`-attributed fn into a `ParsedTest`). The body-walker
//! (`BodyVisitor`, lands at S2.2) is driven inside `extract_parsed_test`.
//!
//! S1.1 ships the skeleton: empty Visit overrides + the seed/drain
//! pair (`new` / `into_parsed_test_file`). Wave 2 fills in the override
//! bodies (S2.1 → top-level discovery; S2.2 → S2.4 → body walker).

use scrap_core::domain::parsed::{ParseDiagnostic, ParsedTest, ParsedTestFile};
use scrap_core::domain::types::FilePath;
use syn::visit::Visit;

/// Top-level walker state. One instance per file: seeded in
/// `SynTestParser::parse_test_source`, drained via
/// `into_parsed_test_file` after `visit_file` finishes.
//
// TODO(S2.1): `path_stack` becomes load-bearing when `visit_item_mod`
// lands its override (push module ident, recurse, pop). The
// `#[allow(dead_code)]` below keeps the S1.1 skeleton warning-free
// until then.
pub(crate) struct TestVisitor {
    /// Module-path accumulator. `visit_item_mod` pushes the module
    /// ident before recursing and pops after; the stack at any point
    /// during the walk represents the qualified-name prefix for any
    /// fn discovered below.
    #[allow(dead_code)] // TODO(S2.1)
    pub(crate) path_stack: Vec<String>,
    /// File-path stamped on every `ParsedTest::identity` (and on the
    /// final `ParsedTestFile::path`).
    pub(crate) file_path: FilePath,
    /// Tests discovered so far. Drained into `ParsedTestFile::tests`.
    pub(crate) tests: Vec<ParsedTest>,
    /// Non-fatal parse-time diagnostics. v0.1 ships empty (syn's
    /// `parse_file` is whole-file fail); reserved for D12-shaped
    /// partial-recovery use by future TS adapters.
    pub(crate) diagnostics: Vec<ParseDiagnostic>,
}

impl TestVisitor {
    /// Seed a fresh visitor for a single file.
    pub(crate) fn new(file_path: FilePath) -> Self {
        Self {
            path_stack: Vec::new(),
            file_path,
            tests: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Drain the accumulated state into the canonical domain shape.
    pub(crate) fn into_parsed_test_file(self) -> ParsedTestFile {
        ParsedTestFile::new(self.file_path, self.tests, self.diagnostics)
    }
}

impl Visit<'_> for TestVisitor {
    // S1.1 ships the empty implementation — `Visit`'s default
    // recursion walks every item, but no overrides extract anything.
    // Result: parsing any source returns `ParsedTestFile { tests: [],
    // diagnostics: [] }`.
    //
    // S2.1 fills in `visit_item_mod` (push/recurse/pop) and
    // `visit_item_fn` (is_test_fn check + extract_parsed_test).
    //
    // Note: S2.1 will need to re-introduce the named `'ast` lifetime
    // (`impl<'ast> Visit<'ast>`) once visit_item_mod / visit_item_fn
    // overrides take `&'ast` references. Until then, `'_` elides the
    // unused-lifetime warning.
}
