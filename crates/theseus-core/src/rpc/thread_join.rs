//! `thread.merge`, `thread.unmerge` and `thread.split` (theseus-vt9k, T8).
//! Threads are the owner's, so each is judged as the other thread methods are
//! (`judge_act(Act::Thread)`); the model's `thread.split` tool is the owner's
//! tap at the gate and calls `split_from_session`.
//!
//! - **Merge** is one frame: both threads' records, the record that lets it
//!   be undone (`thread.merge.<from>`), and the `thread.merged` row. Then the
//!   retired thread's session gets a note at its tail and is retired, each a
//!   frame of its own after the merge's, as `thread.done`'s are.
//! - **Unmerge** is one frame the same way, and opens the retired session
//!   again if the merge was what retired it.
//! - **Split** opens the new thread's session first, with its brief and the
//!   arrangement of quoted turns (two frames), then writes the thread with
//!   the session already bound (one frame holding its record, its place and
//!   its `thread.split` row), then a note at the source's tail. Nothing of
//!   the source is written but that note.

use std::cell::{Cell, RefCell};

use anyhow::{anyhow, bail, Result};
use theseus_protocol::method;
use theseus_protocol::threads::{
    ThreadInfo, ThreadMergeParams, ThreadMergeResult, ThreadNameParams, ThreadSplitParams,
    ThreadState,
};
use theseus_store::{kinds, NewRecord};

use super::server::{route, Conn, RpcFailure};
use super::threads::failure;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread::{ThreadMerged, ThreadUnmerged};
use crate::node::{Node, Origin};
use crate::session::SessionRecord;
use crate::thread_join::{
    day, merge_key, merged_card, merged_note, union_set, union_topics, with_line, MergeRecord,
};

/// The author of the notes a join leaves in a session.
const NOTE_AUTHOR: &str = "thread";

impl Core {
    /// The three methods, each as the connection's answerer.
    pub(super) fn rpc_thread_joins(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::THREAD_MERGE => route(params, |p: ThreadMergeParams| {
                self.thread_merge(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_UNMERGE => route(params, |p: ThreadNameParams| {
                self.thread_unmerge(&p.slug, conn.answerer(p.author, p.discord))
            }),
            method::THREAD_SPLIT_PLAN => route(params, |p: ThreadSplitParams| {
                self.thread_split_plan(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_UNSPLIT => route(
                params,
                |p: theseus_protocol::threads::ThreadUnsplitParams| {
                    self.thread_unsplit(&p, conn.answerer(p.author.clone(), p.discord.clone()))
                },
            ),
            _ => route(params, |p: ThreadSplitParams| {
                self.thread_split(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
        }
    }

    /// `thread.merge`: `from` is folded into `into` and retired.
    pub fn thread_merge(
        &self,
        p: &ThreadMergeParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadMergeResult, RpcFailure> {
        let who = who.into();
        self.judge_thread_pair(&who, method::THREAD_MERGE, &p.from, &p.into)?;
        self.threads_on()?;
        let (from, into) = (p.from.trim().to_string(), p.into.trim().to_string());
        if from == into {
            return Err(RpcFailure::invalid(anyhow!(
                "a thread cannot be merged into itself"
            )));
        }
        let now = theseus_protocol::now_unix_ms();
        let (who_s, via) = (who.who(), who.via());
        let was = Cell::new(ThreadState::Active);
        let left_out = RefCell::new(Vec::<String>::new());
        let surfaces = RefCell::new(Vec::<String>::new());
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let a = board.get(&from).ok_or_else(|| no_thread(&from))?;
                    let b = board.get(&into).ok_or_else(|| no_thread(&into))?;
                    if let Some(m) = &a.merged_into {
                        bail!("the thread {from} was merged into {m} already: `theseus thread unmerge {from}` undoes it");
                    }
                    let live = board.live_of(&into).map_or(into.as_str(), |t| t.slug.as_str());
                    if live != into {
                        bail!("the thread {into} was merged into {live}: merge into that one");
                    }
                    if b.state == ThreadState::Done {
                        bail!("the thread {into} is done: `theseus thread reopen {into}` takes it up again");
                    }
                    was.set(a.state);
                    *surfaces.borrow_mut() = a.surfaces.clone();
                    let (topics, out) = union_topics(&b.topics, &a.topics);
                    *left_out.borrow_mut() = out;
                    let next_b = ThreadInfo {
                        card: merged_card(&b.card, a, now),
                        working_set: union_set(&b.working_set, &a.working_set),
                        topics,
                        sealed: b.sealed.clone().or_else(|| a.sealed.clone()),
                        seal_pending: b.seal_pending.clone().or_else(|| a.seal_pending.clone()),
                        ..b.clone()
                    };
                    let next_a = ThreadInfo {
                        state: ThreadState::Done,
                        merged_into: Some(into.clone()),
                        surfaces: vec![],
                        ..a.clone()
                    };
                    Ok(vec![next_b, next_a])
                },
                |board, ts| {
                    let a = board.get(&from).expect("checked");
                    let record = MergeRecord {
                        into: into.clone(),
                        was: was.get(),
                        surfaces: a.surfaces.clone(),
                        retired_session: a.session_id.clone().filter(|s| self.is_open(s)),
                        at_ms: now,
                        undone: false,
                    };
                    let out = left_out.borrow();
                    Ok(vec![
                        NewRecord::json(kinds::META, Some(&merge_key(&from)), &record)?,
                        fact::row(
                            &ThreadMerged {
                                from: &ts[1],
                                into: &ts[0],
                                was: was.get().as_str(),
                                topics_left_out: &out,
                                archived: &a.surfaces,
                                who: &who_s,
                                via: &via,
                            },
                            None,
                            None,
                        )?,
                    ])
                },
            )
            .map_err(RpcFailure::invalid)?;
        let (b, a) = (written[0].clone(), written[1].clone());
        let out = left_out.borrow().clone();
        self.rec(None).announce(&ThreadMerged {
            from: &a,
            into: &b,
            was: was.get().as_str(),
            topics_left_out: &out,
            archived: &surfaces.borrow(),
            who: &who_s,
            via: &via,
        });
        if let Some(sid) = a.session_id.as_deref().filter(|s| self.is_open(s)) {
            self.leave_note_and_retire(sid, &merged_note(&a.slug, &b.slug, now), &who);
        }
        Ok(self.join_result(a, b, out))
    }

    /// `thread.unmerge`: the thread `slug`, merged away, is back as it was
    /// before the merge. The thread it was merged into keeps what it took
    /// (a quoted card, a joined working set, a seal) and gains a card line.
    pub fn thread_unmerge(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadMergeResult, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_UNMERGE, slug)?;
        self.threads_on()?;
        let slug = slug.trim().to_string();
        let kept: Option<MergeRecord> = self
            .store
            .get_meta(&merge_key(&slug))
            .map_err(|e| RpcFailure::invalid(anyhow!("{e:#}")))?;
        let now = theseus_protocol::now_unix_ms();
        let (who_s, via) = (who.who(), who.via());
        let rec = RefCell::new(None::<MergeRecord>);
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let a = board.get(&slug).ok_or_else(|| no_thread(&slug))?;
                    let Some(into) = a.merged_into.clone() else {
                        bail!("the thread {slug} was not merged into another");
                    };
                    let r = kept
                        .clone()
                        .filter(|r| !r.undone && r.into == into)
                        .ok_or_else(|| {
                            anyhow!(
                                "the merge of {slug} into {into} left no record to undo it from"
                            )
                        })?;
                    let b = board.get(&into).ok_or_else(|| no_thread(&into))?;
                    let next_a = ThreadInfo {
                        state: r.was,
                        merged_into: None,
                        persona: None,
                        surfaces: r.surfaces.clone(),
                        ..a.clone()
                    };
                    let next_b = ThreadInfo {
                        card: with_line(
                            &b.card,
                            &format!(
                                "«{slug}» unmerged, {}: the lines quoted above stay.",
                                day(now)
                            ),
                        ),
                        ..b.clone()
                    };
                    *rec.borrow_mut() = Some(r);
                    Ok(vec![next_b, next_a])
                },
                |_, ts| {
                    let mut r = rec.borrow().clone().expect("checked");
                    r.undone = true;
                    Ok(vec![
                        NewRecord::json(kinds::META, Some(&merge_key(&slug)), &r)?,
                        fact::row(
                            &ThreadUnmerged {
                                from: &ts[1],
                                into: &ts[0],
                                who: &who_s,
                                via: &via,
                            },
                            None,
                            None,
                        )?,
                    ])
                },
            )
            .map_err(RpcFailure::invalid)?;
        let (b, a) = (written[0].clone(), written[1].clone());
        self.rec(None).announce(&ThreadUnmerged {
            from: &a,
            into: &b,
            who: &who_s,
            via: &via,
        });
        let retired = rec
            .borrow()
            .as_ref()
            .and_then(|r| r.retired_session.clone());
        if let Some(sid) = retired {
            if let Err(e) = self.reopen_session(&sid, &who) {
                tracing::warn!(thread = %a.slug, error = %format!("{e:#}"),
                    "an unmerged thread's session was not opened again");
            }
        }
        Ok(self.join_result(a, b, vec![]))
    }

    /// A session opened for a split that did not happen: retired, so it is
    /// not left looking like a conversation. Best effort, and said.
    pub(super) fn drop_orphan(&self, sid: &str) {
        let now = theseus_protocol::now_unix_ms();
        let done = self.store.update_session(sid, |r| {
            r.retired = Some(theseus_protocol::sessions::SessionRetired {
                reason: theseus_protocol::sessions::RetiredReason::Empty,
                at_ms: now,
            });
            Ok(vec![])
        });
        if let Err(e) = done {
            tracing::warn!(session = %sid, error = %format!("{e:#}"), "a split's unused session was not retired");
        }
    }

    /// A note at the tail of `sid`, in a frame of its own; refused while a
    /// turn runs there (the tail is that turn's).
    pub(super) fn write_tail_note(&self, sid: &str, text: &str) -> Result<()> {
        let rec = self
            .store
            .get_session::<SessionRecord>(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let Some(exec) = rec.execution_id else {
            return Ok(());
        };
        let node = Node::relayed(sid, None, Origin::Harness, NOTE_AUTHOR, text);
        self.kernel.frame(&[&exec], |k| {
            if k.holds_turn(&exec) {
                bail!("a turn is running in {sid}");
            }
            k.stage(&[node.record()?])?;
            Ok(())
        })?;
        Ok(())
    }

    /// A merged thread's session: the note, then retired. A step that fails
    /// is warned of; the merge stands either way.
    fn leave_note_and_retire(&self, sid: &str, text: &str, who: &Answerer) {
        if let Err(e) = self.write_tail_note(sid, text) {
            tracing::warn!(error = %format!("{e:#}"), "a merged thread's note was not left in its session");
        }
        if let Err(e) = self.retire_session(sid, who) {
            tracing::warn!(error = %format!("{e:#}"), "a merged thread's session was not retired");
        }
    }

    /// Whether `sid` is a session that has not been retired.
    fn is_open(&self, sid: &str) -> bool {
        self.store
            .get_session::<SessionRecord>(sid)
            .ok()
            .flatten()
            .is_some_and(|r| r.retired.is_none())
    }

    fn join_result(
        &self,
        from: ThreadInfo,
        into: ThreadInfo,
        topics_left_out: Vec<String>,
    ) -> ThreadMergeResult {
        let one = |t: ThreadInfo| self.thread_rows(&mut [t]).pop().expect("one row");
        ThreadMergeResult {
            from: one(from),
            into: one(into),
            topics_left_out,
        }
    }

    pub(super) fn judge_thread_pair(
        &self,
        who: &Answerer,
        method: &'static str,
        a: &str,
        b: &str,
    ) -> Result<(), RpcFailure> {
        let what = format!("the threads {a} and {b}");
        self.judge_act(
            who,
            super::confirms::Act::Thread {
                method,
                what: &what,
            },
        )
        .map_err(failure)
    }
}

pub(super) fn no_thread(slug: &str) -> anyhow::Error {
    anyhow!("no thread {slug}: `theseus thread list` names them")
}

/// A slug taken, or the live limit reached.
pub(super) fn refuse_taken(
    board: &crate::threads::Board,
    slug: &str,
    max_live: usize,
) -> Result<(), RpcFailure> {
    if board.get(slug).is_some() {
        return Err(RpcFailure::invalid(anyhow!("the thread {slug} exists")));
    }
    if board.open() >= max_live {
        return Err(RpcFailure::invalid(anyhow!(
            "{max_live} threads are open ([threads] max_live): finish one first (`theseus thread done`)"
        )));
    }
    Ok(())
}
