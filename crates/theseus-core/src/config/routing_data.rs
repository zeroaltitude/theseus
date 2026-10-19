//! `[routing.data]` (theseus-a1wvq): the owner's data rule. For now only the
//! names Jev reads with a message, as the route pack's `data_names`. The private
//! ones are `[privacy] sensitive_names` (theseus-oejh3), not a list here.

use super::privacy::PrivacyConfig;
use serde::{Deserialize, Serialize};
use theseus_judge::builders::DataNames;

/// Topics the operator names as open, kept out of the public repository.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingData {
    /// Work Jev should read as open: it may go to a model provider outside
    /// the owner's trust.
    #[serde(default)]
    pub open_topics: Vec<String>,
}

impl RoutingData {
    /// The names as Jev's state carries them: the open topics, and the
    /// privacy list as the private ones.
    pub fn names(&self, privacy: &PrivacyConfig) -> DataNames {
        DataNames {
            open: self.open_topics.clone(),
            private: privacy.sensitive_names.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RoutingConfig;

    #[test]
    fn routing_data_topics_parse() {
        let none = PrivacyConfig::default();
        let d: RoutingConfig = toml::from_str("").unwrap();
        assert_eq!(d.data, RoutingData::default());
        assert!(d.data.names(&none).is_empty(), "no names by default");
        let c: RoutingConfig = toml::from_str("[data]\nopen_topics = [\"Widgetco\"]").unwrap();
        let privacy = PrivacyConfig {
            sensitive_names: vec!["Acme".into(), "Acme payroll".into()],
        };
        let names = c.data.names(&privacy);
        assert_eq!(names.open, ["Widgetco"]);
        assert_eq!(names.private, ["Acme", "Acme payroll"]);
        assert!(toml::from_str::<RoutingConfig>("[data]\nopen = [\"x\"]").is_err());
    }

    /// The private list moved to `[privacy] sensitive_names`; the old key is
    /// refused, not read (theseus-oejh3).
    #[test]
    fn routing_data_private_topics_is_refused() {
        let e = toml::from_str::<RoutingConfig>("[data]\nprivate_topics = [\"Acme\"]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("private_topics"), "{e}");
    }
}
