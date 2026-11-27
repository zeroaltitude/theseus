//! The owner's consent by a press and a form (issue 59), written once. A child of `runtime`.
//!
//! What a sign-in from Discord needs, whatever it signs in to (an MCP server, Google's one-time
//! consent, the AWS alerts' confirmation):
//!
//! - **Each interaction acted on once.** Its id is claimed before anything is done
//!   (`Records::claim`), and kept for as long as Discord could deliver it again; a second delivery
//!   does nothing.
//! - **The owner's own press, in a private place, at every step**: the existing judge
//!   (`places::owner_in_private`) is called for the presser and the place pressed in, before any
//!   other work (`Shared::consent_gate`).
//! - **A trusted record of each sign-in begun here** (`Record`): its id, the user whose press began
//!   it, what it signs in to, the private place it was begun in, its stage, its deadline (the
//!   daemon's own lifetime) and the address to open. Every later step (I agree, Paste the address,
//!   the form's submit) needs it: the same user, the right stage, before its deadline. What was
//!   agreed is the record's, set only by that user's own I agree press; no custom id carries it. A
//!   missing, mismatched, stale or wrong-stage record is refused in fixed words before any request,
//!   so an old form never reaches the daemon. A record is retired on success, when a newer one of
//!   its kind replaces it, on sign-out and at its deadline.
//! - **The address shown to its presser alone**, in an ephemeral answer, whole or not at all: an
//!   address too long to show whole is refused, never cut, and every place a sign-in goes through
//!   is shown, over several messages when one cannot hold them.
//! - **A form with one field**, whose value passes through Discord to the daemon's request and is
//!   kept nowhere; a longer value than the form takes is refused, never cut.
//! - **A fixed-word answer.**
//!
//! Each user of this module names its custom-id prefix, its words and its daemon calls; nothing here
//! logs, keeps or says a pasted value.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_core::approval::{Answerer, Surface};
use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};

use super::persona_soul::field_of;
use super::{origin_of, Shared};
use crate::render::Buttons;

/// The form's one field.
pub(crate) const FIELD: &str = "address";
/// Discord's cap on a text input: a longer paste is refused.
pub(crate) const MAX: usize = 4000;
/// Discord's cap on a message's text: what is shown fits whole, or is split, or is refused.
pub(crate) const SHOWN_MAX: usize = 2000;
/// How long Discord may deliver an interaction again: its token lives fifteen minutes.
const CLAIM_FOR: Duration = Duration::from_secs(15 * 60);
/// The interactions remembered at most; the oldest goes first.
const CLAIMS_MAX: usize = 4096;
/// The sign-ins open at most, of every kind; the oldest goes first.
const OPEN_MAX: usize = 16;

/// The daemon requests this module's users sent, by method (tests: a refusal sends none; each test
/// runs in its own process).
#[cfg(test)]
pub(crate) static SENT: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// A request to the daemon is about to go: noted for the tests, nothing else.
pub(crate) fn sending(method: &'static str) {
    #[cfg(test)]
    SENT.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(method);
    let _ = method;
}

/// How many steps wait for a slot (tests).
#[cfg(test)]
pub(crate) static WAITING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A test's pause: armed at one point, reached once, held until released.
#[cfg(test)]
pub(crate) struct Pause {
    at: &'static str,
    pub(crate) reached: tokio::sync::Notify,
    pub(crate) release: tokio::sync::Notify,
}

#[cfg(test)]
static PAUSE: Mutex<Option<std::sync::Arc<Pause>>> = Mutex::new(None);

/// Arm the pause at `at` (tests).
#[cfg(test)]
pub(crate) fn pause_at(at: &'static str) -> std::sync::Arc<Pause> {
    let p = std::sync::Arc::new(Pause {
        at,
        reached: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    *PAUSE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(p.clone());
    p
}

/// A point a test may hold a step at.
#[cfg(test)]
pub(crate) async fn pause_point(at: &'static str) {
    let armed = PAUSE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take_if(|p| p.at == at);
    if let Some(p) = armed {
        p.reached.notify_one();
        p.release.notified().await;
    }
}

/// Outside tests, nothing.
#[cfg(not(test))]
pub(crate) fn pause_point(_at: &'static str) -> std::future::Ready<()> {
    std::future::ready(())
}

/// A word a custom id may carry: a server's name, or a sign-in's id.
pub(crate) fn word_ok(w: &str) -> bool {
    !w.is_empty()
        && w.len() <= 64
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

pub(crate) fn button(id: String, label: &str, style: ButtonStyle) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(id),
        disabled: false,
        emoji: None,
        label: Some(label.to_string()),
        style,
        url: None,
        sku_id: None,
    })
}

pub(crate) fn row(components: Vec<Component>) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        id: None,
        components,
    })]
}

/// **Paste the address**: `<prefix>:paste:<id>`. What was agreed is the record's, never the id's.
pub(crate) fn paste_button(prefix: &str, id: &str) -> Vec<Component> {
    row(vec![button(
        format!("{prefix}:paste:{id}"),
        "Paste the address",
        ButtonStyle::Primary,
    )])
}

/// The form **Paste the address** opens, `<prefix>:m:<id>`: one field.
pub(crate) fn modal(prefix: &str, id: &str, title: &str) -> Value {
    form(
        &format!("{prefix}:m:{id}"),
        title,
        "The address your browser ended on",
        "The whole address, from the bar (at most 4,000 characters). It passes through Discord to \
         Theseus, and is kept nowhere.",
    )
}

/// A form of one field, `head` its custom id, whose value is read by `pasted` alone.
pub(crate) fn form(head: &str, title: &str, label: &str, about: &str) -> Value {
    json!({"type": 9, "data": {
        "custom_id": head,
        "title": title,
        "components": [{
            "type": 18,
            "label": label,
            "description": about,
            "component": {"type": 4, "custom_id": FIELD, "style": 2,
                          "required": true, "max_length": MAX},
        }],
    }})
}

/// A card a turn offered on this pattern, as the courier posts it: its text, its key, its
/// buttons (`Buttons::Consent`, by its kind).
pub(crate) fn card(kind: &str, body: &Value, correlation: &str) -> (String, String, Buttons) {
    let key = match kind {
        "aws_alerts" => "awsc",
        _ => "gsi",
    };
    (
        body["text"].as_str().unwrap_or("").to_string(),
        format!("{key}:{correlation}"),
        Buttons::Consent(kind.to_string()),
    )
}

/// The buttons of a card `card` made.
pub(crate) fn card_buttons(kind: &str) -> Vec<Component> {
    match kind {
        "aws_alerts" => super::aws_alerts::buttons(),
        _ => super::google_sign_in::buttons(),
    }
}

/// The sign-in a form's custom id names: `<prefix>:m:<id>`, and nothing after it.
pub(crate) fn submitted(m: &ModalInteractionData, prefix: &str) -> Option<String> {
    let id = m.custom_id.strip_prefix(prefix)?.strip_prefix(":m:")?;
    word_ok(id).then(|| id.to_string())
}

/// The form's value, read here and handed straight to the daemon's request; `None` when it is
/// longer than the form takes, which is refused, never cut.
pub(crate) fn pasted(m: &ModalInteractionData) -> Option<String> {
    let v = field_of(m, FIELD);
    (v.chars().count() <= MAX).then_some(v)
}

/// The address to open, as a link, and what to do after; `None` when it cannot be shown whole in
/// one message, which is refused, never cut.
pub(crate) fn address_words(url: &str) -> Option<String> {
    let t = format!(
        "🔐 [Open this address to sign in](<{url}>). After you sign in, the page may fail to load: \
         copy the whole address your browser ends on, then press **Paste the address**."
    );
    (t.chars().count() <= SHOWN_MAX).then_some(t)
}

/// `lines` as messages of at most `SHOWN_MAX` characters each, never cutting a line; `None` when
/// one line alone is longer.
pub(crate) fn pages(lines: &[String]) -> Option<Vec<String>> {
    let mut out: Vec<String> = vec![];
    for l in lines {
        if l.chars().count() > SHOWN_MAX {
            return None;
        }
        match out.last_mut() {
            Some(p) if p.chars().count() + 1 + l.chars().count() <= SHOWN_MAX => {
                p.push('\n');
                p.push_str(l);
            }
            _ => out.push(l.clone()),
        }
    }
    Some(out)
}

/// The presser, as the daemon judges it: read off the interaction.
pub(crate) fn presser(i: &Interaction) -> Option<DiscordOrigin> {
    origin_of(
        i.author().map(|u| u.id.get()),
        i.channel.as_ref().map(|c| c.id.get()),
        i.guild_id,
    )
}

/// The user whose press this is.
pub(crate) fn user_of(i: &Interaction) -> Option<u64> {
    i.author().map(|u| u.id.get())
}

/// Where a sign-in begun here is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Begun: its address was made, and shown when nothing needs agreeing to.
    Begun,
    /// Its user pressed I agree (only when the sign-in goes through other places).
    Agreed,
    /// Its form was sent: it is the daemon's now, and no other form is taken.
    Pasted,
}

/// A sign-in this binding began, as trusted state: what every later step is checked against.
#[derive(Debug, Clone)]
pub(crate) struct Record {
    /// The module's prefix: `mcpin`, `gsi`, `awsc`.
    pub(crate) kind: &'static str,
    /// The daemon's sign-in id (or, for a form with no sign-in in the daemon, one made here).
    pub(crate) id: String,
    /// The user whose press began it.
    pub(crate) user: u64,
    /// What it signs in to: the server's name, `google`, `alerts`.
    pub(crate) target: String,
    /// The private place it was begun in.
    pub(crate) origin: DiscordOrigin,
    pub(crate) stage: Stage,
    /// Whether an I agree press is needed before the address.
    pub(crate) needs_agree: bool,
    pub(crate) deadline: Instant,
    /// The address to open.
    pub(crate) url: String,
}

impl Record {
    /// Whether its form may be opened and sent: agreed to, or nothing to agree to.
    pub(crate) fn ready(&self) -> bool {
        match self.stage {
            Stage::Begun => !self.needs_agree,
            Stage::Agreed => true,
            Stage::Pasted => false,
        }
    }
}

/// Why a step was refused before any request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// No such sign-in was begun here, or it was retired.
    Missing,
    /// It was begun by someone else.
    NotYours,
    /// Its deadline passed.
    Stale,
    /// Not at the stage this step needs.
    Stage,
}

/// The binding's records and claimed interactions, both bounded.
#[derive(Default)]
pub(crate) struct Records {
    open: Mutex<Vec<Record>>,
    claimed: Mutex<VecDeque<(u64, Instant)>>,
    /// One per daemon pending slot (MCP's sign-in, Google's consent): a begin, a finish and a
    /// sign-out of that kind run one at a time, from the record's check to its change, so an old
    /// form never reaches the daemon after a newer begin has replaced its slot.
    slots: [tokio::sync::Mutex<()>; 2],
    /// The clock's seam: milliseconds added to now (tests move time with it; nothing sleeps).
    skew_ms: AtomicU64,
}

impl Records {
    pub(crate) fn now(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.skew_ms.load(Ordering::SeqCst))
    }

    /// A new record of sign-in `id` of `kind`, begun now by `user` in `origin`: at its first stage,
    /// with nothing to agree to, no address and its deadline now; its user sets the rest.
    pub(crate) fn fresh(
        &self,
        kind: &'static str,
        id: &str,
        user: u64,
        origin: DiscordOrigin,
    ) -> Record {
        Record {
            kind,
            id: id.to_string(),
            user,
            target: String::new(),
            origin,
            stage: Stage::Begun,
            needs_agree: false,
            deadline: self.now(),
            url: String::new(),
        }
    }

    /// The slot of `kind`, held for a whole begin, finish or sign-out: what is checked under it is
    /// what the daemon still holds.
    pub(crate) async fn slot(&self, kind: &str) -> tokio::sync::MutexGuard<'_, ()> {
        let m = if kind == "gsi" {
            &self.slots[1]
        } else {
            &self.slots[0]
        };
        #[cfg(test)]
        WAITING.fetch_add(1, Ordering::SeqCst);
        let held = m.lock().await;
        #[cfg(test)]
        WAITING.fetch_sub(1, Ordering::SeqCst);
        held
    }

    /// Retire every sign-in of `kind`: the daemon's begin has replaced its slot.
    pub(crate) fn retire_kind(&self, kind: &str) {
        self.open().retain(|r| r.kind != kind);
    }

    /// Move the clock on by `d` (the deadline's tests).
    #[cfg(test)]
    pub(crate) fn advance(&self, d: Duration) {
        let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        self.skew_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// Claim interaction `id`: true the first time it is seen within its life, false after.
    pub(crate) fn claim(&self, id: u64) -> bool {
        let now = self.now();
        let mut c = self
            .claimed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while c
            .front()
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) > CLAIM_FOR)
        {
            c.pop_front();
        }
        if c.iter().any(|(seen, _)| *seen == id) {
            return false;
        }
        if c.len() >= CLAIMS_MAX {
            c.pop_front();
        }
        c.push_back((id, now));
        true
    }

    fn open(&self) -> std::sync::MutexGuard<'_, Vec<Record>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A sign-in begun: it retires every open one of its kind (the daemon holds one at a time), and
    /// every one past its deadline.
    pub(crate) fn begin(&self, r: Record) {
        let now = self.now();
        let mut o = self.open();
        o.retain(|x| x.kind != r.kind && x.deadline > now);
        if o.len() >= OPEN_MAX {
            o.remove(0);
        }
        o.push(r);
    }

    /// The record of sign-in `id` for a step by `who` (its user, and the channel pressed in): the
    /// same user in the place it was begun in, before its deadline, and `ok` of it (the stage the
    /// step needs). A stale one is retired.
    pub(crate) fn step(
        &self,
        kind: &str,
        id: &str,
        who: Option<(u64, &str)>,
        ok: impl Fn(&Record) -> bool,
    ) -> Result<Record, Why> {
        let now = self.now();
        let mut o = self.open();
        let Some(at) = o.iter().position(|r| r.kind == kind && r.id == id) else {
            return Err(Why::Missing);
        };
        if o[at].deadline <= now {
            o.remove(at);
            return Err(Why::Stale);
        }
        let r = &o[at];
        if who != Some((r.user, r.origin.channel_id.as_str())) {
            return Err(Why::NotYours);
        }
        if !ok(r) {
            return Err(Why::Stage);
        }
        Ok(r.clone())
    }

    /// Move sign-in `id` to `stage`.
    pub(crate) fn set(&self, kind: &str, id: &str, stage: Stage) {
        if let Some(r) = self
            .open()
            .iter_mut()
            .find(|r| r.kind == kind && r.id == id)
        {
            r.stage = stage;
        }
    }

    /// Retire sign-in `id`.
    pub(crate) fn retire(&self, kind: &str, id: &str) {
        self.open().retain(|r| !(r.kind == kind && r.id == id));
    }

    /// Retire every sign-in of `kind` for `target` (a sign-out).
    pub(crate) fn retire_target(&self, kind: &str, target: &str) {
        self.open()
            .retain(|r| !(r.kind == kind && r.target == target));
    }

    /// How many are open (tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.open().len()
    }
}

impl Shared {
    /// The gate every step passes first: the interaction claimed (a second delivery does nothing),
    /// then the existing judge of the owner's own press in a private place, for the presser and the
    /// place pressed in. `None`: stop here (a refusal was answered with `not_here`, and showed
    /// nothing else).
    pub(super) async fn consent_gate(
        &self,
        i: &Interaction,
        what: &str,
        not_here: &str,
    ) -> Option<(u64, DiscordOrigin)> {
        if !self.consents.claim(i.id.get()) {
            return None;
        }
        let judged = presser(i).and_then(|d| {
            let who = Answerer {
                label: format!("discord:{}", d.user_id),
                surface: Surface::Discord,
                discord: Some(d.clone()),
                agent: None,
            };
            let ok = theseus_core::places::owner_in_private(
                &who,
                &self.core.runner.place_rule,
                &self.core.cfg,
            );
            let user = user_of(i)?;
            ok.is_ok().then_some((user, d))
        });
        if judged.is_none() {
            self.consent_reply(i, what, not_here, vec![]).await;
        }
        judged
    }

    /// The interaction's first answer, ephemeral, for a step that waits on the daemon: `false` when
    /// Discord refused it, and the step does not go on.
    pub(super) async fn consent_ack(&self, i: &Interaction, what: &str) -> bool {
        use twilight_model::http::interaction::{
            InteractionResponse, InteractionResponseData, InteractionResponseType,
        };
        let deferred = InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(InteractionResponseData {
                flags: Some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        };
        match self
            .interact
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &deferred)
            .await
        {
            Ok(_) => true,
            Err(e) => {
                self.board.error(what, None, e);
                false
            }
        }
    }

    /// An ephemeral answer with controls, to the presser alone; `what` names it in the board's error.
    pub(super) async fn consent_reply(
        &self,
        i: &Interaction,
        what: &str,
        text: &str,
        components: Vec<Component>,
    ) {
        use twilight_model::http::interaction::{
            InteractionResponse, InteractionResponseData, InteractionResponseType,
        };
        let data = InteractionResponseData {
            content: Some(super::command_reply::leaving(&self.core.exits, text)),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            components: Some(components),
            ..Default::default()
        };
        let r = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(data),
        };
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &r)
            .await
        {
            self.board.error(what, None, e);
        }
    }

    /// Edit the acknowledged answer to `text`, with `components`.
    pub(super) async fn consent_finish(
        &self,
        i: &Interaction,
        what: &str,
        text: &str,
        components: &[Component],
    ) {
        let content = super::command_reply::leaving(&self.core.exits, text);
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&content))
            .components(Some(components))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error(what, None, e);
        }
    }

    /// Show `pages` to the presser, whole: the first in the acknowledged answer, each other in an
    /// ephemeral follow-up, `components` on the last.
    pub(super) async fn consent_pages(
        &self,
        i: &Interaction,
        what: &str,
        pages: &[String],
        components: &[Component],
    ) {
        let Some((first, rest)) = pages.split_first() else {
            return;
        };
        if rest.is_empty() {
            return self.consent_finish(i, what, first, components).await;
        }
        self.consent_finish(i, what, first, &[]).await;
        let none = AllowedMentions::default();
        for (n, page) in rest.iter().enumerate() {
            let last = n + 1 == rest.len();
            let content = super::command_reply::leaving(&self.core.exits, page);
            let made = self
                .interact
                .interaction(i.application_id)
                .create_followup(&i.token)
                .content(&content)
                .flags(MessageFlags::EPHEMERAL)
                .allowed_mentions(Some(&none))
                .components(if last { components } else { &[] })
                .await;
            if let Err(e) = made {
                self.board.error(what, None, e);
            }
        }
    }
}

#[cfg(test)]
mod tests;
