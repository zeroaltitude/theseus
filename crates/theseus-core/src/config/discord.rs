//! `[discord]`'s defaults, and which of the binding's messages go out silent
//! (theseus-l1y1).
//!
//! Every message the binding writes notifies, as it always has: the owner
//! wants a lively chat, and a ping for each. `[discord] silent` names the
//! kinds that post without one (Discord's `SUPPRESS_NOTIFICATIONS`: the
//! message posts, and no device notifies). It is empty by default, and it is
//! daemon-wide, not per place: a ping is about the owner's phone, a card for
//! a shared place lands in the owner's DM (so a place's own word would not
//! say which place's applies), and Discord's own channel settings already
//! mute a whole place. Which messages a place shows at all is the bindings
//! file's, per place (`show_tools`, `show_thinking`).

use serde::{Deserialize, Serialize};

use super::DiscordConfig;

/// A kind of message the binding writes, as `[discord] silent` names it.
/// The binding's `policy.rs` maps each of its writes to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The first part of the reply to the owner's own message.
    Answer,
    /// A reply's later parts and its footer, and a reply to anyone else's
    /// message.
    Replies,
    /// The reply of a turn a wake or a task's report started.
    Woken,
    /// A loop's tool line, and a notified call's embed.
    Tools,
    /// A loop's thinking message.
    Thinking,
    /// An approval card (a call's, a budget's, a layer-1 change's), and the
    /// note beside one that went to the DM.
    Cards,
    /// A failed turn, and a failed task's report.
    Failures,
    /// A finished or cancelled task's report, and the task board.
    Tasks,
    /// The notices: a bind, a publish, a budget's or the hours' line, the
    /// restart notice, MCP and Jev notices, glides, and hands lines.
    Notes,
    /// Free space under the state dir: low, below the floor, or back.
    Disk,
}

impl Category {
    /// Every category, in the template's order.
    pub const ALL: [Self; 10] = [
        Self::Answer,
        Self::Replies,
        Self::Woken,
        Self::Tools,
        Self::Thinking,
        Self::Cards,
        Self::Failures,
        Self::Tasks,
        Self::Notes,
        Self::Disk,
    ];
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            token_secret: super::default_discord_token_secret(),
            bindings_file: super::default_bindings_file(),
            edit_interval_ms: super::default_edit_interval_ms(),
            notice_embeds: false,
            silent: Vec::new(),
            rest_proxy: None,
            gateway_proxy: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> anyhow::Result<crate::Config> {
        let base = crate::Config::EXAMPLE_TOML;
        Ok(crate::Config::parse(&format!("{base}\n{toml}"))?.0)
    }

    /// Nothing is silent by default: the template's `silent` is empty, as
    /// the default is, and the template names every category.
    #[test]
    fn nothing_is_silent_by_default_and_the_template_names_every_kind() {
        assert!(DiscordConfig::default().silent.is_empty());
        assert!(crate::Config::example().discord.silent.is_empty());
        let t = crate::Config::EXAMPLE_TOML;
        let line = t
            .lines()
            .find(|l| l.starts_with("silent = "))
            .expect("the template's [discord] silent");
        assert!(line.starts_with("silent = []"), "{line}");
        for c in Category::ALL {
            let name = toml::Value::try_from(c).unwrap();
            let name = name.as_str().unwrap();
            assert!(
                t.contains(&format!("\"{name}\"")),
                "the template names {name}"
            );
        }
    }

    /// Each category loads by its name; a name it does not know is refused.
    #[test]
    fn each_category_loads_and_an_unknown_one_is_refused() {
        let all: Vec<String> = Category::ALL
            .iter()
            .map(|c| format!("{}", toml::Value::try_from(c).unwrap()))
            .collect();
        let doc = format!("[discord]\nsilent = [{}]\n", all.join(", "));
        let base = crate::Config::EXAMPLE_TOML.replace("silent = []", "");
        let cfg = crate::Config::parse(&format!("{base}\n")).unwrap().0;
        assert!(cfg.discord.silent.is_empty());
        let at = base.find("[discord]\n").unwrap();
        let doc = format!("{}{doc}{}", &base[..at], &base[at + "[discord]\n".len()..]);
        let cfg = crate::Config::parse(&doc).unwrap().0;
        assert_eq!(cfg.discord.silent, Category::ALL);
        let bad = doc.replace("\"tools\"", "\"tool_lines\"");
        let e = format!("{:#}", crate::Config::parse(&bad).unwrap_err());
        assert!(e.contains("tool_lines"), "{e}");
        assert!(parse("").is_ok());
    }
}
