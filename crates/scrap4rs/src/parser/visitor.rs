//! `TestVisitor` — top-level walker using `syn::visit::Visit`.
//!
//! Discovers tests by overriding `visit_item_mod` (to maintain the
//! qualified-name path stack) and `visit_item_fn` (to project each
//! `#[test]`-attributed fn into a `ParsedTest`). Per-test body
//! inspection is delegated to `super::body::BodyVisitor`, driven
//! from `extract_parsed_test` in `parser/mod.rs`.

use scrap_core::domain::parsed::{ParseDiagnostic, ParsedTest, ParsedTestFile};
use scrap_core::domain::types::FilePath;
use syn::visit::{self, Visit};
use syn::{ItemFn, ItemMod};

use super::attributes::is_test_fn;
use super::extract_parsed_test;

/// Top-level walker state. One instance per file: seeded in
/// `SynTestParser::parse_test_source`, drained via
/// `into_parsed_test_file` after `visit_file` finishes.
pub(crate) struct TestVisitor {
    /// Module-path accumulator. `visit_item_mod` pushes the module
    /// ident before recursing and pops after; the stack at any point
    /// during the walk represents the qualified-name prefix for any
    /// fn discovered below.
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

impl<'ast> Visit<'ast> for TestVisitor {
    /// Push the module ident onto the path stack, recurse via Visit's
    /// default walk, then pop. The stack maintained here is what
    /// `compose_qualified_name` consumes when projecting test
    /// identities below.
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        self.path_stack.push(node.ident.to_string());
        visit::visit_item_mod(self, node);
        // Drop on the way out so siblings + ancestors see the
        // restored stack.
        self.path_stack.pop();
    }

    /// If the fn has a `#[test]`-like attribute, project it into a
    /// `ParsedTest` and push to the accumulator. Do NOT recurse into
    /// the body — nested fns (rare in test code) are NOT themselves
    /// tests at v0.1, per kickstart plan §11.4.
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if is_test_fn(&node.attrs) {
            let parsed = extract_parsed_test(node, &self.path_stack, &self.file_path);
            self.tests.push(parsed);
        }
        // Deliberately NOT calling `visit::visit_item_fn(self, node)`
        // — nested fns inside a test body are not themselves tests at
        // v0.1 (per kickstart plan §11.4). The no-recurse choice
        // here is independent of the `visit_macro` no-recurse choice
        // documented in `body.rs`.
    }
}
