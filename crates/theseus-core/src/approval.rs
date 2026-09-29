//! Approval (spec §3.9 "Approval", theseus-sgh): an answer to a waiting call,
//! a tool call or a budget question, counts only from a trusted user through
//! a trusted channel. Both lists are `[approval]` in the vault-held config,
//! which agents cannot write. Without that section there is no rule, and
//! every surface answers as it did before: the CLI, the local web UI, and a
//! place's listed Discord users.
//!
//! The answer is judged in one place, `Core::confirm_action`, which every
//! surface reaches through `action.confirm`. The judgment trusts the
//! connection, never what a client says. The listener that accepts a
//! connection names its surface: the Unix socket and `--stdio` are the CLI,
//! the loopback bridge is the web UI, and the in-process binding is Discord.
//! Only the Discord binding may name a Discord channel and user. Who can view
//! a guild channel is the binding's to check, since it holds the Discord API;
//! it reports each check here, and an answer is judged against the latest.

use std::collections::BTreeMap;
use std::sync::RwLock;

use theseus_protocol::{ApprovalChannel, ApprovalStatus, DiscordOrigin};

use crate::config::ApprovalConfig;

/// Where a protocol connection comes from, as the listener that accepted it
/// knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The Unix socket (mode 0600) or `--stdio`: `theseus` on this machine.
    Cli,
    /// The loopback web UI's WebSocket.
    Web,
    /// The Discord binding, in process.
    Discord,
    /// A connection no listener named (a test's). Never a trusted channel.
    Unnamed,
}

impl Surface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Web => "web",
            Self::Discord => "discord",
            Self::Unnamed => "unnamed",
        }
    }

    /// The surface in a sentence.
    fn name(self) -> &'static str {
        match self {
            Self::Cli => "the CLI",
            Self::Web => "the web UI",
            Self::Discord => "the Discord binding",
            Self::Unnamed => "a connection no listener named",
        }
    }
}

/// A protocol connection: the label that names it in logs and the ledger
/// (`sock#3`, `web#1`, `discord`), and its surface.
#[derive(Debug, Clone)]
pub struct Client {
    pub label: String,
    pub surface: Surface,
}

impl Client {
    pub fn new(label: impl Into<String>, surface: Surface) -> Self {
        Self {
            label: label.into(),
            surface,
        }
    }
}

/// A bare label is a connection no listener named.
impl From<String> for Client {
    fn from(label: String) -> Self {
        Self::new(label, Surface::Unnamed)
    }
}

impl From<&str> for Client {
    fn from(label: &str) -> Self {
        Self::new(label, Surface::Unnamed)
    }
}

/// Who answered a waiting call, and through what.
#[derive(Debug, Clone)]
pub struct Answerer {
    /// Shown and ledgered as `by`: the answer's `author`, else the
    /// connection's label.
    pub label: String,
    pub surface: Surface,
    /// What the Discord binding read off the button press.
    pub discord: Option<DiscordOrigin>,
}

/// A bare label (a test, an in-process caller) answers through no surface
/// Theseus knows: it counts only without an `[approval]` section.
impl From<&str> for Answerer {
    fn from(label: &str) -> Self {
        Self {
            label: label.to_string(),
            surface: Surface::Unnamed,
            discord: None,
        }
    }
}

impl Answerer {
    /// The channel the answer came through, as `[approval].channels` would
    /// name it.
    pub fn via(&self) -> String {
        match (self.surface, &self.discord) {
            (Surface::Discord, Some(d)) if d.guild_id.is_none() => "discord:dm".into(),
            (Surface::Discord, Some(d)) => format!("discord:{}", d.channel_id),
            (s, _) => s.as_str().into(),
        }
    }

    /// Who answered: a Discord user by id, anyone else by label.
    pub fn who(&self) -> String {
        match (&self.discord, self.surface) {
            (Some(d), Surface::Discord) => format!("discord:{} ({})", d.user_id, self.label),
            _ => self.label.clone(),
        }
    }
}

/// An entry of `[approval].channels`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Cli,
    Web,
    /// A DM between the bot and a trusted user.
    DiscordDm,
    /// A guild channel, by id: trusted while only trusted users can view it.
    Discord(u64),
}

impl Channel {
    /// One entry, or the error that names the forms.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "cli" => Ok(Self::Cli),
            "web" => Ok(Self::Web),
            "discord:dm" => Ok(Self::DiscordDm),
            _ => s
                .strip_prefix("discord:")
                .and_then(snowflake)
                .map(Self::Discord)
                .ok_or_else(|| {
                    format!(
                        "approval.channels entry {s:?} is not a channel: write \"cli\", \"web\", \
                         \"discord:dm\", or \"discord:<channel id>\""
                    )
                }),
        }
    }

    /// As `[approval].channels` names it.
    pub fn key(self) -> String {
        match self {
            Self::Cli => "cli".into(),
            Self::Web => "web".into(),
            Self::DiscordDm => "discord:dm".into(),
            Self::Discord(id) => format!("discord:{id}"),
        }
    }
}

/// A trusted user, `discord:<user id>`, or the error that names the form.
pub fn parse_user(s: &str) -> Result<u64, String> {
    s.strip_prefix("discord:")
        .and_then(snowflake)
        .ok_or_else(|| {
            format!(
                "approval.trusted_users entry {s:?} is not a surface-qualified id: write \
                 \"discord:<user id>\""
            )
        })
}

/// A Discord id: 15 to 21 decimal digits.
fn snowflake(s: &str) -> Option<u64> {
    let digits = (15..=21).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| s.parse().ok()).flatten()
}

/// What the Discord binding found when it checked who can view a guild
/// channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub trusted: bool,
    /// Why, in words.
    pub detail: String,
    pub at_ms: u64,
}

/// Why an answer does not count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub who: String,
    pub via: String,
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the answer from {} does not count: {}. It keeps waiting for an answer that does",
            self.who, self.why
        )
    }
}

impl std::error::Error for Refusal {}

/// `[approval]`, resolved, with the Discord binding's latest checks.
#[derive(Default)]
pub struct Approval {
    /// None without an `[approval]` section: no rule.
    rules: Option<Rules>,
    /// Guild channel id → the binding's latest check of who can view it.
    checked: RwLock<BTreeMap<u64, Checked>>,
}

struct Rules {
    users: Vec<u64>,
    channels: Vec<Channel>,
}

impl Rules {
    fn lists(&self, c: Channel) -> bool {
        self.channels.contains(&c)
    }

    /// `channels = [...]` as configured, for a reason.
    fn listing(&self) -> String {
        let keys: Vec<String> = self
            .channels
            .iter()
            .map(|c| format!("\"{}\"", c.key()))
            .collect();
        format!("[approval] channels = [{}]", keys.join(", "))
    }
}

impl Approval {
    /// From the loaded config, which has validated every entry.
    pub fn new(cfg: Option<&ApprovalConfig>) -> Self {
        let rules = cfg.map(|c| Rules {
            users: c
                .trusted_users
                .iter()
                .filter_map(|u| parse_user(u).ok())
                .collect(),
            channels: c
                .channels
                .iter()
                .filter_map(|s| Channel::parse(s).ok())
                .collect(),
        });
        Self {
            rules,
            checked: RwLock::default(),
        }
    }

    /// The config has an `[approval]` section.
    pub fn configured(&self) -> bool {
        self.rules.is_some()
    }

    /// Judge an answer: Ok when it counts, else why it does not.
    pub fn judge(&self, a: &Answerer) -> Result<(), Refusal> {
        let Some(rules) = &self.rules else {
            return Ok(());
        };
        let refuse = |why: String| {
            Err(Refusal {
                who: a.who(),
                via: a.via(),
                why,
            })
        };
        if a.discord.is_some() && a.surface != Surface::Discord {
            return refuse(format!(
                "only the Discord binding can name a Discord channel and user, and this answer \
                 came through {}",
                a.surface.name()
            ));
        }
        match a.surface {
            Surface::Cli if rules.lists(Channel::Cli) => Ok(()),
            Surface::Web if rules.lists(Channel::Web) => Ok(()),
            Surface::Cli | Surface::Web => refuse(format!(
                "{} is not a trusted channel ({})",
                a.surface.name(),
                rules.listing()
            )),
            Surface::Unnamed => refuse(format!(
                "it came through {}, which is never a trusted channel",
                a.surface.name()
            )),
            Surface::Discord => {
                let Some(d) = &a.discord else {
                    return refuse("the Discord binding named no channel and user for it".into());
                };
                let mut why = Vec::new();
                if !snowflake(&d.user_id).is_some_and(|u| rules.users.contains(&u)) {
                    why.push(format!(
                        "discord:{} is not a trusted user ([approval] trusted_users)",
                        d.user_id
                    ));
                }
                let channel = snowflake(&d.channel_id);
                match (&d.guild_id, channel) {
                    (None, Some(c))
                        if rules.lists(Channel::DiscordDm) || rules.lists(Channel::Discord(c)) => {}
                    (None, _) => why.push(format!(
                        "a Discord DM is not a trusted channel ({})",
                        rules.listing()
                    )),
                    (Some(_), Some(c)) if rules.lists(Channel::Discord(c)) => {
                        match self.checked(c) {
                            Some(k) if k.trusted => {}
                            Some(k) => why
                                .push(format!("Discord channel {c} is not trusted: {}", k.detail)),
                            None => why.push(format!(
                                "Discord channel {c} is not trusted: who can view it has not \
                                 been checked"
                            )),
                        }
                    }
                    (Some(_), _) => why.push(format!(
                        "Discord channel {} is not a trusted channel ({})",
                        d.channel_id,
                        rules.listing()
                    )),
                }
                if why.is_empty() {
                    Ok(())
                } else {
                    refuse(why.join("; and "))
                }
            }
        }
    }

    /// The trusted Discord users (`[approval].trusted_users`).
    pub fn discord_users(&self) -> Vec<u64> {
        self.rules
            .as_ref()
            .map(|r| r.users.clone())
            .unwrap_or_default()
    }

    /// Every guild channel `[approval].channels` lists.
    pub fn discord_channels(&self) -> Vec<u64> {
        self.rules
            .as_ref()
            .map(|r| {
                r.channels
                    .iter()
                    .filter_map(|c| match c {
                        Channel::Discord(id) => Some(*id),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A DM with this user is a trusted channel: there is no rule, or
    /// `discord:dm` (or the DM's own channel id) is listed and the user is
    /// trusted.
    pub fn trusts_dm(&self, user: u64, channel: Option<u64>) -> bool {
        let Some(r) = &self.rules else {
            return true;
        };
        let listed =
            r.lists(Channel::DiscordDm) || channel.is_some_and(|c| r.lists(Channel::Discord(c)));
        listed && r.users.contains(&user)
    }

    /// A guild channel is a trusted channel: there is no rule, or it is
    /// listed and its latest check found only trusted users can view it.
    pub fn trusts_guild_channel(&self, channel: u64) -> bool {
        let Some(r) = &self.rules else {
            return true;
        };
        r.lists(Channel::Discord(channel)) && self.checked(channel).is_some_and(|k| k.trusted)
    }

    /// Where a Discord card says an approval can also be answered: the
    /// trusted local surfaces, "" for none. None without `[approval]`, where
    /// the card says what it always said.
    pub fn elsewhere(&self) -> Option<String> {
        let r = self.rules.as_ref()?;
        Some(
            match (r.lists(Channel::Web), r.lists(Channel::Cli)) {
                (true, true) => "in the web UI or with `theseus confirm`",
                (true, false) => "in the web UI",
                (false, true) => "with `theseus confirm`",
                (false, false) => "",
            }
            .to_string(),
        )
    }

    /// `[approval].channels` lists this guild channel.
    pub fn lists_discord_channel(&self, channel: u64) -> bool {
        self.rules
            .as_ref()
            .is_some_and(|r| r.lists(Channel::Discord(channel)))
    }

    /// The binding's check of a guild channel. True when the verdict changed
    /// (or is the first), so the binding narrates only changes.
    pub fn report(&self, channel: u64, c: Checked) -> bool {
        let mut g = self.checked.write().unwrap();
        let changed = g
            .get(&channel)
            .is_none_or(|old| old.trusted != c.trusted || old.detail != c.detail);
        g.insert(channel, c);
        changed
    }

    /// The binding's latest check of a guild channel.
    pub fn checked(&self, channel: u64) -> Option<Checked> {
        self.checked.read().unwrap().get(&channel).cloned()
    }

    /// Health's view. `web_on` is `[web] enabled`; `discord` is the Discord
    /// binding's state, when it has reported one.
    pub fn status(&self, web_on: bool, discord: Option<&str>) -> ApprovalStatus {
        let Some(rules) = &self.rules else {
            return ApprovalStatus::default();
        };
        let channels = rules
            .channels
            .iter()
            .map(|&c| {
                let (trusted, detail, checked_at_ms) = match c {
                    Channel::Cli => (
                        true,
                        "the CLI on this machine: the socket is mode 0600, so only the account \
                         that runs Theseus can answer"
                            .into(),
                        0,
                    ),
                    Channel::Web => (
                        true,
                        format!(
                            "the web UI on this machine: loopback only, so anyone who can reach \
                             it from this machine can answer{}",
                            if web_on {
                                ""
                            } else {
                                " (it is off: [web] enabled = false)"
                            }
                        ),
                        0,
                    ),
                    Channel::DiscordDm if rules.users.is_empty() => (
                        false,
                        "no trusted user is listed, so no DM qualifies".into(),
                        0,
                    ),
                    Channel::DiscordDm => (
                        true,
                        "a DM between the bot and a trusted user; nobody else can see it".into(),
                        0,
                    ),
                    Channel::Discord(id) => match self.checked(id) {
                        Some(k) => (k.trusted, k.detail, k.at_ms),
                        None => (false, unchecked(discord), 0),
                    },
                };
                ApprovalChannel {
                    channel: c.key(),
                    state: if trusted { "trusted" } else { "not_trusted" }.into(),
                    detail,
                    checked_at_ms,
                }
            })
            .collect();
        ApprovalStatus {
            configured: true,
            trusted_users: rules.users.iter().map(|u| format!("discord:{u}")).collect(),
            channels,
        }
    }
}

/// Why a listed guild channel has no check yet.
fn unchecked(discord: Option<&str>) -> String {
    match discord {
        Some("starting" | "connecting" | "ready" | "resuming") | None => {
            "not checked yet: the Discord binding checks who can view it once it connects".into()
        }
        Some(state) => format!("not checked: the Discord binding is {state}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDDIE: &str = "159471966640799744";
    const MALLORY: &str = "222222222222222222";
    const CHANNEL: &str = "333333333333333333";
    const GUILD: &str = "712398310421561444";

    fn approval(users: &[&str], channels: &[&str]) -> Approval {
        Approval::new(Some(&ApprovalConfig {
            trusted_users: users.iter().map(|s| s.to_string()).collect(),
            channels: channels.iter().map(|s| s.to_string()).collect(),
        }))
    }

    fn local(surface: Surface) -> Answerer {
        Answerer {
            label: "sock#3".into(),
            surface,
            discord: None,
        }
    }

    fn discord(user: &str, channel: &str, guild: Option<&str>) -> Answerer {
        Answerer {
            label: "discord:eddie".into(),
            surface: Surface::Discord,
            discord: Some(DiscordOrigin {
                user_id: user.into(),
                channel_id: channel.into(),
                guild_id: guild.map(str::to_string),
            }),
        }
    }

    fn why(a: &Approval, who: &Answerer) -> String {
        a.judge(who).unwrap_err().why
    }

    /// Without `[approval]`, every answer counts, a bare label and a Discord
    /// claim from the CLI included: exactly as before theseus-sgh.
    #[test]
    fn without_a_section_every_answer_counts() {
        let a = Approval::new(None);
        assert!(!a.configured());
        for who in [
            Answerer::from("test"),
            local(Surface::Cli),
            local(Surface::Web),
            discord(MALLORY, CHANNEL, Some(GUILD)),
            Answerer {
                discord: Some(DiscordOrigin::default()),
                ..local(Surface::Cli)
            },
        ] {
            assert_eq!(a.judge(&who), Ok(()), "{who:?}");
        }
        assert!(a.trusts_dm(1, None) && a.trusts_guild_channel(1));
        assert_eq!(a.status(true, None), ApprovalStatus::default());
        assert_eq!(a.elsewhere(), None, "a card says what it always said");
    }

    #[test]
    fn a_card_names_the_trusted_local_surfaces() {
        let says = |channels: &[&str]| approval(&[], channels).elsewhere().unwrap();
        assert_eq!(
            says(&["cli", "web"]),
            "in the web UI or with `theseus confirm`"
        );
        assert_eq!(says(&["web"]), "in the web UI");
        assert_eq!(says(&["discord:dm", "cli"]), "with `theseus confirm`");
        assert_eq!(says(&["discord:dm"]), "");
    }

    #[test]
    fn the_cli_and_the_web_ui_count_when_listed() {
        let a = approval(&[], &["web"]);
        assert_eq!(a.judge(&local(Surface::Web)), Ok(()));
        let w = why(&a, &local(Surface::Cli));
        assert_eq!(
            w,
            "the CLI is not a trusted channel ([approval] channels = [\"web\"])"
        );
        let a = approval(&[], &["cli"]);
        assert_eq!(a.judge(&local(Surface::Cli)), Ok(()));
        assert!(why(&a, &local(Surface::Web)).starts_with("the web UI is not a trusted channel"));
        assert!(why(&a, &Answerer::from("test")).contains("never a trusted channel"));
    }

    #[test]
    fn discord_counts_only_from_a_trusted_user_in_a_trusted_channel() {
        let dm = |user| discord(user, "444444444444444444", None);
        let a = approval(&[&format!("discord:{EDDIE}")], &["discord:dm"]);
        assert_eq!(a.judge(&dm(EDDIE)), Ok(()));
        let w = why(&a, &dm(MALLORY));
        assert_eq!(
            w,
            format!("discord:{MALLORY} is not a trusted user ([approval] trusted_users)")
        );
        let r = a.judge(&dm(MALLORY)).unwrap_err();
        assert_eq!(r.via, "discord:dm");
        assert_eq!(r.who, format!("discord:{MALLORY} (discord:eddie)"));
        // A trusted user through an unlisted surface: a guild channel, the CLI.
        let w = why(&a, &discord(EDDIE, CHANNEL, Some(GUILD)));
        assert!(
            w.starts_with(&format!(
                "Discord channel {CHANNEL} is not a trusted channel"
            )),
            "{w}"
        );
        assert!(why(&a, &local(Surface::Cli)).starts_with("the CLI is not a trusted channel"));
        // Both at once: each reason is said.
        let w = why(&a, &discord(MALLORY, CHANNEL, Some(GUILD)));
        assert!(
            w.contains("is not a trusted user") && w.contains("; and Discord channel"),
            "{w}"
        );
        // A DM needs discord:dm listed.
        let a = approval(&[&format!("discord:{EDDIE}")], &["cli"]);
        assert!(why(&a, &dm(EDDIE)).starts_with("a Discord DM is not a trusted channel"));
    }

    /// A listed guild channel counts only while the binding's latest check
    /// found that only trusted users can view it.
    #[test]
    fn a_listed_guild_channel_counts_only_as_its_latest_check_says() {
        let id: u64 = CHANNEL.parse().unwrap();
        let a = approval(
            &[&format!("discord:{EDDIE}")],
            &[&format!("discord:{CHANNEL}")],
        );
        let here = discord(EDDIE, CHANNEL, Some(GUILD));
        assert!(why(&a, &here).ends_with("who can view it has not been checked"));
        assert!(!a.trusts_guild_channel(id));
        let no_intent = Checked {
            trusted: false,
            detail: "cannot be verified without the Server Members intent".into(),
            at_ms: 5,
        };
        assert!(a.report(id, no_intent.clone()));
        assert!(!a.report(id, no_intent), "the same verdict is no change");
        assert_eq!(
            why(&a, &here),
            format!(
                "Discord channel {CHANNEL} is not trusted: cannot be verified without the Server \
                 Members intent"
            )
        );
        assert!(a.report(
            id,
            Checked {
                trusted: true,
                detail: "only trusted users can view it (1)".into(),
                at_ms: 9,
            }
        ));
        assert_eq!(a.judge(&here), Ok(()));
        assert!(a.trusts_guild_channel(id));
        let s = a.status(true, Some("ready"));
        assert_eq!(s.channels[0].state, "trusted");
        assert_eq!(s.channels[0].checked_at_ms, 9);
    }

    /// Only the Discord binding may name a Discord channel and user.
    #[test]
    fn a_discord_claim_from_another_surface_does_not_count() {
        let a = approval(&[&format!("discord:{EDDIE}")], &["cli", "discord:dm"]);
        let forged = Answerer {
            discord: Some(DiscordOrigin {
                user_id: EDDIE.into(),
                channel_id: "444444444444444444".into(),
                guild_id: None,
            }),
            ..local(Surface::Cli)
        };
        assert!(why(&a, &forged).starts_with("only the Discord binding can name"));
        let bare = Answerer {
            discord: None,
            ..discord(EDDIE, CHANNEL, None)
        };
        assert_eq!(
            why(&a, &bare),
            "the Discord binding named no channel and user for it"
        );
    }

    #[test]
    fn entries_parse_or_name_their_forms() {
        assert_eq!(Channel::parse("cli"), Ok(Channel::Cli));
        assert_eq!(Channel::parse("discord:dm"), Ok(Channel::DiscordDm));
        assert_eq!(
            Channel::parse(&format!("discord:{CHANNEL}")),
            Ok(Channel::Discord(CHANNEL.parse().unwrap()))
        );
        for bad in [
            "discord",
            "discord:general",
            "slack:C1",
            "Web",
            "discord:12",
        ] {
            let e = Channel::parse(bad).unwrap_err();
            assert!(e.contains("\"discord:<channel id>\""), "{bad}: {e}");
        }
        assert_eq!(
            parse_user(&format!("discord:{EDDIE}")),
            Ok(EDDIE.parse().unwrap())
        );
        for bad in [EDDIE, "eddie", "discord:eddie", "cli"] {
            assert!(parse_user(bad)
                .unwrap_err()
                .contains("write \"discord:<user id>\""));
        }
    }

    #[test]
    fn health_says_each_channels_state_and_why() {
        let guild_channel = format!("discord:{CHANNEL}");
        let a = approval(&[], &["cli", "web", "discord:dm", &guild_channel]);
        let s = a.status(false, Some("disabled"));
        assert!(s.configured && s.trusted_users.is_empty());
        let states: Vec<(&str, &str)> = s
            .channels
            .iter()
            .map(|c| (c.channel.as_str(), c.state.as_str()))
            .collect();
        assert_eq!(
            states,
            [
                ("cli", "trusted"),
                ("web", "trusted"),
                ("discord:dm", "not_trusted"),
                (guild_channel.as_str(), "not_trusted"),
            ]
        );
        assert!(s.channels[1]
            .detail
            .ends_with("(it is off: [web] enabled = false)"));
        assert_eq!(
            s.channels[2].detail,
            "no trusted user is listed, so no DM qualifies"
        );
        assert_eq!(
            s.channels[3].detail,
            "not checked: the Discord binding is disabled"
        );
    }
}
