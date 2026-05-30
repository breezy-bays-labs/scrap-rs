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

use std::collections::{HashMap, HashSet};

use scrap_core::domain::assertion_sources::{AssertionSource, recognise};
use scrap_core::domain::behavioral_fact::{
    BehavioralFact, FsCallKind, FsReadKind, FsSurfaceCheckKind, ResultDiscardKind,
};
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
    ///
    /// The located filesystem facts (scrap-rs#26 —
    /// [`BehavioralFact::FilesystemWrite`] / `FilesystemSurfaceCheck` /
    /// `FilesystemRead`) are **NOT** deduped: each is a distinct located
    /// event (two writes to two keys are two facts), so their pushes are
    /// unguarded. Mirrors the
    /// [`scrap_core::domain::parsed::ParsedTest::behavioral_facts`]
    /// storage shape.
    pub(crate) behavioral_facts: Vec<BehavioralFact>,
    /// Light, single-pass intra-body binding map: `ident → path_key`,
    /// built by a forward scan of the body's `let` statements
    /// (`visit_local` records each binding before recursing). Used to
    /// resolve a call-site path argument to a stable `path_key` so the
    /// `surface-only-io` correlation can group write/check/read facts.
    ///
    /// **Fail-safe correlation (poison set, scrap-rs#26 cabinet
    /// CRITICAL #1):** a name is only correlatable while it is *provably
    /// singly-bound and not `mut`*. A name that is rebound (in ANY form —
    /// re-`let`, shadow, `for p in`, tuple-destructure, `if let`, match
    /// arm, closure param, …), reassigned (`p = ...` / `p += ...`), or
    /// declared `mut` is **poisoned**: at a call site it resolves to a
    /// FRESH `opaque:<N>` key (never `bind:<name>`), so a write to its
    /// pre-rebind value can never correlate with a check on its
    /// post-rebind value. The poison decision is made by a cheap pre-pass
    /// ([`PoisonScanner`]) run before the main walk. Unknown binding forms
    /// default to poisoned (miss, never misfire). See [`Self::poisoned`].
    ///
    /// **Limits** (richer dataflow is a v0.3+ follow-up): no field paths
    /// (`self.tmp.path()` is opaque); no interprocedural resolution; no
    /// `format!`/`concat!` reduction — each of those resolves to a
    /// DISTINCT `opaque:<N>` key so it can never spuriously correlate with
    /// another unresolved path. Bindings introduced INSIDE a non-assertion
    /// macro are not poison-tracked, but the main walk also never projects
    /// facts from such a name, so no false positive arises.
    fs_bindings: HashMap<String, String>,
    /// Names that are NOT safe to correlate (see [`Self::fs_bindings`]).
    /// Populated once by [`PoisonScanner`] before the main walk; a
    /// poisoned ident always resolves to a fresh `opaque:<N>` key.
    poisoned: HashSet<String>,
    /// Monotonic counter for `opaque:<N>` keys — one per unresolvable
    /// path-argument site, so two opaque sites never share a key.
    opaque_counter: usize,
    /// Monotonic counter for `tempfile-handle:<N>` keys — one per
    /// `NamedTempFile::new()` / `tempfile()` binding.
    tempfile_counter: usize,
}

impl BodyVisitor {
    pub(crate) fn new() -> Self {
        Self {
            assertions: Vec::new(),
            implicit_assertion_sources: Vec::new(),
            behavioral_facts: Vec::new(),
            fs_bindings: HashMap::new(),
            poisoned: HashSet::new(),
            opaque_counter: 0,
            tempfile_counter: 0,
        }
    }

    /// Drive the walk over a test fn's `&syn::Block`. Wrapper over
    /// `visit_block` so the caller doesn't have to import the Visit
    /// trait.
    ///
    /// Runs the binding-poison pre-pass ([`PoisonScanner`]) FIRST so the
    /// main walk's path-key resolution can consult the poison set on the
    /// very first call site (the scan must complete before any resolution
    /// happens — a forward-only scan would miss a rebind that occurs
    /// after the first use).
    pub(crate) fn drive(&mut self, block: &Block) {
        self.poisoned = PoisonScanner::scan(block);
        self.visit_block(block);
    }
}

/// Pre-pass that computes the set of poisoned names for one test body.
///
/// Fail-safe (allowlist) design per scrap-rs#26 cabinet CRITICAL #1:
/// rather than enumerate every rebind FORM (a denylist that always misses
/// one), it counts `Pat::Ident` leaves — EVERY pattern binding form
/// (tuple-`let (a, p)`, struct/slice destructure, `for p in`, `if let` /
/// `while let`, match arms, closure params, `x @ subpat`) reduces to
/// `Pat::Ident` leaves that `syn`'s default `Visit` recursion reaches, so
/// one override (`visit_pat_ident`) covers the open-ended set for free.
///
/// A name is poisoned when ANY of:
/// 1. it is bound (as a `Pat::Ident`) **two or more times** in the body
///    — catches re-`let`, shadow, for-loop collision, tuple rebind, …;
/// 2. any of its bindings carries `mut` (`let mut p` / `ref mut p`) —
///    `mut` is the prerequisite for reassignment in compiled code;
/// 3. it appears as the **target of an assignment** (`p = ...` /
///    `p += ...`) — the only non-pattern rebind, a closed set
///    (`Expr::Assign` plus the compound-assign `Expr::Binary` ops),
///    needed because the parser also sees *uncompiled* fixtures where a
///    reassignment can lack `mut`.
struct PoisonScanner {
    /// Count of `Pat::Ident` bindings seen per name.
    bind_counts: HashMap<String, usize>,
    /// Names directly poisoned by a `mut` binding or an assignment target.
    poisoned: HashSet<String>,
}

impl PoisonScanner {
    /// Scan `block` and return the poisoned-name set.
    fn scan(block: &Block) -> HashSet<String> {
        let mut scanner = Self {
            bind_counts: HashMap::new(),
            poisoned: HashSet::new(),
        };
        scanner.visit_block(block);
        // A name bound 2+ times is poisoned (rebind / shadow / collision).
        for (name, count) in &scanner.bind_counts {
            if *count >= 2 {
                scanner.poisoned.insert(name.clone());
            }
        }
        scanner.poisoned
    }
}

impl<'ast> Visit<'ast> for PoisonScanner {
    fn visit_pat_ident(&mut self, pi: &'ast syn::PatIdent) {
        let name = pi.ident.to_string();
        *self.bind_counts.entry(name.clone()).or_insert(0) += 1;
        // A `mut` (or `ref mut`) binding is poison: reassignment-capable.
        if pi.mutability.is_some() {
            self.poisoned.insert(name);
        }
        syn::visit::visit_pat_ident(self, pi);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        // `p = ...` — the assigned target name is poison. Also covers a
        // compound-assign that syn models as `Expr::Assign` in 2.0.
        if let Some(name) = assign_target_ident(&node.left) {
            self.poisoned.insert(name);
        }
        syn::visit::visit_expr_assign(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        // Compound assignment (`p += ...`, `p *= ...`, ...) is an
        // `Expr::Binary` with an `*Assign` op in syn 2.0; poison the LHS.
        if is_compound_assign(&node.op)
            && let Some(name) = assign_target_ident(&node.left)
        {
            self.poisoned.insert(name);
        }
        syn::visit::visit_expr_binary(self, node);
    }
}

/// `syn::parse::Parser`-shaped fn that parses ONE leading `Expr` and
/// consumes (ignores) any trailing tokens. Used as the
/// `walk_assertion_macro_args` fallback for `assert_matches!(scrutinee,
/// Pat if guard)`, whose pattern arg makes a whole-`Punctuated<Expr>`
/// parse fail; the scrutinee is always the leading expr.
fn parse_leading_expr(input: syn::parse::ParseStream) -> syn::Result<syn::Expr> {
    let expr: syn::Expr = input.parse()?;
    // Drain the rest of the stream so the trailing `, Pat if guard` does
    // not produce an "unexpected token" error that rejects the parse.
    input.parse::<proc_macro2::TokenStream>()?;
    Ok(expr)
}

/// Extract the bare-ident name from an assignment LHS (`p = ...`).
/// Returns `None` for field/index/tuple LHS (those aren't a single
/// correlatable name, so they need no poisoning here).
fn assign_target_ident(target: &syn::Expr) -> Option<String> {
    if let syn::Expr::Path(p) = target
        && p.qself.is_none()
        && p.path.segments.len() == 1
    {
        return Some(p.path.segments[0].ident.to_string());
    }
    None
}

/// `true` for the compound-assignment binary ops (`+=`, `-=`, `*=`, ...).
fn is_compound_assign(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

impl<'ast> Visit<'ast> for BodyVisitor {
    /// Recognise explicit assertion macros AND implicit-source
    /// macros. Whitespace-sensitive path stringification via
    /// `compose_macro_path_string` (NOT `quote!`/`TokenStream`) so
    /// `recognise()`'s exact-string lookups stay accurate.
    ///
    /// **v0.1 token-stream descent boundary (relaxed at scrap-rs#26 for
    /// assertion macros only):** the parser does NOT call the generic
    /// `visit::visit_macro(self, mac)`. For the RECOGNISED assertion
    /// macros ([`ASSERTION_MACRO_NAMES`]) it now best-effort parses the
    /// argument tokens and re-walks each arg through the existing
    /// overrides, so a filesystem call nested inside an assertion —
    /// `assert!(p.exists())`, `assert_eq!(fs::read_to_string(p)?, "x")` —
    /// projects its located fact. This is load-bearing for the
    /// `surface-only-io` correlation, whose canonical idioms put the
    /// surface check and the read-back INSIDE assertion macros. It
    /// extends the existing precedent that already parses assertion-macro
    /// tokens upstream (`super::tautology::extract_tautology_facts`).
    /// Non-assertion macros (`proptest!`, `vec!`, `println!`, `dbg!`) are
    /// still NOT descended into — a `dbg!(fs::read(p))` going unrecognised
    /// is an accepted v0.3+ note.
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

            // scrap-rs#26: descend into the assertion's argument exprs so
            // fs calls nested in the assertion project their located
            // facts. Best-effort: tokens that don't parse as a
            // comma-separated `Expr` list (e.g. `assert_matches!(x,
            // Some(_))` — the pattern arg isn't an `Expr`) are dropped.
            self.walk_assertion_macro_args(&mac.tokens);
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
            // scrap-rs#26: free-function filesystem calls. The recognised
            // path leaf (`write` / `create` / `read` / `open` / ...) keys
            // off the LAST segment so both `std::fs::write` and a bare
            // `write` (rare) match; the family disambiguates by segment.
            self.project_fs_call(&expr_path.path, call.args.first());
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
                .contains(&BehavioralFact::ResultAsserted)
        {
            self.behavioral_facts.push(BehavioralFact::ResultAsserted);
        }
        // scrap-rs#26: method-form filesystem facts.
        self.project_fs_method_call(node);
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
            if !self.behavioral_facts.contains(&fact) {
                self.behavioral_facts.push(fact);
            }
        }

        // scrap-rs#26 binding map: `let <ident> = <rhs>;` records
        // `ident → path_key` (forward scan; recorded BEFORE recursing so
        // a later statement's use resolves through it). A tempfile ctor on
        // the RHS additionally emits a `FilesystemWrite{Tempfile}` — the
        // temp file IS created on disk at construction — and binds the
        // ident to its `tempfile-handle:<N>` key so `f.path()` aliases back.
        if let Some(ident) = local_binding_ident(&local.pat)
            && let Some(init) = &local.init
        {
            self.record_fs_binding(&ident, &init.expr);
        }

        syn::visit::visit_local(self, local);
    }
}

impl BodyVisitor {
    /// Best-effort walk of a recognised assertion macro's argument exprs
    /// (scrap-rs#26). Re-drives each parsed arg through the visitor's
    /// existing overrides, so fs calls (and `.unwrap()` chains) nested
    /// inside the assertion project their facts.
    ///
    /// Two-tier parse (cabinet CRITICAL #2):
    /// 1. Parse the whole token stream as a comma-separated `Expr` list
    ///    (`assert!(e)` / `assert_eq!(a, b)`) and walk every arg.
    /// 2. If that fails — `assert_matches!(scrutinee, Pat if guard)`'s
    ///    second arg is a PATTERN, not an `Expr`, so the whole-list parse
    ///    fails — fall back to walking just the **leading expression**
    ///    (the scrutinee, always arg 0), ignoring the trailing tokens.
    ///    This captures `assert_matches!(fs::read_to_string(p)?, ..)`'s
    ///    read so a genuine read-back is NOT dropped (which would
    ///    false-fire surface-only-io).
    ///
    /// A dropped/unparseable arg can therefore never CAUSE a fire — at
    /// worst a fact is missed, never spuriously added. If even the
    /// leading-expr parse fails, nothing is projected (a miss, the
    /// fail-safe direction).
    ///
    /// Lifetime note: the parsed exprs are locally-owned, but
    /// `self.visit_expr(&e)` typechecks because `BodyVisitor` stores only
    /// owned facts (no `&'ast` borrows), so the `Visit<'ast>` lifetime
    /// unifies with the local borrow.
    fn walk_assertion_macro_args(&mut self, tokens: &proc_macro2::TokenStream) {
        use syn::parse::Parser as _;
        let full = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(args) = full.parse2(tokens.clone()) {
            for e in &args {
                syn::visit::Visit::visit_expr(self, e);
            }
            return;
        }
        // Fallback: parse only the leading `Expr` (the scrutinee) and
        // ignore everything after it. `parse_leading_expr` consumes the
        // rest of the stream so syn's "unexpected trailing tokens" error
        // doesn't reject the whole parse.
        if let Ok(expr) = parse_leading_expr.parse2(tokens.clone()) {
            syn::visit::Visit::visit_expr(self, &expr);
        }
    }

    /// Record a `let <ident> = <rhs>;` binding into [`Self::fs_bindings`].
    ///
    /// If the RHS (after unwrapping one `?` / `.unwrap()` / `.expect(..)`
    /// terminal) is a tempfile constructor, emit a
    /// `FilesystemWrite{Tempfile}` at the binding and map `ident →
    /// tempfile-handle:<N>`. Otherwise map `ident →
    /// resolve_path_key(rhs)` so a later `fs::write(<ident>, ..)` /
    /// `<ident>.exists()` resolves to the same key.
    fn record_fs_binding(&mut self, ident: &str, rhs: &syn::Expr) {
        if is_tempfile_ctor(unwrap_fallible_terminal(rhs)) {
            let key = self.fresh_tempfile_key();
            // Located write: the path-arg span is the ctor expression
            // itself (there is no separate path argument).
            self.behavioral_facts.push(BehavioralFact::FilesystemWrite {
                kind: FsCallKind::Tempfile,
                path_key: key.clone(),
                path_arg_span: span_from_spanned(rhs),
            });
            self.fs_bindings.insert(ident.to_string(), key);
        } else {
            let key = self.resolve_path_key(rhs);
            self.fs_bindings.insert(ident.to_string(), key);
        }
    }

    /// Mint a fresh `tempfile-handle:<N>` key.
    fn fresh_tempfile_key(&mut self) -> String {
        let key = format!("tempfile-handle:{}", self.tempfile_counter);
        self.tempfile_counter += 1;
        key
    }

    /// Mint a fresh `opaque:<N>` key — each unresolvable path-argument
    /// site gets a DISTINCT N so opaque keys never correlate.
    fn fresh_opaque_key(&mut self) -> String {
        let key = format!("opaque:{}", self.opaque_counter);
        self.opaque_counter += 1;
        key
    }

    /// Resolve a path-argument expression to a stable `path_key`.
    ///
    /// The SINGLE source of truth for path-key resolution: the binding
    /// RHS, every write/read path argument, AND every surface-check
    /// receiver all route through here, so a write-site key and a
    /// check-site key for the same path are byte-identical (correlation
    /// hinges on this). Unwraps ONE level of the transparent wrappers
    /// `&e`, `e.as_path()`, `e.as_ref()`, `Path::new(<lit>)`, then:
    /// - string/path literal → `lit:<value>`;
    /// - bare ident, **not poisoned** → `fs_bindings` lookup, else
    ///   `bind:<ident>`;
    /// - bare ident, **poisoned** (rebound / reassigned / `mut`) → a fresh
    ///   `opaque:<N>` (NEVER `bind:<name>` — a name-based fallback would
    ///   re-collide the pre- and post-rebind keys and false-fire; this is
    ///   the cabinet's T2 gate, scrap-rs#26 CRITICAL #1);
    /// - `f.path()` where `f` is a tempfile-handle binding → that handle;
    /// - anything else (`format!`, `concat!`, field path, method chain) →
    ///   a fresh `opaque:<N>`.
    fn resolve_path_key(&mut self, expr: &syn::Expr) -> String {
        match expr {
            // String/path literal → `lit:<value>`.
            syn::Expr::Lit(lit) => match &lit.lit {
                syn::Lit::Str(s) => format!("lit:{}", s.value()),
                // Non-string literal (byte string, int, ...) is not a path.
                _ => self.fresh_opaque_key(),
            },
            // Reference `&p` → recurse into `p`.
            syn::Expr::Reference(r) => self.resolve_path_key(&r.expr),
            // Parenthesised `(p)` → recurse.
            syn::Expr::Paren(p) => self.resolve_path_key(&p.expr),
            // `Path::new(<lit>)` → recurse into the single argument.
            syn::Expr::Call(call) if call_is_path_new(&call.func) => match call.args.first() {
                Some(arg) => self.resolve_path_key(arg),
                None => self.fresh_opaque_key(),
            },
            // Bare ident → poisoned names route to a fresh opaque key
            // (never correlatable); clean names resolve through the
            // binding map, else `bind:<ident>`.
            syn::Expr::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
                let ident = p.path.segments[0].ident.to_string();
                if self.poisoned.contains(&ident) {
                    self.fresh_opaque_key()
                } else {
                    self.fs_bindings
                        .get(&ident)
                        .cloned()
                        .unwrap_or_else(|| format!("bind:{ident}"))
                }
            }
            // `e.as_path()` / `e.as_ref()` → recurse into the receiver.
            // `f.path()` where `f` is a tempfile handle → that handle's key.
            syn::Expr::MethodCall(mc) => {
                if mc.method == "as_path" || mc.method == "as_ref" {
                    self.resolve_path_key(&mc.receiver)
                } else if mc.method == "path"
                    && let Some(key) = self.tempfile_handle_of(&mc.receiver)
                {
                    key
                } else {
                    self.fresh_opaque_key()
                }
            }
            // Everything else (format!/concat! macro, field path, call,
            // ...) is unresolvable → a fresh, non-correlating opaque key.
            _ => self.fresh_opaque_key(),
        }
    }

    /// If `expr` is a bare ident bound to a `tempfile-handle:<N>` key,
    /// return that key. Used so `f.path()` aliases back to the tempfile.
    ///
    /// A **poisoned** handle name (rebound / reassigned / `mut`) returns
    /// `None` so the caller falls through to a fresh opaque key — same
    /// fail-safe rule as the bare-ident resolution: a rebound tempfile
    /// handle must not alias its pre-rebind value's key.
    fn tempfile_handle_of(&self, expr: &syn::Expr) -> Option<String> {
        if let syn::Expr::Path(p) = expr
            && p.qself.is_none()
            && p.path.segments.len() == 1
        {
            let ident = p.path.segments[0].ident.to_string();
            if self.poisoned.contains(&ident) {
                return None;
            }
            if let Some(key) = self.fs_bindings.get(&ident)
                && key.starts_with("tempfile-handle:")
            {
                return Some(key.clone());
            }
        }
        None
    }
}

impl BodyVisitor {
    /// Project a free-function filesystem call (`std::fs::write(p, ..)`,
    /// `File::create(p)`, `fs::read_to_string(p)`, `File::open(p)`,
    /// `fs::metadata(p)`, ...) into the matching located fact.
    ///
    /// `func_path` is the call's func path; `first_arg` is its first
    /// positional argument (the path, for the calls we recognise). The
    /// recognised call family is keyed off the path's last TWO segments
    /// so `File::create` vs `fs::create_dir` disambiguate (a bare
    /// `create` is ambiguous and intentionally NOT matched). `OpenOptions`
    /// open-write is method-form and handled in `project_fs_method_call`.
    fn project_fs_call(&mut self, func_path: &syn::Path, first_arg: Option<&syn::Expr>) {
        let Some(family) = fs_call_family(func_path) else {
            return;
        };
        // `File::open` / `fs::read*` need the path arg; so do the writes.
        let Some(arg) = first_arg else {
            return;
        };
        let key = self.resolve_path_key(arg);
        let span = span_from_spanned(arg);
        let fact = match family {
            FsCallFamily::Write(kind) => BehavioralFact::FilesystemWrite {
                kind,
                path_key: key,
                path_arg_span: span,
            },
            FsCallFamily::Read(kind) => BehavioralFact::FilesystemRead {
                kind,
                path_key: key,
                path_arg_span: span,
            },
            FsCallFamily::Surface(kind) => BehavioralFact::FilesystemSurfaceCheck {
                kind,
                path_key: key,
                path_arg_span: span,
            },
        };
        // Located events — NOT deduped (two writes to two keys = two facts).
        self.behavioral_facts.push(fact);
    }

    /// Project a method-form filesystem fact:
    /// - `p.exists()` / `p.is_file()` / `p.is_dir()` / `p.metadata()` →
    ///   `FilesystemSurfaceCheck` on `key(receiver)` (the RECEIVER is the
    ///   path);
    /// - `OpenOptions::new()…write(true)…open(p)` → `FilesystemWrite`
    ///   `{OpenWrite}` on `key(p)` (the open-write builder chain).
    ///
    /// `File::open(p)` is a free-function `Call`, handled in
    /// `project_fs_call`, not here.
    fn project_fs_method_call(&mut self, node: &syn::ExprMethodCall) {
        // Surface checks: receiver is the path.
        if let Some(kind) = surface_check_kind(&node.method) {
            let key = self.resolve_path_key(&node.receiver);
            self.behavioral_facts
                .push(BehavioralFact::FilesystemSurfaceCheck {
                    kind,
                    path_key: key,
                    path_arg_span: span_from_spanned(&node.receiver),
                });
            return;
        }
        // OpenOptions write-open: `<builder>.open(p)` where the receiver
        // chain configures a write (`.write(true)` / `.append(true)` /
        // `.create(true)` / `.create_new(true)`). The path is the ARGUMENT.
        if node.method == "open"
            && receiver_is_write_openoptions(&node.receiver)
            && let Some(arg) = node.args.first()
        {
            let key = self.resolve_path_key(arg);
            self.behavioral_facts.push(BehavioralFact::FilesystemWrite {
                kind: FsCallKind::OpenWrite,
                path_key: key,
                path_arg_span: span_from_spanned(arg),
            });
        }
    }
}

/// The filesystem-call family a recognised free-function path maps to.
enum FsCallFamily {
    Write(FsCallKind),
    Read(FsReadKind),
    Surface(FsSurfaceCheckKind),
}

/// Recognise a free-function filesystem call by its path's last two
/// segments. Returns the family + kind, or `None` for non-fs calls.
///
/// Disambiguation rule: matches on `<container>::<leaf>` so a bare
/// single-segment leaf (e.g. `create`, `open`) is NOT matched (too
/// ambiguous). `fs::*` matches any module named `fs` (so both
/// `std::fs::write` and a `use std::fs;`-qualified `fs::write` work);
/// `File::*` matches the `File` type.
fn fs_call_family(path: &syn::Path) -> Option<FsCallFamily> {
    let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    let leaf = segs.last()?.as_str();
    // The penultimate segment names the container (`fs` / `File`).
    let container = if segs.len() >= 2 {
        segs[segs.len() - 2].as_str()
    } else {
        ""
    };
    match (container, leaf) {
        ("fs", "write") => Some(FsCallFamily::Write(FsCallKind::Write)),
        ("File", "create") => Some(FsCallFamily::Write(FsCallKind::CreateFile)),
        ("fs", "create_dir" | "create_dir_all") => Some(FsCallFamily::Write(FsCallKind::CreateDir)),
        ("fs", "read") => Some(FsCallFamily::Read(FsReadKind::Read)),
        ("fs", "read_to_string") => Some(FsCallFamily::Read(FsReadKind::ReadToString)),
        ("File", "open") => Some(FsCallFamily::Read(FsReadKind::OpenRead)),
        ("fs", "metadata") => Some(FsCallFamily::Surface(FsSurfaceCheckKind::Metadata)),
        _ => None,
    }
}

/// Map a surface-check method ident to its [`FsSurfaceCheckKind`], or
/// `None` if the method is not a recognised surface check.
///
/// `metadata()` is a surface check, INCLUDING length-only follow-ups
/// (`p.metadata()?.len()`): reading the length is surface inspection,
/// not a content read-back. The trailing `.len()` is a method call on
/// the metadata value (not the path) and projects nothing of its own.
fn surface_check_kind(method: &syn::Ident) -> Option<FsSurfaceCheckKind> {
    if method == "exists" {
        Some(FsSurfaceCheckKind::Exists)
    } else if method == "is_file" {
        Some(FsSurfaceCheckKind::IsFile)
    } else if method == "is_dir" {
        Some(FsSurfaceCheckKind::IsDir)
    } else if method == "metadata" {
        Some(FsSurfaceCheckKind::Metadata)
    } else {
        None
    }
}

/// `true` when a call's func path is `Path::new` (used to unwrap one
/// level: `Path::new(<lit>)` resolves to `key(<lit>)`). Matches on the
/// last two segments so `std::path::Path::new` and a bare `Path::new`
/// both qualify.
fn call_is_path_new(func: &syn::Expr) -> bool {
    if let syn::Expr::Path(p) = func {
        let segs = &p.path.segments;
        if segs.len() >= 2 {
            let leaf = &segs[segs.len() - 1].ident;
            let container = &segs[segs.len() - 2].ident;
            return container == "Path" && leaf == "new";
        }
    }
    false
}

/// `true` when `expr` is a `NamedTempFile::new()` or `tempfile()` /
/// `tempfile::tempfile()` constructor call (the temp file IS created on
/// disk at construction). Used only at a `let`-binding RHS.
fn is_tempfile_ctor(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(p) = call.func.as_ref() else {
        return false;
    };
    let segs: Vec<&syn::Ident> = p.path.segments.iter().map(|s| &s.ident).collect();
    let leaf = match segs.last() {
        Some(l) => *l,
        None => return false,
    };
    // `NamedTempFile::new()` — container `NamedTempFile`, leaf `new`.
    if segs.len() >= 2 {
        let container = segs[segs.len() - 2];
        if container == "NamedTempFile" && leaf == "new" {
            return true;
        }
    }
    // `tempfile()` / `tempfile::tempfile()` — leaf `tempfile`.
    leaf == "tempfile"
}

/// Unwrap ONE fallible terminal (`<e>?`, `<e>.unwrap()`, `<e>.expect(..)`)
/// off `expr` so a `let f = NamedTempFile::new()?;` RHS reduces to the
/// bare ctor for `is_tempfile_ctor`. Non-fallible exprs pass through.
fn unwrap_fallible_terminal(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Try(t) => &t.expr,
        syn::Expr::MethodCall(mc) if mc.method == "unwrap" || mc.method == "expect" => &mc.receiver,
        other => other,
    }
}

/// `true` when a method-call receiver chain is an `OpenOptions` builder
/// configured for writing — `OpenOptions::new()` somewhere at the chain
/// root AND at least one write-enabling option
/// (`.write(true)` / `.append(true)` / `.create(true)` /
/// `.create_new(true)`) on the chain. Walks the receiver chain.
fn receiver_is_write_openoptions(receiver: &syn::Expr) -> bool {
    chain_has_openoptions_root(receiver) && chain_has_write_option(receiver)
}

/// `true` when the receiver chain's root is `OpenOptions::new()`.
fn chain_has_openoptions_root(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(mc) => chain_has_openoptions_root(&mc.receiver),
        syn::Expr::Call(call) => {
            if let syn::Expr::Path(p) = call.func.as_ref() {
                let segs = &p.path.segments;
                if segs.len() >= 2 {
                    let leaf = &segs[segs.len() - 1].ident;
                    let container = &segs[segs.len() - 2].ident;
                    return container == "OpenOptions" && leaf == "new";
                }
            }
            false
        }
        _ => false,
    }
}

/// `true` when some method in the receiver chain enables writing.
fn chain_has_write_option(expr: &syn::Expr) -> bool {
    if let syn::Expr::MethodCall(mc) = expr {
        let m = &mc.method;
        if m == "write" || m == "append" || m == "create" || m == "create_new" {
            return true;
        }
        return chain_has_write_option(&mc.receiver);
    }
    false
}

/// Extract the bound ident from a `let` pattern when it is a plain
/// `Pat::Ident` (`let p = ...`). Returns `None` for `Pat::Wild`
/// (`let _ = ...` — owned by the discard path), `Pat::Type`
/// (`let p: T = ...` — still a binding, but we keep v0.1 narrow to the
/// bare-ident form), tuples, refs, and `mut`/`ref` patterns are flattened
/// to the ident.
fn local_binding_ident(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pi) => Some(pi.ident.to_string()),
        _ => None,
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

    // ─── scrap-rs#26: located filesystem fact projection ────────────────

    /// A `(family-tag, path_key)` projection of one located fs fact.
    /// Drops the `path_arg_span` (driven by syn source positions, not
    /// load-bearing for these assertions) and the kind detail (asserted
    /// separately) so the tests read as `("write", "lit:/tmp/x")`.
    fn fs_keys(source: &str) -> Vec<(&'static str, String)> {
        facts_of(source)
            .into_iter()
            .filter_map(|f| match f {
                BehavioralFact::FilesystemWrite { path_key, .. } => Some(("write", path_key)),
                BehavioralFact::FilesystemSurfaceCheck { path_key, .. } => {
                    Some(("surface", path_key))
                }
                BehavioralFact::FilesystemRead { path_key, .. } => Some(("read", path_key)),
                _ => None,
            })
            .collect()
    }

    /// All located fs facts (kept whole, for kind assertions). Filters out
    /// the non-located `ResultAsserted` / `ResultDiscarded` noise.
    fn fs_facts(source: &str) -> Vec<BehavioralFact> {
        facts_of(source)
            .into_iter()
            .filter(|f| {
                matches!(
                    f,
                    BehavioralFact::FilesystemWrite { .. }
                        | BehavioralFact::FilesystemSurfaceCheck { .. }
                        | BehavioralFact::FilesystemRead { .. }
                )
            })
            .collect()
    }

    // ── Per-kind write projection ───────────────────────────────────────

    #[test]
    fn projects_fs_write_with_literal_key() {
        assert_eq!(
            fs_facts(r#"fn it() { let _ = std::fs::write("/tmp/x.txt", b"d"); }"#),
            vec![BehavioralFact::FilesystemWrite {
                kind: FsCallKind::Write,
                path_key: "lit:/tmp/x.txt".into(),
                path_arg_span: span_of_first_write(
                    r#"fn it() { let _ = std::fs::write("/tmp/x.txt", b"d"); }"#
                ),
            }],
        );
    }

    /// Helper to recover the exact span of the first write fact, so the
    /// per-kind equality test above pins the real syn-derived span rather
    /// than a fabricated one.
    fn span_of_first_write(source: &str) -> scrap_core::domain::types::Span {
        match fs_facts(source).into_iter().next() {
            Some(BehavioralFact::FilesystemWrite { path_arg_span, .. }) => path_arg_span,
            other => panic!("expected a write fact, got {other:?}"),
        }
    }

    #[test]
    fn projects_file_create_as_create_file_write() {
        // `File::create(p)?;` — Try form avoids a `.unwrap()` ResultAsserted.
        let facts =
            fs_facts(r#"fn it() -> std::io::Result<()> { File::create("/tmp/y")?; Ok(()) }"#);
        assert_eq!(facts.len(), 1);
        assert!(matches!(
            facts[0],
            BehavioralFact::FilesystemWrite {
                kind: FsCallKind::CreateFile,
                ..
            }
        ));
    }

    #[test]
    fn projects_create_dir_and_create_dir_all_as_create_dir() {
        for src in [
            r#"fn it() -> std::io::Result<()> { std::fs::create_dir("/tmp/d")?; Ok(()) }"#,
            r#"fn it() -> std::io::Result<()> { std::fs::create_dir_all("/tmp/d/e")?; Ok(()) }"#,
        ] {
            let facts = fs_facts(src);
            assert_eq!(facts.len(), 1, "src: {src}");
            assert!(
                matches!(
                    facts[0],
                    BehavioralFact::FilesystemWrite {
                        kind: FsCallKind::CreateDir,
                        ..
                    }
                ),
                "src: {src}",
            );
        }
    }

    #[test]
    fn projects_openoptions_write_open_as_open_write() {
        let facts = fs_facts(
            r#"fn it() -> std::io::Result<()> { OpenOptions::new().write(true).open("/tmp/w")?; Ok(()) }"#,
        );
        assert_eq!(facts.len(), 1);
        assert!(matches!(
            facts[0],
            BehavioralFact::FilesystemWrite {
                kind: FsCallKind::OpenWrite,
                ..
            }
        ));
    }

    #[test]
    fn openoptions_read_only_open_does_not_project_write() {
        // `.read(true).open(p)` is a READ-configured open — NOT a write.
        // It also is not `File::open`, so it projects no fs fact at all.
        let facts = fs_facts(
            r#"fn it() -> std::io::Result<()> { OpenOptions::new().read(true).open("/tmp/r")?; Ok(()) }"#,
        );
        assert!(
            facts.is_empty(),
            "read-only OpenOptions must not project a write: {facts:?}"
        );
    }

    // ── Per-kind surface-check projection ───────────────────────────────

    #[test]
    fn projects_exists_is_file_is_dir_surface_checks() {
        // Receiver is the path; bound ident `p` resolves to `bind:p`.
        for (method, kind) in [
            ("exists", FsSurfaceCheckKind::Exists),
            ("is_file", FsSurfaceCheckKind::IsFile),
            ("is_dir", FsSurfaceCheckKind::IsDir),
        ] {
            let src = format!("fn it() {{ let _ = p.{method}(); }}");
            let facts = fs_facts(&src);
            assert_eq!(facts.len(), 1, "method {method}");
            assert_eq!(
                facts[0],
                BehavioralFact::FilesystemSurfaceCheck {
                    kind,
                    path_key: "bind:p".into(),
                    path_arg_span: match &facts[0] {
                        BehavioralFact::FilesystemSurfaceCheck { path_arg_span, .. } =>
                            *path_arg_span,
                        _ => unreachable!(),
                    },
                },
            );
        }
    }

    #[test]
    fn projects_metadata_len_only_as_surface_check_not_read() {
        // `fs::metadata(&p)?.len()` — a length-only check is a SURFACE
        // check, never a read. The trailing `.len()` projects nothing.
        let facts = fs_facts(
            r"fn it() -> std::io::Result<()> { let n = std::fs::metadata(&p)?.len(); let _ = n; Ok(()) }",
        );
        assert_eq!(facts.len(), 1);
        assert!(matches!(
            facts[0],
            BehavioralFact::FilesystemSurfaceCheck {
                kind: FsSurfaceCheckKind::Metadata,
                ..
            }
        ));
    }

    // ── Per-kind read projection ────────────────────────────────────────

    #[test]
    fn projects_read_and_read_to_string_and_file_open() {
        for (src, kind) in [
            (
                r#"fn it() -> std::io::Result<()> { let _b = std::fs::read("/tmp/x")?; Ok(()) }"#,
                FsReadKind::Read,
            ),
            (
                r#"fn it() -> std::io::Result<()> { let _s = std::fs::read_to_string("/tmp/x")?; Ok(()) }"#,
                FsReadKind::ReadToString,
            ),
            (
                r#"fn it() -> std::io::Result<()> { let _f = File::open("/tmp/x")?; Ok(()) }"#,
                FsReadKind::OpenRead,
            ),
        ] {
            let facts = fs_facts(src);
            assert_eq!(facts.len(), 1, "src: {src}");
            assert_eq!(
                facts[0],
                BehavioralFact::FilesystemRead {
                    kind,
                    path_key: "lit:/tmp/x".into(),
                    path_arg_span: match &facts[0] {
                        BehavioralFact::FilesystemRead { path_arg_span, .. } => *path_arg_span,
                        _ => unreachable!(),
                    },
                },
                "src: {src}",
            );
        }
    }

    #[test]
    fn projects_bufreader_file_open_as_read() {
        // `BufReader::new(File::open(p))` — the inner `File::open` is a
        // free-function Call that recursion reaches → OpenRead read fact.
        let facts = fs_facts(
            r#"fn it() -> std::io::Result<()> { let _r = std::io::BufReader::new(File::open("/tmp/x")?); Ok(()) }"#,
        );
        assert!(
            facts.iter().any(|f| matches!(
                f,
                BehavioralFact::FilesystemRead {
                    kind: FsReadKind::OpenRead,
                    ..
                }
            )),
            "BufReader::new(File::open(..)) must surface a read: {facts:?}",
        );
    }

    // ── Path-key aliasing forms ─────────────────────────────────────────

    #[test]
    fn aliasing_let_binding_to_literal_resolves_to_lit_key() {
        // `let p = "/tmp/x"; fs::write(p, ..);` — the bound ident resolves
        // through the binding map to the literal's `lit:` key, so a later
        // surface check on the SAME ident correlates.
        assert_eq!(
            fs_keys(
                r#"fn it() { let p = "/tmp/x"; let _ = std::fs::write(p, b"d"); let _ = p.exists(); }"#
            ),
            vec![
                ("write", "lit:/tmp/x".into()),
                ("surface", "lit:/tmp/x".into())
            ],
        );
    }

    #[test]
    fn aliasing_reference_unwraps_one_level() {
        // `fs::write(&p, ..)` resolves `&p` → `p` → `bind:p`.
        assert_eq!(
            fs_keys(r#"fn it() { let _ = std::fs::write(&p, b"d"); }"#),
            vec![("write", "bind:p".into())],
        );
    }

    #[test]
    fn aliasing_path_new_literal_unwraps_to_lit_key() {
        // `Path::new("/tmp/x")` → `lit:/tmp/x` (one-level unwrap).
        assert_eq!(
            fs_keys(
                r#"fn it() -> std::io::Result<()> { File::create(Path::new("/tmp/x"))?; Ok(()) }"#
            ),
            vec![("write", "lit:/tmp/x".into())],
        );
    }

    #[test]
    fn aliasing_as_path_unwraps_to_receiver() {
        // `p.as_path()` → `p` → `bind:p`.
        assert_eq!(
            fs_keys(r#"fn it() { let _ = std::fs::write(p.as_path(), b"d"); }"#),
            vec![("write", "bind:p".into())],
        );
    }

    #[test]
    fn aliasing_tempfile_path_resolves_to_handle_key() {
        // `let f = NamedTempFile::new()?;` emits a Tempfile WRITE at the
        // binding (key tempfile-handle:0), and `f.path().exists()` aliases
        // the receiver back to that same handle → write + surface on ONE key.
        assert_eq!(
            fs_keys(
                r"fn it() -> std::io::Result<()> { let f = NamedTempFile::new()?; let _ = f.path().exists(); Ok(()) }"
            ),
            vec![
                ("write", "tempfile-handle:0".into()),
                ("surface", "tempfile-handle:0".into()),
            ],
        );
    }

    #[test]
    fn aliasing_opaque_format_path_gets_distinct_opaque_keys() {
        // `format!(..)` is unresolvable → a DISTINCT opaque key per site.
        // Two such sites must NOT share a key (so they can't correlate).
        let keys = fs_keys(
            r#"fn it() { let _ = std::fs::write(format!("/tmp/{}", a), b"d"); let _ = std::fs::metadata(format!("/tmp/{}", b)); }"#,
        );
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].0, "write");
        assert_eq!(keys[1].0, "surface");
        assert!(keys[0].1.starts_with("opaque:"));
        assert!(keys[1].1.starts_with("opaque:"));
        assert_ne!(
            keys[0].1, keys[1].1,
            "distinct opaque sites must get distinct keys"
        );
    }

    // ── Rebind-poison resolution (cabinet CRITICAL #1) ──────────────────

    #[test]
    fn poisoned_rebound_ident_resolves_to_distinct_opaque_keys() {
        // T2 at the projection level: a `mut` (rebound) name must resolve
        // to a FRESH opaque key at EACH site — never a shared `bind:p` (a
        // name-based fallback would re-collide and false-fire). The write
        // and the surface check land on TWO DIFFERENT opaque keys, so the
        // detector cannot correlate them.
        let keys = fs_keys(
            "fn it() { let mut p = make_path(); let _ = std::fs::write(&p, b\"d\"); p = make_other(); let _ = p.exists(); }",
        );
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].0, "write");
        assert_eq!(keys[1].0, "surface");
        assert!(
            keys[0].1.starts_with("opaque:"),
            "poisoned write key must be opaque, got {}",
            keys[0].1,
        );
        assert!(
            keys[1].1.starts_with("opaque:"),
            "poisoned check key must be opaque, got {}",
            keys[1].1,
        );
        assert_ne!(
            keys[0].1, keys[1].1,
            "a poisoned name must yield DISTINCT opaque keys per site (never a shared bind:p)",
        );
    }

    #[test]
    fn singly_bound_non_mut_ident_shares_one_key_across_sites() {
        // Positive control at the projection level (guards against
        // over-poisoning): a clean singly-bound non-`mut` name resolves to
        // the SAME key at the write and the check, so they correlate. The
        // `let p = make_path();` binding maps `p` to the resolved key of
        // its (non-literal) RHS — a single `opaque:0` — and both `&p` and
        // `p.exists()` look that up, so they SHARE it (not two distinct
        // opaque keys, which is the poisoned case).
        let keys = fs_keys(
            "fn it() { let p = make_path(); let _ = std::fs::write(&p, b\"d\"); let _ = p.exists(); }",
        );
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].0, "write");
        assert_eq!(keys[1].0, "surface");
        assert_eq!(
            keys[0].1, keys[1].1,
            "a clean singly-bound name must share ONE key across sites (so it correlates)",
        );
    }

    #[test]
    fn unbound_clean_ident_resolves_to_shared_bind_key() {
        // A name that is NEVER `let`-bound in the body (e.g. a fn
        // parameter `p`) and is not poisoned falls back to `bind:<ident>`
        // — and shares it across sites, so a write + check on a param path
        // correlates. Pins the `bind:p` fallback path distinctly from the
        // let-bound case above.
        let keys = fs_keys(
            "fn it(p: &std::path::Path) { let _ = std::fs::write(p, b\"d\"); let _ = p.exists(); }",
        );
        assert_eq!(
            keys,
            vec![("write", "bind:p".into()), ("surface", "bind:p".into())],
            "an unbound, un-poisoned name must share its `bind:` key across sites",
        );
    }

    // ── Read-back round-trip on the same key ────────────────────────────

    #[test]
    fn write_then_read_back_emit_read_on_same_key() {
        // `fs::write(p, ..); fs::read_to_string(p);` — both resolve `p` to
        // the SAME `lit:` key, so the write and the read correlate.
        let keys = fs_keys(
            r#"fn it() -> std::io::Result<()> { std::fs::write("/tmp/x", b"d")?; let _s = std::fs::read_to_string("/tmp/x")?; Ok(()) }"#,
        );
        assert_eq!(
            keys,
            vec![
                ("write", "lit:/tmp/x".into()),
                ("read", "lit:/tmp/x".into())
            ],
        );
    }

    // ── Located events are NOT deduped ──────────────────────────────────

    #[test]
    fn two_writes_to_different_keys_are_two_events() {
        // Located facts must NOT dedup (contrast the presence-fact dedup
        // for ResultAsserted/ResultDiscarded). Two writes to two keys → two.
        assert_eq!(
            fs_keys(
                r#"fn it() { let _ = std::fs::write("/tmp/a", b"d"); let _ = std::fs::write("/tmp/b", b"d"); }"#
            ),
            vec![
                ("write", "lit:/tmp/a".into()),
                ("write", "lit:/tmp/b".into())
            ],
        );
    }

    #[test]
    fn two_writes_to_same_key_are_still_two_events() {
        // Even same-key located facts are NOT deduped — they are distinct
        // observations (distinct spans). The detector's grouping collapses
        // them at correlation time, not at projection time.
        assert_eq!(
            fs_keys(
                r#"fn it() { let _ = std::fs::write("/tmp/a", b"d"); let _ = std::fs::write("/tmp/a", b"e"); }"#
            ),
            vec![
                ("write", "lit:/tmp/a".into()),
                ("write", "lit:/tmp/a".into())
            ],
        );
    }

    // ── Non-fs calls project nothing ────────────────────────────────────

    #[test]
    fn unrelated_calls_project_no_fs_facts() {
        assert!(fs_facts("fn it() { let _ = compute(); foo.bar(); v.len(); }").is_empty());
    }

    // ── assert_matches! scrutinee descent (cabinet CRITICAL #2) ─────────

    #[test]
    fn assert_matches_scrutinee_read_is_projected() {
        // `assert_matches!(fs::read_to_string(p)?, Ok(s) if ...)`'s second
        // arg is a PATTERN, so the whole-arglist `Punctuated<Expr>` parse
        // fails. The leading-Expr fallback must still capture the
        // scrutinee `fs::read_to_string(p)?` so its read fact projects
        // (otherwise a genuine read-back is dropped → false fire).
        let facts = fs_facts(
            "fn it() -> std::io::Result<()> { let p = \"/tmp/x\"; assert_matches!(std::fs::read_to_string(p)?, Ok(s) if s == \"x\"); Ok(()) }",
        );
        assert!(
            facts.iter().any(|f| matches!(
                f,
                BehavioralFact::FilesystemRead {
                    kind: FsReadKind::ReadToString,
                    ..
                }
            )),
            "assert_matches! scrutinee read must project a FilesystemRead: {facts:?}",
        );
    }

    #[test]
    fn non_assertion_macro_does_not_descend() {
        // Descent is scoped to RECOGNISED assertion macros only. A
        // top-level `vec![std::fs::write(p, ..)]` and a `dbg!(...)` must
        // NOT have their inner fs calls projected (the v0.1 boundary holds
        // for non-assertion macros).
        assert!(
            fs_facts("fn it() { let _ = vec![std::fs::write(p, b\"d\")]; }").is_empty(),
            "vec! is not an assertion macro — must not descend",
        );
        assert!(
            fs_facts("fn it() { dbg!(std::path::Path::new(p).exists()); }").is_empty(),
            "dbg! is not an assertion macro — must not descend",
        );
    }
}
