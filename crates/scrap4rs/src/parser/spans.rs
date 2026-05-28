//! Span/line helpers — convert `syn::spanned::Spanned` to `domain::Span`.
//!
//! Lives in the parser module tree because every helper takes a
//! `syn::*` reference. The domain crate stays AST-pure.
//!
//! Surface: `line_to_u32` (saturating cast for `LineColumn::line`),
//! `column_to_u32_1based` (saturating cast + 0→1-based shift for
//! `LineColumn::column`), `span_from_spanned` (line+column-range
//! projection from any `syn::spanned::Spanned` node),
//! `compute_body_line_count` (`syn::Block`-specific helper for
//! `ParsedTest::body_line_count`).

use scrap_core::domain::types::Span;
use syn::Block;
use syn::spanned::Spanned;

/// Saturating cast from `proc_macro2::LineColumn::line` (`usize`) to
/// `domain::Span`'s `u32`.
///
/// Defensive: line numbers don't realistically exceed `u32::MAX`
/// (~4 billion lines) in any sane source file, but the saturating
/// cast keeps the parser panic-free under pathological inputs (e.g.
/// a fuzz-generated source that somehow reports an absurd line).
pub(crate) fn line_to_u32(line: usize) -> u32 {
    u32::try_from(line).unwrap_or(u32::MAX)
}

/// Saturating cast + 0-based → 1-based shift for
/// `proc_macro2::LineColumn::column` (`usize`).
///
/// proc-macro2's `column` is **0-based** (the first column on a line
/// is `0`); `domain::Span` columns are **1-based** (consistent with
/// the 1-based line fields and SARIF region semantics). We add 1 with
/// `saturating_add` so a pathological `u32::MAX` column does not wrap.
pub(crate) fn column_to_u32_1based(column: usize) -> u32 {
    u32::try_from(column).unwrap_or(u32::MAX).saturating_add(1)
}

/// Project any `syn::spanned::Spanned` node into the domain's
/// line+column-range `Span`.
///
/// Requires the `span-locations` feature on `proc-macro2` (pinned at
/// the workspace level in `Cargo.toml`). Without that feature, both
/// `start.line` and `end.line` are zero for every node and the
/// resulting placeholder `Span::new(1, 1, 1, 1)` is meaningless.
///
/// `proc_macro2::Span::start()` returns `LineColumn { line, column }`
/// — line is **1-based**, column is **0-based** (converted to 1-based
/// via [`column_to_u32_1based`]). A `start.line == 0` value is the
/// proc-macro2 sentinel for "no usable span info" (synthetic spans
/// from procedural expansion, etc.); we defensively clamp those to the
/// placeholder span `Span::new(1, 1, 1, 1)` rather than panic via
/// `Span::new`'s `debug_assert!`. `parse_error_from_syn_error` uses
/// the parallel shape for parser failures specifically.
///
/// Called from `extract_parsed_test` (every test fn's identity span)
/// and from `BodyVisitor::visit_macro` (every recognised assertion's
/// span).
pub(crate) fn span_from_spanned<T: Spanned>(node: &T) -> Span {
    let syn_span = node.span();
    let start = syn_span.start();
    let end = syn_span.end();
    let start_line = line_to_u32(start.line);
    let end_line = line_to_u32(end.line);
    let start_column = column_to_u32_1based(start.column);
    let end_column = column_to_u32_1based(end.column);

    // Synthetic-span defense: if start.line is 0 (the proc-macro2
    // "no span info" sentinel), or if end < start (shouldn't happen
    // for parsed sources, but the saturating shape stays panic-free),
    // emit a placeholder span at 1:1..1:1 instead of tripping the
    // `Span::new` debug_assert.
    if start_line == 0 || end_line < start_line {
        Span::new(1, 1, 1, 1)
    } else {
        Span::new(start_line, end_line, start_column, end_column)
    }
}

/// Body line count for a test fn — counts interior lines INCLUDING
/// both braces.
///
/// Formula: `close_line.saturating_sub(open_line)`.
///
/// Examples:
/// - Single-line body `{ assert!(true); }` (open and close both on
///   line 5) returns `0`.
/// - 3-line body block (open on line 5, close on line 8) returns `3`.
///
/// Saturating `u32::saturating_sub` defends against pathological
/// inverted spans. The docstring pins to the formula above
/// deliberately — an "N-1 for N-line bodies" mental model is
/// misleading.
pub(crate) fn compute_body_line_count(block: &Block) -> u32 {
    let open = line_to_u32(block.brace_token.span.open().start().line);
    let close = line_to_u32(block.brace_token.span.close().start().line);
    close.saturating_sub(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_to_u32_in_range() {
        assert_eq!(line_to_u32(0), 0);
        assert_eq!(line_to_u32(42), 42);
        assert_eq!(line_to_u32(usize::from(u16::MAX)), u32::from(u16::MAX));
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn line_to_u32_saturates_overflow() {
        // u32::MAX as usize fits cleanly on 64-bit; one above saturates.
        let overflowed = (u32::MAX as usize).saturating_add(1);
        assert_eq!(line_to_u32(overflowed), u32::MAX);
    }

    #[test]
    fn column_to_u32_1based_shifts_zero_to_one() {
        // proc-macro2 columns are 0-based; the domain is 1-based.
        assert_eq!(column_to_u32_1based(0), 1, "0-based column 0 → 1-based 1");
        assert_eq!(column_to_u32_1based(4), 5, "0-based column 4 → 1-based 5");
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn column_to_u32_1based_saturates_without_wrapping() {
        // A pathological max column must NOT wrap to 0 after the +1.
        assert_eq!(column_to_u32_1based(usize::MAX), u32::MAX);
        // u32::MAX as a usize column saturates to u32::MAX (no wrap).
        assert_eq!(column_to_u32_1based(u32::MAX as usize), u32::MAX);
    }

    #[test]
    fn span_from_spanned_real_source_extracts_line_range() {
        // syn::parse_file gives us spans with real line numbers
        // (because proc-macro2 has span-locations enabled).
        let source = "// line 1\n#[test]\nfn it() {}\n";
        let file: syn::File = syn::parse_str(source).expect("parses");
        let item = &file.items[0];

        // The test fn lives on lines 2-3 (attribute + signature+body).
        let span = span_from_spanned(item);
        assert!(span.start_line >= 1, "start line is 1-based");
        assert!(
            span.end_line >= span.start_line,
            "end_line {} must be >= start_line {}",
            span.end_line,
            span.start_line,
        );
    }

    #[test]
    fn span_from_spanned_extracts_1based_columns() {
        // `#[test]` starts at column 0 (0-based) in proc-macro2 →
        // start_column 1 (1-based) in the domain. The leading `#`
        // of the attribute is the span start.
        let source = "#[test]\nfn it() {\n    assert!(true);\n}\n";
        let file: syn::File = syn::parse_str(source).expect("parses");
        let item = &file.items[0];
        let span = span_from_spanned(item);
        assert!(
            span.start_column >= 1,
            "start_column is 1-based (>= 1), got {}",
            span.start_column,
        );
        assert!(
            span.end_column >= 1,
            "end_column is 1-based (>= 1), got {}",
            span.end_column,
        );
        // The attribute `#` is the first character on line 1 → 1-based
        // column 1.
        assert_eq!(span.start_line, 1);
        assert_eq!(span.start_column, 1, "leading `#` is at 1-based column 1");
    }

    #[test]
    fn span_from_spanned_synthetic_span_is_placeholder() {
        // An ident built with `proc_macro2::Span::call_site()` carries
        // the synthetic "no span info" span — line 0 / column 0 outside
        // a proc-macro context. The helper must clamp to the
        // placeholder 1:1..1:1 rather than panic via Span::new's
        // debug_assert.
        let ident = syn::Ident::new("synthetic", proc_macro2::Span::call_site());
        let span = span_from_spanned(&ident);
        assert_eq!(span.start_line, 1);
        assert_eq!(span.end_line, 1);
        assert_eq!(span.start_column, 1);
        assert_eq!(span.end_column, 1);
    }

    fn parse_first_fn_block(source: &str) -> Block {
        let file: syn::File = syn::parse_str(source).expect("parses");
        for item in file.items {
            if let syn::Item::Fn(f) = item {
                return *f.block;
            }
        }
        panic!("source contains no fn item");
    }

    #[test]
    fn compute_body_line_count_single_line_body_is_zero() {
        // `{ assert!(true); }` on one line — open brace and close
        // brace on the same line; formula `close - open == 0`.
        let block = parse_first_fn_block("#[test] fn it() { assert!(true); }");
        assert_eq!(compute_body_line_count(&block), 0);
    }

    #[test]
    fn compute_body_line_count_three_interior_lines_is_three() {
        // `fn it() {\n  let x = 1;\n  assert_eq!(x, 1);\n}` —
        // open brace on line 1, close brace on line 4; formula
        // `close - open == 3`. Matches the cucumber scenario's
        // expectation pinned at parser.feature:127.
        let source = "fn it() {\n    let x = 1;\n    assert_eq!(x, 1);\n}";
        let block = parse_first_fn_block(source);
        assert_eq!(compute_body_line_count(&block), 3);
    }

    #[test]
    fn compute_body_line_count_empty_body_one_line_is_zero() {
        let block = parse_first_fn_block("fn it() {}");
        assert_eq!(compute_body_line_count(&block), 0);
    }
}
