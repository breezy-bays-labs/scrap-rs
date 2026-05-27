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
use scrap_core::domain::behavioral_fact::BehavioralFact;
use scrap_core::domain::parsed::ParsedAssertion;
use std::collections::BTreeSet;
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
    /// `visit_expr_method_call` (method-call sources: `.unwrap()`,
    /// `.expect(...)`). `BTreeSet` dedupes naturally — multiple
    /// `.unwrap()` calls in the same body produce one fact entry.
    /// Mirrors the
    /// [`scrap_core::domain::parsed::ParsedTest::behavioral_facts`]
    /// storage shape.
    pub(crate) behavioral_facts: BTreeSet<BehavioralFact>,
}

impl BodyVisitor {
    pub(crate) fn new() -> Self {
        Self {
            assertions: Vec::new(),
            implicit_assertion_sources: Vec::new(),
            behavioral_facts: BTreeSet::new(),
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
            self.assertions
                .push(ParsedAssertion::new(leaf, raw_args, span));
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
    /// The `BTreeSet` dedupes naturally — multiple panic-chain calls in
    /// the same body produce one fact entry.
    ///
    /// **DOES** call `visit::visit_expr_method_call(self, node)` —
    /// method-call chains nest naturally (`x.unwrap().unwrap()`), and
    /// recursion finds every fact in the body. (Per-call dedup is
    /// already provided by `BTreeSet`.)
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
        {
            self.behavioral_facts.insert(BehavioralFact::ResultAsserted);
        }
        syn::visit::visit_expr_method_call(self, node);
    }
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
    fn expected_only_result_asserted() -> BTreeSet<BehavioralFact> {
        [BehavioralFact::ResultAsserted].into()
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
        // Two `.unwrap()` calls → one fact entry (BTreeSet dedupes).
        let item = parse_test_fn(
            "fn it() { let x: Result<u32, ()> = Ok(1); let y: Result<u32, ()> = Ok(2); let _ = x.unwrap(); let _ = y.unwrap(); }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.behavioral_facts.len(), 1);
        assert_eq!(visitor.behavioral_facts, expected_only_result_asserted());
    }

    #[test]
    fn body_visitor_empty_body_no_behavioral_facts() {
        let item = parse_test_fn("fn it() {}");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.behavioral_facts.is_empty());
    }

    #[test]
    fn body_visitor_ignores_unrelated_method_calls() {
        // `.push(1)` and `.len()` are not `.unwrap()`/`.expect()`;
        // they must NOT fire BehavioralFact::ResultAsserted.
        let item = parse_test_fn(
            "fn it() { let mut v: Vec<u32> = Vec::new(); v.push(1); let _ = v.len(); }",
        );
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.behavioral_facts.is_empty());
    }
}
