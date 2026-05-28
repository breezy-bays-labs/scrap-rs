//! SARIF rule metadata — one [`RuleDescription`] per [`SmellCategory`].
//!
//! Lives in a dedicated module so the human-facing rule names + short
//! / full descriptions stay in ONE place. Both the SARIF reporter's
//! `tool.driver.rules[]` builder and any future reporter that surfaces
//! per-category prose (markdown rule appendix, HTML help panel) read
//! from here rather than re-stating the text.
//!
//! The `rule_id` for each category is NOT stored here — it is the
//! category's wire string ([`SmellCategory::as_wire_str`]), the single
//! source of truth shared with the JSON envelope's `by_smell` keys and
//! the `Smell::category` serde rename. Storing it twice would risk
//! drift between the SARIF `ruleId` and the rest of the wire surface.

use crate::domain::smell::SmellCategory;

/// Static rule metadata for one smell category — drives a SARIF
/// `reportingDescriptor` (`name`, `shortDescription`, `fullDescription`).
///
/// `id` is intentionally absent: it is `category.as_wire_str()`,
/// threaded in by the reporter so the SARIF `ruleId` cannot drift from
/// the rest of the wire surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleDescription {
    /// SARIF `reportingDescriptor.name` — `PascalCase` human-facing
    /// rule name (SARIF convention; distinct from the `snake_case`
    /// `id`).
    pub name: &'static str,
    /// SARIF `shortDescription.text` — one-line summary.
    pub short_description: &'static str,
    /// SARIF `fullDescription.text` — multi-sentence detail.
    pub full_description: &'static str,
}

/// Look up the [`RuleDescription`] for a smell category.
///
/// Total over `SmellCategory`'s v0.1 variants. The match is the single
/// definition site for SARIF rule prose; adding a new `SmellCategory`
/// variant (v0.3+) requires a new arm here (the `#[non_exhaustive]`
/// enum means the compiler won't force it, so the
/// `every_category_has_a_rule_description` test pins the coverage).
#[must_use]
pub fn rule_description(category: SmellCategory) -> RuleDescription {
    match category {
        SmellCategory::ZeroAssertion => RuleDescription {
            name: "ZeroAssertion",
            short_description: "Test exercises the system under test but never asserts on the result.",
            full_description: "The test body invokes the code under test but contains no assertion \
                               (and no implicit-assertion source such as proptest, quickcheck, \
                               cucumber, trybuild, insta, kani, or #[should_panic]). A passing \
                               run proves only that the code did not panic — add assertions for \
                               the function's observable effects.",
        },
        SmellCategory::TautologicalAssertion => RuleDescription {
            name: "TautologicalAssertion",
            short_description: "Assertion is structurally incapable of failing.",
            full_description: "An assertion such as assert!(true), assert_eq!(x, x), or a \
                               literal-vs-literal comparison can never fail and therefore conveys \
                               no information. Replace the tautology with an assertion that \
                               exercises the actual behavior under test.",
        },
        SmellCategory::NoOpIo => RuleDescription {
            name: "NoOpIo",
            short_description: "I/O is performed but its result is discarded.",
            full_description: "The test performs I/O (file open/read, HTTP request, etc.) whose \
                               result is bound to `_` or otherwise discarded with no follow-up \
                               check. Inspect or assert on the data returned by the I/O call so \
                               the test verifies behavior rather than mere execution.",
        },
        SmellCategory::SurfaceOnlyIo => RuleDescription {
            name: "SurfaceOnlyIo",
            short_description: "Test asserts only on I/O surface metadata, not the payload.",
            full_description: "The test asserts only on surface-level metadata of an I/O operation \
                               (a status code, file existence) without inspecting the substantive \
                               payload. Assert on the returned data, not just that the operation \
                               appeared to occur.",
        },
        SmellCategory::LargeExample => RuleDescription {
            name: "LargeExample",
            short_description: "Test body exceeds the configured line threshold.",
            full_description: "The test body is longer than the configured line threshold, a \
                               signal that it tests multiple behaviors at once or carries heavy \
                               inline setup. Split it into focused examples or extract setup \
                               helpers so each test has a single reason to fail.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_a_rule_description() {
        // Pins coverage over the v0.1 SmellCategory slate. A new
        // variant (v0.3+) that forgets a `rule_description` arm fails
        // to compile (the match is exhaustive); this test additionally
        // pins that the prose fields are non-empty.
        for category in [
            SmellCategory::ZeroAssertion,
            SmellCategory::TautologicalAssertion,
            SmellCategory::NoOpIo,
            SmellCategory::SurfaceOnlyIo,
            SmellCategory::LargeExample,
        ] {
            let desc = rule_description(category);
            assert!(!desc.name.is_empty(), "{category:?} name must be non-empty");
            assert!(
                !desc.short_description.is_empty(),
                "{category:?} short_description must be non-empty",
            );
            assert!(
                !desc.full_description.is_empty(),
                "{category:?} full_description must be non-empty",
            );
        }
    }

    #[test]
    fn rule_name_is_pascal_case_per_sarif_convention() {
        // SARIF reportingDescriptor.name is conventionally PascalCase,
        // distinct from the snake_case id (the wire string). Pin the
        // mapping for one representative category.
        assert_eq!(
            rule_description(SmellCategory::ZeroAssertion).name,
            "ZeroAssertion",
        );
        // And the id (wire string) is the snake_case form, sourced
        // separately from SmellCategory::as_wire_str.
        assert_eq!(SmellCategory::ZeroAssertion.as_wire_str(), "zero_assertion");
    }
}
