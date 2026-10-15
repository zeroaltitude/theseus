//! `/why` (theseus-ighf): what Theseus was shown for this place's last turn,
//! and why each piece was there. The words are `theseus_protocol::why`'s,
//! the same function `theseus why` prints, in its short form: each section's
//! top items and a count of the rest, in one message that fits Discord's
//! limit, never split. A read of the core (`context.explain`,
//! `policy.explain`, `node.list`, `ledger.tail`): it writes no frame and no
//! ledger row, and adds nothing to a turn.
//!
//! - **Ephemeral**: the deferred acknowledgement carries the flag, so only
//!   the presser sees the answer.
//! - **A shared place hides its sources**: a recalled note from an imported
//!   or another session is only said to have been there, and its words and
//!   labels are left out (`WhyInput::hide_sources`, forced by the place's
//!   class). A private place shows them.

use serde_json::Value;
use theseus_protocol::context::{ContextExplainParams, ContextExplainResult};
use theseus_protocol::why::{tools_called, turn_rows, why_short, WhyInput};
use theseus_protocol::{
    method, LedgerTailParams, LedgerTailResult, NodeListParams, NodeListResult,
    PolicyExplainParams, PolicyExplainResult,
};
use tokio::sync::{mpsc, oneshot};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::CommandBuilder;

use super::{Control, Place, PlaceMsg, Shared};

/// Discord's limit on a message's text.
const LIMIT: usize = 2000;
/// The code fence around the text, so nothing in it renders as markdown or
/// mentions anyone.
const FENCE: &str = "```";
/// The items each section keeps in the short form.
const TOP: usize = 3;
/// The most rows read for a turn's tool calls and route decision.
const ROWS: usize = 1000;

/// `/why`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "why",
        "What Theseus was shown for its last turn here, and why each piece was there (only you see it)",
        CommandType::ChatInput,
    )
    .build()
}

/// The answer's text: the short form in a code fence, within `LIMIT`.
pub(super) fn fenced(
    x: &ContextExplainResult,
    p: Option<&PolicyExplainResult>,
    route: Option<&Value>,
    tools: &[theseus_protocol::why::ToolCalled],
) -> String {
    let body = why_short(
        &WhyInput {
            explain: x,
            policy: p,
            route,
            tools,
            hide_sources: x.class != "private",
        },
        TOP,
        LIMIT - 2 * FENCE.len() - 2,
    );
    format!("{FENCE}\n{}\n{FENCE}", body.replace(FENCE, "'''"))
}

impl Shared {
    /// An interaction of `/why`: true when it was one, and answered.
    pub(super) async fn why_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
    ) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        if c.name == "digest" {
            return self.digest_interaction(i, tx, who).await;
        }
        if c.name != "why" {
            return false;
        }
        // Acknowledged at once (Discord allows three seconds), ephemeral: the
        // flag on the deferral is what the later edit keeps.
        let deferred = InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(InteractionResponseData {
                flags: Some(MessageFlags::EPHEMERAL),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            }),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &deferred)
            .await
        {
            self.board.error("why response", None, e);
        }
        let (rtx, rrx) = oneshot::channel();
        let _ = tx.send(PlaceMsg::Control {
            cmd: Control::Why,
            by: who.to_string(),
            reply: rtx,
        });
        let text = rrx
            .await
            .unwrap_or_else(|_| "The place did not answer.".into());
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&text))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error("why reply", None, e);
        }
        true
    }

    /// The `why` of `session_id`'s last turn.
    pub(super) async fn why_text(&self, session_id: &str) -> Result<String, String> {
        let sid = session_id.to_string();
        let x: ContextExplainResult = self
            .rpc
            .call(
                method::CONTEXT_EXPLAIN,
                ContextExplainParams {
                    session_id: sid.clone(),
                    turn_id: None,
                },
            )
            .await
            .map_err(|e| e.message)?;
        let p: Option<PolicyExplainResult> = self
            .rpc
            .call(
                method::POLICY_EXPLAIN,
                PolicyExplainParams {
                    session_id: Some(sid.clone()),
                    tool: None,
                },
            )
            .await
            .ok();
        let turn = x.turn_id.clone().unwrap_or_default();
        let route: Option<LedgerTailResult> = self
            .rpc
            .call(
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(ROWS),
                    kind: Some("route.decided".into()),
                    session_id: Some(sid.clone()),
                    ..LedgerTailParams::default()
                },
            )
            .await
            .ok();
        let nodes: Option<NodeListResult> = self
            .rpc
            .call(
                method::NODE_LIST,
                NodeListParams {
                    session_id: Some(sid),
                    kind: Some("tool_call".into()),
                    n: Some(ROWS),
                },
            )
            .await
            .ok();
        let route = route.and_then(|r| turn_rows(&r, &turn).pop());
        let tools = nodes.map(|n| tools_called(&n, &turn)).unwrap_or_default();
        Ok(fenced(&x, p.as_ref(), route.as_ref(), &tools))
    }
}

impl Place {
    /// `/why`'s text, for this place's session.
    pub(super) async fn why(&self) -> String {
        match self.shared.why_text(&self.session_id).await {
            Ok(t) => t,
            Err(e) => format!("⚠️ {e}"),
        }
    }
}
