//! `[routing.data]` (theseus-a1wvq): the owner's data rule. For now only the
//! names Jev reads with a message, as route.v6's `data_names`.

use serde::{Deserialize, Serialize};
use theseus_judge::builders::DataNames;

/// Topics the operator names, kept out of the public repository: this table is
/// the only place an organisation or project name lives.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingData {
    /// Work Jev should read as open: it may go to a model provider outside
    /// the owner's trust.
    #[serde(default)]
    pub open_topics: Vec<String>,
    /// Matter Jev should read as private: it never does.
    #[serde(default)]
    pub private_topics: Vec<String>,
}

impl RoutingData {
    /// The names as Jev's state carries them.
    pub fn names(&self) -> DataNames {
        DataNames {
            open: self.open_topics.clone(),
            private: self.private_topics.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RoutingConfig;

    #[test]
    fn routing_data_topics_parse() {
        let d: RoutingConfig = toml::from_str("").unwrap();
        assert_eq!(d.data, RoutingData::default());
        assert!(d.data.names().is_empty(), "no names by default");
        let c: RoutingConfig = toml::from_str(
            "[data]\nopen_topics = [\"Widgetco\"]\nprivate_topics = [\"Acme\", \"Acme payroll\"]",
        )
        .unwrap();
        let names = c.data.names();
        assert_eq!(names.open, ["Widgetco"]);
        assert_eq!(names.private, ["Acme", "Acme payroll"]);
        assert!(toml::from_str::<RoutingConfig>("[data]\nopen = [\"x\"]").is_err());
    }
}
