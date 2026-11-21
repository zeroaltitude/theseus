//! A message goes to a thread by its address (theseus-rha2; the owner,
//! 2026-11-21: an unaddressed message stays in the place's own conversation).
//! These are the wire shapes: how a turn was placed
//! (`TurnSubmitResult.thread`), and a move afterwards (`thread.move`). The
//! router itself is the core's `thread_route`. `RouteHow` keeps the values
//! the router wrote while it guessed (`Quiet`, `Visible`, `Guess`, `Asked`,
//! `NewThread`), so rows and results of an earlier build still read.

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
