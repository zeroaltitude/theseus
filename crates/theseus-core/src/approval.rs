//! Approval (spec §3.9 "Approval", theseus-sgh): an answer to a waiting call,
//! a tool call or a budget question, counts only from a trusted user through
//! a trusted channel. Both lists are `[approval]` in the vault-held config,
//! which agents cannot write. Without that section the rule fails closed
//! (review 2's consideration 2): the owner's CLI and Discord DM answer, and
//! nothing else. The DM is one the bindings file binds, which only its user
//! reaches; the web UI and a guild channel answer only once `[approval]`
//! names them.
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
pub use crate::peer::Peer;

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
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Cli => "the CLI",
            Self::Web => "the web UI",
            Self::Discord => "the Discord binding",
            Self::Unnamed => "a connection no listener named",
        }
    }
}

/// A protocol connection: the label that names it in logs and the ledger
/// (`sock#3`, `web#1`, `discord`), its surface, and the process on the other
/// end when the listener knows one (theseus-6qy).
#[derive(Debug, Clone)]
pub struct Client {
    pub label: String,
    pub surface: Surface,
    pub peer: Peer,
}

impl Client {
    pub fn new(label: impl Into<String>, surface: Surface) -> Self {
        Self {
            label: label.into(),
            surface,
            peer: Peer::None,
        }
    }

    /// With the process on the other end, as the listener read it.
    pub fn with_peer(mut self, peer: Peer) -> Self {
        self.peer = peer;
        self
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
    /// The process that asked, as the connection knows it (theseus-6qy).
    pub peer: Peer,
}

/// A bare label, a test's, answers as the CLI on this machine does. Tests
/// only: every answer in the daemon comes through a listener, which names
/// its surface.
#[cfg(test)]
impl From<&str> for Answerer {
    fn from(label: &str) -> Self {
        Self {
            label: label.to_string(),
            surface: Surface::Cli,
            discord: None,
            peer: Peer::None,
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

/// How much of the rule an act must meet (theseus-sgh).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bar {
    /// A trusted user through a trusted channel: an answer, and the undo of
    /// a tightening.
    Trusted,
    /// Any surface that can answer an approval: a "should have asked" press.
    Surface,
}

/// `[approval]`, resolved, with the Discord binding's latest checks.
pub struct Approval {
    rules: Rules,
    /// Guild channel id → the binding's latest check of who can view it.
    checked: RwLock<BTreeMap<u64, Checked>>,
}

/// No `[approval]` section: the owner's CLI and Discord DM.
impl Default for Approval {
    fn default() -> Self {
        Self::new(None)
    }
}

struct Rules {
    users: Vec<u64>,
    channels: Vec<Channel>,
    /// There is no `[approval]` section: the channels are the CLI and a DM,
    /// and a DM counts for its user, whom the bindings file binds, as the
    /// binding lets nobody else reach it.
    owner_only: bool,
}

impl Rules {
    fn lists(&self, c: Channel) -> bool {
        self.channels.contains(&c)
    }

    /// `channels = [...]` as configured, for a reason.
    fn listing(&self) -> String {
        if self.owner_only {
            return "there is no [approval] section, so only the CLI and a Discord DM the bindings \
                    file binds answer: name it in [approval] channels to add it"
                .into();
        }
        let keys: Vec<String> = self
            .channels
            .iter()
            .map(|c| format!("\"{}\"", c.key()))
            .collect();
        format!("[approval] channels = [{}]", keys.join(", "))
    }

    /// `user` may answer in a DM: a trusted user, or without a section,
    /// whoever the binding let reach it.
    fn trusts_user(&self, user: u64) -> bool {
        self.owner_only || self.users.contains(&user)
    }
}

impl Approval {
    /// From the loaded config, which has validated every entry. Without an
    /// `[approval]` section: the owner's CLI and Discord DM (review 2's
    /// consideration 2).
    pub fn new(cfg: Option<&ApprovalConfig>) -> Self {
        let rules = match cfg {
            Some(c) => Rules {
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
                owner_only: false,
            },
            None => Rules {
                users: Vec::new(),
                channels: vec![Channel::Cli, Channel::DiscordDm],
                owner_only: true,
            },
        };
        Self {
            rules,
            checked: RwLock::default(),
        }
    }

    /// The config has an `[approval]` section.
    pub fn configured(&self) -> bool {
        !self.rules.owner_only
    }

    /// Judge an answer: Ok when it counts, else why it does not. The undo of
    /// a tightening loosens, so it is judged the same way (theseus-sgh).
    pub fn judge(&self, a: &Answerer) -> Result<(), Refusal> {
        self.judge_at(a, Bar::Trusted)
    }

    /// Judge a "should have asked" press (theseus-sgh). It only makes calls
    /// ask, so any surface that can answer an approval may press one: the
    /// CLI, the web UI, or the Discord binding, which lets only a place's
    /// listed users press anything. With `[approval]` or without.
    pub fn judge_tighten(&self, a: &Answerer) -> Result<(), Refusal> {
        self.judge_at(a, Bar::Surface)
    }

    fn judge_at(&self, a: &Answerer, bar: Bar) -> Result<(), Refusal> {
        let rules = &self.rules;
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
        match (a.surface, &a.discord) {
            (Surface::Unnamed, _) => refuse(format!(
                "it came through {}, which is never a trusted channel",
                a.surface.name()
            )),
            (Surface::Discord, None) => {
                refuse("the Discord binding named no channel and user for it".into())
            }
            _ if bar == Bar::Surface => Ok(()),
            (Surface::Cli, _) if rules.lists(Channel::Cli) => Ok(()),
            (Surface::Web, _) if rules.lists(Channel::Web) => Ok(()),
            (Surface::Cli | Surface::Web, _) => refuse(format!(
                "{} is not a trusted channel ({})",
                a.surface.name(),
                rules.listing()
            )),
            (Surface::Discord, Some(d)) => {
                let mut why = Vec::new();
                if !snowflake(&d.user_id).is_some_and(|u| rules.trusts_user(u)) {
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

    /// The trusted Discord users (`[approval].trusted_users`); none without
    /// a section, where only a DM's own user answers.
    pub fn discord_users(&self) -> Vec<u64> {
        self.rules.users.clone()
    }

    /// Every guild channel `[approval].channels` lists.
    pub fn discord_channels(&self) -> Vec<u64> {
        self.rules
            .channels
            .iter()
            .filter_map(|c| match c {
                Channel::Discord(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// A DM with this user is a trusted channel: `discord:dm` (or the DM's
    /// own channel id) is listed and the user is trusted, or there is no
    /// section and the bindings file binds the DM.
    pub fn trusts_dm(&self, user: u64, channel: Option<u64>) -> bool {
        let r = &self.rules;
        let listed =
            r.lists(Channel::DiscordDm) || channel.is_some_and(|c| r.lists(Channel::Discord(c)));
        listed && r.trusts_user(user)
    }

    /// A guild channel is a trusted channel: it is listed, and its latest
    /// check found only trusted users can view it. Never without a section.
    pub fn trusts_guild_channel(&self, channel: u64) -> bool {
        self.rules.lists(Channel::Discord(channel))
            && self.checked(channel).is_some_and(|k| k.trusted)
    }

    /// Where a Discord card says an approval can also be answered: the
    /// trusted local surfaces, "" for none.
    pub fn elsewhere(&self) -> String {
        let r = &self.rules;
        match (r.lists(Channel::Web), r.lists(Channel::Cli)) {
            (true, true) => "in the web UI or with `theseus confirm`",
            (true, false) => "in the web UI",
            (false, true) => "with `theseus confirm`",
            (false, false) => "",
        }
        .to_string()
    }

    /// `[approval].channels` lists this guild channel.
    pub fn lists_discord_channel(&self, channel: u64) -> bool {
        self.rules.lists(Channel::Discord(channel))
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
        let rules = &self.rules;
        let channels: Vec<ApprovalChannel> = rules
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
                    Channel::DiscordDm if rules.owner_only => (
                        true,
                        "a DM the bindings file binds, between the bot and its user; nobody \
                         else can see it"
                            .into(),
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
        // Beyond the owner's CLI and DM, what may answer now: the web UI
        // while it is on, and each guild channel its latest check trusts.
        let open = channels
            .iter()
            .filter(|c| c.state == "trusted" && c.channel != "cli" && c.channel != "discord:dm")
            .filter(|c| c.channel != "web" || web_on)
            .map(|c| c.channel.clone())
            .collect();
        ApprovalStatus {
            configured: !rules.owner_only,
            trusted_users: rules.users.iter().map(|u| format!("discord:{u}")).collect(),
            channels,
            open,
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

    const EDDIE: &str = "271828182845904523";
    const MALLORY: &str = "222222222222222222";
    const CHANNEL: &str = "333333333333333333";
    const GUILD: &str = "314159265358979323";

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
            peer: Peer::None,
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
            peer: Peer::None,
        }
    }

    fn why(a: &Approval, who: &Answerer) -> String {
        a.judge(who).unwrap_err().why
    }

    /// Review 2's consideration 2: without `[approval]` the rule fails
    /// closed. The owner's CLI answers, and so does a Discord DM, which only
    /// the user its binding names reaches. The web UI, a guild channel, a
    /// connection no listener named, and a Discord claim from the CLI do not,
    /// and each refusal says how to add a surface. Health is not open.
    #[test]
    fn without_a_section_only_the_cli_and_a_bound_dm_answer() {
        let a = Approval::new(None);
        assert!(!a.configured());
        for who in [local(Surface::Cli), discord(MALLORY, CHANNEL, None)] {
            assert_eq!(a.judge(&who), Ok(()), "{who:?}");
        }
        let named = "there is no [approval] section, so only the CLI and a Discord DM the \
                     bindings file binds answer: name it in [approval] channels to add it";
        for (who, says) in [
            (local(Surface::Web), "the web UI is not a trusted channel"),
            (
                discord(MALLORY, CHANNEL, Some(GUILD)),
                "Discord channel 333333333333333333 is not a trusted channel",
            ),
            (local(Surface::Unnamed), "never a trusted channel"),
            (
                Answerer {
                    discord: Some(DiscordOrigin::default()),
                    ..local(Surface::Cli)
                },
                "only the Discord binding can name",
            ),
        ] {
            let w = why(&a, &who);
            assert!(w.contains(says), "{w}");
            if !says.contains("never") && !says.contains("only the Discord") {
                assert!(w.ends_with(&format!("({named})")), "{w}");
            }
        }
        assert!(a.trusts_dm(1, None) && !a.trusts_guild_channel(1));
        assert_eq!(a.elsewhere(), "with `theseus confirm`");
        let s = a.status(true, None);
        assert!(!s.configured && s.open.is_empty(), "{s:?}");
        let listed: Vec<(&str, &str)> = s
            .channels
            .iter()
            .map(|c| (c.channel.as_str(), c.state.as_str()))
            .collect();
        assert_eq!(listed, [("cli", "trusted"), ("discord:dm", "trusted")]);
    }

    /// Health says `approval: open` while a surface beyond the owner's CLI
    /// and DM may answer: the web UI while it is on, and a guild channel its
    /// latest check trusts (review 2's consideration 2).
    #[test]
    fn health_is_open_while_more_than_the_cli_and_a_dm_may_answer() {
        let open = |a: &Approval, web_on: bool| a.status(web_on, Some("ready")).open;
        let a = approval(&[&format!("discord:{EDDIE}")], &["cli", "discord:dm"]);
        assert!(open(&a, true).is_empty());
        let a = approval(
            &[&format!("discord:{EDDIE}")],
            &["cli", "web", "discord:dm", &format!("discord:{CHANNEL}")],
        );
        assert_eq!(
            open(&a, true),
            ["web"],
            "an unchecked channel cannot answer yet"
        );
        assert!(open(&a, false).is_empty(), "the web UI is off");
        let id: u64 = CHANNEL.parse().unwrap();
        let check = |trusted| Checked {
            trusted,
            detail: "invented".into(),
            at_ms: 1,
        };
        a.report(id, check(true));
        assert_eq!(open(&a, true), ["web", &format!("discord:{CHANNEL}")]);
        a.report(id, check(false));
        assert_eq!(open(&a, false), Vec::<String>::new());
    }

    #[test]
    fn a_card_names_the_trusted_local_surfaces() {
        let says = |channels: &[&str]| approval(&[], channels).elsewhere();
        assert_eq!(
            says(&["cli", "web"]),
            "in the web UI or with `theseus confirm`"
        );
        assert_eq!(says(&["web"]), "in the web UI");
        assert_eq!(says(&["discord:dm", "cli"]), "with `theseus confirm`");
        assert_eq!(says(&["discord:dm"]), "");
    }

    /// "Should have asked" only makes calls ask, so any surface that can
    /// answer an approval may press it, trusted or not; the undo loosens, so
    /// it takes the whole rule (`judge`). A connection no listener named and
    /// a Discord claim from the CLI are refused either way (theseus-sgh).
    #[test]
    fn a_tightening_needs_a_known_surface_and_its_undo_the_whole_rule() {
        let a = approval(&[&format!("discord:{EDDIE}")], &["discord:dm"]);
        for who in [
            local(Surface::Cli),
            local(Surface::Web),
            discord(MALLORY, CHANNEL, Some(GUILD)),
            discord(EDDIE, CHANNEL, None),
        ] {
            assert_eq!(a.judge_tighten(&who), Ok(()), "{who:?}");
        }
        assert!(why(&a, &local(Surface::Cli)).starts_with("the CLI is not a trusted channel"));
        assert!(why(&a, &discord(MALLORY, CHANNEL, Some(GUILD))).contains("not a trusted user"));
        assert_eq!(a.judge(&discord(EDDIE, CHANNEL, None)), Ok(()));
        let forged = Answerer {
            discord: Some(DiscordOrigin {
                user_id: EDDIE.into(),
                channel_id: CHANNEL.into(),
                guild_id: None,
            }),
            ..local(Surface::Cli)
        };
        let unnamed = local(Surface::Unnamed);
        let bare = Answerer {
            discord: None,
            ..discord(EDDIE, CHANNEL, None)
        };
        for (who, says) in [
            (&forged, "only the Discord binding can name"),
            (&unnamed, "never a trusted channel"),
            (&bare, "named no channel and user"),
        ] {
            for r in [a.judge_tighten(who), a.judge(who)] {
                assert!(r.unwrap_err().why.contains(says), "{who:?}");
            }
        }
        // Without a section, too, a press takes a known surface, and its undo
        // the owner's rule.
        let none = Approval::new(None);
        for who in [local(Surface::Web), discord(MALLORY, CHANNEL, Some(GUILD))] {
            assert_eq!(none.judge_tighten(&who), Ok(()), "{who:?}");
            assert!(none.judge(&who).is_err(), "{who:?}");
        }
        for who in [&forged, &unnamed] {
            assert!(none.judge_tighten(who).is_err(), "{who:?}");
        }
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
        assert!(why(&a, &local(Surface::Unnamed)).contains("never a trusted channel"));
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
