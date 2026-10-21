//! A message that names no thread finds its own (theseus-rha2; the owner,
//! 2026-10-09: each stream cleanly addressable to Theseus, and ask only when
//! it truly cannot tell). These are the wire shapes: how a turn was routed
//! (`TurnSubmitResult.thread`), the one-line question when the router cannot
//! tell (`thread.asked`, answered by `thread.answer`), and the recovery when
//! it was wrong (`thread.move`). The router itself is the core's
//! `thread_route`.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// How a message found its thread. Everything but `Quiet` and `Home` shows
/// in the reply's tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RouteHow {
    /// Addressed: `--thread`, a leading `#slug`, `/t slug`, "in the <title>
    /// thread:", or the place the thread is bound to. Certain.
    Addressed,
    /// A reply to one of a thread's replies. Certain.
    Reply,
    /// Sure enough (0.90 or more): the tag is quiet.
    Quiet,
    /// Likely (0.60 to 0.90): the tag is visible and offers a move.
    Visible,
    /// Jev did not answer in `[threads] route_wait_ms`: the router's own pick
    /// stands, tagged as a guess.
    Guess,
    /// The owner answered "which thread?".
    Asked,
    /// The owner accepted a new thread for it.
    NewThread,
    /// A one-off, or the router had no thread for it: the place's own
    /// conversation.
    Home,
    /// The owner moved it (`thread.move`) and it ran again here.
    Moved,
}

/// Where a turn ran, when the place had threads to choose between. The
/// reply's footer words it (`→ pr round`, `→ pr round · ↪ move`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadTag {
    /// The thread's slug; none: the place's own conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub slug: Option<String>,
    /// Its title (the word shown), or `home`.
    pub title: String,
    pub how: RouteHow,
    /// The fused score the choice stood on (1 when addressed).
    pub confidence: f64,
    /// Whether the tag offers a move (below the act bar, or a guess).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub movable: bool,
}

impl ThreadTag {
    /// The tag as a reply shows it: `→ pr round`, with ` · ↪ move` when the
    /// choice was below the act bar. A message that ran at home, quietly,
    /// has none.
    pub fn words(&self) -> Option<String> {
        if self.slug.is_none() && !self.movable {
            return None;
        }
        let guess = if self.how == RouteHow::Guess {
            " (a guess)"
        } else {
            ""
        };
        let moveable = if self.movable {
            " \u{b7} \u{21aa} move"
        } else {
            ""
        };
        Some(format!("\u{2192} {}{guess}{moveable}", self.title))
    }
}

/// One choice of a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AskOption {
    /// What an answer names: `1`, `2`, …, `other`, `new` or `home`.
    pub key: String,
    /// The label: the thread's title, `New thread «…»`, or `home`.
    pub label: String,
}

/// "Is this about the pr round or the security fix?" (`thread.asked`): the
/// router could not tell, and the turn waits for the answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadAsk {
    pub ask_id: String,
    /// The place's own session, where the message arrived.
    pub session_id: String,
    /// The place's target (`discord:dm:…`), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    /// The one line.
    pub question: String,
    pub options: Vec<AskOption>,
    /// The start of the message that waits.
    pub message: String,
    /// When the wait ends and the message runs at home.
    pub expires_ms: u64,
    /// With an `other` option (theseus-98bz.4): every live, unsealed thread,
    /// its slug as the key, any of which answers the question.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<AskOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadAnswerParams {
    pub ask_id: String,
    /// An option's key: `1`, `2`, …, `new`, `home` (`n` and `h` too), or a
    /// live thread's slug (theseus-98bz.4).
    pub choice: String,
    /// Who answered, and from where: a card's press carries the presser's ids,
    /// which the core judges as an approval's (the owner, from a private place).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadAnswerResult {
    /// The thread the message went to; none: home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub routed: Option<String>,
}

/// "That was for the security fix": `thread.move`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadMoveParams {
    /// The place's session, or the thread session the message was routed to.
    pub session_id: String,
    /// A thread's slug, or `home`.
    pub to: String,
    /// The turn to move; none: the last unaddressed message routed in this
    /// place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadMoveResult {
    /// Where the message had been routed (a slug or `home`) and where it went.
    pub from: String,
    pub to: String,
    /// The turn that ran it again in the right thread.
    pub turn_id: String,
    /// The note left in the wrong thread, when it had a session of its own.
    pub redirected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(slug: Option<&str>, how: RouteHow, movable: bool) -> ThreadTag {
        ThreadTag {
            slug: slug.map(str::to_string),
            title: slug.unwrap_or("home").replace('-', " "),
            how,
            confidence: 0.7,
            movable,
        }
    }

    /// The reply's tag: quiet is the title alone, a likely one offers a move,
    /// a guess says so, and a quiet home message has none.
    #[test]
    fn the_tag_says_where_a_turn_ran_and_offers_a_move_below_the_bar() {
        let quiet = tag(Some("pr-round"), RouteHow::Quiet, false);
        assert_eq!(quiet.words().as_deref(), Some("\u{2192} pr round"));
        let likely = tag(Some("pr-round"), RouteHow::Visible, true);
        assert_eq!(
            likely.words().as_deref(),
            Some("\u{2192} pr round \u{b7} \u{21aa} move")
        );
        let guess = tag(Some("pr-round"), RouteHow::Guess, true);
        assert_eq!(
            guess.words().as_deref(),
            Some("\u{2192} pr round (a guess) \u{b7} \u{21aa} move")
        );
        assert_eq!(tag(None, RouteHow::Home, false).words(), None);
        assert!(tag(None, RouteHow::Home, true).words().is_some());
    }
}
