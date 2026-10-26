//! `[routing.data]` (theseus-a1wvq): the owner's data rule. For now only the
//! names Jev reads with a message, as the route pack's `data_names`. The private
//! ones are `[privacy] sensitive_names` (theseus-oejh3), not a list here. And
//! the trees an open request's tools may reach (`open_paths`, theseus-a1wvq.7).

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
    /// Trees open data lives in, `~/` or absolute: an open request's file
    /// tools reach these alone, and its programs run only in a session
    /// whose directory is under one (`open_view::open_roots`). Default none.
    #[serde(default, deserialize_with = "rooted")]
    pub open_paths: Vec<String>,
    /// Read-only paths the owner certifies hold nothing private, `~/` or
    /// absolute, beyond the system's own (`data::public_reach`): an L1 job
    /// whose recorded view binds only these and the open roots may have its
    /// output screened for a restricted-origin model (theseus-ouqru). Owner-set
    /// and additive, as `open_paths` is: only an edit of the config widens it.
    /// Default none.
    #[serde(default, deserialize_with = "rooted")]
    pub public_reach: Vec<String>,
}

/// `open_paths`, each absolute or under `~/`: a relative path would be the
/// daemon's directory's, so it is refused.
fn rooted<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let paths = Vec::<String>::deserialize(d)?;
    match paths
        .iter()
        .find(|p| !(p.starts_with('/') || p.starts_with("~/")))
    {
        Some(p) => Err(serde::de::Error::custom(format!(
            "{p:?} must be an absolute path or start with ~/"
        ))),
        None => Ok(paths),
    }
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

    /// `open_paths` parses, defaults empty, refuses a relative entry, and is
    /// expanded and canonical as the open roots; an unknown key is refused.
    #[test]
    fn open_paths_parse() {
        let d: RoutingConfig = toml::from_str("[data]").unwrap();
        assert!(d.data.open_paths.is_empty(), "none by default");
        let c: RoutingConfig =
            toml::from_str("[data]\nopen_paths = [\"~/src/widgetco\", \"/srv/open\"]").unwrap();
        assert_eq!(c.data.open_paths, ["~/src/widgetco", "/srv/open"]);
        let e = toml::from_str::<RoutingConfig>("[data]\nopen_paths = [\"src\"]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("must be an absolute path"), "{e}");
        let e = toml::from_str::<RoutingConfig>("[data]\nopen_roots = [\"/srv\"]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("open_roots"), "{e}");
        let mut cfg = crate::Config::example();
        cfg.routing.data = c.data;
        let home = crate::config::expand("~/");
        let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Task, None);
        let roots = crate::open_view::open_roots(&cfg, &rec);
        assert_eq!(roots.len(), 2);
        assert!(roots[0].starts_with(theseus_tools::paths::canonical_best_effort(&home)));
        assert!(roots[0].ends_with("src/widgetco"), "{roots:?}");
        assert_eq!(roots[1], std::path::Path::new("/srv/open"));
    }

    /// `public_reach` (theseus-ouqru) parses, defaults empty, takes `~/` and
    /// absolute paths alone, and is never one of the open paths.
    #[test]
    fn public_reach_parses() {
        let d: RoutingConfig = toml::from_str("[data]").unwrap();
        assert!(d.data.public_reach.is_empty(), "none by default");
        let c: RoutingConfig =
            toml::from_str("[data]\npublic_reach = [\"~/.cargo/registry\", \"/opt/tools\"]")
                .unwrap();
        assert_eq!(c.data.public_reach, ["~/.cargo/registry", "/opt/tools"]);
        assert!(c.data.open_paths.is_empty());
        let e = toml::from_str::<RoutingConfig>("[data]\npublic_reach = [\"tools\"]")
            .unwrap_err()
            .to_string();
        assert!(e.contains("must be an absolute path"), "{e}");
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
