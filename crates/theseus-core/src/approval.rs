//! Approval (spec §3.9 "Approval"; theseus-zmgb): an answer to a waiting
//! call, a tool call or a budget question, counts only from a private place,
//! by the owner, under the place rule (`places::owner_in_private`). The
//! private places are the CLI, the web UI (checked to the daemon's own uid at
//! accept), a DM with the owner, and a guild channel the bindings file binds
//! `private = true`, whose viewers the binding reads at its start. The owner
//! is `[places] owner`, else the person of a DM the bindings file binds. A
//! shared place's cards go to the owner's DM. The undo of a tightening, a
//! trust, and a publish take the same rule. The `[approval]` matrix of
//! trusted users and channels it replaces is retired: a config that still
//! has the section loads, with a warning.
//!
//! The answer is judged in one place, `Core::judge_act`, which every surface
//! reaches through its method. The judgment trusts the connection, never
//! what a client says. The listener that accepts a connection names its
//! surface: the Unix socket and `--stdio` are the CLI, the loopback bridge is
//! the web UI, and the in-process binding is Discord. Only the Discord
//! binding may name a Discord channel and user.

use theseus_protocol::DiscordOrigin;

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
    /// A connection no listener named (a test's). Never a private place.
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
    /// The place the answer came from, as a refusal names it: `cli`, `web`,
    /// `discord:dm`, or `discord:<channel id>`.
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

    /// Why this answerer's surface can make no approval-like act at all,
    /// before the place rule is asked: a connection no listener named, the
    /// binding naming nobody, or a Discord claim from another surface.
    pub(crate) fn unknown(&self) -> Option<String> {
        match (self.surface, &self.discord) {
            (s, Some(_)) if s != Surface::Discord => Some(format!(
                "only the Discord binding can name a Discord channel and user, and this answer \
                 came through {}",
                s.name()
            )),
            (Surface::Unnamed, _) => Some(format!(
                "it came through {}, which is never a private place",
                Surface::Unnamed.name()
            )),
            (Surface::Discord, None) => {
                Some("the Discord binding named no channel and user for it".into())
            }
            _ => None,
        }
    }
}

/// Where a Discord card says an approval can also be answered: the owner's
/// own surfaces, both private places (`web_on`: `[web] enabled`).
pub fn elsewhere(web_on: bool) -> &'static str {
    if web_on {
        "in the web UI or with `theseus confirm`"
    } else {
        "with `theseus confirm`"
    }
}

/// A Discord id as a `[places] owner` entry names it, `discord:<user id>`,
/// or the error that names the form.
pub fn parse_user(s: &str) -> Result<u64, String> {
    s.strip_prefix("discord:")
        .and_then(snowflake)
        .ok_or_else(|| format!("{s:?} is not a surface-qualified id: write \"discord:<user id>\""))
}

/// A Discord id: 15 to 21 decimal digits.
fn snowflake(s: &str) -> Option<u64> {
    let digits = (15..=21).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| s.parse().ok()).flatten()
}

/// Why an act does not count.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn discord(user: &str, guild: Option<&str>) -> Option<DiscordOrigin> {
        Some(DiscordOrigin {
            user_id: user.into(),
            channel_id: "444444444444444444".into(),
            guild_id: guild.map(str::to_string),
        })
    }

    fn answerer(surface: Surface, discord: Option<DiscordOrigin>) -> Answerer {
        Answerer {
            label: "x".into(),
            surface,
            discord,
            peer: Peer::None,
        }
    }

    /// The places an answer names: `cli`, `web`, `discord:dm`, or the guild
    /// channel's id; and the surfaces that can make no act at all.
    #[test]
    fn an_answerer_names_its_place_and_an_unknown_surface_why() {
        use Surface::{Cli, Discord, Unnamed, Web};
        let eddie = "271828182845904523";
        assert_eq!(answerer(Cli, None).via(), "cli");
        assert_eq!(answerer(Web, None).via(), "web");
        assert_eq!(answerer(Discord, discord(eddie, None)).via(), "discord:dm");
        let guild = answerer(Discord, discord(eddie, Some("314159265358979323")));
        assert_eq!(guild.via(), "discord:444444444444444444");
        assert_eq!(guild.who(), format!("discord:{eddie} (x)"));
        for a in [answerer(Cli, None), answerer(Web, None), guild] {
            assert_eq!(a.unknown(), None, "{a:?}");
        }
        let why = |a: Answerer| a.unknown().unwrap();
        assert!(why(answerer(Unnamed, None)).contains("never a private place"));
        assert!(why(answerer(Discord, None)).contains("named no channel and user"));
        assert!(why(answerer(Cli, discord(eddie, None)))
            .starts_with("only the Discord binding can name a Discord channel and user"));
    }

    #[test]
    fn a_user_is_a_surface_qualified_snowflake() {
        assert_eq!(
            parse_user("discord:271828182845904523"),
            Ok(271_828_182_845_904_523)
        );
        for bad in ["eddie", "discord:eddie", "discord:12", "271828182845904523"] {
            assert!(
                parse_user(bad)
                    .unwrap_err()
                    .contains("surface-qualified id"),
                "{bad}"
            );
        }
    }
}
