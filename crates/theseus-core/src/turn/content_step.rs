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
    /// A private place is the CLI, the web UI, a DM with an owner, and a guild
    /// channel the bindings bind privately: places where the only person who
    /// speaks is an owner. Every other place is shared, and a guest there is
    /// exactly who an injection would come from, so their message is asked like
    /// any other text Theseus did not write. Erring toward `PersonMessage` errs
    /// toward asking, which is the safe direction.
    pub(crate) fn message_kind(&self, session: &str) -> ItemKind {
        match self.view_of(session).class {
            crate::places::PlaceClass::Private => ItemKind::OperatorMessage,
            crate::places::PlaceClass::Shared => ItemKind::PersonMessage,
        }
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
