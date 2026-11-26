//! `alerts.confirm_offer` (issue 59): when the owner asks in words to confirm the alerts
//! subscription, a turn can only OFFER the control. The tool posts one small card to the owner's
//! direct messages with **Confirm alerts**, and does nothing else: it confirms nothing, and says
//! so in its result. Only the owner's own press of the button, in a private place, opens the form
//! whose token goes to the daemon's confirm (`rpc/aws.rs`, `rpc/owner_press.rs`). It is offered
//! only where an AWS account is bound (`toolrun.rs`), but it is not one of AWS's tools and its name
//! is not `aws.`'s: it is no AWS act.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::places::PlaceClass;
use crate::toolrun::TurnCtx;

pub const OFFER: &str = "alerts.confirm_offer";

/// The card's words: fixed.
pub const CARD: &str = "🔐 Confirm the AWS alerts subscription: press **Confirm alerts** here, in \
     your direct messages, and paste the Token=… value from the link in SNS's email (copy the \
     link's address; do not open it), or the whole address. It is confirmed so that only the \
     account can unsubscribe it.";

/// What the tool answers: the offer was posted, and nothing was confirmed.
pub const OFFERED: &str = "Offered the owner a Confirm alerts button in their direct messages. \
     Nothing was confirmed: only the owner's own press of the button, and the token they paste, \
     confirms the subscription.";

/// `alerts.confirm_offer`.
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
        "Offer the owner a Confirm alerts button, when they ask in words to confirm the AWS alerts \
         subscription. Posts a card to the owner's direct messages; it confirms nothing and takes \
         no token: only the owner's own press of the button, and the token they paste into its \
         form, does. Offered in a private place."
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
            summary: "offer the owner a Confirm alerts button".into(),
            ..Default::default()
        })
    }
}

/// Run the offer: the card to the owner's direct messages, or why not.
pub fn run(tc: &TurnCtx<'_>) -> Result<(String, Value), String> {
    if tc.class != PlaceClass::Private {
        return Err("the alerts' confirmation is offered only in a private place".into());
    }
    let (post, frame) = tc
        .outbox
        .stage_to_operator(None, json!({"kind": "aws_alerts", "text": CARD}))
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.store
        .append(&frame)
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.outbox.posted(&post);
    Ok((
        OFFERED.to_string(),
        json!({"offered": "alerts", "confirmed": false}),
    ))
}
