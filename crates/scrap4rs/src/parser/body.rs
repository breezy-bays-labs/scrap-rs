//! `BodyVisitor` — per-test body walker using `syn::visit::Visit`.
//!
//! Walks the body of one `#[test]`-attributed fn and accumulates
//! domain facts:
//! - explicit assertion macros (`visit_macro` leaf-name match against
//!   the v0.1 set: `assert` / `assert_eq` / `assert_ne` /
//!   `assert_matches` / `panic` / `unimplemented` / `todo`) →
//!   `Vec<ParsedAssertion>`,
//! - macro-form implicit-assertion sources (`visit_macro` `recognise()`
//!   branch: `proptest!`, `kani::*`, `insta::assert_*!`,
//!   `pretty_assertions::*`, `*_proptest` suffix),
//! - cucumber `.await` chain terminal (`visit_expr_await` walks the
//!   `ExprAwait::base` for `World::cucumber().run(...)` or
//!   `cucumber::Cucumber::run(...)` shapes; fabricates the synthetic
//!   `"cucumber::run"` key for `recognise()`),
//! - function-call implicit sources (`visit_expr_call` for
//!   `quickcheck::quickcheck(...)`, `trybuild::TestCases::*`, etc.)
//!   → `Vec<AssertionSource>`.
//!
//! Driven from `extract_parsed_test` in `parser/mod.rs` — fresh
//! visitor per test fn. `Vec`-based accumulators preserve emission
//! order (useful for debugging which body construct triggered
//! recognition).

use scrap_core::domain::assertion_sources::{AssertionSource, recognise};
use scrap_core::domain::behavioral_fact::{BehavioralFact, ResultDiscardKind};
use scrap_core::domain::parsed::ParsedAssertion;
use syn::Block;
use syn::visit::Visit;

use super::assertions::compose_macro_path_string;
use super::spans::span_from_spanned;

/// Set of v0.1 assertion-macro leaf-segment names. Matched against
/// the LEAF segment of the macro path so `pretty_assertions::assert_eq`
/// matches `"assert_eq"` (and is also picked up as an implicit
/// `PrettyAssertions` source via the `recognise()` branch).
const ASSERTION_MACRO_NAMES: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "assert_matches",
    "panic",
    "unimplemented",
    "todo",
];

/// Set of method-call idents that the body walker treats as the
/// explicit-panic-is-the-assertion pattern. Matched against
/// `syn::ExprMethodCall::method` (an `Ident`); each fires
/// `BehavioralFact::ResultAsserted`.
///
/// v0.1 recognises both the happy-path forms (`.unwrap()` / `.expect(...)`)
/// AND the error-path forms (`.unwrap_err()` / `.expect_err(...)`). The
/// `*_err` siblings are canonical Rust "assert this failed" idioms on
/// `Result`; without them, a test like
/// `let _ = parse_invalid_input().unwrap_err();` produces a
/// false-positive zero-assertion finding (gemini MEDIUM 2026-05-27 on
/// PR #82). v0.3+ may layer in finer-grained variants (`.unwrap_or_else`,
/// `.unwrap_unchecked`, etc.) if recall/precision data indicates.
const PANIC_CHAIN_METHOD_NAMES: &[&str] = &["unwrap", "expect", "unwrap_err", "expect_err"];

/// Per-test body walker. Constructed fresh in `extract_parsed_test`,
/// drained via the field accessors after the body visit completes.
pub(crate) struct BodyVisitor {
    /// Explicit assertions found in the body. Populated by
    /// `visit_macro` (leaf-segment match against
    /// [`ASSERTION_MACRO_NAMES`]).
    pub(crate) assertions: Vec<ParsedAssertion>,
    /// Implicit-assertion sources found in the body. Populated by
    /// `visit_macro` (macro-form sources via `recognise()`:
    /// `proptest`, `kani`, `insta`, `pretty_assertions`, `*_proptest`
    /// suffix), `visit_expr_await` (cucumber chain), and
    /// `visit_expr_call` (function-call sources: `quickcheck`,
    /// `trybuild`). Emission order is preserved (`Vec`, not
    /// `BTreeSet`) — useful for debugging which body construct
    /// triggered recognition.
    pub(crate) implicit_assertion_sources: Vec<AssertionSource>,
    /// Body-shape behavioral facts found in the body. Populated by
    /// `visit_expr_method_call` (`.unwrap()` / `.expect(...)` chains →
    /// `ResultAsserted`) and `visit_local` (`let _ = ...;` discards →
    /// `ResultDiscarded`). `Vec` (not `BTreeSet`) preserves emission
    /// order and admits the located fact variants arriving at
    /// scrap-rs#26; the "≥1 of shape X" presence-fact dedup the two
    /// existing variants relied on is enforced here at **projection**
    /// (each push is guarded against an already-present equal fact).
    /// Mirrors the
    /// [`scrap_core::domain::parsed::ParsedTest::behavioral_facts`]
    /// storage shape.
    pub(crate) behavioral_facts: Vec<BehavioralFact>,
}

impl BodyVisitor {
    pub(crate) fn new() -> Self {
        Self {
            assertions: Vec::new(),
            implicit_assertion_sources: Vec::new(),
            behavioral_facts: Vec::new(),
        }
    }

    /// Drive the walk over a test fn's `&syn::Block`. Wrapper over
    /// `visit_block` so the caller doesn't have to import the Visit
    /// trait.
    pub(crate) fn drive(&mut self, block: &Block) {
        self.visit_block(block);
    }
}

impl<'ast> Visit<'ast> for BodyVisitor {
    /// Recognise explicit assertion macros AND implicit-source
    /// macros. Whitespace-sensitive path stringification via
    /// `compose_macro_path_string` (NOT `quote!`/`TokenStream`) so
    /// `recognise()`'s exact-string lookups stay accurate.
    ///
    /// **v0.1 boundary: do NOT call `visit::visit_macro(self, mac)` here.**
    /// The parser inspects the macro's immediate path identity only;
    /// token-stream descent is out of scope. Wrapped/custom macros
    /// (e.g. a hypothetical `my_proptest!` wrapping `proptest!`) are
    /// tracked under v0.3+ surface if real codebases push back at
    /// adoption time.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let path = compose_macro_path_string(&mac.path);

        // Explicit assertion macros: leaf-segment match against the
        // v0.1 set. The leaf (rightmost segment) is what makes
        // `pretty_assertions::assert_eq` match `"assert_eq"` here
        // while ALSO being recognised as an implicit
        // `PrettyAssertions` source by the recognise() branch below.
        if let Some(leaf) = path.rsplit("::").next()
            && ASSERTION_MACRO_NAMES.contains(&leaf)
        {
            let raw_args = stringify_tokens_opt(&mac.tokens);
            let span = span_from_spanned(mac);
            let (arguments_identical, single_arg_value) =
                super::tautology::extract_tautology_facts(&mac.tokens);
            self.assertions.push(ParsedAssertion::new(
                leaf,
                raw_args,
                span,
                arguments_identical,
                single_arg_value,
            ));
        }

        // Implicit-assertion sources via the recognise() contract
        // (proptest!, kani::*, insta::assert_*!, pretty_assertions::*,
        // *_proptest suffix). Do NOT short-circuit on the
        // explicit-assertion branch above — a macro can be BOTH (e.g.
        // `pretty_assertions::assert_eq` produces both a
        // ParsedAssertion AND an AssertionSource).
        if let Some(src) = recognise(&path) {
            self.implicit_assertion_sources.push(src);
        }

        // v0.1 boundary: NO visit::visit_macro recursion. See the
        // doc-comment block above for the rationale (wrapped/custom
        // macros are out of scope at v0.1; v0.3+ surface follow-up).
    }

    /// Cucumber `.await` chain recognition.
    ///
    /// `.await` desugars to `syn::Expr::Await(ExprAwait)` with its own
    /// dedicated Visit method — it is NOT a method call. This override
    /// checks if `node.base` is a cucumber chain (e.g.
    /// `World::cucumber().run(...)`) and, if so, fabricates the
    /// synthetic `"cucumber::run"` key for `recognise()`.
    ///
    /// **DOES** call `visit::visit_expr_await(self, node)` (unlike
    /// `visit_macro` where v0.1 forbids recursion) — `.await` chains
    /// can nest (`outer().await.inner().await`) and recursion finds
    /// every cucumber chain in the body.
    fn visit_expr_await(&mut self, node: &'ast syn::ExprAwait) {
        if is_cucumber_chain(&node.base)
            && let Some(src) = recognise("cucumber::run")
        {
            self.implicit_assertion_sources.push(src);
        }
        syn::visit::visit_expr_await(self, node);
    }

    /// Function-call implicit-source recognition.
    ///
    /// For `Expr::Call` whose `.func` is `Expr::Path` (e.g.
    /// `quickcheck::quickcheck(prop)`, `trybuild::TestCases::new()`),
    /// stringify the path and pass it through `recognise()`. The
    /// hand-rolled join via `compose_macro_path_string` reuses the
    /// same whitespace-free convention.
    ///
    /// **DOES** call `visit::visit_expr_call(self, call)` — function
    /// calls can nest (`outer(inner_implicit_call())`) and recursion
    /// finds every implicit source in the body.
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = call.func.as_ref() {
            let path_str = compose_macro_path_string(&expr_path.path);
            if let Some(src) = recognise(&path_str) {
                self.implicit_assertion_sources.push(src);
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    /// Method-call recognition for behavioral-fact projection.
    ///
    /// Recognises the v0.1 panic-chain method-call idioms in
    /// [`PANIC_CHAIN_METHOD_NAMES`] (`.unwrap()` / `.expect(...)` happy
    /// path + `.unwrap_err()` / `.expect_err(...)` error path) anywhere
    /// in the body, projecting each as `BehavioralFact::ResultAsserted`.
    /// Presence-fact dedup happens at **projection** (scrap-rs#112 `Vec`
    /// storage): the push is skipped when a `ResultAsserted` is already
    /// recorded, so multiple panic-chain calls in the same body produce
    /// one fact entry and the wire carries no duplicate `result_asserted`.
    ///
    /// **DOES** call `visit::visit_expr_method_call(self, node)` —
    /// method-call chains nest naturally (`x.unwrap().unwrap()`), and
    /// recursion finds every fact in the body. (The projection-time
    /// guard collapses the repeats into one entry.)
    ///
    /// No type inference. A `.unwrap()` on a non-Result/Option value
    /// (literal `().unwrap()`) is vanishingly rare in real test code
    /// and would be a parse error against most APIs; v0.1 ships with
    /// the shape-only recognition.
    ///
    /// **Note on `node.method == "unwrap"`** (`CEng` SHOULD-FOLD
    /// 2026-05-26): `syn::ExprMethodCall::method` is `syn::Ident`.
    /// Comparison to `&str` works via syn's `impl PartialEq<str> for
    /// Ident` (stable since syn 1.0); no `.to_string()` allocation per
    /// call site. Sibling precedent at `body.rs::is_cucumber_chain`
    /// (`s.ident == "cucumber"`) and at `attributes.rs::is_test_fn`
    /// (`seg.ident == name`).
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if PANIC_CHAIN_METHOD_NAMES
            .iter()
            .any(|&name| node.method == name)
            && !self
                .behavioral_facts
                .iter()
                .any(|f| matches!(f, BehavioralFact::ResultAsserted))
        {
            self.behavioral_facts.push(BehavioralFact::ResultAsserted);
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    /// Discarded-`Result` recognition for `no-op-io` (scrap-rs#25).
    ///
    /// Recognises the `let _ = <expr>;` shape — a bare wildcard binding
    /// (NOT `let _: T = ...;`, which is an intentional explicit binding)
    /// whose initializer is a `Result`-shaped expr per
    /// [`classify_discard_init`]. Projects
    /// [`BehavioralFact::ResultDiscarded`] with the matched
    /// [`ResultDiscardKind`].
    ///
    /// FP boundary lives in `classify_discard_init` (flat free fn,
    /// nesting 0, to keep this override's cognitive complexity low for
    /// the CRAP gate): literals, paths, macros, control-flow exprs, and
    /// panic-chain-terminated chains (`x.unwrap()` → `ResultAsserted`
    /// instead) all return `None`.
    ///
    /// **DOES** call `visit::visit_local(self, local)` — discards can
    /// nest inside `if`/`match` arms and inner blocks; recursion finds
    /// every one. Same-kind discards are deduped at **projection**
    /// (scrap-rs#112 `Vec` storage): the push is skipped when an equal
    /// `ResultDiscarded { kind }` is already recorded, so two `Call`
    /// discards collapse to one entry while a `Call` + `ResultCtor`
    /// pair both survive.
    fn visit_local(&mut self, local: &'ast syn::Local) {
        // Bare `_` only — `let _: T = ...;` (Pat::Type) is an intentional
        // explicit binding and never projects (hard FP guard, scrap-rs#25).
        if matches!(local.pat, syn::Pat::Wild(_))
            && let Some(init) = &local.init
            && let Some(kind) = classify_discard_init(&init.expr)
        {
            let fact = BehavioralFact::ResultDiscarded { kind };
            if !self.behavioral_facts.iter().any(|f| f == &fact) {
                self.behavioral_facts.push(fact);
            }
        }
        syn::visit::visit_local(self, local);
    }
}

/// Classify the initializer of a `let _ = <expr>;` discard into a
/// [`ResultDiscardKind`], or `None` for shapes that must NOT project a
/// discard fact.
///
/// **Flat by design** (nesting 0, simple match arms) so its cognitive
/// complexity stays low for the CRAP gate; every arm is covered by a
/// unit test so the `(1 − coverage)³` CRAP penalty collapses to ~0.
///
/// Projects:
/// - [`ResultDiscardKind::ResultCtor`] — `Ok(..)` / `Err(..)` call.
/// - [`ResultDiscardKind::ResultAdapter`] — `x.ok()` / `x.err()` method.
/// - [`ResultDiscardKind::Call`] — any other function or method call.
///
/// Transparent wrappers are unwrapped by recursing into the inner expr:
/// - `Await` — `let _ = foo().await;` is textbook no-op-io: the awaited
///   future genuinely ran and its `Result` was dropped. Classify by the
///   awaited base's shape. (NB: only the AWAITED form projects; a bare
///   un-awaited `let _ = some_future();` is classified by its own
///   `Expr::Call` shape as `Call` — recognising un-awaited futures as
///   darkness needs type info we lack in v0.1, and is scrap-rs#98's job.)
/// - `Paren` — `let _ = (foo());` recurses to the inner expr, so a
///   parenthesised literal (`(5)`) still resolves to `None`.
///
/// Returns `None` (do NOT project) for:
/// - panic-chain-terminated method calls (`x.unwrap()` / `.expect(..)` /
///   `.unwrap_err()` / `.expect_err(..)`) — these project
///   [`BehavioralFact::ResultAsserted`] via `visit_expr_method_call`, so
///   projecting a discard too would be a contradictory double-classify;
/// - every non-call shape: literals, paths/idents, macros (`vec![]`),
///   tuples, `if`/`match`/block exprs, references, etc. `#[non_exhaustive]`
///   on `ResultDiscardKind` is the forward-compat hatch — there is no
///   catch-all `Other` kind (matches `ParseDiagnosticKind` discipline).
fn classify_discard_init(expr: &syn::Expr) -> Option<ResultDiscardKind> {
    match expr {
        // `Ok(..)` / `Err(..)` constructor call → ResultCtor.
        syn::Expr::Call(call) if call_func_is_result_ctor(&call.func) => {
            Some(ResultDiscardKind::ResultCtor)
        }
        // Panic-chain method terminal (`x.unwrap()` / `.expect(..)` /
        // `*_err`) → None: `visit_expr_method_call` owns it as
        // ResultAsserted, so projecting a discard too would be a
        // contradictory double-classify.
        syn::Expr::MethodCall(mc) if method_is_panic_chain(&mc.method) => None,
        // `.ok()` / `.err()` Result↔Option adapter → ResultAdapter.
        syn::Expr::MethodCall(mc) if method_is_result_adapter(&mc.method) => {
            Some(ResultDiscardKind::ResultAdapter)
        }
        // Any other free-function or method call → Call.
        syn::Expr::Call(_) | syn::Expr::MethodCall(_) => Some(ResultDiscardKind::Call),
        // Transparent wrappers: classify by the inner expr (Gemini C1).
        syn::Expr::Await(a) => classify_discard_init(&a.base),
        syn::Expr::Paren(p) => classify_discard_init(&p.expr),
        // Everything else (literal, path, macro, tuple, control-flow,
        // reference, ...) is not a discarded-Result shape.
        _ => None,
    }
}

/// `true` when a call's func path leaf is `Ok` or `Err` (the `Result`
/// constructors). Only inspects the LEAF segment so `core::result::Result::Ok`
/// and a bare `Ok` both match.
fn call_func_is_result_ctor(func: &syn::Expr) -> bool {
    if let syn::Expr::Path(expr_path) = func
        && let Some(seg) = expr_path.path.segments.last()
    {
        return seg.ident == "Ok" || seg.ident == "Err";
    }
    false
}

/// `true` for the panic-chain method idents (delegates to the same
/// [`PANIC_CHAIN_METHOD_NAMES`] set the `ResultAsserted` projection uses,
/// so the two stay in lock-step).
fn method_is_panic_chain(method: &syn::Ident) -> bool {
    PANIC_CHAIN_METHOD_NAMES.iter().any(|&name| method == name)
}

/// `true` for the `Result`↔`Option` adapter methods `.ok()` / `.err()`.
fn method_is_result_adapter(method: &syn::Ident) -> bool {
    method == "ok" || method == "err"
}

/// Detect whether an `&Expr` (the receiver of an `.await`) is a
/// cucumber chain we should project as `AssertionSource::Cucumber`.
///
/// Two canonical shapes recognised:
/// 1. `World::cucumber().run("tests").await` — the AST is
///    `Expr::Await { base: MethodCall(method: "run", receiver:
///    Call(World::cucumber)) }`. We descend into the `MethodCall`'s
///    receiver and match the Call whose func path's last segment is
///    `"cucumber"`.
/// 2. `cucumber::Cucumber::run(...).await` — the AST is
///    `Expr::Await { base: Call(func: cucumber::Cucumber::run) }`.
///    We match the Call whose func path contains a segment named
///    `"cucumber"` (anywhere in the path; this catches the namespace
///    form).
///
/// Pinned by unit tests below:
/// - Positive: `World::cucumber().run("tests/features")` (form 1),
///   `cucumber::Cucumber::run(...)` (form 2).
/// - Negative: `futures::future::ready(())`,
///   `World::cucumber()` (chain head, no `.run().await` terminal),
///   `tokio::time::sleep(...)`.
fn is_cucumber_chain(base: &syn::Expr) -> bool {
    match base {
        syn::Expr::MethodCall(mc) => {
            // Form 1 chain: this MethodCall's receiver is the
            // `World::cucumber()` Call. Descend.
            is_cucumber_chain(&mc.receiver)
        }
        syn::Expr::Call(call) => {
            // Match if the Call's func path is `World::cucumber`
            // (form 1 chain root) OR contains a segment named
            // `cucumber` anywhere (form 2: `cucumber::Cucumber::run`).
            if let syn::Expr::Path(expr_path) = call.func.as_ref() {
                return expr_path
                    .path
                    .segments
                    .iter()
                    .any(|s| s.ident == "cucumber");
            }
            false
        }
        // Other Expr variants don't extend a chain in ways that
        // matter for cucumber recognition.
        _ => false,
    }
}

/// Convert a macro's `tokens` to `Option<String>` for `raw_args`.
///
/// Empty token streams (the `assert!()` form, vanishingly rare in
/// real test code) project to `None` for wire-format efficiency
/// (`raw_args` is `#[serde(skip_serializing_if = "Option::is_none")]`).
/// Non-empty streams stringify via `TokenStream::Display` — the
/// whitespace fidelity is documented as load-bearing for `raw_args`
/// (contrast with `compose_macro_path_string` where whitespace is
/// stripped).
fn stringify_tokens_opt(tokens: &proc_macro2::TokenStream) -> Option<String> {
    let s = tokens.to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::ItemFn;

    fn parse_test_fn(source: &str) -> ItemFn {
        let file: syn::File = syn::parse_str(source).expect("source parses");
        file.items
            .into_iter()
            .find_map(|i| {
                if let syn::Item::Fn(f) = i {
                    Some(f)
                } else {
                    None
                }
            })
            .expect("source contains an fn")
    }

    #[test]
    fn body_visitor_recovers_single_assert() {
        let item = parse_test_fn("fn t() { assert!(true); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert");
        assert_eq!(visitor.assertions[0].raw_args, Some("true".to_string()));
    }

    #[test]
    fn body_visitor_recovers_assert_eq_with_args() {
        let item = parse_test_fn("fn t() { assert_eq!(1, 1); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert_eq");
        // proc-macro2 `TokenStream::Display` stringifies with a
        // single space between comma-separated args.
        assert_eq!(visitor.assertions[0].raw_args, Some("1 , 1".to_string()));
    }

    #[test]
    fn body_visitor_recovers_multiple_assertions_in_order() {
        let item = parse_test_fn("fn t() { assert!(true); assert_eq!(1, 1); assert_ne!(2, 3); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 3);
        let names: Vec<&str> = visitor.assertions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["assert", "assert_eq", "assert_ne"]);
    }

    #[test]
    fn body_visitor_recognises_panic_unimplemented_todo() {
        let item = parse_test_fn("fn t() { panic!(); unimplemented!(); todo!(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        let names: Vec<&str> = visitor.assertions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["panic", "unimplemented", "todo"]);
    }

    #[test]
    fn body_visitor_leaf_segment_match_for_namespaced_assert_eq() {
        // `pretty_assertions::assert_eq` matches `"assert_eq"` via
        // leaf-segment match here AND is recognised as
        // `AssertionSource::PrettyAssertions` via the recognise()
        // branch (see `body_visitor_recognises_pretty_assertions_as_both`).
        let item = parse_test_fn("fn t() { pretty_assertions::assert_eq!(1, 1); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert_eq");
    }

    #[test]
    fn body_visitor_no_assertion_in_empty_body_produces_empty() {
        let item = parse_test_fn("fn t() {}");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.assertions.is_empty());
    }

    #[test]
    fn body_visitor_ignores_non_assertion_macros() {
        // `println!` and `vec!` are not in the v0.1 assertion set;
        // they must not produce ParsedAssertion entries.
        let item = parse_test_fn("fn t() { println!(\"hi\"); let _ = vec![1, 2]; }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.assertions.is_empty());
    }

    #[test]
    fn body_visitor_recovers_nested_assertion_in_if_branch() {
        // Visit's default recursion walks into Expr::If; the
        // assertion macro inside the arm still surfaces.
        let item = parse_test_fn("fn t() { if true { assert!(true); } }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert");
    }

    #[test]
    fn body_visitor_empty_args_macro_produces_none_raw_args() {
        let item = parse_test_fn("fn t() { panic!(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.assertions[0].name, "panic");
        assert_eq!(visitor.assertions[0].raw_args, None);
    }

    // ─── visit_macro implicit-source recognition ────────────────────

    #[test]
    fn body_visitor_recognises_proptest_macro() {
        let item = parse_test_fn("fn t() { proptest! { |(x in any::<u32>())| { let _ = x; } } }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Proptest]
        );
        // proptest! is not in the explicit set; assertions stays empty.
        assert!(visitor.assertions.is_empty());
    }

    #[test]
    fn body_visitor_recognises_kani_macro() {
        let item = parse_test_fn("fn t() { let x: u32 = kani::any!(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Kani]
        );
    }

    #[test]
    fn body_visitor_recognises_insta_assert_snapshot() {
        let item = parse_test_fn("fn t() { insta::assert_snapshot!(\"rendered\"); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Insta]
        );
    }

    #[test]
    fn body_visitor_recognises_pretty_assertions_as_both() {
        // The load-bearing dual-recognition case: `pretty_assertions::assert_eq`
        // matches the leaf-segment explicit assertion branch (→
        // ParsedAssertion("assert_eq")) AND the recognise() prefix
        // branch (→ AssertionSource::PrettyAssertions). Both must
        // populate — they're two different facts the detector layer
        // consumes independently.
        let item = parse_test_fn("fn t() { pretty_assertions::assert_eq!(1, 1); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert_eq");
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::PrettyAssertions]
        );
    }

    #[test]
    fn body_visitor_recognises_suffix_proptest() {
        // The `*_proptest` suffix rule from recognise() — custom
        // proptest-derived macros (e.g. `my_proptest!`) project to
        // `Proptest`.
        let item = parse_test_fn("fn t() { my_proptest! { x in 0..10 } }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Proptest]
        );
    }

    #[test]
    fn body_visitor_skips_non_implicit_macros() {
        // `vec!` and `println!` are neither explicit assertions nor
        // implicit-source macros. Both vecs stay empty.
        let item = parse_test_fn("fn t() { let _ = vec![1, 2]; println!(\"hi\"); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.assertions.is_empty());
        assert!(visitor.implicit_assertion_sources.is_empty());
    }

    // ─── visit_expr_await / visit_expr_call / is_cucumber_chain ─────

    fn parse_expr(source: &str) -> syn::Expr {
        syn::parse_str(source).expect("expr parses")
    }

    #[test]
    fn is_cucumber_chain_positive_world_cucumber_run() {
        // The AST `World::cucumber().run("x")` has the chain shape:
        // MethodCall(method: run, receiver: Call(World::cucumber)).
        // After the parent `.await` strips itself off, `is_cucumber_chain`
        // sees the MethodCall and descends into the receiver Call.
        let expr = parse_expr("World::cucumber().run(\"x\")");
        assert!(is_cucumber_chain(&expr));
    }

    #[test]
    fn is_cucumber_chain_positive_cucumber_cucumber_run() {
        // `cucumber::Cucumber::run(...)` is a single Call to a path
        // containing `cucumber` segments. The path-segment match
        // catches it.
        let expr = parse_expr("cucumber::Cucumber::run(world)");
        assert!(is_cucumber_chain(&expr));
    }

    #[test]
    fn is_cucumber_chain_negative_non_cucumber_await_receiver() {
        let expr = parse_expr("futures::future::ready(())");
        assert!(!is_cucumber_chain(&expr));
    }

    #[test]
    fn is_cucumber_chain_negative_tokio_sleep() {
        let expr = parse_expr("tokio::time::sleep(d)");
        assert!(!is_cucumber_chain(&expr));
    }

    #[test]
    fn body_visitor_recognises_cucumber_await_chain() {
        // The full integration: a test body whose body invokes the
        // canonical cucumber chain. `visit_expr_await` fires;
        // `is_cucumber_chain` matches; recognise("cucumber::run")
        // returns `Cucumber`.
        let item =
            parse_test_fn("async fn it() { World::cucumber().run(\"tests/features\").await; }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Cucumber]
        );
    }

    #[test]
    fn body_visitor_does_not_fire_on_non_cucumber_await() {
        let item = parse_test_fn(
            "async fn it() { tokio::time::sleep(std::time::Duration::from_secs(0)).await; }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.implicit_assertion_sources.is_empty());
    }

    #[test]
    fn body_visitor_recognises_quickcheck_function_call() {
        // `quickcheck::quickcheck(prop)` — Expr::Call with func path
        // matching the exact-key rule in recognise().
        let item = parse_test_fn("fn it() { quickcheck::quickcheck(prop); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Quickcheck]
        );
    }

    #[test]
    fn body_visitor_recognises_trybuild_function_call() {
        // `trybuild::TestCases::new()` — Expr::Call with path
        // matching the `trybuild::TestCases::*` prefix rule.
        let item = parse_test_fn("fn it() { trybuild::TestCases::new(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Trybuild]
        );
    }

    // ─── visit_expr_method_call (BehavioralFact::ResultAsserted) ────────

    /// Expected `behavioral_facts` shape: only `ResultAsserted`. Inlined
    /// at each call site (instead of factored into the
    /// `assert_only_result_asserted` helper alone) so the zero-assertion
    /// self-check (`crates/scrap4rs/tests/self_check.rs`) sees a direct
    /// macro-level assertion at the leaf and doesn't fire on the test
    /// — helper-delegated assertions are invisible to the parser per
    /// the v0.1 zero-assertion test-helper-delegation discovery
    /// (scrap-rs#30 issue body).
    fn expected_only_result_asserted() -> Vec<BehavioralFact> {
        vec![BehavioralFact::ResultAsserted]
    }

    #[test]
    fn body_visitor_recognises_unwrap_chain() {
        // `let _ = x.unwrap();` — Expr::MethodCall with method ident "unwrap".
        let item = parse_test_fn("fn it() { let x: Result<u32, ()> = Ok(1); let _ = x.unwrap(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_recognises_expect_chain() {
        // `let _ = x.expect("msg");` — Expr::MethodCall with method ident "expect".
        let item =
            parse_test_fn("fn it() { let x: Option<u32> = Some(1); let _ = x.expect(\"msg\"); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_recognises_nested_method_calls() {
        // `let _ = foo().bar().unwrap();` — the visit recurses through
        // the receiver chain and still fires on the terminal .unwrap().
        let src = "fn it() { let _ = std::iter::repeat(1u32).take(1).next().unwrap(); }";
        let item = parse_test_fn(src);
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_recognises_unwrap_err_chain() {
        // `let _ = x.unwrap_err();` — canonical "assert this is Err" idiom
        // on Result. v0.1 recognises the error-path siblings to .unwrap()
        // alongside the happy path (gemini MEDIUM 2026-05-27 fold-in).
        let item =
            parse_test_fn("fn it() { let x: Result<u32, ()> = Err(()); let _ = x.unwrap_err(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_recognises_expect_err_chain() {
        // `let _ = x.expect_err("msg");` — the message-carrying error-path
        // sibling to `.expect(...)`. Recognised by the v0.1 panic-chain
        // ident set (gemini MEDIUM 2026-05-27 fold-in).
        let item = parse_test_fn(
            "fn it() { let x: Result<u32, ()> = Err(()); let _ = x.expect_err(\"expected Err\"); }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_dedupes_multiple_unwrap_chains() {
        // Two `.unwrap()` calls → one fact entry. With `Vec` storage
        // (scrap-rs#112) the presence-fact dedup is enforced at
        // projection (the guarded push in `visit_expr_method_call`), not
        // by `BTreeSet` set-admission; the wire carries exactly one
        // `result_asserted`.
        let item = parse_test_fn(
            "fn it() { let x: Result<u32, ()> = Ok(1); let y: Result<u32, ()> = Ok(2); let _ = x.unwrap(); let _ = y.unwrap(); }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts.len(), 1);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_distinct_unwrap_chains_project_one_result_asserted() {
        // scrap-rs#112 projection-dedup pin: two SYNTACTICALLY DISTINCT
        // panic-chain terminals (`a.unwrap()` then `b.expect(..)`) both
        // map to the SAME presence fact `ResultAsserted`. With `Vec`
        // storage the guarded push must collapse them to exactly ONE
        // entry — proving dedup survives the BTreeSet→Vec migration even
        // when the two source idents differ.
        let item = parse_test_fn(
            "fn it() { let a: Result<u32, ()> = Ok(1); let b: Option<u32> = Some(2); let _ = a.unwrap(); let _ = b.expect(\"two\"); }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.behavioral_facts,
            vec![BehavioralFact::ResultAsserted],
            "two distinct panic-chain terminals project exactly one ResultAsserted",
        );
    }

    #[test]
    fn body_visitor_empty_body_no_behavioral_facts() {
        let item = parse_test_fn("fn it() {}");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.behavioral_facts.is_empty());
    }

    #[test]
    fn body_visitor_unrelated_method_calls_do_not_fire_result_asserted() {
        // `.push(1)` and `.len()` are not panic-chain idents → no
        // ResultAsserted. But `let _ = v.len();` is a bare-wildcard
        // discard of a method call → ResultDiscarded { Call } (scrap-rs#25
        // — the v0.1 over-fire: `Call` fires on any discarded call, not
        // just I/O; see no_op_io module docs). The `v.push(1)` statement
        // call is NOT bound by `let _`, so it does not project.
        let item = parse_test_fn(
            "fn it() { let mut v: Vec<u32> = Vec::new(); v.push(1); let _ = v.len(); }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(
            !visitor
                .behavioral_facts
                .contains(&BehavioralFact::ResultAsserted),
            "`.len()`/`.push()` are not panic-chain idents — no ResultAsserted",
        );
        assert_eq!(
            visitor.behavioral_facts,
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    // ─── visit_local (BehavioralFact::ResultDiscarded) ──────────────────

    /// Drive a body fragment and return its `behavioral_facts`.
    fn facts_of(source: &str) -> Vec<BehavioralFact> {
        let item = parse_test_fn(source);
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        visitor.behavioral_facts
    }

    #[test]
    fn discard_of_call_projects_call_kind() {
        // `let _ = compute();` — bare wildcard discard of a free-fn call.
        assert_eq!(
            facts_of("fn it() { let _ = compute(); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    #[test]
    fn discard_of_method_call_projects_call_kind() {
        // `let _ = obj.do_thing();` — bare wildcard discard of a method
        // call (non-panic-chain, non-adapter) → Call.
        assert_eq!(
            facts_of("fn it() { let obj = Thing; let _ = obj.do_thing(); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    #[test]
    fn discard_of_ok_ctor_projects_result_ctor_kind() {
        assert_eq!(
            facts_of("fn it() { let _ = Ok::<u32, ()>(1); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::ResultCtor,
            }],
        );
    }

    #[test]
    fn discard_of_err_ctor_projects_result_ctor_kind() {
        assert_eq!(
            facts_of("fn it() { let _ = Err::<(), u32>(1); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::ResultCtor,
            }],
        );
    }

    #[test]
    fn discard_of_ok_adapter_projects_result_adapter_kind() {
        // `let _ = x.ok();` — Option↔Result adapter.
        assert_eq!(
            facts_of("fn it() { let x: Result<u32, ()> = Ok(1); let _ = x.ok(); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::ResultAdapter,
            }],
        );
    }

    #[test]
    fn discard_of_err_adapter_projects_result_adapter_kind() {
        assert_eq!(
            facts_of("fn it() { let x: Result<u32, ()> = Ok(1); let _ = x.err(); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::ResultAdapter,
            }],
        );
    }

    // ── FP guards: shapes that MUST NOT project a ResultDiscarded ───────

    #[test]
    fn type_ascribed_unit_discard_does_not_project() {
        // `let _: () = foo();` — Pat::Type, intentional must-use silencer.
        // HARD FP GUARD (scrap-rs#25 AC).
        assert!(facts_of("fn it() { let _: () = foo(); }").is_empty());
    }

    #[test]
    fn type_ascribed_non_unit_discard_does_not_project() {
        // `let _: T = foo();` — any type ascription is an explicit binding.
        assert!(facts_of("fn it() { let _: u32 = foo(); }").is_empty());
    }

    #[test]
    fn question_mark_propagation_does_not_project() {
        // `foo()?;` is Expr::Try, not a `let _ =` local. Early-return, not
        // a discard.
        assert!(facts_of("fn it() -> Result<(), ()> { foo()?; Ok(()) }").is_empty());
    }

    #[test]
    fn named_binding_of_try_does_not_project() {
        // `let x = foo()?;` binds a named pattern → not Pat::Wild.
        assert!(facts_of("fn it() -> Result<(), ()> { let x = foo()?; Ok(()) }").is_empty());
    }

    #[test]
    fn discard_of_literal_does_not_project() {
        assert!(facts_of("fn it() { let _ = 5; }").is_empty());
    }

    #[test]
    fn discard_of_path_does_not_project() {
        assert!(facts_of("fn it() { let x = 1; let _ = x; }").is_empty());
    }

    #[test]
    fn discard_of_macro_does_not_project() {
        // `vec![1, 2]` is Expr::Macro, not a call.
        assert!(facts_of("fn it() { let _ = vec![1, 2]; }").is_empty());
    }

    #[test]
    fn discard_of_tuple_does_not_project() {
        assert!(facts_of("fn it() { let _ = (1, 2); }").is_empty());
    }

    #[test]
    fn discard_of_reference_does_not_project() {
        assert!(facts_of("fn it() { let x = 1; let _ = &x; }").is_empty());
    }

    #[test]
    fn discard_of_awaited_call_projects_call_kind() {
        // `let _ = foo().await;` — the awaited future genuinely ran and
        // its Result was dropped (textbook no-op-io). Expr::Await
        // recurses into the base call → Call (Gemini C1). `.await`
        // requires an async fn to parse.
        assert_eq!(
            facts_of("async fn it() { let _ = fetch().await; }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    #[test]
    fn discard_of_parenthesized_call_projects_call_kind() {
        // `let _ = (foo());` — Expr::Paren recurses to the inner call.
        assert_eq!(
            facts_of("fn it() { let _ = (compute()); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    #[test]
    fn discard_of_parenthesized_literal_does_not_project() {
        // `let _ = (5);` — Paren recurses to a literal → None (FP guard
        // holds through the transparent wrapper).
        assert!(facts_of("fn it() { let _ = (5); }").is_empty());
    }

    #[test]
    fn discard_of_awaited_unwrap_chain_projects_result_asserted_only() {
        // `let _ = foo().await.unwrap();` — the await wraps a panic-chain
        // terminal: classify_discard_init recurses Await → MethodCall
        // (unwrap) → None, so NO ResultDiscarded; the inner .unwrap()
        // still projects ResultAsserted via visit_expr_method_call.
        assert_eq!(
            facts_of("async fn it() { let _ = fetch().await.unwrap(); }"),
            vec![BehavioralFact::ResultAsserted],
        );
    }

    #[test]
    fn discard_of_unwrap_chain_projects_result_asserted_only() {
        // `let _ = x.unwrap();` — panic-chain terminal. ResultAsserted is
        // projected by visit_expr_method_call; classify_discard_init
        // returns None so NO contradictory ResultDiscarded is added.
        assert_eq!(
            facts_of("fn it() { let x: Result<u32, ()> = Ok(1); let _ = x.unwrap(); }"),
            vec![BehavioralFact::ResultAsserted],
        );
    }

    #[test]
    fn discard_nested_in_if_arm_projects() {
        // visit_local recursion finds discards inside control-flow arms.
        assert_eq!(
            facts_of("fn it() { if true { let _ = compute(); } }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    #[test]
    fn two_same_kind_discards_dedupe() {
        // Projection-time dedup (scrap-rs#112 `Vec` storage) — two
        // Call-kind discards → one fact entry. No per-fact line is
        // carried, so the two equal `ResultDiscarded { Call }` facts
        // collapse via the guarded push in `visit_local`.
        assert_eq!(
            facts_of("fn it() { let _ = a(); let _ = b(); }"),
            vec![BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::Call,
            }],
        );
    }

    #[test]
    fn two_different_kind_discards_both_project() {
        // A Call discard and a ResultCtor discard → two distinct facts.
        assert_eq!(
            facts_of("fn it() { let _ = a(); let _ = Ok::<u32, ()>(1); }"),
            vec![
                BehavioralFact::ResultDiscarded {
                    kind: ResultDiscardKind::Call,
                },
                BehavioralFact::ResultDiscarded {
                    kind: ResultDiscardKind::ResultCtor,
                },
            ],
        );
    }
}
