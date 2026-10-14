//! `[tui]` (theseus-f0bi9): how the terminal clients ping the owner. The TUI
//! (and `theseus status --watch`, by its own flag) reads it from `health`;
//! `--notify` overrides it.

use serde::{Deserialize, Serialize};

/// The default of `notify`: one constant, so a change of mind is one line.
pub const NOTIFY: &str = "auto";

/// `[tui]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuiConfig {
    /// `auto` (a Windows toast under WSL, OSC 9 in a terminal that has it,
    /// else the bell), `toast`, `bell`, `osc9`, `osc777`, or `off`.
    #[serde(default = "notify")]
    pub notify: String,
}

fn notify() -> String {
    NOTIFY.to_string()
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self { notify: notify() }
    }
}

impl TuiConfig {
    /// Whether `notify` names a way the clients know.
    pub fn validate(&self) -> anyhow::Result<()> {
        if !theseus_protocol::notices::NOTIFY_KINDS.contains(&self.notify.as_str()) {
            anyhow::bail!(
                "tui.notify = {:?} is not one of {}",
                self.notify,
                theseus_protocol::notices::NOTIFY_KINDS.join(", ")
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_templates_tui_section_is_the_default_and_a_wrong_kind_is_refused() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.tui, TuiConfig::default());
        assert_eq!(cfg.tui.notify, "auto");
        assert!(crate::Config::EXAMPLE_TOML.contains("[tui]\nnotify = \"auto\""));
        let with = |v: &str| {
            crate::Config::parse(
                &crate::Config::EXAMPLE_TOML
                    .replace("notify = \"auto\"", &format!("notify = \"{v}\"")),
            )
        };
        for kind in theseus_protocol::notices::NOTIFY_KINDS {
            assert_eq!(with(kind).unwrap().0.tui.notify, kind);
        }
        let e = with("loud").unwrap_err().to_string();
        assert!(e.contains("tui.notify"), "{e}");
    }
}
