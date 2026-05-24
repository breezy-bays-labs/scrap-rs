//! syn-based Rust-source parser adapter for `TestParserPort`.
//!
//! Lands with scrap-rs#12. The single public type, [`SynTestParser`],
//! implements [`scrap_core::ports::parser::TestParserPort`] and
//! projects a Rust source file (`&str`) into the language-agnostic
//! [`scrap_core::domain::parsed::ParsedTestFile`] shape.
//!
//! All `syn` types are confined to this module tree — see
//! [`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-hexagonal-layout.md)
//! for the layering invariant. `scrap-core` stays AST-pure; the
//! `ast-purity` CI grep enforces.

mod spans;
mod visitor;

use scrap_core::domain::parsed::ParsedTestFile;
use scrap_core::domain::types::{FilePath, Span};
use scrap_core::ports::parser::{ParseError, TestParserPort};
use syn::visit::Visit;

use self::visitor::TestVisitor;

/// Zero-sized parser adapter implementing
/// [`scrap_core::ports::parser::TestParserPort`].
///
/// Stateless — every call to [`SynTestParser::parse_test_source`]
/// constructs a fresh internal `TestVisitor` and drains it into a
/// `ParsedTestFile`. Safe to share across threads (`Send + Sync` are
/// emergent properties — there are no fields to violate either).
///
/// Per ADR D11, the `dyn TestParserPort` trait surface deliberately
/// does NOT advertise `Send + Sync`; concrete adapter types like this
/// one can still be `Send + Sync`, which the trait-surface assertions
/// in `tests/parser_surface.rs` pin both directions.
#[derive(Debug, Default, Clone, Copy)]
pub struct SynTestParser;

impl SynTestParser {
    /// Construct a fresh parser. Stateless — every call returns the
    /// same zero-sized value, but `::new` is the canonical
    /// constructor per D10 and may grow optional config in future
    /// versions (e.g. a `with_macro_aliases(...)` extension).
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl TestParserPort for SynTestParser {
    fn parse_test_source(
        &self,
        source: &str,
        path: &FilePath,
    ) -> Result<ParsedTestFile, ParseError> {
        let file = syn::parse_file(source).map_err(|e| parse_error_from_syn_error(&e))?;
        let mut visitor = TestVisitor::new(path.clone());
        visitor.visit_file(&file);
        Ok(visitor.into_parsed_test_file())
    }
}

/// Map a `syn::Error` from `syn::parse_file` into a
/// `ParseError::Syntax`.
///
/// `syn`'s `parse_file` is whole-file fail (no partial recovery), so
/// every reachable code path here returns
/// `ParseError::Syntax { message, span }`. The span is `Some(_)` when
/// `syn::Error::span()` reports a meaningful location (line >= 1 with
/// `end >= start`); `None` otherwise (e.g. synthetic errors from
/// procedural sources without span info).
///
/// Note: `core::error::Error` is the future-proofed trait (stabilized
/// in Rust 1.81; toolchain at write-time is 1.93). `ParseError`
/// derives `std::error::Error` via `thiserror`, which is identical to
/// `core::error::Error` post-1.81. No explicit re-impl needed here.
fn parse_error_from_syn_error(err: &syn::Error) -> ParseError {
    let message = err.to_string();
    let syn_span = err.span();
    let start_line = u32::try_from(syn_span.start().line).unwrap_or(u32::MAX);
    let end_line = u32::try_from(syn_span.end().line).unwrap_or(u32::MAX);

    // Span::new debug-asserts start <= end and a 1-based line range.
    // Guard against both:
    //   - start_line == 0: proc-macro2's "no usable span info" sentinel.
    //   - end_line < start_line: defensive; shouldn't happen for
    //     parse errors but the saturating shape stays panic-free.
    let span = if start_line == 0 || end_line < start_line {
        None
    } else {
        Some(Span::new(start_line, end_line))
    };

    ParseError::Syntax { message, span }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_source_returns_empty_inventory() {
        let parser = SynTestParser::new();
        let file = parser
            .parse_test_source("", &FilePath::new("empty.rs"))
            .expect("empty source parses cleanly");

        assert_eq!(file.path, FilePath::new("empty.rs"));
        assert!(
            file.tests.is_empty(),
            "no items in empty source — tests must be empty"
        );
        assert!(
            file.diagnostics.is_empty(),
            "no recoverable parse issues — diagnostics must be empty"
        );
    }

    #[test]
    fn parse_malformed_source_returns_syntax_error() {
        let parser = SynTestParser::new();
        let result =
            parser.parse_test_source("fn missing_brace() {", &FilePath::new("malformed.rs"));

        let err = result.expect_err("unclosed brace must surface ParseError::Syntax");
        match &err {
            ParseError::Syntax { message, .. } => {
                assert!(
                    !message.is_empty(),
                    "syntax error must carry a non-empty message"
                );
            }
            // ParseError is `#[non_exhaustive]`; the future-compat
            // wildcard is required even though Syntax is the only
            // variant today.
            _ => panic!("expected ParseError::Syntax, got {err:?}"),
        }
    }

    #[test]
    fn parse_well_formed_source_with_no_tests_returns_empty_inventory() {
        // S1.1 ships empty Visit overrides, so even a source with
        // `#[test] fn it() {}` projects to zero ParsedTests. S2.1
        // flips this expectation when it implements visit_item_fn.
        let parser = SynTestParser::new();
        let source = "#[test] fn it() {}";
        let file = parser
            .parse_test_source(source, &FilePath::new("placeholder.rs"))
            .expect("well-formed source parses");

        // At S1.1, the walker is still empty — no tests recovered.
        // This expectation flips to `== 1` when S2.1 lands the
        // visit_item_fn override.
        assert_eq!(
            file.tests.len(),
            0,
            "S1.1 ships empty Visit overrides — Wave 2 flips this to 1"
        );
    }
}
