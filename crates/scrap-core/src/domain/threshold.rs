//! `ThresholdMode` — the strict/default/lenient cutoff selector.
//!
//! Penalty-sum cutoffs are intentionally not pinned in v0.1 — each
//! detector PR (P13–P17) will set its own contribution and the cutoff
//! tables land alongside the scorecard-row reporter (P21). The mode
//! itself is stable wire surface from day one.

use serde::{Deserialize, Serialize};

/// Threshold mode chosen by `--threshold-mode` or `scrap.toml`.
///
/// `Default` is the suggested gate for new adopters; `Strict` adds
/// pressure for repos that already enforce assertion discipline;
/// `Lenient` is for legacy crates being onboarded incrementally.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ThresholdMode {
    /// Strict mode — tightest cutoffs.
    #[serde(rename = "strict")]
    Strict,
    /// Default mode — middle cutoffs.
    #[default]
    #[serde(rename = "default")]
    Default,
    /// Lenient mode — loosest cutoffs.
    #[serde(rename = "lenient")]
    Lenient,
}

impl ThresholdMode {
    /// Stable wire string. Used by scorecard-row and SARIF reporters
    /// that need the value outside a serde context.
    #[must_use]
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Default => "default",
            Self::Lenient => "lenient",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_default_mode() {
        assert_eq!(ThresholdMode::default(), ThresholdMode::Default);
    }

    #[test]
    fn serializes_snake_case() {
        let cases = [
            (ThresholdMode::Strict, "strict"),
            (ThresholdMode::Default, "default"),
            (ThresholdMode::Lenient, "lenient"),
        ];
        for (mode, wire) in cases {
            assert_eq!(
                serde_json::to_value(mode).unwrap(),
                serde_json::Value::String(wire.into()),
            );
            assert_eq!(mode.as_wire_str(), wire);
        }
    }

    #[test]
    fn ordering_strict_default_lenient() {
        assert!(ThresholdMode::Strict < ThresholdMode::Default);
        assert!(ThresholdMode::Default < ThresholdMode::Lenient);
    }
}
