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
    /// notices, and glides. Not the AWS spend-safety alerts, which no list silences.
    Notices,
    /// The restart notice, MCP's changes, and low free space or its return.
    Ops,
}

impl Category {
    /// Every category, in the template's order.
    pub const ALL: [Self; 9] = [
        Self::Cards,
        Self::Failures,
        Self::Answer,
        Self::LaterParts,
        Self::Woken,
        Self::ToolLines,
        Self::Reports,
        Self::Notices,
        Self::Ops,
    ];
}

pub(super) fn default_task_channel_after_minutes() -> u64 {
    10
}
pub(super) fn default_task_channel_cap() -> u64 {
    25
}
pub(super) fn default_task_channel_keep_days() -> u64 {
    14
}

/// `[discord] digest_at`'s default: eight in the morning and six in the evening.
pub fn default_digest_at() -> Vec<String> {
    vec!["08:00".into(), "18:00".into()]
}

/// `digest_at` as (hour, minute) pairs, each `HH:MM` on a 24-hour clock.
pub fn digest_slots(at: &[String]) -> anyhow::Result<Vec<(u32, u32)>> {
    at.iter()
        .map(|t| {
            let hm = t
                .split_once(':')
                .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)));
            match hm {
                Some((h, m)) if h < 24 && m < 60 && t.len() == 5 => Ok((h, m)),
                _ => anyhow::bail!("[discord] digest_at: {t:?} is not a time like \"08:00\""),
            }
        })
        .collect()
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
            live_board: true,
            task_channels: true,
            task_channel_after_minutes: default_task_channel_after_minutes(),
            task_channel_cap: default_task_channel_cap(),
            task_channel_keep_days: default_task_channel_keep_days(),
            status_line: true,
            app_emojis: true,
            digest_at: default_digest_at(),
            rest_proxy: None,
            gateway_proxy: None,
            forum: true,
            forum_channel: default_forum_channel(),
            task_home: default_task_home(),
            thread: Vec::new(),
            calendar: Default::default(),
        }
    }
}

/// The default forum's name (theseus-bh3t.6).
pub fn default_forum_channel() -> String {
    "theseus".into()
}

/// The task home's name (theseus-xlxp4).
pub fn default_task_home() -> String {
    "theseus-tasks".into()
}

/// `[discord] task_home`: empty (off), a Discord id, or a name, as `forum_channel` is.
pub fn validate_task_home(v: &str) -> anyhow::Result<()> {
    if v.is_empty() {
        return Ok(());
    }
    validate_forum_channel(v).map_err(|_| {
        anyhow::anyhow!(
            "discord.task_home {v:?} is not empty (off), a Discord id (15-21 digits) nor a name \
             (1-100 characters, no `#`)"
        )
    })
}

/// `[discord] forum_channel`: a Discord id (15 to 21 digits) or a name of 1 to
/// 100 characters with no `#`.
pub fn validate_forum_channel(v: &str) -> anyhow::Result<()> {
    let digits = v.bytes().all(|c| c.is_ascii_digit());
    let id = (15..=21).contains(&v.len()) && digits;
    let name = !v.is_empty() && v.chars().count() <= 100 && !v.contains('#');
    if !id && !name {
        anyhow::bail!(
            "discord.forum_channel {v:?} is neither a Discord id (15-21 digits) nor a name \
             (1-100 characters, no `#`)"
        );
    }
    Ok(())
}

/// `[[discord.thread]]` (theseus-sjzm): the channel `channel` (an id the
/// bindings file names, bound private) runs as the thread `thread`'s place:
/// its messages go to that thread's session. `/thread bind` does the same
/// from Discord and records it in the thread's surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadBind {
    pub channel: String,
    pub thread: String,
}

impl ThreadBind {
    /// Each channel and each thread named once, the channel a Discord id and
    /// the thread a slug.
    pub fn validate_all(list: &[Self]) -> anyhow::Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for b in list {
            let id_ok = (15..=21).contains(&b.channel.len())
                && b.channel.bytes().all(|c| c.is_ascii_digit());
            if !id_ok {
                anyhow::bail!(
                    "discord.thread: channel {:?} is not a Discord id (15-21 digits, as a string)",
                    b.channel
                );
            }
            if !theseus_protocol::threads::slug_ok(&b.thread) {
                anyhow::bail!(
                    "discord.thread: {:?} is not a thread slug (lower-case letters, digits and hyphens)",
                    b.thread
                );
            }
            if !seen.insert(("c", b.channel.clone())) || !seen.insert(("t", b.thread.clone())) {
                anyhow::bail!(
                    "discord.thread: channel {} or thread {} is named twice",
                    b.channel,
                    b.thread
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> anyhow::Result<crate::Config> {
        let base = crate::Config::EXAMPLE_TOML;
        Ok(crate::Config::parse(&format!("{base}\n{toml}"))?.0)
    }

    /// The task home is on by default, in the template as in the code, and an
    /// empty name turns it off; a name or an id is valid, a `#` is not.
    #[test]
    fn the_task_home_is_on_by_default_and_empty_is_off() {
        assert_eq!(DiscordConfig::default().task_home, "theseus-tasks");
        assert_eq!(crate::Config::example().discord.task_home, "theseus-tasks");
        for ok in ["", "theseus-tasks", "900000000000000012"] {
            assert!(validate_task_home(ok).is_ok(), "{ok:?}");
        }
        assert!(validate_task_home("#tasks").is_err());
        assert!(validate_task_home(&"x".repeat(101)).is_err());
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
        for old in ["tools", "replies", "tasks", "notes", "disk", "thinking"] {
            let bad = doc.replace("\"tool_lines\"", &format!("\"{old}\""));
            let e = format!("{:#}", crate::Config::parse(&bad).unwrap_err());
            assert!(e.contains(old), "{e}");
        }
        assert!(parse("").is_ok());
        let window = doc.replace("ping_window_secs = 0 ", "ping_window_secs = 30 ");
        let cfg = crate::Config::parse(&window).unwrap().0;
        assert_eq!(cfg.discord.ping_window_secs, 30);
    }

    /// `[[discord.thread]]` loads, and a bad id, a bad slug or a channel or
    /// thread named twice is refused (theseus-sjzm).
    #[test]
    fn a_discord_thread_binding_loads_and_a_bad_one_is_refused() {
        let ok = parse(
            "[[discord.thread]]\nchannel = \"123456789012345678\"\nthread = \"tide-gauges\"\n",
        )
        .unwrap();
        assert_eq!(ok.discord.thread.len(), 1);
        assert_eq!(ok.discord.thread[0].thread, "tide-gauges");
        assert!(crate::Config::example().discord.thread.is_empty());
        let one =
            |c: &str, t: &str| format!("[[discord.thread]]\nchannel = \"{c}\"\nthread = \"{t}\"\n");
        let twice = format!(
            "{}{}",
            one("123456789012345678", "a"),
            one("123456789012345678", "b")
        );
        for bad in [
            one("lab", "a"),
            one("123456789012345678", "Bad Slug"),
            twice,
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }
}
