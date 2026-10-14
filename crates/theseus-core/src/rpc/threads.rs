//! `thread.new`, `.list`, `.show`, `.edit`, `.pause`, `.resume`, `.done` and
//! `.reopen` (theseus-nhew), and the thread a `turn.submit` names. A thread is
//! private always, so every one is the owner's, from a private place, judged
//! as the standing records' writes are (`judge_act(Act::Thread)`), and the CLI
//! refuses the writes inside a job. A write is one frame: the thread's record
//! and its ledger row. `thread.done` also leaves the card to memory as a note
//! in the thread's session and retires that session, each a frame of its own
//! after the thread's.

use std::cell::RefCell;
use std::collections::BTreeSet;

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::threads::{
    place_of, slug_ok, ThreadEditParams, ThreadInfo, ThreadListParams, ThreadListResult,
    ThreadNameParams, ThreadNewParams, ThreadRow, ThreadState, WorkingSet, MAX_CARD,
    MAX_LINE_CHARS, MAX_TOPICS,
};
use theseus_protocol::{error_code, method, SessionOpenParams};
use theseus_store::{kinds, NewRecord, Page};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::fact::thread::{
    Change, ThreadCreated, ThreadDone, ThreadEdited, ThreadPaused, ThreadReopened, ThreadResumed,
};
use crate::ledger::LedgerRow;
use crate::node::{Node, Origin};
use crate::session::SessionRecord;
use crate::threads::Board;

/// The most entries one list of a working set holds: it rides prompts later.
const MAX_SET: usize = 64;
/// The longest persona name.
const MAX_PERSONA: usize = 80;
/// The author of the note `thread.done` leaves in a session.
const NOTE_AUTHOR: &str = "thread";

/// The five moves between states.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Move {
    Pause,
    Resume,
    Done,
    Reopen,
}

impl Move {
    fn method(self) -> &'static str {
        match self {
            Move::Pause => method::THREAD_PAUSE,
            Move::Resume => method::THREAD_RESUME,
            Move::Done => method::THREAD_DONE,
            Move::Reopen => method::THREAD_REOPEN,
        }
    }

    fn to(self) -> ThreadState {
        match self {
            Move::Pause => ThreadState::Paused,
            Move::Resume | Move::Reopen => ThreadState::Active,
            Move::Done => ThreadState::Done,
        }
    }

    /// The states the move starts from; the one it ends in is a no-op.
    fn from(self) -> &'static [ThreadState] {
        match self {
            Move::Pause => &[ThreadState::Active],
            Move::Resume => &[ThreadState::Paused],
            Move::Done => &[ThreadState::Active, ThreadState::Paused],
            Move::Reopen => &[ThreadState::Done],
        }
    }

    fn row(self, c: Change<'_>) -> Result<NewRecord> {
        match self {
            Move::Pause => fact::row(&ThreadPaused(c), None, None),
            Move::Resume => fact::row(&ThreadResumed(c), None, None),
            Move::Done => fact::row(&ThreadDone(c), None, None),
            Move::Reopen => fact::row(&ThreadReopened(c), None, None),
        }
    }
}

impl Core {
    /// The eight methods, each as the connection's answerer.
    pub(super) fn rpc_threads(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::THREAD_NEW => route(params, |p: ThreadNewParams| {
                self.thread_new(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_LIST => route(params, |p: ThreadListParams| {
                self.thread_list(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_SHOW => route(params, |p: ThreadNameParams| {
                self.thread_show(&p.slug, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_EDIT => route(params, |p: ThreadEditParams| {
                self.thread_edit(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::THREAD_PAUSE => route(params, |p: ThreadNameParams| {
                self.thread_pause(&p.slug, conn.answerer(p.author, p.discord))
            }),
            method::THREAD_RESUME => route(params, |p: ThreadNameParams| {
                self.thread_resume(&p.slug, conn.answerer(p.author, p.discord))
            }),
            method::THREAD_DONE => route(params, |p: ThreadNameParams| {
                self.thread_done(&p.slug, conn.answerer(p.author, p.discord))
            }),
            method::THREAD_SEAL | method::THREAD_UNSEAL | method::THREAD_SEAL_DISMISS => {
                super::thread_seal::route_seal(self, name, params, conn)
            }
            _ => route(params, |p: ThreadNameParams| {
                self.thread_reopen(&p.slug, conn.answerer(p.author, p.discord))
            }),
        }
    }

    /// The threads in memory (`Board`): built by two META scans the first
    /// time, after serving (`warm_threads`) or at the first reader. Never a
    /// store read on a turn's path once built. A store that cannot be read
    /// gives an empty board, warned of, and is tried again.
    pub fn threads(&self) -> std::sync::Arc<Board> {
        self.runner.threads.all(&self.store).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "the threads cannot be read");
            Default::default()
        })
    }

    /// Read the threads now, off the start path: after serving, on the
    /// blocking pool. A reader that comes sooner reads them itself, once.
    pub fn warm_threads(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.runner.threads.all(&core.store) {
                tracing::warn!(error = %format!("{e:#}"), "the threads cannot be read");
            }
        });
    }

    /// Judge an act on the threads: the owner, from a private place.
    pub(super) fn judge_thread(
        &self,
        who: &Answerer,
        method: &'static str,
        slug: &str,
    ) -> Result<(), RpcFailure> {
        let what = format!("the thread {slug}");
        self.judge_act(
            who,
            Act::Thread {
                method,
                what: &what,
            },
        )
        .map_err(failure)
    }

    pub(super) fn threads_on(&self) -> Result<(), RpcFailure> {
        match self.cfg.threads.enabled {
            true => Ok(()),
            false => Err(RpcFailure::new(
                error_code::REFUSED,
                "threads are off here ([threads] enabled = false)",
            )),
        }
    }

    /// `thread.new`.
    pub fn thread_new(
        &self,
        p: &ThreadNewParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_NEW, &p.slug)?;
        self.threads_on()?;
        let slug = p.slug.trim().to_string();
        if !slug_ok(&slug) {
            return Err(RpcFailure::invalid(anyhow!(
                "{slug:?} is not a slug: lower-case letters, digits and hyphens, at most 40, \
                 starting with a letter or digit"
            )));
        }
        let title = match &p.title {
            Some(t) => line(t, "title")?,
            None => slug.clone(),
        };
        let goal = line(p.goal.as_deref().unwrap_or(""), "goal")?;
        let topics = topics_of(p.topics.as_deref().unwrap_or(&[]))?;
        let now = theseus_protocol::now_unix_ms();
        let max_live = self.cfg.threads.max_live as usize;
        let (who_s, via) = (who.who(), who.via());
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    if let Some(t) = board.get(&slug) {
                        bail!(
                            "the thread {slug} exists ({}){}",
                            t.state.as_str(),
                            if t.state == ThreadState::Done {
                                ": `theseus thread reopen` takes it up again"
                            } else {
                                ""
                            }
                        );
                    }
                    if board.open() >= max_live {
                        bail!(
                            "{max_live} threads are open ([threads] max_live): finish one first \
                             (`theseus thread done`)"
                        );
                    }
                    let (split_from, sealed) =
                        super::thread_seal::starting_seal(board, p, &who_s, now)?;
                    Ok(vec![ThreadInfo {
                        slug: slug.clone(),
                        title: title.clone(),
                        goal: goal.clone(),
                        topics: topics.clone(),
                        working_set: WorkingSet::default(),
                        card: vec![],
                        persona: None,
                        state: ThreadState::Active,
                        sealed,
                        seal_pending: None,
                        surfaces: vec![],
                        parent: None,
                        merged_into: None,
                        split_from,
                        session_id: None,
                        last_active_ms: now,
                        created_at_ms: now,
                    }])
                },
                |_, ts| super::thread_seal::created_rows(ts, &who_s, &via),
            )
            .map_err(RpcFailure::invalid)?;
        let t = written.into_iter().next().expect("one thread");
        self.rec(None)
            .announce(&ThreadCreated(change(&t, &who_s, &via, &[])));
        Ok(ThreadRow {
            thread: t,
            context_tokens: None,
            today_usd: 0.0,
        })
    }

    /// `thread.list`: the active and paused threads (or those in `state`),
    /// newest activity first.
    pub fn thread_list(
        &self,
        p: &ThreadListParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadListResult, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_LIST, "list")?;
        let board = self.threads();
        let mut shown: Vec<ThreadInfo> = board
            .all()
            .filter(|t| match p.state {
                Some(s) => t.state == s,
                None => t.state != ThreadState::Done,
            })
            .cloned()
            .collect();
        let rows = self.thread_rows(&mut shown);
        Ok(ThreadListResult {
            threads: rows,
            total: board.len() as u32,
        })
    }

    /// `thread.show`.
    pub fn thread_show(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_SHOW, slug)?;
        let t = self.thread_named(slug)?;
        Ok(self
            .thread_rows(&mut [t])
            .pop()
            .expect("one row for one thread"))
    }

    pub(super) fn thread_named(&self, slug: &str) -> Result<ThreadInfo, RpcFailure> {
        self.threads().get(slug.trim()).cloned().ok_or_else(|| {
            RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no thread {slug}: `theseus thread list` names them"),
            )
        })
    }

    /// The threads with what their sessions hold: the last activity, the
    /// context's tokens, and today's dollars. Reads the sessions, the
    /// compilations and today's spend rows: a read for a person, never a
    /// turn's path.
    pub(super) fn thread_rows(&self, threads: &mut [ThreadInfo]) -> Vec<ThreadRow> {
        let sessions: BTreeSet<String> = threads
            .iter()
            .filter_map(|t| t.session_id.clone())
            .collect();
        let today = self.spent_today_by(&sessions);
        let mut rows: Vec<ThreadRow> = threads
            .iter_mut()
            .map(|t| {
                let rec = t
                    .session_id
                    .as_deref()
                    .and_then(|s| self.store.get_session::<SessionRecord>(s).ok().flatten());
                let tokens = rec
                    .as_ref()
                    .and_then(|r| r.compilation_id.as_deref())
                    .and_then(|c| self.store.get_compilation(c).ok().flatten())
                    .and_then(|c| c.budget)
                    .map(|b| b.used_tokens);
                if let Some(r) = &rec {
                    t.last_active_ms = t.last_active_ms.max(r.last_active_ms);
                }
                ThreadRow {
                    today_usd: t
                        .session_id
                        .as_ref()
                        .and_then(|s| today.get(s))
                        .copied()
                        .unwrap_or(0.0),
                    thread: t.clone(),
                    context_tokens: tokens,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            (b.thread.last_active_ms, &a.thread.slug)
                .cmp(&(a.thread.last_active_ms, &b.thread.slug))
        });
        rows
    }

    /// What `sessions` spent since local midnight, by session, from the
    /// ledger's spend rows: nothing while the index's shape is built.
    fn spent_today_by(
        &self,
        sessions: &BTreeSet<String>,
    ) -> std::collections::BTreeMap<String, f64> {
        let mut out = std::collections::BTreeMap::new();
        if sessions.is_empty() {
            return out;
        }
        let since = crate::learning::local_midnight(theseus_protocol::now_unix_ms());
        let tags: Vec<String> = crate::day_ceiling::SPEND_KINDS
            .iter()
            .map(|k| theseus_store::pages::ledger_kind(k))
            .collect();
        let mut before = None;
        loop {
            let page = Page {
                kind: kinds::LEDGER,
                tags: tags.clone(),
                after: None,
                before,
                since_ms: Some(since),
                until_ms: None,
                limit: 512,
            };
            let Ok(Some(got)) = self.store.ledger_page(&page) else {
                return out;
            };
            for r in &got.records {
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                let Some(s) = row.session_id.as_deref().filter(|s| sessions.contains(*s)) else {
                    continue;
                };
                if row.at_unix_ms < since {
                    continue;
                }
                if let Some(m) = crate::day_ceiling::spend_of(&row) {
                    *out.entry(s.to_string()).or_insert(0.0) += m as f64 / 1_000_000.0;
                }
            }
            match (got.more, got.first) {
                (true, Some(first)) => before = Some(first),
                _ => return out,
            }
        }
    }

    /// `thread.edit`: each field given replaces what the thread held.
    pub fn thread_edit(
        &self,
        p: &ThreadEditParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_EDIT, &p.slug)?;
        let title = match &p.title {
            Some(t) => Some(line(t, "title")?).filter(|t| !t.is_empty()),
            None => None,
        };
        if p.title.is_some() && title.is_none() {
            return Err(RpcFailure::invalid(anyhow!(
                "a thread's title cannot be empty"
            )));
        }
        let goal = p.goal.as_deref().map(|g| line(g, "goal")).transpose()?;
        let topics = p.topics.as_deref().map(topics_of).transpose()?;
        let working_set = p.working_set.as_ref().map(working_set_of).transpose()?;
        let card = p.card.as_deref().map(card_of).transpose()?;
        let persona = match p.persona.as_deref().map(str::trim) {
            Some(x) if x.chars().count() > MAX_PERSONA => {
                return Err(RpcFailure::invalid(anyhow!(
                    "a persona's name is at most {MAX_PERSONA} characters"
                )))
            }
            Some("") => Some(None),
            Some(x) => Some(Some(x.to_string())),
            None => None,
        };
        let (who_s, via) = (who.who(), who.via());
        let changed = RefCell::new(Vec::<&'static str>::new());
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let cur = board.get(p.slug.trim()).ok_or_else(|| {
                        anyhow!("no thread {}: `theseus thread list` names them", p.slug)
                    })?;
                    if cur.state == ThreadState::Done {
                        bail!(
                            "the thread {} is done: `theseus thread reopen` takes it up again",
                            cur.slug
                        );
                    }
                    let mut next = cur.clone();
                    let mut ch = Vec::new();
                    let mut set = |name: &'static str, differs: bool, apply: &mut dyn FnMut()| {
                        if differs {
                            apply();
                            ch.push(name);
                        }
                    };
                    if let Some(t) = &title {
                        set("title", *t != next.title, &mut || next.title = t.clone());
                    }
                    if let Some(g) = &goal {
                        set("goal", *g != next.goal, &mut || next.goal = g.clone());
                    }
                    if let Some(t) = &topics {
                        set("topics", *t != next.topics, &mut || next.topics = t.clone());
                    }
                    if let Some(w) = &working_set {
                        set("working_set", *w != next.working_set, &mut || {
                            next.working_set = w.clone();
                        });
                    }
                    if let Some(c) = &card {
                        set("card", *c != next.card, &mut || next.card = c.clone());
                    }
                    if let Some(x) = &persona {
                        set("persona", *x != next.persona, &mut || {
                            next.persona = x.clone()
                        });
                    }
                    if ch.is_empty() {
                        return Ok(vec![]);
                    }
                    *changed.borrow_mut() = ch;
                    Ok(vec![next])
                },
                |_, ts| {
                    ts.iter()
                        .map(|t| {
                            let ch = changed.borrow();
                            fact::row(&ThreadEdited(change(t, &who_s, &via, &ch)), None, None)
                        })
                        .collect()
                },
            )
            .map_err(RpcFailure::invalid)?;
        let Some(t) = written.into_iter().next() else {
            // Nothing differed: the thread as it is, and no frame.
            let t = self.thread_named(&p.slug)?;
            return Ok(self.thread_rows(&mut [t]).pop().expect("one row"));
        };
        self.rec(None)
            .announce(&ThreadEdited(change(&t, &who_s, &via, &changed.borrow())));
        Ok(self.thread_rows(&mut [t]).pop().expect("one row"))
    }

    /// `thread.pause`: an active thread takes no message until resumed.
    pub fn thread_pause(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        self.thread_move(Move::Pause, slug, who)
    }

    /// `thread.resume`.
    pub fn thread_resume(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        self.thread_move(Move::Resume, slug, who)
    }

    /// `thread.done`: the thread ends, its session retires and its card goes
    /// to memory.
    pub fn thread_done(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        self.thread_move(Move::Done, slug, who)
    }

    /// `thread.reopen`.
    pub fn thread_reopen(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        self.thread_move(Move::Reopen, slug, who)
    }

    /// A move between states. A move to the state the thread is in writes
    /// nothing.
    fn thread_move(
        &self,
        mv: Move,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, mv.method(), slug)?;
        if mv == Move::Reopen {
            self.threads_on()?;
        }
        let slug = slug.trim().to_string();
        let (who_s, via) = (who.who(), who.via());
        let max_live = self.cfg.threads.max_live as usize;
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let cur = board.get(&slug).ok_or_else(|| {
                        anyhow!("no thread {slug}: `theseus thread list` names them")
                    })?;
                    if cur.state == mv.to() {
                        return Ok(vec![]);
                    }
                    if !mv.from().contains(&cur.state) {
                        bail!(
                            "the thread {slug} is {}: it cannot be {} from there",
                            cur.state.as_str(),
                            mv.to().as_str()
                        );
                    }
                    if mv == Move::Reopen && board.open() >= max_live {
                        bail!("{max_live} threads are open ([threads] max_live): finish one first");
                    }
                    let mut next = cur.clone();
                    next.state = mv.to();
                    Ok(vec![next])
                },
                |_, ts| {
                    ts.iter()
                        .map(|t| mv.row(change(t, &who_s, &via, &[])))
                        .collect()
                },
            )
            .map_err(RpcFailure::invalid)?;
        let Some(t) = written.into_iter().next() else {
            let t = self.thread_named(&slug)?;
            return Ok(self.thread_rows(&mut [t]).pop().expect("one row"));
        };
        let c = change(&t, &who_s, &via, &[]);
        let rec = self.rec(None);
        match mv {
            Move::Pause => rec.announce(&ThreadPaused(c)),
            Move::Resume => rec.announce(&ThreadResumed(c)),
            Move::Done => rec.announce(&ThreadDone(c)),
            Move::Reopen => rec.announce(&ThreadReopened(c)),
        }
        if mv == Move::Done {
            self.finish_thread_session(&t, &who);
        }
        Ok(self.thread_rows(&mut [t]).pop().expect("one row"))
    }

    /// A finished thread's session: its card is left in it as a note, which
    /// the memory pass and the index read as they read any node, and it is
    /// retired. The thread is done already; a step that fails is warned of
    /// (the card is in the `thread.done` row either way).
    fn finish_thread_session(&self, t: &ThreadInfo, who: &Answerer) {
        let Some(sid) = t.session_id.as_deref() else {
            return;
        };
        if let Err(e) = self.write_card_note(sid, t) {
            tracing::warn!(thread = %t.slug, error = %format!("{e:#}"),
                "a finished thread's card was not left in its session");
        }
        if let Err(e) = self.retire_session(sid, who) {
            tracing::warn!(thread = %t.slug, error = %format!("{e:#}"),
                "a finished thread's session was not retired");
        }
    }

    /// The card as a harness note in the session, in a frame of its own.
    fn write_card_note(&self, sid: &str, t: &ThreadInfo) -> Result<()> {
        let rec = self
            .store
            .get_session::<SessionRecord>(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let Some(exec) = rec.execution_id else {
            return Ok(());
        };
        let node = Node::relayed(sid, None, Origin::Harness, NOTE_AUTHOR, &card_note(t));
        self.kernel.frame(&[&exec], |k| {
            if k.holds_turn(&exec) {
                bail!("a turn is running in {sid}");
            }
            k.stage(&[node.record()?])?;
            Ok(())
        })?;
        Ok(())
    }

    /// The session a thread's next turn runs in (`turn.submit { thread }`):
    /// the place's current one, or, on first use and when it was retired, a
    /// new one opened and bound to the virtual place (`bind_place_to`, which
    /// supersedes the one it replaces). Only for the owner, from a private
    /// place, and only while the thread is active.
    pub(super) fn thread_session(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<SessionRecord, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::TURN_SUBMIT, slug)?;
        self.threads_on()?;
        let slug = slug.trim();
        let t = self.thread_named(slug)?;
        if t.state != ThreadState::Active {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "the thread {slug} is {}: {}",
                    t.state.as_str(),
                    match t.state {
                        ThreadState::Paused => "`theseus thread resume` lets it take messages",
                        _ => "`theseus thread reopen` takes it up again",
                    }
                ),
            ));
        }
        self.runner
            .threads
            .bind(&self.store, slug, |board| {
                let cur = board.get(slug).and_then(|t| t.session_id.clone());
                if let Some(rec) = cur
                    .as_deref()
                    .and_then(|s| self.store.get_session::<SessionRecord>(s).ok().flatten())
                    .filter(|r| r.retired.is_none())
                {
                    return Ok((rec, None));
                }
                let rec = self.open_session(SessionOpenParams {
                    label: Some(place_of(slug)),
                    ..SessionOpenParams::default()
                })?;
                self.bind_place_to(&place_of(slug), &rec.session_id)?;
                let bound = rec.session_id.clone();
                Ok((rec, Some(bound)))
            })
            .map_err(RpcFailure::invalid)
    }
}

pub(super) fn change<'a>(
    thread: &'a ThreadInfo,
    who: &'a str,
    via: &'a str,
    changed: &'a [&'static str],
) -> Change<'a> {
    Change {
        thread,
        who,
        via,
        changed,
    }
}

/// The note `thread.done` leaves: the thread's title, goal and card.
fn card_note(t: &ThreadInfo) -> String {
    let mut s = format!("Thread {} ({}) is done.", t.slug, t.title);
    if !t.goal.is_empty() {
        s.push_str(&format!(" Goal: {}.", t.goal));
    }
    if t.card.is_empty() {
        s.push_str(" Its card was empty.");
    } else {
        s.push_str(" Its card:");
        for l in &t.card {
            s.push_str(&format!("\n- {l}"));
        }
    }
    s
}

/// A line of words: whitespace folded to single spaces, at most
/// `MAX_LINE_CHARS`.
pub(super) fn line(text: &str, what: &str) -> Result<String, RpcFailure> {
    let l = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if l.chars().count() > MAX_LINE_CHARS {
        return Err(RpcFailure::invalid(anyhow!(
            "a thread's {what} is at most {MAX_LINE_CHARS} characters"
        )));
    }
    Ok(l)
}

fn topics_of(topics: &[String]) -> Result<Vec<String>, RpcFailure> {
    let topics = unique(topics.iter().map(String::as_str));
    if topics.len() > MAX_TOPICS {
        return Err(RpcFailure::invalid(anyhow!(
            "a thread has at most {MAX_TOPICS} topics"
        )));
    }
    Ok(topics)
}

fn card_of(card: &[String]) -> Result<Vec<String>, RpcFailure> {
    let lines = card
        .iter()
        .map(|l| line(l, "card line"))
        .collect::<Result<Vec<_>, _>>()?;
    let lines: Vec<String> = lines.into_iter().filter(|l| !l.is_empty()).collect();
    if lines.len() > MAX_CARD {
        return Err(RpcFailure::invalid(anyhow!(
            "a thread's card is at most {MAX_CARD} lines"
        )));
    }
    Ok(lines)
}

fn working_set_of(w: &WorkingSet) -> Result<WorkingSet, RpcFailure> {
    let each = |what: &str, v: &[String]| -> Result<Vec<String>, RpcFailure> {
        let v = unique(v.iter().map(String::as_str));
        match v.len() > MAX_SET {
            true => Err(RpcFailure::invalid(anyhow!(
                "a thread's working set holds at most {MAX_SET} {what}"
            ))),
            false => Ok(v),
        }
    };
    Ok(WorkingSet {
        roots: each("roots", &w.roots)?,
        repos: each("repos", &w.repos)?,
        branches: each("branches", &w.branches)?,
        refs: each("refs", &w.refs)?,
        files: each("files", &w.files)?,
        hosts: each("hosts", &w.hosts)?,
    })
}

/// The entries trimmed, the empty ones dropped, each kept once, in order.
fn unique<'a>(items: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    items
        .map(str::trim)
        .filter(|s| !s.is_empty() && seen.insert(s.to_string()))
        .map(str::to_string)
        .collect()
}

/// A write's error on the wire: a judged refusal is `REFUSED`.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a thread request from {} does not count: {}. Threads are the owner's, from a \
                 private place.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
