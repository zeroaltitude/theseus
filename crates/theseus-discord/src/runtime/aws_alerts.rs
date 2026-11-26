//! The AWS alerts subscription's confirmation from Discord, with no one at the machine (issue 59). A
//! child of `runtime`, on the shared pattern (`consent.rs`).
//!
//! A turn can only **offer** the control: `alerts.confirm_offer` posts one `aws_alerts` card to the
//! owner's direct messages, with **Confirm alerts**. Every step after that is the owner's own:
//!
//! - **Confirm alerts** (`awsc:in:alerts`) opens a form (`awsc:m:<id>`) with one field: the Token=… value from the
//!   link in SNS's email, or the whole address (copied, never opened).
//! - Its submit is `aws.confirm_alerts { token }` as the presser, which the daemon judges
//!   (`rpc/aws.rs`: an owner, in a private place, read off the press as every press is) and
//!   confirms exactly as `theseus aws confirm-alerts` does.
//!
//! Each press and each submit is claimed once and passes the owner-in-private judge first; the
//! press begins this binding's record of the form, and the submit needs it (the same user, before its
//! deadline, not sent before). The token passes through Discord to the daemon and is kept nowhere;
//! every answer is fixed words.

use std::time::Duration;

use serde_json::Value;
use theseus_protocol::{error_code, method, AwsConfirmAlertsParams, AwsConfirmAlertsResult};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ButtonStyle, Component};

use super::consent::{self, button, row, Stage, Why};
use super::Shared;
use crate::rpc_client::CallError;

const PREFIX: &str = "awsc";
/// The card's one target: the bound account's alerts subscription.
const TARGET: &str = "alerts";
/// The modal's custom id: `awsc:m:<id>`.
pub(crate) const MODAL: &str = "awsc:m:";
const WHAT: &str = "aws alerts reply";
/// How long a form opened here may be sent: there is no sign-in in the daemon to outlive.
pub(crate) const FORM_FOR: Duration = Duration::from_secs(10 * 60);

pub(crate) const NOT_HERE: &str = "🔐 Only you can confirm the alerts, and only in a private \
     place: your direct messages with Theseus, or a channel bound private. Nothing was confirmed.";
pub(crate) const NOT_YOURS: &str =
    "🔐 That form was opened by someone else, so it goes no further here. Nothing was sent.";
pub(crate) const EXPIRED: &str =
    "🔐 That form is no longer open: it ran out, was sent already, or Theseus restarted. Press \
     **Confirm alerts** again.";
pub(crate) const NOT_THIS: &str = "🔐 That is not a confirmation token. Press **Confirm alerts** \
     again, and paste the long Token=… value from the link in SNS's email, or the whole address.";
pub(crate) const AWS_NO: &str =
    "🔐 AWS did not confirm it, and nothing was kept. Press **Confirm alerts** to try again.";
pub(crate) const PASTE_TOO_LONG: &str =
    "🔐 That is longer than the form takes (4,000 characters), so nothing was sent.";

/// The card's one button.
pub(crate) fn buttons() -> Vec<Component> {
    row(vec![button(
        format!("{PREFIX}:in:{TARGET}"),
        "Confirm alerts",
        ButtonStyle::Primary,
    )])
}

/// Whether `custom_id` is this card's button.
pub(crate) fn parse(custom_id: &str) -> bool {
    custom_id
        .strip_prefix(PREFIX)
        .and_then(|r| r.strip_prefix(":in:"))
        == Some(TARGET)
}

/// The form the button opens, for record `id`: one field, for the token.
pub(crate) fn modal(id: &str) -> Value {
    consent::form(
        &format!("{MODAL}{id}"),
        "Confirm the alerts",
        "The token from SNS's email",
        "The Token=… value, or the whole link (copy it; do not open it; at most 4,000 \
         characters). It passes through Discord to Theseus, and is kept nowhere.",
    )
}

/// What the presser is told when it is confirmed: the account, and whether only it can unsubscribe.
pub(crate) fn done_words(r: &AwsConfirmAlertsResult) -> String {
    if r.authenticated {
        format!(
            "✅ The alerts subscription is confirmed for `{}`: only the account can unsubscribe it.",
            r.account
        )
    } else {
        format!(
            "⚠️ The alerts subscription for `{}` was confirmed before without authentication, so \
             an alert's unsubscribe link still works: subscribe the address again and confirm the \
             new token.",
            r.account
        )
    }
}

/// A failure, in fixed words: never any part of what the daemon or the presser said.
pub(crate) fn failure(e: &CallError) -> &'static str {
    if e.code == error_code::REFUSED {
        NOT_HERE
    } else if e.message.starts_with("that is not a confirmation token") {
        NOT_THIS
    } else {
        AWS_NO
    }
}

fn refused(why: Why) -> &'static str {
    match why {
        Why::NotYours => NOT_YOURS,
        Why::Missing | Why::Stale | Why::Stage => EXPIRED,
    }
}

impl Shared {
    /// **Confirm alerts**: the form opened, and its record begun, for the presser alone.
    pub(super) async fn aws_alerts_press(&self, i: &Interaction) {
        let Some((user, origin)) = self.consent_gate(i, WHAT, NOT_HERE).await else {
            return;
        };
        let id = format!("{:x}", i.id.get());
        let rec = consent::record(
            &self.consents,
            PREFIX,
            &id,
            user,
            TARGET,
            origin,
            false,
            FORM_FOR,
            "",
        );
        self.consents.begin(rec);
        self.respond_modal(i, &modal(&id)).await;
    }

    /// The form's submit: the daemon's confirm, as the presser, for the record's form. The field goes
    /// into the request and nowhere else.
    pub(super) async fn aws_alerts_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        let Some((user, origin)) = self.consent_gate(i, WHAT, NOT_HERE).await else {
            return;
        };
        let Some(id) = consent::submitted(m, PREFIX) else {
            return self.consent_reply(i, WHAT, EXPIRED, vec![]).await;
        };
        if let Err(why) = self.consents.step(PREFIX, &id, Some(user), |r| r.ready()) {
            return self.consent_reply(i, WHAT, refused(why), vec![]).await;
        }
        let Some(token) = consent::pasted(m) else {
            return self.consent_reply(i, WHAT, PASTE_TOO_LONG, vec![]).await;
        };
        self.consents.set(PREFIX, &id, Stage::Pasted);
        if !self.consent_ack(i, WHAT).await {
            return;
        }
        let params = AwsConfirmAlertsParams {
            account: None,
            token,
            author: Some(who.to_string()),
            discord: Some(origin),
        };
        consent::sending(method::AWS_CONFIRM_ALERTS);
        let r = self
            .rpc
            .call::<_, AwsConfirmAlertsResult>(method::AWS_CONFIRM_ALERTS, params)
            .await;
        self.consents.retire(PREFIX, &id);
        let text = match r {
            Ok(done) => done_words(&done),
            Err(e) => failure(&e).to_string(),
        };
        self.consent_finish(i, WHAT, &text, &[]).await;
    }
}

#[cfg(test)]
mod tests;
