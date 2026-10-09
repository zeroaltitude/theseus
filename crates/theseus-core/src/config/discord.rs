//! `[discord]`'s defaults, and which of the binding's messages go out silent
//! (theseus-l1y1).
//!
//! Every message the binding writes pings, as it always has: today's pings by
//! default, and silence is per category, per place. `[discord] silent` names
//! the categories that post without a ping (Discord's
//! `SUPPRESS_NOTIFICATIONS`: the message posts, and no device notifies); a
//! `[[channel]]` or `[[dm]]` in the bindings file may give its own list, which
//! takes the daemon's place there. `ping_window_secs`, 0 (off) by default,
//! holds a place to one ping in that long, and a place may set its own too.
//! Which messages a place shows at all is the bindings file's, per place
//! (`show_tools`, `show_thinking`).

use serde::{Deserialize, Serialize};

use super::DiscordConfig;

/// A chat category of message the binding writes, as `[discord] silent`
/// names it. The binding's `policy.rs` maps each of its writes to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// An approval card (a call's, a budget's, a layer-1 change's), and the
    /// note beside one that went to the DM.
    Cards,
    /// A failed turn, a failed task's report, and free space below the floor.
    Failures,
    /// The first part of the reply to the owner's own message.
    Answer,
    /// A reply's later parts and its footer, and a reply to anyone else's
    /// message.
    LaterParts,
    /// The reply of a turn a wake or a task's report started.
    Woken,
    /// A loop's tool line, and a notified call's embed.
    ToolLines,
    /// A finished or cancelled task's report, and a hands group's line.
    Reports,
    /// The notices: a bind, a publish, a budget's or the hours' line, Jev's
    /// notices, and glides.
    Notices,
    /// The restart notice, MCP's changes, and low free space or its return.
    Ops,
    /// A loop's thinking message.
    Thinking,
}

impl Category {
    /// Every category, in the template's order.
    pub const ALL: [Self; 10] = [
        Self::Cards,
        Self::Failures,
        Self::Answer,
        Self::LaterParts,
        Self::Woken,
        Self::ToolLines,
        Self::Reports,
        Self::Notices,
        Self::Ops,
        Self::Thinking,
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
            ping_window_secs: 0,
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

    /// Nothing is silent and no window holds a ping by default: the
    /// template's `silent` is empty and its `ping_window_secs` 0, as the
    /// defaults are, and the template names every category.
    #[test]
    fn nothing_is_silent_by_default_and_the_template_names_every_kind() {
        assert!(DiscordConfig::default().silent.is_empty());
        assert_eq!(DiscordConfig::default().ping_window_secs, 0);
        let example = crate::Config::example();
        assert!(example.discord.silent.is_empty());
        assert_eq!(example.discord.ping_window_secs, 0);
        let t = crate::Config::EXAMPLE_TOML;
        let line = t
            .lines()
            .find(|l| l.starts_with("silent = "))
            .expect("the template's [discord] silent");
        assert!(line.starts_with("silent = []"), "{line}");
        assert!(
            t.contains("\nping_window_secs = 0 "),
            "the template's window"
        );
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
        // An earlier cut's names are no category's, and are refused.
        for old in ["tools", "replies", "tasks", "notes", "disk"] {
            let bad = doc.replace("\"tool_lines\"", &format!("\"{old}\""));
            let e = format!("{:#}", crate::Config::parse(&bad).unwrap_err());
            assert!(e.contains(old), "{e}");
        }
        assert!(parse("").is_ok());
        let window = doc.replace("ping_window_secs = 0 ", "ping_window_secs = 30 ");
        let cfg = crate::Config::parse(&window).unwrap().0;
        assert_eq!(cfg.discord.ping_window_secs, 30);
    }
}
