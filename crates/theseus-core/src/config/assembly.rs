//! `[context] assembly` (issue 44): how a request's context is assembled, `"transcript"` (as built) or
//! `"view"` (`crate::view`). One value for every place, or a default and values by place. Absent:
//! `"view"` everywhere. Read only by `ContextConfig::assembly_for`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The two ways a request's context is assembled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assembly {
    /// Today's compiled transcript.
    Transcript,
    /// The view: slots, a gate and one budget.
    #[default]
    View,
}

/// `[context] assembly`: `assembly = "view"`, or
/// `assembly = { default = "transcript", places = { "<place>" = "view" } }`, a place as the ledger writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssemblySetting {
    All(Assembly),
    ByPlace(AssemblyByPlace),
}

/// The table form of `[context] assembly`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyByPlace {
    #[serde(default)]
    pub default: Assembly,
    #[serde(default)]
    pub places: BTreeMap<String, Assembly>,
}

impl AssemblySetting {
    /// Whether any place is set to `view`.
    pub fn any_view(&self) -> bool {
        match self {
            Self::All(a) => *a == Assembly::View,
            Self::ByPlace(t) => {
                t.default == Assembly::View || t.places.values().any(|a| *a == Assembly::View)
            }
        }
    }

    /// The assembly for `place`.
    pub fn for_place(&self, place: &str) -> Assembly {
        match self {
            Self::All(a) => *a,
            Self::ByPlace(t) => t.places.get(place).copied().unwrap_or(t.default),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ContextConfig;
    use super::Assembly;

    fn context(toml: &str) -> ContextConfig {
        toml::from_str(toml).expect("loads")
    }

    #[test]
    fn assembly_defaults_to_the_view_and_reads_by_place() {
        assert_eq!(context("").assembly_for("local"), Assembly::View);
        assert_eq!(
            context("assembly = \"view\"").assembly_for("local"),
            Assembly::View
        );
        let by = context(
            "[assembly]\ndefault = \"transcript\"\nplaces = { \"discord:1\" = \"view\" }\n",
        );
        assert_eq!(by.assembly_for("discord:1"), Assembly::View);
        assert_eq!(by.assembly_for("local"), Assembly::Transcript);
        assert!(toml::from_str::<ContextConfig>("assembly = \"other\"").is_err());
        assert!(!context("assembly = \"view\"").is_empty());
    }

    /// A view reads the exchange ledger: a view with the ledger off is refused at load, saying why.
    #[test]
    fn a_view_with_the_ledger_off_is_refused() {
        let err = context("ledger = false\nassembly = \"view\"")
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs the ledger on"), "{err}");
        let by = "ledger = false\n[assembly]\nplaces = { \"local\" = \"view\" }\n";
        assert!(context(by).validate().is_err());
        assert!(context("ledger = false\nassembly = \"transcript\"")
            .validate()
            .is_ok());
        assert!(context("assembly = \"view\"").validate().is_ok());
    }
}
