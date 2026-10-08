//! The owner's correction of routing by a reaction (theseus-q31l): ⬆️ on a
//! turn's reply says it should have run on a stronger model, ⬇️ on a cheaper
//! one. A reaction is `route.correct` on that turn, as the reactor, with the
//! place it came from, so the core judges it as it judges a label: the owner,
//! from a private place. A counted one is acknowledged with ✅ on the reply; a
//! refused one changes nothing, and its row says why. Any other reaction, or
//! one on a message that is no reply of this process's, is left alone.
//!
//! No route footer with buttons on Discord: a reaction does the same at no
//! cost to the conversation, where a button row on every reply would be one
//! more line under each answer and a press a round trip (the cockpit shows
//! its footer, `RouteFooter`). A child of `runtime`, apart from it for the
//! shape budget's file ceiling.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::{json, Value};
use theseus_protocol::route::RouteCorrectParams;
use theseus_protocol::{DiscordOrigin, LedgerKind};
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::channel::message::EmojiReactionType;
use twilight_model::gateway::GatewayReaction;

use super::Shared;

/// How many replies, by message, a reaction can find.
pub(crate) const KEPT: usize = 512;

/// The replies this process posted, by Discord message, with their turns.
#[derive(Default)]
pub(crate) struct Replies(Mutex<VecDeque<(u64, String)>>);

impl Replies {
    /// A message a write landed, under its key: a turn's reply when its key
    /// is the turn's (`turn_…:<part>`).
    pub(crate) fn noted(&self, message: u64, key: &str) {
        let Some(turn) = turn_of_key(key) else { return };
        let mut r = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if r.iter().any(|(m, _)| *m == message) {
            return;
        }
        r.push_back((message, turn.to_string()));
        while r.len() > KEPT {
            r.pop_front();
        }
    }

    /// The turn whose reply `message` is.
    pub(crate) fn turn(&self, message: u64) -> Option<String> {
        let r = self.0.lock().unwrap_or_else(|e| e.into_inner());
        r.iter()
            .rev()
            .find(|(m, _)| *m == message)
            .map(|(_, t)| t.clone())
    }
}

/// A reply's key names its turn: `turn_…:0`, `turn_…:footer`.
pub(crate) fn turn_of_key(key: &str) -> Option<&str> {
    let (turn, _) = key.split_once(':')?;
    turn.starts_with("turn_").then_some(turn)
}

/// What a reaction says: ⬆️ `stronger`, ⬇️ `cheaper`, with or without the
/// emoji's variation selector.
pub(crate) fn direction(emoji: &EmojiReactionType) -> Option<&'static str> {
    let EmojiReactionType::Unicode { name } = emoji else {
        return None;
    };
    match name.trim_end_matches('\u{fe0f}') {
        "\u{2b06}" => Some("stronger"),
        "\u{2b07}" => Some("cheaper"),
        _ => None,
    }
}

/// The footer's word on where routing ran a turn: `routed: quick`, or
/// `routed: correction` when the owner's correction placed it.
pub(crate) fn routed(r: &theseus_protocol::TurnSubmitResult) -> Option<String> {
    let t = r.route.as_ref()?;
    let what = t.source.clone().or_else(|| t.mode.clone())?;
    Some(format!("routed: {what}"))
}

/// The place a reaction came from, as the bindings name it.
fn place_of(r: &GatewayReaction) -> String {
    match r.guild_id {
        None => format!("discord:dm:{}", r.user_id),
        Some(_) => format!("discord:channel:{}", r.channel_id),
    }
}

/// A reaction the gateway brought, answered beside its loop.
pub(crate) async fn reacted(shared: std::sync::Arc<Shared>, r: GatewayReaction) {
    shared.on_reaction(r).await;
}

impl Shared {
    /// A reaction on one of this process's replies: ⬆️ or ⬇️ corrects the
    /// turn's routing, as the reactor, from where they reacted.
    pub(crate) async fn on_reaction(&self, r: GatewayReaction) {
        if r.user_id.get() == self.bot_id() {
            return;
        }
        let Some(to) = direction(&r.emoji) else {
            return;
        };
        let Some(turn) = self.replies.turn(r.message_id.get()) else {
            return;
        };
        let place = place_of(&r);
        let Ok(Some(session)) = self.core.outbox.place_session(&place) else {
            return;
        };
        let p = RouteCorrectParams {
            session_id: session.clone(),
            turn_id: Some(turn.clone()),
            to: to.into(),
            via: Some("reaction".into()),
            provenance: Some(format!("{}:{to}", r.message_id)),
            discord: Some(DiscordOrigin {
                user_id: r.user_id.to_string(),
                channel_id: r.channel_id.to_string(),
                guild_id: r.guild_id.map(|g| g.to_string()),
            }),
        };
        let res = self
            .rpc
            .call::<_, Value>(theseus_protocol::method::ROUTE_CORRECT, p)
            .await;
        self.core.binding_ledger(
            LedgerKind::DiscordLabel,
            Some(&session),
            json!({"turn": turn, "correction": to, "via": "reaction", "by": r.user_id.to_string(),
                   "ok": res.is_ok(), "error": res.as_ref().err().map(|e| e.message.clone())}),
        );
        if res.is_ok() {
            let ok = RequestReactionType::Unicode { name: "\u{2705}" };
            let _ = self
                .http
                .create_reaction(r.channel_id, r.message_id, &ok)
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_says_where_routing_ran_the_turn() {
        let mut r = theseus_protocol::TurnSubmitResult::default();
        assert_eq!(routed(&r), None);
        r.route = Some(theseus_protocol::route::TurnRoute {
            mode: Some("quick".into()),
            reason: "detour".into(),
            from: "sonnet".into(),
            source: None,
        });
        assert_eq!(routed(&r).as_deref(), Some("routed: quick"));
        r.route.as_mut().unwrap().source = Some("correction".into());
        assert_eq!(routed(&r).as_deref(), Some("routed: correction"));
    }

    #[test]
    fn up_and_down_name_a_direction_and_nothing_else_does() {
        let u = |s: &str| EmojiReactionType::Unicode { name: s.into() };
        assert_eq!(direction(&u("\u{2b06}\u{fe0f}")), Some("stronger"));
        assert_eq!(direction(&u("\u{2b06}")), Some("stronger"));
        assert_eq!(direction(&u("\u{2b07}\u{fe0f}")), Some("cheaper"));
        assert_eq!(direction(&u("\u{1f44d}")), None);
        let custom = EmojiReactionType::Custom {
            animated: false,
            id: twilight_model::id::Id::new(1),
            name: Some("up".into()),
        };
        assert_eq!(direction(&custom), None);
    }

    /// A reply's keys name its turn; a card's, a note's and the board's none.
    /// The replies kept are bounded, the oldest out.
    #[test]
    fn a_reply_is_found_by_its_message_and_the_oldest_go() {
        assert_eq!(turn_of_key("turn_0a1b:0"), Some("turn_0a1b"));
        assert_eq!(turn_of_key("turn_0a1b:footer"), Some("turn_0a1b"));
        assert_eq!(turn_of_key("confirm:act_1"), None);
        assert_eq!(turn_of_key("board"), None);
        let r = Replies::default();
        r.noted(7, "confirm:act_1");
        assert_eq!(r.turn(7), None);
        for m in 0..(KEPT as u64 + 2) {
            r.noted(m + 100, &format!("turn_{m}:0"));
        }
        assert_eq!(r.turn(100), None, "the oldest out");
        assert_eq!(r.turn(102).as_deref(), Some("turn_2"));
        r.noted(102, "turn_2:footer");
        assert_eq!(r.turn(102).as_deref(), Some("turn_2"));
    }
}
