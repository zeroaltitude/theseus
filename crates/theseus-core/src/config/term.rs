//! `[tools.term]` (theseus-ggqf): what a terminal's close leaves running.
//!
//! A program started in the background in a terminal (`nohup app &`, a
//! `setsid` child, a server that daemonizes) keeps running after its
//! session ends and after the daemon stops, as one started through
//! `proc.run` does: one rule for both ways of running a program. The
//! terminal's own program and its foreground process group still end.
//! `term.close`, a cancel and a `/stop` end everything, as before.

use serde::{Deserialize, Serialize};

/// The default of `keep_background`: one constant, so a change of mind is
/// one line.
pub const KEEP_BACKGROUND: bool = true;

/// `[tools.term]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermToolsConfig {
    /// Background processes a terminal started survive its session's end
    /// and the daemon's stop. `false`: every close kills everything the
    /// terminal started, as before theseus-ggqf.
    #[serde(default = "keep_background")]
    pub keep_background: bool,
}

fn keep_background() -> bool {
    KEEP_BACKGROUND
}

impl Default for TermToolsConfig {
    fn default() -> Self {
        Self {
            keep_background: KEEP_BACKGROUND,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[tools.term]` is the default, and a note may turn
    /// it off.
    #[test]
    fn the_templates_term_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.tools.term, TermToolsConfig::default());
        assert!(cfg.tools.term.keep_background);
        assert!(crate::Config::EXAMPLE_TOML.contains("[tools.term]\nkeep_background = true"));
        assert!(crate::config::ToolsConfig::default().term.keep_background);
        let with = |line: &str| {
            crate::Config::from_toml(
                &crate::Config::EXAMPLE_TOML.replace("keep_background = true", line),
            )
        };
        assert!(
            !with("keep_background = false")
                .unwrap()
                .tools
                .term
                .keep_background
        );
        assert!(with("keep = true").is_err(), "an unknown key is refused");
    }
}
