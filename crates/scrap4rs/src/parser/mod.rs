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

mod assertions;
mod attributes;
mod body;
mod spans;
mod visitor;

use scrap_core::domain::parsed::{ParsedTest, ParsedTestFile};
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use scrap_core::ports::parser::{ParseError, TestParserPort};
use syn::visit::Visit;
use syn::{Ident, ItemFn};

use self::attributes::{extract_attributes, extract_opt_outs};
use self::body::BodyVisitor;
use self::spans::{compute_body_line_count, span_from_spanned};
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

/// Compose a fully-qualified `QualifiedName` from a module-path stack
/// and a fn ident.
///
/// Path joined with `"::"`. Empty stack (free fn at file root)
/// returns just the fn name; stack `["auth", "login_tests"]` + fn
/// `it_logs_in` returns `"auth::login_tests::it_logs_in"`.
///
/// Hand-rolled join (no `quote!`) — consistent with the
/// `compose_macro_path_string` discipline (no whitespace injection).
pub(crate) fn compose_qualified_name(path_stack: &[String], fn_ident: &Ident) -> QualifiedName {
    let leaf = fn_ident.to_string();
    let qualified = if path_stack.is_empty() {
        leaf
    } else {
        let mut joined = path_stack.join("::");
        joined.push_str("::");
        joined.push_str(&leaf);
        joined
    };
    QualifiedName::new(qualified)
}

/// Project one `syn::ItemFn` (already confirmed `is_test_fn` true) into
/// the domain `ParsedTest` shape.
///
/// Drives the helpers from `attributes` + `spans` + (S2.2+)
/// `body::BodyVisitor`. The body-walker integration is currently a
/// TODO(S2.2) stub returning empty `(assertions, implicit_sources)`;
/// S2.2 swaps in the real `BodyVisitor::drive(&item.block)` call.
/// S2.4 additionally wires in `N24
/// implicit_sources_from_attributes` for the `#[should_panic]` →
/// `AssertionSource::ShouldPanic` attribute path.
pub(crate) fn extract_parsed_test(
    item: &ItemFn,
    path_stack: &[String],
    file_path: &FilePath,
) -> ParsedTest {
    let attributes = extract_attributes(item);
    let opt_outs = extract_opt_outs(item);
    let body_line_count = compute_body_line_count(&item.block);
    let qualified_name = compose_qualified_name(path_stack, &item.sig.ident);
    let identity_span = span_from_spanned(item);

    // S2.2: drive the BodyVisitor over the test fn's block to recover
    // explicit assertions. S2.3 will extend BodyVisitor with the
    // implicit-source macro path; S2.4 adds the visit_expr_await
    // (cucumber chain) + visit_expr_call (function-call implicit
    // sources) overrides and the implicit_sources_from_attributes
    // attribute-path merge.
    let mut body_visitor = BodyVisitor::new();
    body_visitor.drive(&item.block);
    let assertions = body_visitor.assertions;

    // TODO(S2.3 + S2.4): merge body-walker implicit sources (S2.3)
    // AND `implicit_sources_from_attributes(item)` (S2.4's
    // attribute-path projection for `#[should_panic]` →
    // `AssertionSource::ShouldPanic`) into the final
    // `implicit_assertion_sources` vec via Vec::extend.
    let implicit_assertion_sources = Vec::new();

    ParsedTest::new(
        TestIdentity::new(file_path.clone(), qualified_name, identity_span),
        attributes,
        assertions,
        body_line_count,
        implicit_assertion_sources,
        opt_outs,
    )
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
    fn parse_bare_test_fn_yields_one_parsed_test() {
        // S2.1 flip from the S1.1 baseline: with `visit_item_fn`
        // wired, a bare `#[test]` fn projects to one `ParsedTest`.
        // Body-walker stubs still keep `assertions` /
        // `implicit_assertion_sources` empty (S2.2 + S2.3 + S2.4
        // light those up).
        let parser = SynTestParser::new();
        let source = "#[test] fn it() {}";
        let file = parser
            .parse_test_source(source, &FilePath::new("placeholder.rs"))
            .expect("well-formed source parses");

        assert_eq!(file.tests.len(), 1, "one #[test] fn → one ParsedTest");
        let parsed = &file.tests[0];
        assert_eq!(parsed.identity.qualified_name.as_str(), "it");
        assert_eq!(parsed.attributes.len(), 1);
        assert_eq!(parsed.attributes[0].name, "test");
        assert_eq!(parsed.attributes[0].raw, None);
        // Body integration is S2.2+ stubbed: empty until then.
        assert!(parsed.assertions.is_empty());
        assert!(parsed.implicit_assertion_sources.is_empty());
        assert!(parsed.opt_outs.is_empty());
    }

    #[test]
    fn parse_nested_mod_test_composes_qualified_name() {
        // S2.1 verifies the path-stack walking: a fn discovered at
        // depth-2 module nesting gets a `qualified_name` joined
        // with `::`.
        let parser = SynTestParser::new();
        let source = "mod auth { mod login_tests { #[test] fn it_logs_in() {} } }";
        let file = parser
            .parse_test_source(source, &FilePath::new("nested.rs"))
            .expect("well-formed source parses");

        assert_eq!(file.tests.len(), 1);
        assert_eq!(
            file.tests[0].identity.qualified_name.as_str(),
            "auth::login_tests::it_logs_in"
        );
    }
}
