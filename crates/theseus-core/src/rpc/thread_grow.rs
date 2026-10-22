//! After a turn that ran in a thread, the thread's working set grows from
//! what the turn touched and what its message named (theseus-98bz.5; the
//! pure part is `crate::thread_grow`). `turn_submit` asks `grow_for` before
//! the turn, which costs a place with no thread one read of the board in
//! memory and nothing else, and `thread_grow` after it, once the reply has
//! streamed: one bounded page of the session's nodes after the position the
//! turn started from (never the transcript), and a frame only when the set
//! changed: the record and its `thread.edited` row (`changed: working_set`,
//! by the harness). `[threads] working_set_grows = false` writes nothing.

use std::path::PathBuf;

use theseus_protocol::threads::{ThreadInfo, ThreadState, WorkingSet};
use theseus_store::pages::ledger_session;
use theseus_store::{kinds, Page, Store as _};

use super::thread_flow::Routing;
use super::threads::change;
use super::Core;
use crate::fact;
use crate::fact::thread::ThreadEdited;
use crate::node::{Body, Node};
use crate::session::SessionRecord;
use crate::thread_grow::{grow, touched_in};

/// The row of a set the harness grew.
fn grown(t: &ThreadInfo) -> ThreadEdited<'_> {
    ThreadEdited(change(t, HARNESS, HARNESS, &["working_set"]))
}

/// The most nodes of the session read after the turn's start.
const NODES: usize = 1000;
/// Who grows a set: the harness, through no surface.
const HARNESS: &str = "harness";

/// A turn whose thread may grow: the thread, the session the turn runs on
/// and its directory, the position the turn started after, and what the
/// message named.
pub(super) struct Grow {
    slug: String,
    session: String,
    dir: PathBuf,
    from: u64,
    named: WorkingSet,
}

impl Core {
    /// The thread a turn about to run in `session` grows, if any: the one
    /// the router placed it in, else the one whose session it is (`ask
    /// --thread`, a Discord thread, a thread's own session). Home grows
    /// nothing, and a board not yet read is not read for it.
    pub(super) fn grow_for(
        &self,
        routing: Option<&Routing>,
        session: &SessionRecord,
        text: &str,
    ) -> Option<Grow> {
        if !self.cfg.threads.working_set_grows {
            return None;
        }
        let board = self.runner.threads.held()?;
        let slug = match routing {
            Some(r) => r.slug.clone()?,
            None => board.of_session(&session.session_id)?.slug.clone(),
        };
        Some(Grow {
            slug,
            session: session.session_id.clone(),
            dir: self.tools.cwd_for(session.dir.as_deref()),
            from: self.store.last_position(),
            named: crate::threads_import::extract::working_set([text]),
        })
    }

    /// After the turn `turn_id`: its calls' paths and its message's entities
    /// join the thread's set, in one frame with its row, when they change it.
    pub(super) fn thread_grow(&self, g: &Grow, turn_id: &str) {
        let calls = match self.turn_calls(&g.session, turn_id, g.from) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(thread = %g.slug, error = %format!("{e:#}"), "a thread's working set was not grown");
                return;
            }
        };
        let touched = touched_in(&crate::thread_split::touched(&calls), &g.dir);
        let written = self.runner.threads.write(
            &self.store,
            |board| {
                let Some(cur) = board.get(&g.slug).filter(|t| t.state != ThreadState::Done) else {
                    return Ok(vec![]);
                };
                let elsewhere: Vec<&WorkingSet> = board
                    .all()
                    .filter(|t| t.slug != cur.slug && t.state == ThreadState::Active)
                    .map(|t| &t.working_set)
                    .collect();
                let set = grow(&cur.working_set, &touched, &g.named, &elsewhere);
                if set == cur.working_set {
                    return Ok(vec![]);
                }
                Ok(vec![ThreadInfo {
                    working_set: set,
                    ..cur.clone()
                }])
            },
            |_, ts| {
                ts.iter()
                    .map(|t| fact::row(&grown(t), Some(&g.session), Some(turn_id)))
                    .collect()
            },
        );
        match written {
            Ok(ts) => {
                for t in &ts {
                    self.announce_thread(&t.slug, &grown(t));
                }
            }
            Err(e) => {
                tracing::warn!(thread = %g.slug, error = %format!("{e:#}"), "a thread's working set was not written");
            }
        }
    }

    /// The inputs of the turn's own tool calls: one page of the session's
    /// nodes after `from`, filtered by the turn.
    fn turn_calls(
        &self,
        session: &str,
        turn_id: &str,
        from: u64,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        let page = Page {
            kind: kinds::NODE,
            tags: vec![ledger_session(session)],
            after: Some(from),
            limit: NODES,
            ..Page::default()
        };
        let got = theseus_store::blocking(|| self.store.inner().page(&page))?;
        let Some(got) = got else {
            anyhow::bail!("the index is still being built");
        };
        let mut calls = Vec::new();
        for r in &got.records {
            let n: Node = r.decode()?;
            if n.turn_id.as_deref() != Some(turn_id) {
                continue;
            }
            if let Body::ToolCall { input, .. } = n.body {
                calls.push(input);
            }
        }
        Ok(calls)
    }
}
