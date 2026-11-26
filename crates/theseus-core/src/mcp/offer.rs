//! `mcp.sign_in_offer` (issue 59): when the owner asks in words to sign in to
//! an MCP server, a turn can only OFFER the control. The tool posts one small
//! card to the owner's direct messages, with **Sign in** and **Sign out**
//! buttons for the named server, and does nothing else: it begins no sign-in,
//! makes no address, and says so in its result. Only the owner's own press of
//! a button, in a private place, begins one (`rpc/mcp_login.rs`). It names a
//! server that signs in (an `oauth` section); any other is refused.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::places::PlaceClass;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const OFFER: &str = "mcp.sign_in_offer";

/// What a press from Discord is told when it is not the owner's own, in a
/// private place (`rpc/mcp_login.rs`): fixed words, with the place rule's own
/// reason after them.
pub const NOT_YOURS: &str = "a sign-in from Discord is the owner's own press, in a private place \
                             (a direct message with Theseus, or a channel bound private)";

/// The outbox kind of the card, to the owner's direct messages.
pub const KIND: &str = "mcp_sign_in";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    server: String,
}

/// The card's words for `server`: fixed, with its name.
pub fn card_text(server: &str) -> String {
    format!(
        "🔐 Sign in to the MCP server `{server}`: press **Sign in** here, in your direct messages. \
         You are shown where you sign in, then the address to open, then a form for the address \
         your browser ends on. **Sign out** forgets its sign-in."
    )
}

/// What the tool answers: the offer was posted, and nothing began.
pub fn offered(server: &str) -> String {
    format!(
        "Offered the owner a Sign in button for the MCP server `{server}` in their direct \
         messages. Nothing has started: no sign-in began and no address was made. Only the \
         owner's own press of the button begins one."
    )
}

/// `mcp.sign_in_offer`.
pub struct Offer;

impl Tool for Offer {
    fn name(&self) -> &str {
        OFFER
    }

    fn description(&self) -> &str {
        "Offer the owner a Sign in button for an MCP server that signs in (one with an `oauth` \
         section), when they ask in words to sign in to it. Posts a card with Sign in and Sign out \
         buttons to the owner's direct messages; it begins nothing, makes no address and never \
         signs in: only the owner's own press of the button does. Offered in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {"type": "string", "description": "The MCP server's name, as attached."}
            },
            "required": ["server"],
            "additionalProperties": false
        })
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

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: Input = parse(input)?;
        Ok(Plan {
            summary: format!(
                "offer the owner a Sign in button for MCP server {}",
                i.server
            ),
            ..Default::default()
        })
    }
}

/// Run the offer: the card to the owner's direct messages, or why not.
pub fn run(rt: &ToolRuntime, tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    if tc.class != PlaceClass::Private {
        return Err("a sign-in is offered only in a private place".into());
    }
    let i: Input = parse(input)?;
    let board = rt
        .extend
        .board()
        .ok_or("the MCP board is not running, so no sign-in can be offered")?;
    let signs_in = board
        .server(&i.server)
        .map(|s| s.cfg.oauth.is_some())
        .ok_or_else(|| format!("no MCP server named {} is attached", i.server))?;
    if !signs_in {
        return Err(format!(
            "the MCP server {} does not sign in (its settings have no `oauth`), so there is \
             nothing to sign in to",
            i.server
        ));
    }
    let body = json!({"kind": "mcp_sign_in", "server": i.server, "text": card_text(&i.server)});
    let (post, frame) = tc
        .outbox
        .stage_to_operator(None, body)
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.store
        .append(&frame)
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.outbox.posted(&post);
    Ok((
        offered(&i.server),
        json!({"offered": i.server, "began": false}),
    ))
}
