//! Google's one-time consent from Discord, with no one at the machine (issue 59). A child of
//! `runtime`, on the shared pattern (`consent.rs`).
//!
//! Every step is claimed once and passes the owner-in-private judge first; Paste and the form's
//! submit need this binding's record of the consent (the same user, its stage, before its
//! deadline), and are refused in fixed words before any request otherwise.
//!
//! A turn can only **offer** the control: `google.sign_in_offer` posts one `google_sign_in` card
//! to the owner's direct messages, with **Connect Google** and **Revoke Google**. Every step after
//! that is the owner's own press, which the daemon judges (`rpc/google.rs`: an owner, in a private
//! place, read off the press as every press is):
//!
//! - **Connect Google** (`gsi:in:google`) is `google.auth.begin` with `manual` (the paste, as
//!   `theseus google auth` takes it from a shell with no browser). The presser alone is shown what
//!   the consent asks for, the address to open as a link, and **Paste the address**.
//! - **Paste the address** opens a form with one field: the whole address the browser ended on. Its
//!   submit is `google.auth.finish { pasted }`: the daemon keeps what Google returns exactly where
//!   and how `theseus google auth` keeps it (`redeem_and_keep`).
//! - **Revoke Google** (`gsi:out:google`) is `google.revoke`.
//!
//! The pasted value passes through Discord to the daemon and is kept nowhere; every answer is fixed
//! words.

use std::time::Duration;

use serde_json::Value;
use theseus_protocol::google::{
    GoogleAuthBeginParams, GoogleAuthBeginResult, GoogleAuthFinishParams, GoogleAuthFinishResult,
    GoogleRevokeParams, GoogleRevokeResult,
};
use theseus_protocol::{error_code, method};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ButtonStyle, Component};

use super::consent::{self, address_words, button, row, word_ok, Stage, Why};
use super::Shared;
use crate::rpc_client::CallError;

const PREFIX: &str = "gsi";
/// The modal's custom id: `gsi:m:<consent id>`.
pub(crate) const MODAL: &str = "gsi:m:";
/// The card's one target: there is one Google account.
const TARGET: &str = "google";
const WHAT: &str = "google consent reply";

pub(crate) const NOT_HERE: &str = "🔐 Only you can connect Google, and only in a private place: \
     your direct messages with Theseus, or a channel bound private. Nothing began.";
pub(crate) const NOT_YOURS: &str =
    "🔐 That consent was begun by someone else, so it goes no further here. Nothing was sent.";
pub(crate) const EXPIRED: &str =
    "🔐 That consent is no longer open: it ran out, a newer one began, or \
     Theseus restarted. Press **Connect Google** again.";
pub(crate) const NOT_THIS: &str =
    "🔐 That is not the address of this consent. Press **Connect Google** \
     again, and paste the whole address your browser ends on.";
pub(crate) const REFUSED: &str = "🔐 The consent was refused, and nothing was kept.";
pub(crate) const GOOGLE_NO: &str =
    "🔐 Google said no, and nothing was kept. Press **Connect Google** to try again.";
pub(crate) const MAY_BE_KEPT: &str =
    "🔐 Google is connected for now, but keeping its grant could not be made sure of: it may or \
     may not be used after Theseus restarts. Check with `theseus google status`, or press \
     **Connect Google** again.";
pub(crate) const NO_BEGIN: &str = "🔐 The consent could not begin. Nothing was kept.";
pub(crate) const TOO_LONG: &str =
    "🔐 This consent's address is too long to show whole here, so it is not shown: run \
     `theseus google auth` at the terminal. Nothing was kept.";
pub(crate) const PASTE_TOO_LONG: &str =
    "🔐 That is longer than the form takes (4,000 characters), so nothing was sent. Run \
     `theseus google auth` at the terminal.";
pub(crate) const REVOKED: &str = "🔐 Google's grant is ended, and forgotten here.";
pub(crate) const NO_REVOKE: &str = "🔐 The revoke could not be done.";

/// A press of a control of this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    In,
    Out,
    Paste(String),
}

pub(crate) fn parse(custom_id: &str) -> Option<Press> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (verb, word) = rest.split_once(':')?;
    match verb {
        "in" if word == TARGET => Some(Press::In),
        "out" if word == TARGET => Some(Press::Out),
        "paste" => word_ok(word).then(|| Press::Paste(word.into())),
        _ => None,
    }
}

/// The card's two buttons.
pub(crate) fn buttons() -> Vec<Component> {
    row(vec![
        button(
            format!("{PREFIX}:in:{TARGET}"),
            "Connect Google",
            ButtonStyle::Primary,
        ),
        button(
            format!("{PREFIX}:out:{TARGET}"),
            "Revoke Google",
            ButtonStyle::Secondary,
        ),
    ])
}

/// The form "Paste the address" opens: one field.
pub(crate) fn modal(id: &str) -> Value {
    consent::modal(PREFIX, id, "Finish connecting Google")
}

/// What the presser is shown, line by line: the account, and each thing the consent asks for.
pub(crate) fn begun_lines(b: &GoogleAuthBeginResult) -> Vec<String> {
    let mut t = vec![format!("🔐 You will connect Google for `{}`.", b.account)];
    if !b.asks.is_empty() {
        t.push("It asks for:".into());
        t.extend(b.asks.iter().map(|a| format!("- {a}")));
    }
    t
}

/// A failure, in fixed words: never any part of what the daemon or the presser said.
pub(crate) fn failure(e: &CallError) -> &'static str {
    if e.code == error_code::REFUSED {
        return NOT_HERE;
    }
    // By how the daemon's words begin: what follows may hold what was pasted.
    let m = e.message.as_str();
    if m.starts_with("no sign-in is open") || m.starts_with("the redirect was taken") {
        EXPIRED
    } else if m.starts_with("the redirect's state") || m.starts_with("the address holds no code") {
        NOT_THIS
    } else if m.starts_with("that sign-in was for a different account")
        || m.starts_with("Google named no account")
        || m.starts_with("Google granted none")
    {
        REFUSED
    } else {
        GOOGLE_NO
    }
}

/// What a finished consent says: connected and kept, or connected with its keeping uncertain.
pub(crate) fn done_words(done: &GoogleAuthFinishResult) -> String {
    if done.store == "memory" {
        return MAY_BE_KEPT.into();
    }
    format!(
        "✅ Google is connected for `{}`: {} granted.",
        done.account,
        if done.granted.is_empty() {
            "nothing".to_string()
        } else {
            done.granted.join(", ")
        }
    )
}

fn refused(why: Why) -> &'static str {
    match why {
        Why::NotYours => NOT_YOURS,
        Why::Missing | Why::Stale | Why::Stage => EXPIRED,
    }
}

impl Shared {
    /// A press of this module's controls.
    pub(super) async fn google_sign_in_press(&self, i: &Interaction, press: Press, who: &str) {
        let Some((user, origin)) = self.consent_gate(i, WHAT, NOT_HERE).await else {
            return;
        };
        match press {
            Press::In => self.google_begin(i, who, user, origin).await,
            Press::Paste(id) => match self.consents.step(
                PREFIX,
                &id,
                Some((user, origin.channel_id.as_str())),
                |r| r.ready(),
            ) {
                Ok(_) => self.respond_modal(i, &modal(&id)).await,
                Err(why) => self.consent_reply(i, WHAT, refused(why), vec![]).await,
            },
            Press::Out => self.google_revoke_press(i, who, origin).await,
        }
    }

    /// **Connect Google**: the daemon's manual begin, as the presser; the record begun.
    async fn google_begin(
        &self,
        i: &Interaction,
        who: &str,
        user: u64,
        origin: theseus_protocol::DiscordOrigin,
    ) {
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        let params = GoogleAuthBeginParams {
            manual: true,
            author: Some(who.to_string()),
            discord: Some(origin.clone()),
        };
        consent::sending(method::GOOGLE_AUTH_BEGIN);
        let r = self
            .rpc
            .call::<_, GoogleAuthBeginResult>(method::GOOGLE_AUTH_BEGIN, params)
            .await;
        let b = match r {
            Ok(b) => b,
            Err(e) if e.code == error_code::REFUSED => {
                return self.consent_finish(i, WHAT, NOT_HERE, &[]).await
            }
            Err(_) => return self.consent_finish(i, WHAT, NO_BEGIN, &[]).await,
        };
        let (Some(address), Some(mut shown)) =
            (address_words(&b.url), consent::pages(&begun_lines(&b)))
        else {
            return self.consent_finish(i, WHAT, TOO_LONG, &[]).await;
        };
        let rec = consent::Record {
            target: TARGET.to_string(),
            deadline: self.consents.now() + Duration::from_secs(b.expires_secs),
            url: b.url.clone(),
            ..self.consents.fresh(PREFIX, &b.auth_id, user, origin)
        };
        self.consents.begin(rec);
        shown.push(address);
        self.consent_pages(i, WHAT, &shown, &consent::paste_button(PREFIX, &b.auth_id))
            .await;
    }

    /// The form's submit: the daemon's manual finish, as the presser, for the record's consent. The
    /// field goes into the request and nowhere else.
    pub(super) async fn google_sign_in_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some((user, origin)) = self.consent_gate(i, WHAT, NOT_HERE).await else {
            return;
        };
        let Some(auth_id) = consent::submitted(m, PREFIX) else {
            return self.consent_reply(i, WHAT, EXPIRED, vec![]).await;
        };
        if let Err(why) = self.consents.step(
            PREFIX,
            &auth_id,
            Some((user, origin.channel_id.as_str())),
            |r| r.ready(),
        ) {
            return self.consent_reply(i, WHAT, refused(why), vec![]).await;
        }
        let Some(pasted) = consent::pasted(m) else {
            return self.consent_reply(i, WHAT, PASTE_TOO_LONG, vec![]).await;
        };
        self.consents.set(PREFIX, &auth_id, Stage::Pasted);
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        let params = GoogleAuthFinishParams {
            auth_id: auth_id.clone(),
            pasted: Some(pasted),
            wait_secs: 0,
            author: Some(who.to_string()),
            discord: Some(origin),
        };
        consent::sending(method::GOOGLE_AUTH_FINISH);
        let r = self
            .rpc
            .call::<_, GoogleAuthFinishResult>(method::GOOGLE_AUTH_FINISH, params)
            .await;
        self.consents.retire(PREFIX, &auth_id);
        let (text, controls) = match r {
            Ok(done) if done.state == "done" => (done_words(&done), buttons()),
            Ok(_) => (EXPIRED.to_string(), vec![]),
            Err(e) => (failure(&e).to_string(), vec![]),
        };
        self.consent_finish(i, WHAT, &text, &controls).await;
    }

    /// **Revoke Google**: the daemon's revoke, as the presser; any open consent retired.
    async fn google_revoke_press(
        &self,
        i: &Interaction,
        who: &str,
        origin: theseus_protocol::DiscordOrigin,
    ) {
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        self.consents.retire_target(PREFIX, TARGET);
        let params = GoogleRevokeParams {
            author: Some(who.to_string()),
            discord: Some(origin),
        };
        consent::sending(method::GOOGLE_REVOKE);
        let r = self
            .rpc
            .call::<_, GoogleRevokeResult>(method::GOOGLE_REVOKE, params)
            .await;
        let text = match r {
            Ok(_) => REVOKED,
            Err(e) if e.code == error_code::REFUSED => NOT_HERE,
            Err(_) => NO_REVOKE,
        };
        self.consent_finish(i, WHAT, text, &[]).await;
    }
}

#[cfg(test)]
mod tests;
