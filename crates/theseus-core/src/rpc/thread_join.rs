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
use serde_json::{json, Value};
use theseus_protocol::threads::{
    place_of, slug_ok, ThreadInfo, ThreadMergeParams, ThreadMergeResult, ThreadNameParams,
    ThreadSplitParams, ThreadSplitResult, ThreadState,
};
use theseus_protocol::{error_code, method, SessionOpenParams};
use theseus_store::{kinds, NewRecord};

use super::server::{route, Conn, RpcFailure};
use super::threads::{failure, line};
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread::{ThreadMerged, ThreadSplit, ThreadUnmerged};
use crate::node::{Node, Origin};
use crate::session::SessionRecord;
use crate::thread_join::{
    day, merge_key, merged_card, merged_note, split_note, union_set, union_topics, with_line,
    MergeRecord,
};
use crate::thread_split::{self, Pick};

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
                    if let Some(m) = &b.merged_into {
                        bail!("the thread {into} was merged into {m}: merge into that one");
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
                            anyhow!("the merge of {slug} into {into} left no record to undo it from")
                        })?;
                    let b = board.get(&into).ok_or_else(|| no_thread(&into))?;
                    let next_a = ThreadInfo {
                        state: r.was,
                        merged_into: None,
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
        let retired = rec.borrow().as_ref().and_then(|r| r.retired_session.clone());
        if let Some(sid) = retired {
            if let Err(e) = self.reopen_session(&sid, &who) {
                tracing::warn!(thread = %a.slug, error = %format!("{e:#}"),
                    "an unmerged thread's session was not opened again");
            }
        }
        Ok(self.join_result(a, b, vec![]))
    }

    /// `thread.split`, from a person: turns of `from` by position.
    pub fn thread_split(
        &self,
        p: &ThreadSplitParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadSplitResult, RpcFailure> {
        let who = who.into();
        self.judge_thread_pair(&who, method::THREAD_SPLIT, &p.from, &p.slug)?;
        let pick = match (&p.range, &p.positions) {
            (Some([a, b]), None) => Pick::Range(*a, *b),
            (None, Some(ps)) if !ps.is_empty() => Pick::Positions(ps.clone()),
            _ => {
                return Err(RpcFailure::invalid(anyhow!(
                    "name the turns to split: a range [first, last] of positions, or a list of \
                     positions (the `@` numbers of `theseus history`)"
                )))
            }
        };
        let ask = SplitAsk {
            from: p.from.trim(),
            slug: &p.slug,
            title: p.title.as_deref(),
            goal: p.goal.as_deref(),
            pick,
            who: &who.who(),
            via: &who.via(),
            note: Note::Node,
        };
        self.split_thread(&ask).map(|(r, _)| r)
    }

    /// `thread.split`, from the model's tool in the session `session`, which
    /// serves the thread to split. Its result text is the note the source
    /// keeps, so nothing else is written there; the owner's tap was the gate.
    pub fn split_from_session(
        &self,
        session: &str,
        slug: &str,
        title: Option<&str>,
        goal: Option<&str>,
        pick: Pick,
    ) -> Result<(String, Value), String> {
        let from = self
            .threads()
            .of_session(session)
            .map(|t| t.slug.clone())
            .ok_or("Not split: this session serves no thread")?;
        let ask = SplitAsk {
            from: &from,
            slug,
            title,
            goal,
            pick,
            who: "model",
            via: thread_split::SPLIT,
            note: Note::Result,
        };
        let (r, text) = self
            .split_thread(&ask)
            .map_err(|e| format!("Not split: {}", e.message))?;
        let meta = json!({"thread": r.thread.thread.slug, "from": from, "quoted": r.quoted,
                          "first_pos": r.first_pos, "last_pos": r.last_pos});
        Ok((text, meta))
    }

    /// Open the new thread with the turns quoted. The note for the source:
    /// written to its tail (a person's call) or returned (the tool's result).
    fn split_thread(&self, ask: &SplitAsk<'_>) -> Result<(ThreadSplitResult, String), RpcFailure> {
        self.threads_on()?;
        let slug = ask.slug.trim();
        if !slug_ok(slug) {
            return Err(RpcFailure::invalid(anyhow!(
                "{slug:?} is not a slug: lower-case letters, digits and hyphens, at most 40, \
                 starting with a letter or digit"
            )));
        }
        let title = match ask.title {
            Some(t) => line(t, "title")?,
            None => slug.to_string(),
        };
        let goal = line(ask.goal.unwrap_or(""), "goal")?;
        let board = self.threads();
        let a = board.get(ask.from).ok_or_else(|| {
            RpcFailure::new(error_code::NOT_FOUND, no_thread(ask.from).to_string())
        })?;
        if let Some(m) = &a.merged_into {
            return Err(RpcFailure::invalid(anyhow!(
                "the thread {} was merged into {m}: split from that one",
                a.slug
            )));
        }
        let source = a.session_id.clone().ok_or_else(|| {
            RpcFailure::invalid(anyhow!("the thread {} has no turns yet: nothing to split", a.slug))
        })?;
        let nodes = self
            .store
            .transcript(&source)
            .map_err(|e| RpcFailure::invalid(anyhow!("{e:#}")))?;
        let chosen = thread_split::choose(&nodes, &ask.pick)
            .map_err(|e| RpcFailure::invalid(anyhow!(e)))?;
        let quoted = chosen.pieces.len() as u32;
        let now = theseus_protocol::now_unix_ms();
        let max_live = self.cfg.threads.max_live as usize;
        let new = ThreadInfo {
            slug: slug.to_string(),
            title: title.clone(),
            goal,
            topics: a.topics.clone(),
            working_set: chosen.working_set.clone(),
            card: vec![format!(
                "split from «{}», {}: {} quoted at the head of this thread.",
                a.slug,
                day(now),
                crate::narrative::count(quoted as u64, "message", "messages")
            )],
            persona: a.persona.clone(),
            state: ThreadState::Active,
            sealed: a.sealed.clone(),
            seal_pending: a.seal_pending.clone(),
            surfaces: vec![],
            parent: None,
            merged_into: None,
            split_from: Some(a.slug.clone()),
            session_id: None,
            last_active_ms: now,
            created_at_ms: now,
        };
        // Cheap refusals before anything is opened; the write checks again
        // under the board's lock.
        refuse_taken(&board, slug, max_live)?;

        // The session, holding what its source holds, with its brief and the
        // quoted turns in place before any message can reach it.
        let rec = self
            .open_session(SessionOpenParams {
                label: Some(place_of(slug)),
                opened_from: Some(source.clone()),
                ..SessionOpenParams::default()
            })
            .map_err(RpcFailure::invalid)?;
        let sid = rec.session_id.clone();
        if let Err(e) = self.write_quoted_turns(&rec, &new, ask.from, &chosen) {
            self.drop_orphan(&sid);
            return Err(RpcFailure::invalid(e));
        }
        let place = place_of(slug);
        let bound = self.outbox.place_record(&place, &sid).map_err(RpcFailure::invalid)?;
        let (from_s, pos) = (ask.from.to_string(), (chosen.first_pos, chosen.last_pos));
        let written = self
            .runner
            .threads
            .write_bound(
                &self.store,
                |board| {
                    refuse_taken(board, slug, max_live).map_err(|e| anyhow!(e.message))?;
                    let a = board.get(&from_s).ok_or_else(|| no_thread(&from_s))?;
                    if a.merged_into.is_some() {
                        bail!("the thread {from_s} was merged away while the split opened");
                    }
                    Ok(vec![new.clone()])
                },
                |_, ts| {
                    Ok(vec![
                        bound,
                        fact::row(
                            &ThreadSplit {
                                thread: &ts[0],
                                from: &from_s,
                                quoted,
                                first_pos: pos.0,
                                last_pos: pos.1,
                                who: ask.who,
                                via: ask.via,
                            },
                            None,
                            None,
                        )?,
                    ])
                },
                Some((slug, &sid)),
            )
            .map_err(|e| {
                self.drop_orphan(&sid);
                RpcFailure::invalid(e)
            })?;
        let mut t = written.into_iter().next().expect("one thread");
        t.session_id = Some(sid.clone());
        if let Err(e) = self.outbox.place_bound(&place, &sid) {
            tracing::warn!(thread = %slug, error = %format!("{e:#}"), "the split thread's place was not bound");
        }
        self.rec(None).announce(&ThreadSplit {
            thread: &t,
            from: ask.from,
            quoted,
            first_pos: pos.0,
            last_pos: pos.1,
            who: ask.who,
            via: ask.via,
        });
        let text = split_note(ask.from, slug, &title, quoted, pos.0, pos.1);
        let noted = match ask.note {
            Note::Result => true,
            Note::Node => self.write_tail_note(&source, &text).is_ok_and(|()| true),
        };
        let thread = self.thread_rows(&mut [t]).pop().expect("one row");
        Ok((
            ThreadSplitResult {
                thread,
                from: ask.from.to_string(),
                quoted,
                first_pos: pos.0,
                last_pos: pos.1,
                noted,
            },
            text,
        ))
    }

    /// The new session's first nodes: the brief, the arrangement of quoted
    /// turns, and an edge from the arrangement to each quoted node. One frame
    /// under the session's execution.
    fn write_quoted_turns(
        &self,
        rec: &SessionRecord,
        new: &ThreadInfo,
        from: &str,
        chosen: &thread_split::Chosen,
    ) -> Result<()> {
        let sid = &rec.session_id;
        let exec = rec
            .execution_id
            .clone()
            .ok_or_else(|| anyhow!("session {sid} has no execution"))?;
        let author = format!("{NOTE_AUTHOR}:{from}");
        let text = thread_split::brief(
            new,
            from,
            chosen.pieces.len() as u32,
            chosen.first_pos,
            chosen.last_pos,
        );
        let brief = Node::relayed(sid, None, Origin::Harness, &author, &text);
        let arrangement = Node::arrangement(sid, &author, chosen.pieces.clone(), false);
        let mut records = vec![brief.record()?, arrangement.record()?];
        for p in &chosen.pieces {
            records.push(
                crate::graph::Edge::new(
                    crate::graph::EdgeKind::DerivedFrom,
                    &arrangement.id,
                    &p.node,
                    crate::graph::VIA_ARRANGEMENT,
                )
                .record()?,
            );
        }
        self.kernel.frame(&[&exec], |k| {
            k.stage(&records)?;
            Ok(())
        })?;
        Ok(())
    }

    /// A session opened for a split that did not happen: retired, so it is
    /// not left looking like a conversation. Best effort, and said.
    fn drop_orphan(&self, sid: &str) {
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

    fn judge_thread_pair(
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

/// How a split names its source and where the source's note goes.
struct SplitAsk<'a> {
    from: &'a str,
    slug: &'a str,
    title: Option<&'a str>,
    goal: Option<&'a str>,
    pick: Pick,
    who: &'a str,
    via: &'a str,
    note: Note,
}

#[derive(Clone, Copy)]
enum Note {
    /// A node at the source's tail.
    Node,
    /// The tool's result is the note.
    Result,
}

fn no_thread(slug: &str) -> anyhow::Error {
    anyhow!("no thread {slug}: `theseus thread list` names them")
}

/// A slug taken, or the live limit reached.
fn refuse_taken(board: &crate::threads::Board, slug: &str, max_live: usize) -> Result<(), RpcFailure> {
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
