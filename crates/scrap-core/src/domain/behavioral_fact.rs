//! `BehavioralFact` enum — typed projection of body-call-shape facts
//! the adapter parser recognises and detectors consume.
//!
//! Lands with scrap-rs#30 (introduces the `ResultAsserted` variant +
//! `ParsedTest.behavioral_facts` field + the parser visitor). scrap-rs#25
//! adds the `ResultDiscarded { kind }` variant + the [`ResultDiscardKind`]
//! shape taxonomy that drives the `no-op-io` detector.
//!
//! `AsyncEscape` (a future-built-but-never-awaited signal) was originally
//! sketched for scrap-rs#25 but is intentionally NOT modeled here: it is
//! **test-darkness** (did the assertion's code path actually run?), a
//! separate detector pillar with its own discriminator, tracked by the
//! darkness-detection epic. The scrap-rs#25 surface is `no-op-io` only.
//!
//! Why a separate `BehavioralFact` from `AssertionSource`:
//! - [`crate::domain::assertion_sources::AssertionSource`] is data-driven
//!   recognition for FRAMEWORK runner shells (proptest, kani, insta, ...).
//!   Path-string matched via `recognise()`.
//! - `BehavioralFact` is shape-recognition for LANGUAGE idioms
//!   (`.unwrap()`/`.expect()` chains, `let _ = ...` discards, etc.).
//!   Walked via syn-visit overrides, not path-string-matched.
//! - Both feed into detector logic but the projection mechanics differ;
//!   keeping the enums separate keeps the parser-side code paths
//!   discoverable.
//!
//! No `syn` dependency — the parser produces these typed facts at the
//! adapter boundary; the domain holds only the enum.
//!
//! ## Wire shape note (heterogeneous array as of scrap-rs#25)
//!
//! `ParsedTest::behavioral_facts` serializes as a JSON array. Before
//! scrap-rs#25 every variant was unit-only, so the array was `string[]`
//! (`["result_asserted"]`). `ResultDiscarded` is the **first
//! data-carrying variant**, so the array is now heterogeneous —
//! `(string | object)[]`, e.g.
//! `["result_asserted", {"result_discarded": {"kind": "call"}}]`. The
//! mokumo scorecard + the future napi-rs FFI consumer must handle both
//! the bare-string and externally-tagged-object forms.
//!
//! TODO(scrap-rs#73): once `adr-port-surface-and-domain-conventions`
//! lands, link to it for the dumb-parser/smart-detector boundary (D10)
//! rationale.
//!
//! ## Located filesystem facts (scrap-rs#26)
//!
//! The three filesystem variants — [`BehavioralFact::FilesystemWrite`],
//! [`BehavioralFact::FilesystemSurfaceCheck`],
//! [`BehavioralFact::FilesystemRead`] — are the first **located**
//! behavioral facts: each carries the coordinates the adapter computes
//! per `adr-port-surface-and-domain-conventions` D3 (identity =
//! `path_key`, position = `path_arg_span`). The `surface-only-io`
//! detector composes them (D4): it groups facts by `path_key` and fires
//! when some key has a write + a surface check but no read. The adapter
//! says *what* (write/check/read at this key); the core says *is-bad*
//! (the unread surface check). Because these variants carry a `String`
//! (`path_key`) + a [`crate::domain::types::Span`], the enum can no
//! longer derive `Copy` / `PartialOrd` / `Ord` — those derives were
//! removed at scrap-rs#26 (the breadcrumb the pre-#26 module doc above
//! left). `Vec` storage (scrap-rs#112) already dropped any reliance on
//! `Ord` set-admission, so the removal is non-breaking; the kept derives
//! are `Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`.
//!
//! TODO(scrap-rs#117): extract a `BehavioralFactCatalog` query-API
//! (e.g. `facts_for_key(&str)`, `keys()`) once the taxonomy passes
//! ~10 variants. After scrap-rs#26 it is at 5 (`ResultAsserted`,
//! `ResultDiscarded`, `FilesystemWrite`, `FilesystemSurfaceCheck`,
//! `FilesystemRead`); the per-key grouping currently lives inline in
//! `surface_only_io::detect`. Defer the catalog until a second
//! correlation detector (or the v0.3 fact expansion) would otherwise
//! duplicate the grouping logic.

use crate::domain::types::Span;
use serde::{Deserialize, Serialize};

/// Heuristic shape of a discarded (`let _ = <expr>;`) initializer, as
/// recognised by the adapter parser. **No type inference** — the parser
/// classifies the syntactic form only, so `Call` fires on any discarded
/// function/method call regardless of its real return type.
///
/// Language-agnostic by design (per the Semantic-Facts cross-port rule):
/// the variant names describe *expression shapes*, not Rust-specific
/// types, so a future scrap4ts adapter can populate the same kinds for
/// TypeScript discards without inventing a faithful TS AST.
///
/// No catch-all `Other` variant: `#[non_exhaustive]` already provides
/// the forward-compat hatch (mirrors
/// [`crate::domain::parsed::ParseDiagnosticKind`]'s discipline). The
/// parser's classifier returns `None` (do-not-project) for every shape
/// outside this set — literals, paths, macros, tuples, control-flow
/// exprs, references, and panic-chain-terminated chains (which project
/// [`BehavioralFact::ResultAsserted`] instead).
///
/// Wire format is `snake_case`; per-variant `#[serde(rename = "...")]`
/// is belt-and-suspenders against future `rename_all` drift.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultDiscardKind {
    /// `let _ = some_call(...);` / `let _ = x.method(...);` — a
    /// function or method call whose result is dropped.
    #[serde(rename = "call")]
    Call,
    /// `let _ = Ok(...);` / `let _ = Err(...);` — an explicit
    /// `Result`-constructor call whose value is dropped.
    #[serde(rename = "result_ctor")]
    ResultCtor,
    /// `let _ = x.ok();` / `let _ = x.err();` — the
    /// `Result`↔`Option` conversion adapters, dropped.
    #[serde(rename = "result_adapter")]
    ResultAdapter,
}

/// Body-shape behavioral facts the adapter parser recognises.
///
/// `#[non_exhaustive]` per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md)'s
/// enum discipline; new variants land additively as detector PRs introduce
/// new language-shape facts. The wire format is `snake_case`; per-variant
/// `#[serde(rename = "...")]` is belt-and-suspenders against future
/// `rename_all` drift (matches sibling [`crate::domain::assertion_sources::AssertionSource`]
/// + [`crate::domain::opt_outs::OptOut`] discipline).
///
/// Storage: `Vec<BehavioralFact>` on `ParsedTest` (migrated from
/// `BTreeSet` at scrap-rs#112). Two reasons drove the switch, both
/// looking ahead to the located, correlation-carrying fact variants
/// arriving at scrap-rs#26:
/// 1. **Correlation facts must not dedup-collapse.** A `BTreeSet`
///    silently merges two facts that compare equal; the #26 located
///    variants carry distinct `String` path-keys + `Span`s that are
///    semantically separate observations and must each survive on the
///    wire. The "≥1 of shape X" presence-fact dedup the two existing
///    variants relied on now happens at **projection** in the parser
///    adapter (`scrap4rs::parser::body::BodyVisitor`), not via
///    set-admission.
/// 2. **`Span` must not be forced into an `Ord` wire-ordering.**
///    `BTreeSet` admission demands `Ord`; a `Span`-carrying variant
///    would force a total order on source coordinates with no
///    meaningful semantics — the same reason [`crate::domain::types::FilePath`]
///    refuses to derive `Ord` for the wire contract. A `Vec` preserves
///    the parser's natural emission order instead.
///
/// The `Copy`/`PartialOrd`/`Ord` derives were **removed at scrap-rs#26**:
/// the located filesystem variants below carry a `String` (`path_key`)
/// and a [`crate::domain::types::Span`], neither of which the located
/// fact may meaningfully order (the same `Ord`-refusal precedent as
/// [`crate::domain::types::FilePath`]) — and a `String`-carrying enum
/// cannot be `Copy`. `Vec` storage (scrap-rs#112) already dropped any
/// reliance on `Ord` set-admission, so the removal is non-breaking.
/// Kept derives: `Debug, Clone, PartialEq, Eq, Hash, Serialize,
/// Deserialize`.
///
/// **Per-fact located spans (as of scrap-rs#26):** the filesystem
/// variants carry a `path_arg_span` (the span of the path-argument
/// expression at the call site). The two original variants
/// (`ResultAsserted`, `ResultDiscarded`) stay whole-test: their
/// detectors (`zero-assertion`, `no-op-io`) emit a fn-level verdict and
/// no v0.1 consumer reads a per-discard line.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehavioralFact {
    /// Body contains a `.unwrap()` / `.expect(...)` (or the `*_err`
    /// error-path siblings) method-call chain anywhere in the test fn's
    /// body.
    ///
    /// Recognised syntactically by the adapter parser
    /// (`scrap4rs::parser::body::BodyVisitor::visit_expr_method_call`
    /// against the method ident); no type inference is performed —
    /// `.unwrap()` on any value type fires the recognition. Detector-side
    /// consumption (zero-assertion + no-op-io suppression) is the
    /// detector's concern; this variant only encodes the syntactic shape.
    #[serde(rename = "result_asserted")]
    ResultAsserted,
    /// Body contains a `let _ = <Result-shaped expr>;` discard — a bare
    /// wildcard binding (NOT `let _: T = ...;` type-ascribed) whose
    /// initializer is one of the [`ResultDiscardKind`] shapes.
    ///
    /// Recognised by `BodyVisitor::visit_local` delegating to
    /// `classify_discard_init`; drives the `no-op-io` detector
    /// (scrap-rs#25). `kind` records the heuristic shape; see
    /// [`ResultDiscardKind`] for the do-NOT-project boundary.
    #[serde(rename = "result_discarded")]
    ResultDiscarded {
        /// The heuristic shape of the discarded initializer.
        kind: ResultDiscardKind,
    },
    /// A **located** filesystem-write call: the test created or wrote a
    /// file/dir at `path_key` (e.g. `fs::write(p, ..)`, `File::create(p)`,
    /// `NamedTempFile::new()`). The first half of the `surface-only-io`
    /// correlation — a write whose effect on disk a later surface check
    /// might inspect.
    ///
    /// `path_key` is the adapter-computed identity (`lit:<value>` /
    /// `bind:<ident>` / `tempfile-handle:<N>` / `opaque:<N>` — see
    /// `scrap4rs::parser::body` for the resolution rules); `path_arg_span`
    /// locates the path-argument expression. Distinct `opaque:<N>` keys
    /// never group together, so an unresolvable path can never spuriously
    /// correlate with another.
    #[serde(rename = "filesystem_write")]
    FilesystemWrite {
        /// The flavour of write/create call recognised.
        kind: FsCallKind,
        /// Adapter-computed path identity used for correlation grouping.
        path_key: String,
        /// Span of the path-argument expression at the call site.
        path_arg_span: Span,
    },
    /// A **located** filesystem-surface check: the test inspected only
    /// surface metadata of `path_key` (existence / file-or-dir / length)
    /// without reading the content back — e.g. `p.exists()`,
    /// `p.is_file()`, `fs::metadata(p).len()`. The middle term of the
    /// `surface-only-io` correlation.
    ///
    /// A surface check is honest evidence the test looked at *something*
    /// (`assert!(p.exists())` is a real `ParsedAssertion` that suppresses
    /// `zero-assertion`) — but checking only the surface, after a write,
    /// with no content read-back, is exactly the `surface-only-io` smell.
    #[serde(rename = "filesystem_surface_check")]
    FilesystemSurfaceCheck {
        /// The flavour of surface check recognised.
        kind: FsSurfaceCheckKind,
        /// Adapter-computed path identity used for correlation grouping.
        path_key: String,
        /// Span of the path-argument expression at the call site.
        path_arg_span: Span,
    },
    /// A **located** filesystem read-back of `path_key`'s content — e.g.
    /// `fs::read_to_string(p)`, `fs::read(p)`, `File::open(p)`,
    /// `BufReader::new(File::open(p))`. The presence of a read for a key
    /// **disarms** `surface-only-io` for that key: the test inspected the
    /// substantive payload, not just the surface.
    #[serde(rename = "filesystem_read")]
    FilesystemRead {
        /// The flavour of read call recognised.
        kind: FsReadKind,
        /// Adapter-computed path identity used for correlation grouping.
        path_key: String,
        /// Span of the path-argument expression at the call site.
        path_arg_span: Span,
    },
}

/// Flavour of a filesystem **write/create** call the adapter recognises,
/// driving [`BehavioralFact::FilesystemWrite`].
///
/// Language-agnostic by shape, like [`ResultDiscardKind`]: the variant
/// names describe *what kind of side effect on disk* the call performs,
/// not Rust-specific API surface, so a future scrap4ts adapter can
/// populate the same kinds. Wire format is `snake_case`; per-variant
/// `#[serde(rename = "...")]` is belt-and-suspenders against
/// `rename_all` drift. No catch-all `Other` — `#[non_exhaustive]` is
/// the forward-compat hatch.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsCallKind {
    /// `fs::write(p, ..)` — write whole-contents to a path.
    #[serde(rename = "write")]
    Write,
    /// `File::create(p)` — create/truncate a file handle.
    #[serde(rename = "create_file")]
    CreateFile,
    /// `fs::create_dir(p)` / `fs::create_dir_all(p)` — create a directory.
    #[serde(rename = "create_dir")]
    CreateDir,
    /// `tempfile::NamedTempFile::new()` / `tempfile::tempfile()` — a temp
    /// file IS created on disk at construction; the handle aliases a
    /// `tempfile-handle:<N>` key.
    #[serde(rename = "tempfile")]
    Tempfile,
    /// `OpenOptions::new()…write(true)…open(p)` — open a path for writing.
    #[serde(rename = "open_write")]
    OpenWrite,
}

/// Flavour of a filesystem **surface check** the adapter recognises,
/// driving [`BehavioralFact::FilesystemSurfaceCheck`]. A surface check
/// inspects metadata (existence / kind / length) but NOT content.
///
/// Same `snake_case` + `#[non_exhaustive]` discipline as
/// [`FsCallKind`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsSurfaceCheckKind {
    /// `p.exists()` / `Path::exists(p)` — existence only.
    #[serde(rename = "exists")]
    Exists,
    /// `p.is_file()` — is-a-file surface predicate.
    #[serde(rename = "is_file")]
    IsFile,
    /// `p.is_dir()` — is-a-directory surface predicate.
    #[serde(rename = "is_dir")]
    IsDir,
    /// `fs::metadata(p)` / `p.metadata()` — metadata only, INCLUDING
    /// length-only checks like `fs::metadata(p)?.len()`. Reading the
    /// length is still a surface inspection, NOT a content read-back, so
    /// it is a surface check (not a [`FsReadKind`]).
    #[serde(rename = "metadata")]
    Metadata,
}

/// Flavour of a filesystem **content read** the adapter recognises,
/// driving [`BehavioralFact::FilesystemRead`]. A read inspects the
/// substantive payload and disarms `surface-only-io` for its key.
///
/// Same `snake_case` + `#[non_exhaustive]` discipline as
/// [`FsCallKind`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsReadKind {
    /// `fs::read(p)` — read whole-contents to bytes.
    #[serde(rename = "read")]
    Read,
    /// `fs::read_to_string(p)` — read whole-contents to a `String`.
    #[serde(rename = "read_to_string")]
    ReadToString,
    /// `File::open(p)` — open a file handle for reading.
    #[serde(rename = "open_read")]
    OpenRead,
    /// Buffered read over an opened file. **Reserved — not yet projected
    /// by the scrap4rs adapter at v0.1.** `BufReader::new(File::open(p))`
    /// already surfaces a read via its inner `File::open(p)` →
    /// [`FsReadKind::OpenRead`] (the read-presence the correlation needs),
    /// so the adapter does not additionally emit a distinct `BufRead`
    /// fact. The variant is kept on the wire surface for a future adapter
    /// that wants to distinguish buffered from unbuffered reads without an
    /// envelope `schema_version` bump.
    #[serde(rename = "buf_read")]
    BufRead,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Wire-key pin: every variant round-trips its documented form ──

    #[test]
    fn behavioral_fact_result_asserted_serializes_bare_string() {
        // Unit variant → bare snake_case string (the pre-scrap-rs#25 form
        // the mokumo/FFI consumer compiled against; must stay stable).
        let variant = BehavioralFact::ResultAsserted;
        let json = serde_json::to_value(&variant).unwrap();
        assert_eq!(json, serde_json::Value::String("result_asserted".into()));
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn behavioral_fact_result_discarded_serializes_externally_tagged_object() {
        // Data-carrying variant → externally-tagged object:
        // {"result_discarded": {"kind": "call"}}. Pins the heterogeneous
        // (string | object)[] wire shape the consumer must handle.
        let variant = BehavioralFact::ResultDiscarded {
            kind: ResultDiscardKind::Call,
        };
        let json = serde_json::to_value(&variant).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"result_discarded": {"kind": "call"}})
        );
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn result_discard_kind_serializes_snake_case() {
        for (kind, wire) in [
            (ResultDiscardKind::Call, "call"),
            (ResultDiscardKind::ResultCtor, "result_ctor"),
            (ResultDiscardKind::ResultAdapter, "result_adapter"),
        ] {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: ResultDiscardKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, kind);
        }
    }

    // ── Vec emission-order discipline (post scrap-rs#112 storage) ──

    #[test]
    fn behavioral_fact_vec_serializes_in_emission_order() {
        // Storage is now `Vec<BehavioralFact>` (scrap-rs#112): the wire
        // array reflects **emission order**, NOT `Ord`-sorted order. A
        // `ResultDiscarded`-then-`ResultAsserted` emission serializes in
        // exactly that order — the reverse of the BTreeSet's old
        // `Ord`-sorted "ResultAsserted-first" contract — proving order
        // now tracks emission rather than declaration order. The
        // per-fact wire shape (heterogeneous `(string | object)[]`) is
        // unchanged.
        let facts = vec![
            BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::ResultCtor,
            },
            BehavioralFact::ResultAsserted,
        ];
        assert_eq!(
            serde_json::to_value(&facts).unwrap(),
            serde_json::json!([{"result_discarded": {"kind": "result_ctor"}}, "result_asserted"]),
        );
    }

    // ── Located filesystem facts (scrap-rs#26) ──────────────────────

    /// A sample span reused across the fs-fact wire pins.
    fn sample_span() -> Span {
        Span::new(3, 3, 5, 18)
    }

    #[test]
    fn behavioral_fact_filesystem_write_serializes_externally_tagged_object() {
        let variant = BehavioralFact::FilesystemWrite {
            kind: FsCallKind::Write,
            path_key: "lit:/tmp/out.txt".into(),
            path_arg_span: sample_span(),
        };
        let json = serde_json::to_value(&variant).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "filesystem_write": {
                    "kind": "write",
                    "path_key": "lit:/tmp/out.txt",
                    "path_arg_span": {
                        "start_line": 3,
                        "end_line": 3,
                        "start_column": 5,
                        "end_column": 18,
                    },
                }
            }),
        );
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn behavioral_fact_filesystem_surface_check_serializes_externally_tagged_object() {
        let variant = BehavioralFact::FilesystemSurfaceCheck {
            kind: FsSurfaceCheckKind::Exists,
            path_key: "bind:p".into(),
            path_arg_span: sample_span(),
        };
        let json = serde_json::to_value(&variant).unwrap();
        assert_eq!(json["filesystem_surface_check"]["kind"], "exists");
        assert_eq!(json["filesystem_surface_check"]["path_key"], "bind:p");
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn behavioral_fact_filesystem_read_serializes_externally_tagged_object() {
        let variant = BehavioralFact::FilesystemRead {
            kind: FsReadKind::ReadToString,
            path_key: "tempfile-handle:0".into(),
            path_arg_span: sample_span(),
        };
        let json = serde_json::to_value(&variant).unwrap();
        assert_eq!(json["filesystem_read"]["kind"], "read_to_string");
        assert_eq!(json["filesystem_read"]["path_key"], "tempfile-handle:0");
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn fs_call_kind_serializes_snake_case() {
        for (kind, wire) in [
            (FsCallKind::Write, "write"),
            (FsCallKind::CreateFile, "create_file"),
            (FsCallKind::CreateDir, "create_dir"),
            (FsCallKind::Tempfile, "tempfile"),
            (FsCallKind::OpenWrite, "open_write"),
        ] {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: FsCallKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn fs_surface_check_kind_serializes_snake_case() {
        for (kind, wire) in [
            (FsSurfaceCheckKind::Exists, "exists"),
            (FsSurfaceCheckKind::IsFile, "is_file"),
            (FsSurfaceCheckKind::IsDir, "is_dir"),
            (FsSurfaceCheckKind::Metadata, "metadata"),
        ] {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: FsSurfaceCheckKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn fs_read_kind_serializes_snake_case() {
        for (kind, wire) in [
            (FsReadKind::Read, "read"),
            (FsReadKind::ReadToString, "read_to_string"),
            (FsReadKind::OpenRead, "open_read"),
            (FsReadKind::BufRead, "buf_read"),
        ] {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: FsReadKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn located_fs_facts_in_one_vec_do_not_dedup_collapse() {
        // Two writes to two distinct keys are two events: the `Vec`
        // storage (scrap-rs#112) preserves both, unlike a set that would
        // collapse equal-comparing facts. Pins that located facts survive
        // as separate observations on the wire.
        let facts = vec![
            BehavioralFact::FilesystemWrite {
                kind: FsCallKind::Write,
                path_key: "lit:a.txt".into(),
                path_arg_span: sample_span(),
            },
            BehavioralFact::FilesystemWrite {
                kind: FsCallKind::Write,
                path_key: "lit:b.txt".into(),
                path_arg_span: sample_span(),
            },
        ];
        let json = serde_json::to_value(&facts).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 2);
    }
}
