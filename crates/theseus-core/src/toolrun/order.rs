//! The gate's order (§3.9), written once. `gate` runs it for every call the
//! model makes, and `policy.explain` (step 42a) runs it for a call inside
//! the roots, watching the decision after each layer, so the screen that
//! says why a call waits reads the order the gate keeps and cannot drift
//! from it.
//!
//! The order: the place rule's refusal and the place's ceiling's
//! (`refusal`); then the call's own policy (`sandbox::unbrokered`: L1's, or
//! L0's floor, lists, and posture with its tightening), a granted secret's
//! posture, a language server's start (by a call, L2, or an edit, L3), a
//! shared place's word on a private address, the place's floor, a glide's
//! place rule (38b), T1's hold, and an MCP client's floor (`order`). Each
//! after the first only raises the posture.

use serde_json::Value;
use theseus_protocol::ExternalText;
use theseus_tools::{Plan, Tool};

use super::ToolRuntime;
use crate::ceiling::PlaceView;
use crate::policy::{Decision, Posture};
use crate::sandbox::Bound;

/// What the gate reads of where a call runs, besides the call: its place,
/// its session's hold (read only for a call that acts, so a read costs no
/// record), an MCP client's floor (step 41b), and a glide's places (38b),
/// resolved from its input; none for any other call.
pub(crate) struct At<'a> {
    pub place: PlaceView,
    pub held: &'a dyn Fn() -> Result<Option<ExternalText>, String>,
    pub mcp: &'a dyn Fn() -> Option<Posture>,
    pub glide: Option<&'a crate::glide::Resolved>,
    /// What the call's plan was made with: its session's directory
    /// (theseus-aab7), which a batch's steps are planned in too.
    pub ctx: &'a theseus_tools::ToolCtx,
}

/// A layer of the order after the place's refusal, as `order` hands each
/// decision to its watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    /// The call's own policy: L1's, or L0's floor, lists, and posture.
    Policy,
    /// A granted secret's posture (theseus-dcy).
    Grant,
    /// A language server's first start on a root: by an `lsp.*` call (L2),
    /// or by an edit (L3).
    Lsp,
    /// A shared place's word on a fetch of a private address (theseus-94a6).
    SharedFetch,
    /// The place's floor (step 38a), and an extension's load's (43b).
    Floor,
    /// A glide's place rule, and a post's destination's floor (38b).
    Glide,
    /// T1's hold after external text (theseus-9bp).
    Hold,
    /// An MCP client's floor (step 41b).
    McpClient,
}

impl ToolRuntime {
    /// Why the place refuses a planned call: the place rule's word, then the
    /// ceiling's. None: the order decides it.
    pub(crate) fn refusal(&self, place: PlaceView, tool: &str, plan: &Plan) -> Option<String> {
        crate::places::refusal(place.class, tool, plan, &self.public_roots)
            .or_else(|| place.ceiling.and_then(|c| c.refusal(tool)))
    }

    /// The gate's decision for a planned call the place does not refuse,
    /// with the job's class it binds. `seen` gets the decision after each
    /// layer, in the order.
    pub(crate) fn order(
        &self,
        at: &At<'_>,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
        seen: &mut dyn FnMut(Layer, &Decision),
    ) -> (Decision, Bound) {
        let tightened = self.tightened.get(tool.name());
        let t = tightened.as_ref().map(crate::tighten::as_tightened);
        let (decision, bound) = crate::sandbox::unbrokered(self, tool, plan, input, t);
        seen(Layer::Policy, &decision);
        let decision = self.brokered(tool.name(), plan, input, decision);
        seen(Layer::Grant, &decision);
        // A call that starts a language server is a run too (L2).
        let decision = crate::lsp::gate(self, tool, plan, decision);
        // And an edit that starts one (L3).
        let decision = crate::lsp::edits::gate(self, at.place.class, tool, plan, decision);
        seen(Layer::Lsp, &decision);
        // A private address's card in a shared place says where the page
        // goes (theseus-94a6).
        let decision = crate::places::private_fetch(at.place.class, plan, decision);
        seen(Layer::SharedFetch, &decision);
        // No looser than the place's floor (step 38a), before T1's hold.
        let decision = match at.place.ceiling {
            Some(c) => c.floor(decision, tool.name(), &plan.summary),
            None => decision,
        };
        // An extension's call: no looser than its load's ceiling (43b).
        let decision = self
            .extend
            .floors
            .floor(tool.name(), decision, &plan.summary);
        seen(Layer::Floor, &decision);
        // A glide (38b): out of a private place, or between two shared
        // ones, it asks first; a post is no looser than where it goes.
        let decision = match at.glide {
            Some(g) => g.gate(decision, tool.name(), &plan.summary),
            None => decision,
        };
        seen(Layer::Glide, &decision);
        // After the whole order (theseus-9bp): a call that acts in a
        // session that read external text waits. A read and a one-shot
        // `wake.at` keep their postures (T1b), and cost no record read; a
        // repeating wake is persistence, and is held (37a).
        let class = plan.class.unwrap_or(tool.class());
        let held = if crate::external::exempt(class, tool.name(), input) {
            Ok(None)
        } else {
            (at.held)()
        };
        let decision = crate::external::gate(
            decision,
            class,
            &held,
            self.external_text,
            tool.name(),
            input,
            &plan.summary,
        );
        seen(Layer::Hold, &decision);
        // An MCP client's session (step 41b): its calls that act wait.
        let decision =
            crate::mcp_server::floor(decision, class, (at.mcp)(), tool.name(), &plan.summary);
        seen(Layer::McpClient, &decision);
        (decision, bound)
    }
}
