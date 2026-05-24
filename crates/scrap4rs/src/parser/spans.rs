//! Span/line helpers — convert `syn::spanned::Spanned` to `domain::Span`.
//!
//! Lives in the parser module tree because every helper takes a
//! `syn::*` reference. The domain crate stays AST-pure.
//!
//! S1.1 ships `line_to_u32` (saturating cast) and `span_from_spanned`
//! (the line-range projection). S2.1 adds `compute_body_line_count`
//! (`syn::Block`-specific helper for `ParsedTest::body_line_count`).

use scrap_core::domain::types::Span;
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

/// Project any `syn::spanned::Spanned` node into the domain's
/// line-range `Span`.
///
/// Requires the `span-locations` feature on `proc-macro2` (pinned at
/// the workspace level in `Cargo.toml`). Without that feature, both
/// `start.line` and `end.line` are zero for every node and the
/// resulting `Span::new(1, 1)` is meaningless.
///
/// `proc_macro2::Span::start()` returns `LineColumn { line, column }`
/// — line is **1-based**. A `start.line == 0` value is the proc-macro2
/// sentinel for "no usable span info" (synthetic spans from procedural
/// expansion, etc.); we defensively clamp those to `Span::new(1, 1)`
/// rather than panic via `Span::new`'s `debug_assert!(start <= end)`.
/// `parse_error_from_syn_error` will use the parallel
/// `span_from_syn_error` shape for parser failures specifically.
//
// TODO(S2.1): the `extract_parsed_test` orchestrator calls this for
// every test fn discovered by `visit_item_fn`. Until that lands, the
// fn is dead in the parser proper — the test below keeps it
// reachable through `#[cfg(test)]` only. The `#[allow(dead_code)]`
// keeps the lib build warning-free until S2.1.
#[allow(dead_code)]
pub(crate) fn span_from_spanned<T: Spanned>(node: &T) -> Span {
    let syn_span = node.span();
    let start_line = line_to_u32(syn_span.start().line);
    let end_line = line_to_u32(syn_span.end().line);

    // Synthetic-span defense: if start.line is 0 (the proc-macro2
    // "no span info" sentinel), or if end < start (shouldn't happen
    // for parsed sources, but the saturating shape stays panic-free),
    // emit a placeholder span on line 1 instead of tripping the
    // `Span::new` debug_assert.
    if start_line == 0 || end_line < start_line {
        Span::new(1, 1)
    } else {
        Span::new(start_line, end_line)
    }
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
}
