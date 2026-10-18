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
use theseus_protocol::initiative::InitiativeRateParams;
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
pub(crate) struct Replies(
    Mutex<VecDeque<(u64, String)>>,
    /// The turns that ran in a thread and carried its tag in the DM, with
    /// the thread's slug (theseus-sjzm).
    Mutex<VecDeque<(String, String)>>,
    /// A task's messages that are no turn's reply (theseus-bh3t.2).
    super::task_react::TaskMsgs,
);

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

    /// `turn` ran in the thread `slug` and its reply carries the thread's tag.
    pub(crate) fn thread_turn(&self, turn: &str, slug: &str) {
        let mut t = self.1.lock().unwrap_or_else(|e| e.into_inner());
        if t.iter().any(|(x, _)| x == turn) {
            return;
        }
        t.push_back((turn.to_string(), slug.to_string()));
        while t.len() > KEPT {
            t.pop_front();
        }
    }

    /// The thread a reply to `message` addresses: the one whose tag the
    /// message carried.
    pub(crate) fn thread_of(&self, message: u64) -> Option<String> {
        let turn = self.turn(message)?;
        let t = self.1.lock().unwrap_or_else(|e| e.into_inner());
        t.iter().find(|(x, _)| *x == turn).map(|(_, s)| s.clone())
    }

    /// The messages that carry a task, for a reaction to find.
    pub(crate) fn tasks(&self) -> &super::task_react::TaskMsgs {
        &self.2
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

/// A reply's key names its turn: `turn_…:0`, `turn_…:footer`; a note Theseus
/// wrote first names its own id, `ini:<id>` (theseus-slvl.9).
pub(crate) fn turn_of_key(key: &str) -> Option<&str> {
    if let Some(id) = key.strip_prefix("ini:") {
        return Some(id);
    }
    let (turn, _) = key.split_once(':')?;
    turn.starts_with("turn_").then_some(turn)
}

/// What a 👍 or 👎 says of a message Theseus sent first (theseus-slvl.9).
pub(crate) fn verdict(emoji: &EmojiReactionType) -> Option<&'static str> {
    let EmojiReactionType::Unicode { name } = emoji else {
        return None;
    };
    match name.trim_end_matches('\u{fe0f}') {
        "\u{1f44d}" => Some("up"),
        "\u{1f44e}" => Some("down"),
        _ => None,
    }
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

/// A persona's reaction (route.v5, theseus-c1uz): the emoji a `[personas]`
/// table gives it, with or without the variation selector. The key is what
/// `route.correct` names as `persona:<key>`.
pub(crate) fn persona_emoji<'a>(
    emoji: &EmojiReactionType,
    personas: impl IntoIterator<Item = (&'a String, Option<&'a str>)>,
) -> Option<String> {
    let EmojiReactionType::Unicode { name } = emoji else {
        return None;
    };
    let name = name.trim_end_matches('\u{fe0f}');
    personas
        .into_iter()
        .find(|(_, e)| e.is_some_and(|e| e.trim_end_matches('\u{fe0f}') == name))
        .map(|(k, _)| format!("persona:{k}"))
}

/// The footer's word on where routing ran a turn: `routed: quick`, or
/// `routed: correction` when the owner's correction placed it.
pub(crate) fn routed(r: &theseus_protocol::TurnSubmitResult) -> Option<String> {
    let t = r.route.as_ref()?;
    let who = t.persona.as_ref().map(|p| p.words());
    let what = match (t.source.as_deref(), &t.mode) {
        (Some("task brief"), Some(m)) => Some(format!("{m} (task brief)")),
        (src, mode) => src.map(str::to_string).or_else(|| mode.clone()),
    };
    match (who, what) {
        (Some(w), Some(m)) => Some(format!("routed: {w} · {m}")),
        (Some(w), None) => Some(format!("routed: {w}")),
        (None, Some(m)) => Some(format!("routed: {m}")),
        (None, None) => None,
    }
}

/// The place a reaction came from, as the bindings name it.
pub(super) fn place_of(r: &GatewayReaction) -> String {
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
    pub(crate) async fn on_reaction(self: &std::sync::Arc<Self>, r: GatewayReaction) {
        if r.user_id.get() == self.bot_id() {
            return;
        }
        if self.task_reaction(&r).await {
            return;
        }
        let Some(turn) = self.replies.turn(r.message_id.get()) else {
            return;
        };
        let place = place_of(&r);
        let Ok(Some(session)) = self.core.outbox.place_session(&place) else {
            return;
        };
        if let Some(rating) = verdict(&r.emoji) {
            self.rate_initiative(&r, session, turn, rating).await;
            return;
        }
        let personas = self
            .core
            .cfg
            .personas
            .iter()
            .map(|(k, p)| (k, p.emoji.as_deref()));
        let to = match direction(&r.emoji) {
            Some(d) => d.to_string(),
            None => match persona_emoji(&r.emoji, personas) {
                Some(p) => p,
                None => return,
            },
        };
        let to = to.as_str();
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

impl Shared {
    /// A 👍 or 👎 on a reply or a note: the owner's verdict on what Theseus
    /// said first. The core says whether it was (`initiative.rate`); a
    /// reaction on any other reply is no verdict and changes nothing.
    async fn rate_initiative(
        &self,
        r: &GatewayReaction,
        session: String,
        turn: String,
        rating: &'static str,
    ) {
        let p = InitiativeRateParams {
            session_id: Some(session.clone()),
            turn_id: Some(turn.clone()),
            rating: rating.into(),
            via: Some("reaction".into()),
            discord: Some(DiscordOrigin {
                user_id: r.user_id.to_string(),
                channel_id: r.channel_id.to_string(),
                guild_id: r.guild_id.map(|g| g.to_string()),
            }),
        };
        let res = self
            .rpc
            .call::<_, Value>(theseus_protocol::method::INITIATIVE_RATE, p)
            .await;
        // Not a message sent first: no ledger line, since most replies are not.
        if res.is_ok() {
            self.core.binding_ledger(
                LedgerKind::DiscordLabel,
                Some(&session),
                json!({"turn": turn, "rating": rating, "via": "reaction", "by": r.user_id.to_string()}),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumb_is_a_verdict_and_an_arrow_is_not() {
        let u = |s: &str| EmojiReactionType::Unicode { name: s.into() };
        assert_eq!(verdict(&u("\u{1f44d}")), Some("up"));
        assert_eq!(verdict(&u("\u{1f44e}\u{fe0f}")), Some("down"));
        assert_eq!(verdict(&u("\u{2b06}")), None);
        assert_eq!(turn_of_key("ini:ini_0a1b"), Some("ini_0a1b"));
        assert_eq!(turn_of_key("note:abc"), None);
        assert_eq!(turn_of_key("turn_x:0"), Some("turn_x"));
    }

    #[test]
    fn the_footer_says_where_routing_ran_the_turn() {
        let mut r = theseus_protocol::TurnSubmitResult::default();
        assert_eq!(routed(&r), None);
        r.route = Some(theseus_protocol::route::TurnRoute {
            mode: Some("quick".into()),
            reason: "detour".into(),
            from: "sonnet".into(),
            source: None,
            ..Default::default()
        });
        assert_eq!(routed(&r).as_deref(), Some("routed: quick"));
        r.route.as_mut().unwrap().source = Some("correction".into());
        assert_eq!(routed(&r).as_deref(), Some("routed: correction"));
        r.route.as_mut().unwrap().source = Some("task brief".into());
        r.route.as_mut().unwrap().mode = Some("routine_coding".into());
        assert_eq!(
            routed(&r).as_deref(),
            Some("routed: routine_coding (task brief)")
        );
    }

    #[test]
    fn the_footer_says_who_answered_and_why() {
        let mut r = theseus_protocol::TurnSubmitResult {
            route: Some(theseus_protocol::route::TurnRoute {
                mode: Some("deep_coding".into()),
                reason: "verdict".into(),
                from: "opus".into(),
                persona: Some(theseus_protocol::route::RoutePersona {
                    key: Some("birch".into()),
                    name: "Birch".into(),
                    how: "jev_shown".into(),
                    why: "\"CVE\" \u{b7} 0.74 \u{b7} next alder 0.12".into(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            routed(&r).as_deref(),
            Some("routed: Birch \u{b7} \"CVE\" \u{b7} 0.74 \u{b7} next alder 0.12 \u{b7} deep_coding")
        );
        // No mode, but a persona: it still says who.
        r.route.as_mut().unwrap().mode = None;
        assert!(routed(&r).unwrap().starts_with("routed: Birch"));
    }

    /// A persona's own emoji, with or without the variation selector, is a
    /// correction naming it; an emoji no persona has is nothing.
    #[test]
    fn a_personas_emoji_corrects_to_that_persona() {
        let u = |s: &str| EmojiReactionType::Unicode { name: s.into() };
        let birch = "birch".to_string();
        let alder = "alder".to_string();
        let ps = || vec![(&birch, Some("\u{1f332}")), (&alder, None)];
        assert_eq!(
            persona_emoji(&u("\u{1f332}"), ps()).as_deref(),
            Some("persona:birch")
        );
        assert_eq!(
            persona_emoji(&u("\u{1f332}\u{fe0f}"), ps()).as_deref(),
            Some("persona:birch")
        );
        assert_eq!(persona_emoji(&u("\u{1f44d}"), ps()), None);
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
