//! Items that enter a session through a turn (theseus-9ymz0): the operator's
//! message, a steer, a task's brief, the reports a turn takes. Each is queued
//! for content judgment once its node is written (`JudgeService::ask_content`)
//! and the turn goes on: nothing waits here. A tool result is queued at its own
//! site (`toolrun/complete.rs`), which every file read, fetch, search and
//! program output reaches.

use super::*;
use crate::judge::content::ContentItem;

impl TurnRunner {
    /// A message node a turn just wrote, in the session `session`.
    pub(crate) fn content_message(&self, session: &str, node: &Node, kind: ItemKind, source: &str) {
        if let Some(item) = ContentItem::of_message(node, kind, source) {
            self.judge.ask_content(session, vec![item]);
        }
    }

    /// Whose words a message in `session` is, for the injection question
    /// (theseus-9ymz0). The owner's own text is never asked it (the owner,
    /// 2026-10-17: "my own DM or channel text should never cause a flag") —
    /// **his own**, and no one else's.
    ///
    /// So both must hold, as `places::owner_in_private` requires for an
    /// approval: the place is private **and** the author is an owner (`owner`,
    /// which `Core::owner_input` judged from who spoke and where, and the
    /// sink carries). A private place is not enough: a guild channel bound
    /// private, and every channel of a trusted guild, hold other people too,
    /// and a guest there is exactly who an injection would come from. Every
    /// other message is asked like any text Theseus did not write. Erring
    /// toward `PersonMessage` errs toward asking, which is the safe direction.
    pub(crate) fn message_kind(&self, session: &str, owner: bool) -> ItemKind {
        match (owner, self.view_of(session).class) {
            (true, crate::places::PlaceClass::Private) => ItemKind::OperatorMessage,
            _ => ItemKind::PersonMessage,
        }
    }

    /// The message node a turn just wrote for its input: judged as the
    /// operator's own only when its author is an owner (`message_kind`).
    pub(super) fn content_input(&self, t: &Turn<'_>, node: &Node) {
        let kind = self.message_kind(t.tc.session_id, t.tc.sink.owner);
        self.content_message(t.tc.session_id, node, kind, "operator message");
    }

    /// The nodes a turn took in together (its reports), as one batch.
    pub(super) fn content_nodes(
        &self,
        session: &str,
        nodes: &[Node],
        kind: ItemKind,
        source: &str,
    ) {
        let items: Vec<ContentItem> = nodes
            .iter()
            .filter_map(|n| ContentItem::of_message(n, kind, source))
            .collect();
        if !items.is_empty() {
            self.judge.ask_content(session, items);
        }
    }

    /// The turn's replies, judged as inputs are (issue 44): **each assistant node the turn said, its own
    /// text under its own id**, never the turn's joined output. A task's turns are not: their output is the
    /// report its parent judges ([`ItemKind::TaskReport`]).
    ///
    /// Called from `finish` **before the turn's session hold is taken**: `ask_content` lands a configured
    /// name's label at once (`land_deterministic`, which takes the session's lock), and the thread that holds
    /// the hold for the turn's last frame must not take it again (the review of round 1). A turn that fails
    /// before `finish` queues none: its written replies read "not judged then".
    ///
    /// **Off unless the owner turns it on** (`[content_based_security] judge_replies`, false by default): a
    /// reply's ask shares the judge's day and the daemon's ceiling with the inputs', whose judgment feeds the
    /// injection label, and no allowance of its own can be reserved for it without changing how an input is
    /// admitted (issue 44, round 3). Off, no reply is offered at all, and each reads "not judged then".
    ///
    /// Only the model's words are items: an assistant node no provider call wrote (no `correlation_id`: the
    /// personal step's fixed held reply) is the harness's own text, and is not asked about.
    pub(super) fn content_replies(&self, t: &Turn<'_>) {
        if t.tc.task.is_some() || !self.cfg.content_based_security.judge_replies {
            return;
        }
        let items: Vec<ContentItem> = (t.said.iter())
            .filter_map(|(_, id)| {
                let (_, node) = t.tc.store.get_node(id).ok().flatten()?;
                let Body::AssistantMessage {
                    blocks,
                    correlation_id: Some(_),
                    ..
                } = &node.body
                else {
                    return None;
                };
                let text = crate::provider::text_of(blocks);
                (!text.trim().is_empty()).then(|| {
                    ContentItem::new(id, ItemKind::Reply, "reply", &text, node.created_at_ms)
                })
            })
            .collect();
        if !items.is_empty() {
            self.judge.ask_content(t.tc.session_id, items);
        }
    }

    /// A task's first turn has no message: its brief is its first item.
    pub(super) fn content_brief(&self, t: &Turn<'_>) {
        let sid = t.tc.session_id;
        if t.tc.task.is_none() || t.tc.guard.turn != 1 {
            return;
        }
        if let Ok(Some(node)) = crate::task::brief(t.tc.store, sid) {
            self.content_message(sid, &node, ItemKind::TaskBrief, "task brief");
        }
    }
}
