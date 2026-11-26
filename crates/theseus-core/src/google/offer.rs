//! `google.sign_in_offer` (issue 59): when the owner asks in words to connect Google (or its
//! grant is gone), a turn can only OFFER the control. The tool posts one small card to the owner's
//! direct messages with **Connect Google** and **Revoke Google**, and does nothing else: it begins
//! no consent, makes no address, and says so in its result. Only the owner's own press of a button,
//! in a private place, begins one (`rpc/google.rs`, `rpc/owner_press.rs`). It is among Google's
//! tools, so it is offered only where a Google account is bound.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::places::PlaceClass;
use crate::toolrun::TurnCtx;

pub const OFFER: &str = "google.sign_in_offer";

/// The card's words: fixed.
pub fn card_text(account: &str) -> String {
    format!(
        "🔐 Connect Google for `{account}`: press **Connect Google** here, in your direct messages. \
         You are shown what it asks for and the address to open, then a form for the address \
         your browser ends on. **Revoke Google** ends the grant."
    )
}

/// What the tool answers: the offer was posted, and nothing began.
pub const OFFERED: &str = "Offered the owner a Connect Google button in their direct messages. \
     Nothing has started: no consent began and no address was made. Only the owner's own press \
     of the button begins one.";

/// `google.sign_in_offer`: the account is the bound one's, read when it runs.
pub struct Offer;

/// The tool.
pub fn tool() -> Arc<dyn Tool> {
    Arc::new(Offer)
}

impl Tool for Offer {
    fn name(&self) -> &str {
        OFFER
    }

    fn description(&self) -> &str {
        "Offer the owner a Connect Google button, when they ask in words to connect Google or its \
         grant is gone. Posts a card with Connect and Revoke buttons to the owner's direct \
         messages; it begins nothing, makes no address and never connects: only the owner's own \
         press of the button does. Offered in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, _input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        Ok(Plan {
            summary: "offer the owner a Connect Google button".into(),
            ..Default::default()
        })
    }
}

/// Run the offer: the card to the owner's direct messages, or why not.
pub fn run(account: &str, tc: &TurnCtx<'_>) -> Result<(String, Value), String> {
    if tc.class != PlaceClass::Private {
        return Err("a Google consent is offered only in a private place".into());
    }
    let (post, frame) = tc
        .outbox
        .stage_to_operator(
            None,
            json!({"kind": "google_sign_in", "account": account, "text": card_text(account)}),
        )
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.store
        .append(&frame)
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.outbox.posted(&post);
    Ok((
        OFFERED.to_string(),
        json!({"offered": "google", "began": false}),
    ))
}
