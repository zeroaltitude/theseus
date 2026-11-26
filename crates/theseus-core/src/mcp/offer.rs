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

/// One card per target in this window (issue 59), for every card of the pattern (an MCP server's,
/// Google's, the alerts'), whoever stages it: a repeated offer within it stages nothing new and says
/// so. Fixed: no owner setting.
pub const ONE_CARD_PER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn card_key(target: &str) -> String {
    format!("consent.card_at.{target}")
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Whether a card for `target` may be staged now: `Err` with the minutes since the last one, when
/// it is still within `ONE_CARD_PER`.
pub fn card_due(store: &crate::store::Store, target: &str) -> Result<(), u64> {
    let window = u64::try_from(ONE_CARD_PER.as_millis()).unwrap_or(u64::MAX);
    match store.get_meta::<u64>(&card_key(target)).ok().flatten() {
        Some(at) if unix_ms().saturating_sub(at) < window => {
            Err(unix_ms().saturating_sub(at) / 60_000)
        }
        _ => Ok(()),
    }
}

/// A card for `target` was staged now.
pub fn card_staged(store: &crate::store::Store, target: &str) {
    if let Err(e) = store.put_meta(&card_key(target), &unix_ms()) {
        tracing::warn!(target, error = %format!("{e:#}"), "a sign-in card's time was not kept");
    }
}

/// What a repeated offer answers: nothing new was posted.
pub fn already_offered(what: &str, minutes: u64) -> String {
    format!(
        "A card for {what} was posted to the owner's direct messages {minutes} minute(s) ago and \
         is still there, so no new one was posted. Nothing has started."
    )
}

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
         section), when they ask in words to sign in to it. It posts one card, with Sign in and \
         Sign out buttons, to the owner's direct messages, at most once per server in fifteen \
         minutes. It starts no sign-in and changes no server: only the owner's own press of the \
         button does. Offered in a private place."
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

/// Keep that `key`'s change was told.
fn mark(store: &crate::store::Store, key: &str, transition: &str) {
    if let Err(e) = store.put_meta(key, &transition.to_string()) {
        tracing::warn!(key, error = %format!("{e:#}"), "the sign-in card's mark was not kept");
    }
}

/// Whether the change `transition` was told already: by a card staged for it, or by one for
/// `target` still in the owner's direct messages (then it is marked told).
fn told_already(store: &crate::store::Store, key: &str, target: &str, transition: &str) -> bool {
    let marked = store.get_meta::<String>(key).ok().flatten();
    if marked.as_deref() == Some(transition) {
        return true;
    }
    if card_due(store, target).is_err() {
        mark(store, key, transition);
        return true;
    }
    false
}

/// The META key that says for which transition `server`'s card was last staged.
fn offered_key(server: &str) -> String {
    format!("mcp.sign_in_offered.{server}")
}

impl super::McpBoard {
    /// The daemon's own offer (issue 59): when a server that signs in comes to need a sign-in (its
    /// sign-in lost for good, or an attach whose trial was refused for want of one), the same card
    /// the tool stages, to the owner's direct messages, ONCE for `transition`: kept in the store, so
    /// a retry and a restart for the same transition stage none. A server with no `oauth` gets none.
    pub(crate) fn offer_sign_in(&self, server: &str, transition: &str) {
        let signs_in = self
            .server(server)
            .cloned()
            .or_else(|| self.loaded_server(server))
            .is_some_and(|s| s.cfg.oauth.is_some());
        let Some(core) = self.core.get().and_then(std::sync::Weak::upgrade) else {
            return;
        };
        if !signs_in {
            return;
        }
        let key = offered_key(server);
        let target = format!("mcp:{server}");
        if told_already(&core.store, &key, &target, transition) {
            return;
        }
        let staged = core
            .outbox
            .stage_to_operator(
                None,
                json!({"kind": "mcp_sign_in", "server": server, "text": card_text(server)}),
            )
            .and_then(|(post, frame)| {
                core.store.append(&frame)?;
                core.outbox.posted(&post);
                Ok(())
            });
        match staged {
            Ok(()) => {
                card_staged(&core.store, &target);
                mark(&core.store, &key, transition);
            }
            Err(e) => {
                tracing::warn!(server, error = %format!("{e:#}"), "the sign-in card was not posted")
            }
        }
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
    // Configured, or attached in this run: the lookup the sign-in itself uses.
    let signs_in = board
        .server(&i.server)
        .cloned()
        .or_else(|| board.loaded_server(&i.server))
        .map(|s| s.cfg.oauth.is_some())
        .ok_or_else(|| format!("no MCP server named {} is attached", i.server))?;
    if !signs_in {
        return Err(format!(
            "the MCP server {} does not sign in (its settings have no `oauth`), so there is \
             nothing to sign in to",
            i.server
        ));
    }
    let target = format!("mcp:{}", i.server);
    if let Err(minutes) = card_due(tc.store, &target) {
        return Ok((
            already_offered(&format!("the MCP server `{}`", i.server), minutes),
            json!({"offered": i.server, "began": false, "posted": false}),
        ));
    }
    let (post, frame) = tc
        .outbox
        .stage_to_operator(
            None,
            json!({"kind": "mcp_sign_in", "server": i.server, "text": card_text(&i.server)}),
        )
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.store
        .append(&frame)
        .map_err(|e| format!("the offer was not posted: {e:#}"))?;
    tc.outbox.posted(&post);
    card_staged(tc.store, &target);
    Ok((
        offered(&i.server),
        json!({"offered": i.server, "began": false, "posted": true}),
    ))
}
