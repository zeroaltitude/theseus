//! Google's one-time consent from Discord, with no one at the machine (issue 59). A child of
//! `runtime`, on the shared pattern (`consent.rs`).
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
//! The pasted value goes from the form to the daemon and nowhere else; every answer is fixed words.

use serde_json::Value;
use theseus_protocol::google::{
    GoogleAuthBeginParams, GoogleAuthBeginResult, GoogleAuthFinishParams, GoogleAuthFinishResult,
    GoogleRevokeParams, GoogleRevokeResult,
};
use theseus_protocol::{error_code, method};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ButtonStyle, Component};

use super::consent::{self, address_words, button, row, word_ok};
use super::Shared;
use crate::rpc_client::CallError;

const PREFIX: &str = "gsi";
/// The modal's custom id: `gsi:m:<consent id>:0`.
pub(crate) const MODAL: &str = "gsi:m:";
/// The card's one target: there is one Google account.
const TARGET: &str = "google";

pub(crate) const NOT_HERE: &str = "🔐 Only you can connect Google, and only in a private place: \
     your direct messages with Theseus, or a channel bound private. Nothing began.";
pub(crate) const EXPIRED: &str =
    "🔐 That consent is no longer open: it ran out, a newer one began, or \
     Theseus restarted. Press **Connect Google** again.";
pub(crate) const NOT_THIS: &str =
    "🔐 That is not the address of this consent. Press **Connect Google** \
     again, and paste the whole address your browser ends on.";
pub(crate) const REFUSED: &str = "🔐 The consent was refused, and nothing was kept.";
pub(crate) const GOOGLE_NO: &str =
    "🔐 Google said no, and nothing was kept. Press **Connect Google** to try again.";
pub(crate) const NO_BEGIN: &str = "🔐 The consent could not begin. Nothing was kept.";
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
        "paste" => {
            let (id, _) = word.split_once(':')?;
            word_ok(id).then(|| Press::Paste(id.into()))
        }
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
    consent::modal(PREFIX, id, false, "Finish connecting Google")
}

/// What the presser is shown: the account, what the consent asks for, and the address.
pub(crate) fn begun_words(b: &GoogleAuthBeginResult) -> String {
    let mut t = format!("🔐 You will connect Google for `{}`.", b.account);
    if !b.asks.is_empty() {
        t.push_str(&format!("\nIt asks for: {}.", b.asks.join("; ")));
    }
    format!("{t}\n{}", address_words(&b.url))
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

impl Shared {
    /// A press of this module's controls.
    pub(super) async fn google_sign_in_press(&self, i: &Interaction, press: Press, who: &str) {
        match press {
            Press::In => self.google_begin(i, who).await,
            Press::Paste(id) => self.respond_modal(i, &modal(&id)).await,
            Press::Out => self.google_revoke_press(i, who).await,
        }
    }

    /// **Connect Google**: the daemon's manual begin, as the presser.
    async fn google_begin(&self, i: &Interaction, who: &str) {
        self.ack_ephemeral(i).await;
        let params = GoogleAuthBeginParams {
            manual: true,
            author: Some(who.to_string()),
            discord: consent::presser(i),
        };
        let r = self
            .rpc
            .call::<_, GoogleAuthBeginResult>(method::GOOGLE_AUTH_BEGIN, params)
            .await;
        let (text, controls) = match r {
            Ok(b) => (
                begun_words(&b),
                consent::paste_button(PREFIX, &b.auth_id, false),
            ),
            Err(e) if e.code == error_code::REFUSED => (NOT_HERE.to_string(), vec![]),
            Err(_) => (NO_BEGIN.to_string(), vec![]),
        };
        self.consent_finish(i, "google consent reply", &text, &controls)
            .await;
    }

    /// The form's submit: the daemon's manual finish, as the presser. The field goes into the
    /// request and nowhere else.
    pub(super) async fn google_sign_in_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some((auth_id, _)) = consent::submitted(m, PREFIX) else {
            return self.tell(i, EXPIRED.into()).await;
        };
        self.ack_ephemeral(i).await;
        let params = GoogleAuthFinishParams {
            auth_id,
            pasted: Some(consent::pasted(m)),
            wait_secs: 0,
            author: Some(who.to_string()),
            discord: consent::presser(i),
        };
        let r = self
            .rpc
            .call::<_, GoogleAuthFinishResult>(method::GOOGLE_AUTH_FINISH, params)
            .await;
        let (text, controls) = match r {
            Ok(done) if done.state == "done" => (
                format!(
                    "✅ Google is connected for `{}`: {} granted.",
                    done.account,
                    if done.granted.is_empty() {
                        "nothing".to_string()
                    } else {
                        done.granted.join(", ")
                    }
                ),
                buttons(),
            ),
            Ok(_) => (EXPIRED.to_string(), vec![]),
            Err(e) => (failure(&e).to_string(), vec![]),
        };
        self.consent_finish(i, "google consent reply", &text, &controls)
            .await;
    }

    /// **Revoke Google**: the daemon's revoke, as the presser.
    async fn google_revoke_press(&self, i: &Interaction, who: &str) {
        self.ack_ephemeral(i).await;
        let params = GoogleRevokeParams {
            author: Some(who.to_string()),
            discord: consent::presser(i),
        };
        let r = self
            .rpc
            .call::<_, GoogleRevokeResult>(method::GOOGLE_REVOKE, params)
            .await;
        let text = match r {
            Ok(_) => REVOKED,
            Err(e) if e.code == error_code::REFUSED => NOT_HERE,
            Err(_) => NO_REVOKE,
        };
        self.consent_finish(i, "google consent reply", text, &[])
            .await;
    }
}

#[cfg(test)]
mod tests;
