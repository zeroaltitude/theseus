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
//! place rule (38b), and an MCP client's floor (`order`). Each
//! after the first only raises the posture.

use serde_json::Value;
use theseus_tools::{Plan, Tool};

use super::ToolRuntime;
use crate::ceiling::PlaceView;
use crate::policy::{Decision, Posture};
use crate::sandbox::Bound;

/// What the gate reads of where a call runs, besides the call: its place,
/// an MCP client's floor (step 41b), and a glide's places (38b),
/// resolved from its input; none for any other call.
pub(crate) struct At<'a> {
    pub place: PlaceView,
    /// The call's session's content labels (theseus-9ymz0), read only when
    /// the injection freeze is asked about: a call that is a read, and any
    /// call where `[content_based_security] injection` is `notify`, never reads the record.
    pub labels: &'a dyn Fn() -> Option<crate::content::ContentFlags>,
    pub mcp: &'a dyn Fn() -> Option<Posture>,
    pub glide: Option<&'a crate::glide::Resolved>,
    /// The call's session: a read of its own kept outputs reads as one
    /// inside the roots (theseus-v73m). None for `policy.explain`'s probe.
    pub session: Option<&'a str>,
    /// What the call's plan was made with: its session's directory
    /// (theseus-aab7), which a batch's steps are planned in too.
    pub ctx: &'a theseus_tools::ToolCtx,
    /// The grant of the routine whose run this call is in (theseus-52tut):
    /// none for any other session.
    pub routine: Option<&'a std::sync::Arc<crate::routine::grant::Grant>>,
}

/// A layer of the order after the place's refusal, as `order` hands each
/// decision to its watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    /// The call's own policy: L1's, or L0's floor, lists, and posture.
    Policy,
    /// The call's conversation's mode (theseus-lyxy).
    Mode,
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
    /// A routine's grant (theseus-52tut): its named tools run unasked.
    Routine,
    /// An MCP client's floor (step 41b).
    McpClient,
}

impl ToolRuntime {
    /// Why the place refuses a planned call: the place rule's word, then the
    /// ceiling's. None: the order decides it.
    pub(crate) fn refusal(&self, place: PlaceView, tool: &str, plan: &Plan) -> Option<String> {
        // The sealed holdout sets come first, in every place (theseus-pw1q.5).
        self.sealed
            .refusal(plan)
            .or_else(|| match place.draft {
                Some(reach) => reach.refusal(tool, plan),
                None => crate::places::refusal(place.class, tool, plan, &self.public_roots),
            })
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
        // A read of the session's own kept outputs (theseus-v73m) is judged
        // as one inside the roots: its paths are not listed, nor is any
        // other of the state dir's.
        let own;
        let plan = match at.session {
            Some(s) if self.outputs.own_read(s, tool.name(), plan) => {
                own = Plan {
                    resources: vec![],
                    ..plan.clone()
                };
                &own
            }
            _ => plan,
        };
        let tightened = self.tightened.get(tool.name());
        let t = tightened.as_ref().map(crate::tighten::as_tightened);
        // A self task's file tools work in its own tree: the approve list's
        // hold on the state dir does not reach it (theseus-pw1q.21.9).
        let own_tree = at
            .session
            .filter(|_| tool.name().starts_with("fs."))
            .and_then(|session| crate::rsi::own_tree::of(self, session));
        // ... and its reads, and git's, reach the trees root (theseus-qh2tt).
        let open_to = at
            .session
            .and_then(|session| crate::rsi::own_tree::open_to(self, session, tool));
        let (decision, bound) =
            crate::sandbox::unbrokered(self, tool, plan, input, t, open_to.as_deref());
        // A self session's soft hold is a notice that runs: self-improvement
        // fails open (theseus-cudar). The floor and the keel still wait, and
        // every later layer (web text, a place's floor) may still hold it.
        let decision =
            crate::rsi::own_tree::failing_open(self, at.session, tool, plan, bound.l1(), decision);
        seen(Layer::Policy, &decision);
        // The conversation's mode (theseus-lyxy).
        let decision =
            crate::policy::modes::gate(self, at, tool, plan, input, bound.l1(), decision);
        seen(Layer::Mode, &decision);
        let decision = self.brokered(tool.name(), plan, input, decision);
        seen(Layer::Grant, &decision);
        // A call that starts a language server is a run too (L2).
        let decision = crate::lsp::gate(self, tool, plan, decision);
        // And an edit that starts one (L3).
        let decision = crate::lsp::edits::gate(
            self,
            at.place.class,
            tool,
            plan,
            own_tree.as_deref(),
            decision,
        );
        seen(Layer::Lsp, &decision);
        // A private address's card in a shared place says where the page
        // goes (theseus-94a6).
        let decision = crate::places::private_fetch(at.place.class, plan, decision);
        seen(Layer::SharedFetch, &decision);
        // No looser than the place's floor (step 38a).
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
        // A routine's run: the tools its grant names run without asking
        // (theseus-52tut).
        let decision =
            crate::routine::grant::gate(self, at, tool, plan, input, bound.l1(), decision);
        seen(Layer::Routine, &decision);
        let class = plan.class.unwrap_or(tool.class());
        // A sealed thread's read asks the owner first (theseus-f2g0).
        let decision = crate::thread_read::gate(self, tool, input, decision);
        // An MCP client's session (step 41b): its calls that act wait.
        let decision =
            crate::mcp_server::floor(decision, class, (at.mcp)(), tool.name(), &plan.summary);
        seen(Layer::McpClient, &decision);
        (decision, bound)
    }
}
