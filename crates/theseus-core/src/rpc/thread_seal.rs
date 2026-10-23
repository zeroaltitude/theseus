//! Sealed threads (theseus-f2g0; the owner, 2026-10-09 D12: **Jev
//! recommends; the owner seals**). A sealed thread's notes never come into
//! another thread's recall, or any recall, ambiently (`theseus_memory::
//! threads`: the lens holds them out of every pack, the rerank included).
//!
//! - **`thread.seal`** is the owner's act, from a private place
//!   (`judge_act(Act::Thread)`): the thread's record and its `thread.sealed`
//!   row in one frame. The answer reports what had already crossed, from the
//!   `derived_from` edges of `Recall` nodes that name the thread's notes
//!   ("2 facts were already used in «pr round»"); with `redirect`, a harness
//!   note in each of those sessions says the facts are withdrawn.
//! - **`thread.unseal`**: the owner, from a private place.
//! - **A proposal** ([`Core::thread_recommend_seal`], the entry point the core
//!   and a later Jev pack call; row T10a, theseus-c1uz, brings the Jev
//!   question that calls it): sets `seal_pending`, writes `thread.seal_proposed`
//!   and posts a card to the owner's DM with **Seal this thread** and **Not
//!   needed**. While it waits the thread is held out of other threads' recall
//!   (a tightening only). **`thread.seal_dismiss`** ("Not needed") clears it,
//!   writes `thread.seal_dismissed` (a label for Jev's learning report) and
//!   remembers the thread's content digest (META `thread.seal_dismissed.<slug>`),
//!   so it is proposed once until its content changes.
//! - **Inheritance**: a thread made `sealed`, or split from a sealed thread
//!   (`split_from`), starts sealed; a task's session is its parent thread's
//!   (`thread_recall`), so it is held with it; [`merged_seal`] is the rule for
//!   a merge, which the router's row calls.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use theseus_protocol::threads::{
    ThreadCrossed, ThreadInfo, ThreadNameParams, ThreadNewParams, ThreadRow, ThreadSealParams,
    ThreadSealResult,
};
use theseus_protocol::{error_code, method};
use theseus_store::{kinds, NewRecord};

use super::server::RpcFailure;
use super::threads::line;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread_seal::{
    SealChange, ThreadSealDismissed, ThreadSealProposed, ThreadSealed, ThreadUnsealed,
};
use crate::graph::{Edge, EdgeKind, VIA_RECALL};
use crate::node::{Body, Node, Origin};
use crate::session::SessionRecord;

/// The reason a seal carries when the owner gives none.
const OWNERS_SEAL: &str = "sealed by the owner";
/// The most sessions of one thread a seal's report reads (its chain of
/// successors).
const MAX_SESSIONS: usize = 50;
/// The author of a redirect note.
const NOTE_AUTHOR: &str = "thread";

/// META key of a dismissed proposal's content digest.
pub fn dismissed_key(slug: &str) -> String {
    format!("thread.seal_dismissed.{slug}")
}

/// A digest of what a thread says of itself: a dismissed proposal is not made
/// again until it changes.
pub fn content_digest(t: &ThreadInfo) -> String {
    let words = json!([
        t.title,
        t.goal,
        t.card,
        t.working_set.roots,
        t.working_set.repos,
        t.working_set.branches,
        t.working_set.refs,
        t.working_set.files,
        t.working_set.hosts
    ]);
    hex::encode(&Sha256::digest(words.to_string().as_bytes())[..8])
}

fn sc<'a>(t: &'a ThreadInfo, who: &'a str, via: &'a str, reason: &'a str) -> SealChange<'a> {
    SealChange {
        slug: &t.slug,
        who,
        via,
        reason,
        crossed: 0,
        digest: "",
    }
}

/// What a new thread starts with besides its words (theseus-f2g0): the thread
/// it splits from, and its seal, which is the owner's request or inherited
/// from a sealed parent.
pub(super) fn starting_seal(
    board: &crate::threads::Board,
    p: &ThreadNewParams,
    who: &str,
    now_ms: u64,
) -> Result<(Option<String>, Option<theseus_protocol::threads::Seal>)> {
    let parent = match p.split_from.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Some(
            board
                .get(s)
                .ok_or_else(|| anyhow!("no thread {s} to split from"))?,
        ),
        _ => None,
    };
    let inherited = parent.and_then(|s| crate::threads::inherited_seal(s, who, now_ms));
    let seal = match (inherited, p.sealed) {
        (Some(s), _) => Some(s),
        (None, true) => Some(theseus_protocol::threads::Seal {
            by: who.to_string(),
            at_ms: now_ms,
            reason: "sealed when made".into(),
        }),
        (None, false) => None,
    };
    Ok((parent.map(|s| s.slug.clone()), seal))
}

/// The rows a new thread's frame holds: its `thread.created`, and a
/// `thread.sealed` when it starts sealed.
pub(super) fn created_rows(ts: &[ThreadInfo], who: &str, via: &str) -> Result<Vec<NewRecord>> {
    let mut rows = Vec::new();
    for t in ts {
        let c = super::threads::change(t, who, via, &[]);
        rows.push(fact::row(
            &crate::fact::thread::ThreadCreated(c),
            None,
            None,
        )?);
        if let Some(s) = &t.sealed {
            rows.push(fact::row(
                &ThreadSealed(sc(t, who, via, &s.reason)),
                None,
                None,
            )?);
        }
    }
    Ok(rows)
}

impl Core {
    /// `thread.seal`.
    pub fn thread_seal(
        &self,
        p: &ThreadSealParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadSealResult, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_SEAL, &p.slug)?;
        self.threads_on()?;
        let reason = match p.reason.as_deref() {
            Some(r) if !r.trim().is_empty() => line(r, "reason")?,
            _ => OWNERS_SEAL.to_string(),
        };
        let (who_s, via) = (who.who(), who.via());
        let now = theseus_protocol::now_unix_ms();
        let slug = p.slug.trim().to_string();
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let cur = board.get(&slug).ok_or_else(|| {
                        anyhow!("no thread {slug}: `theseus thread list` names them")
                    })?;
                    if cur.sealed.is_some() {
                        return Ok(vec![]);
                    }
                    let mut next = cur.clone();
                    next.sealed = Some(theseus_protocol::threads::Seal {
                        by: who_s.clone(),
                        at_ms: now,
                        reason: reason.clone(),
                    });
                    next.seal_pending = None;
                    Ok(vec![next])
                },
                |_, ts| {
                    ts.iter()
                        .map(|t| fact::row(&ThreadSealed(sc(t, &who_s, &via, &reason)), None, None))
                        .collect()
                },
            )
            .map_err(RpcFailure::invalid)?;
        let Some(t) = written.into_iter().next() else {
            // Sealed already: as it is, and nothing written.
            let t = self.thread_named(&slug)?;
            return Ok(ThreadSealResult {
                thread: self.thread_rows(&mut [t]).pop().expect("one row"),
                crossed: Vec::new(),
            });
        };
        self.rec(None)
            .announce(&ThreadSealed(sc(&t, &who_s, &via, &reason)));
        let mut crossed = theseus_store::blocking(|| self.thread_crossed(&t)).unwrap_or_else(|e| {
            tracing::warn!(thread = %t.slug, error = %format!("{e:#}"),
                "what crossed out of a sealed thread could not be read");
            Vec::new()
        });
        if p.redirect {
            self.redirect_crossed(&t, &mut crossed);
        }
        Ok(ThreadSealResult {
            thread: self.thread_rows(&mut [t]).pop().expect("one row"),
            crossed,
        })
    }

    /// `thread.unseal`: the owner, from a private place, opens it again.
    pub fn thread_unseal(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_UNSEAL, slug)?;
        self.threads_on()?;
        let (who_s, via) = (who.who(), who.via());
        let slug = slug.trim().to_string();
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let cur = board.get(&slug).ok_or_else(|| {
                        anyhow!("no thread {slug}: `theseus thread list` names them")
                    })?;
                    if cur.sealed.is_none() {
                        return Ok(vec![]);
                    }
                    let mut next = cur.clone();
                    next.sealed = None;
                    Ok(vec![next])
                },
                |_, ts| {
                    ts.iter()
                        .map(|t| {
                            fact::row(
                                &ThreadUnsealed(sc(t, &who_s, &via, "opened again")),
                                None,
                                None,
                            )
                        })
                        .collect()
                },
            )
            .map_err(RpcFailure::invalid)?;
        let t = match written.into_iter().next() {
            Some(t) => {
                self.rec(None)
                    .announce(&ThreadUnsealed(sc(&t, &who_s, &via, "opened again")));
                t
            }
            None => self.thread_named(&slug)?,
        };
        Ok(self.thread_rows(&mut [t]).pop().expect("one row"))
    }

    /// `thread.seal_dismiss`: "Not needed". The proposal ends, the answer is a
    /// label row, and the content's digest is remembered so the thread is not
    /// proposed again until it changes.
    pub fn thread_seal_dismiss(
        &self,
        slug: &str,
        who: impl Into<Answerer>,
    ) -> Result<ThreadRow, RpcFailure> {
        let who = who.into();
        self.judge_thread(&who, method::THREAD_SEAL_DISMISS, slug)?;
        self.threads_on()?;
        let (who_s, via) = (who.who(), who.via());
        let slug = slug.trim().to_string();
        let asked = std::cell::RefCell::new(String::new());
        let written = self
            .runner
            .threads
            .write(
                &self.store,
                |board| {
                    let cur = board.get(&slug).ok_or_else(|| {
                        anyhow!("no thread {slug}: `theseus thread list` names them")
                    })?;
                    let Some(reason) = cur.seal_pending.clone() else {
                        return Ok(vec![]);
                    };
                    *asked.borrow_mut() = reason;
                    let mut next = cur.clone();
                    next.seal_pending = None;
                    Ok(vec![next])
                },
                |_, ts| {
                    let mut rows = Vec::new();
                    for t in ts {
                        let digest = content_digest(t);
                        let reason = asked.borrow();
                        let c = SealChange {
                            digest: &digest,
                            ..sc(t, &who_s, &via, &reason)
                        };
                        rows.push(fact::row(&ThreadSealDismissed(c), None, None)?);
                        rows.push(NewRecord::json(
                            kinds::META,
                            Some(&dismissed_key(&t.slug)),
                            &digest,
                        )?);
                    }
                    Ok(rows)
                },
            )
            .map_err(RpcFailure::invalid)?;
        let t = match written.into_iter().next() {
            Some(t) => {
                let digest = content_digest(&t);
                let reason = asked.borrow();
                self.rec(None).announce(&ThreadSealDismissed(SealChange {
                    digest: &digest,
                    ..sc(&t, &who_s, &via, &reason)
                }));
                t
            }
            None => {
                return Err(RpcFailure::new(
                    error_code::REFUSED,
                    format!("no seal is proposed for the thread {slug}"),
                ))
            }
        };
        Ok(self.thread_rows(&mut [t]).pop().expect("one row"))
    }

    /// Propose sealing `slug` (the core's entry point; a later Jev pack calls
    /// it): `seal_pending` is set, the card goes to the owner's DM, and the
    /// thread is held out of other threads' recall until it is answered.
    /// Nothing is proposed for a thread already sealed or pending, one done,
    /// or one the owner said "not needed" to while its content is as it was.
    /// True when a proposal was made.
    pub fn thread_recommend_seal(&self, slug: &str, reason: &str) -> Result<bool> {
        self.threads_on().map_err(|e| anyhow!("{}", e.message))?;
        let reason = line(reason, "reason").map_err(|e| anyhow!("{}", e.message))?;
        let reason = if reason.is_empty() {
            "it looks embargoed".to_string()
        } else {
            reason
        };
        let slug = slug.trim().to_string();
        let posted = std::cell::RefCell::new(None);
        let written = self.runner.threads.write(
            &self.store,
            |board| {
                let Some(cur) = board.get(&slug) else {
                    anyhow::bail!("no thread {slug}");
                };
                let dismissed: Option<String> = self.store.get_meta(&dismissed_key(&slug))?;
                if cur.sealed.is_some()
                    || cur.seal_pending.is_some()
                    || cur.state == theseus_protocol::threads::ThreadState::Done
                    || dismissed.is_some_and(|d| d == content_digest(cur))
                {
                    return Ok(vec![]);
                }
                let mut next = cur.clone();
                next.seal_pending = Some(reason.clone());
                Ok(vec![next])
            },
            |_, ts| {
                let mut rows = Vec::new();
                for t in ts {
                    rows.push(fact::row(
                        &ThreadSealProposed(sc(t, "theseus", "core", &reason)),
                        None,
                        None,
                    )?);
                    let body = json!({"kind": "seal_card", "slug": t.slug, "title": t.title,
                        "reason": reason,
                        "text": seal_text(&t.title, &t.slug, &reason)});
                    let (post, more) = self.outbox.stage_to_operator(None, body)?;
                    rows.extend(more);
                    *posted.borrow_mut() = Some(post);
                }
                Ok(rows)
            },
        )?;
        let Some(t) = written.into_iter().next() else {
            return Ok(false);
        };
        if let Some(post) = posted.borrow().as_ref() {
            self.outbox.posted(post);
        }
        self.rec(None)
            .announce(&ThreadSealProposed(sc(&t, "theseus", "core", &reason)));
        Ok(true)
    }

    /// The sessions of a thread, newest first: its current one and the ones
    /// before it in its place's chain.
    pub(crate) fn thread_sessions(&self, t: &ThreadInfo) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut cur = t.session_id.clone();
        while let Some(s) = cur {
            if out.contains(&s) || out.len() >= MAX_SESSIONS {
                break;
            }
            cur = self
                .store
                .get_session::<SessionRecord>(&s)
                .ok()
                .flatten()
                .and_then(|r| r.supersedes)
                .map(|l| l.session_id);
            out.push(s);
        }
        out
    }

    /// What of the thread's notes recall already put into other sessions: the
    /// `derived_from` edges (`via recall`) into its nodes, grouped by the
    /// session of the `Recall` node that holds them. A read for a person: one
    /// scope read for each node of the thread's sessions.
    fn thread_crossed(&self, t: &ThreadInfo) -> Result<Vec<ThreadCrossed>> {
        let board = self.threads();
        let memo = self.runner.memory.thread_memo();
        let mut by_recall: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for sid in self.thread_sessions(t) {
            for (_, n) in self.store.session_nodes(&sid)? {
                if matches!(n.body, Body::Recall { .. }) {
                    continue;
                }
                for r in self.store.scope_after(&Edge::scope_into(&n.id), 0)? {
                    if r.kind != kinds::EDGE {
                        continue;
                    }
                    let e: Edge = r.decode()?;
                    if e.via == VIA_RECALL
                        && EdgeKind::named(&e.kind) == Some(EdgeKind::DerivedFrom)
                    {
                        by_recall.entry(e.from).or_default().insert(n.id.clone());
                    }
                }
            }
        }
        let mut by_session: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (recall, sources) in by_recall {
            if let Some((_, node)) = self.store.get_node(&recall)? {
                by_session
                    .entry(node.session_id.clone())
                    .or_default()
                    .extend(sources);
            }
        }
        let mut out = Vec::new();
        for (session, sources) in by_session {
            let slug = memo.slug_of(&self.store, &board, &session);
            if slug.as_deref() == Some(t.slug.as_str()) {
                continue;
            }
            let title = slug
                .as_deref()
                .and_then(|s| board.get(s))
                .map(|x| x.title.clone());
            out.push(ThreadCrossed {
                session_id: session,
                thread: slug,
                title,
                facts: sources.len() as u32,
                redirected: false,
            });
        }
        Ok(out)
    }

    /// A redirect note in each session the thread's notes crossed into: they
    /// are withdrawn there. Said without the notes' words. A session with a
    /// turn running is left for the owner to ask again.
    fn redirect_crossed(&self, t: &ThreadInfo, crossed: &mut [ThreadCrossed]) {
        for c in crossed.iter_mut() {
            let text = format!(
                "Redirect: {} {} from the sealed thread «{}» ({}) were recalled into this \
                 conversation before it was sealed. They are withdrawn: do not rely on them, \
                 repeat them, or carry their details into other work, and treat what you drew \
                 from them as unverified.",
                c.facts,
                if c.facts == 1 { "fact" } else { "facts" },
                t.title,
                t.slug
            );
            match self.write_harness_note(&c.session_id, &text) {
                Ok(()) => c.redirected = true,
                Err(e) => tracing::warn!(session = %c.session_id, error = %format!("{e:#}"),
                    "a redirect note was not written"),
            }
        }
    }

    /// A harness note in `sid`, in a frame of its own; refused while a turn
    /// runs there.
    fn write_harness_note(&self, sid: &str, text: &str) -> Result<()> {
        let rec = self
            .store
            .get_session::<SessionRecord>(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let Some(exec) = rec.execution_id else {
            anyhow::bail!("session {sid} has no execution to hold a note");
        };
        let node = Node::relayed(sid, None, Origin::Harness, NOTE_AUTHOR, text);
        self.kernel.frame(&[&exec], |k| {
            if k.holds_turn(&exec) {
                anyhow::bail!("a turn is running in {sid}");
            }
            k.stage(&[node.record()?])?;
            Ok(())
        })?;
        Ok(())
    }
}

/// Whether `text` is the owner's explicit verb: `/seal`, or "seal this
/// thread", in any case with its closing punctuation (an explicit verb, not
/// routing).
pub fn is_seal_word(text: &str) -> bool {
    let t = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_lowercase();
    t == "/seal" || t == "seal this thread"
}

impl Core {
    /// The owner's explicit verb in a thread's session (`/seal`, "seal this
    /// thread"): the thread is sealed before the turn runs, as the owner's
    /// act from where they said it (`thread.seal`, judged as any thread act),
    /// and a harness note in the session says so and what had crossed, so
    /// the turn can answer. A message from a job, a prompt, or an MCP client
    /// is no verb. A refusal leaves the turn to run as an ordinary message.
    pub(super) fn seal_by_word(
        &self,
        p: &theseus_protocol::TurnSubmitParams,
        conn: super::server::Conn<'_>,
        session: &str,
    ) {
        if p.opened_from.is_some()
            || p.prompt.is_some()
            || !p.attachments.is_empty()
            || !is_seal_word(&p.input)
            || matches!(
                conn.surface,
                crate::approval::Surface::Mcp | crate::approval::Surface::Unnamed
            )
        {
            return;
        }
        let Some(t) = self.threads().of_session(session).cloned() else {
            return;
        };
        if t.sealed.is_some() {
            return;
        }
        let params = ThreadSealParams {
            slug: t.slug.clone(),
            reason: Some("sealed by the owner's word in the thread".into()),
            ..Default::default()
        };
        let who = conn.answerer(p.author.clone(), None);
        let Ok(r) = self.thread_seal(&params, who) else {
            return;
        };
        let mut note = format!(
            "The owner sealed this thread ({}): its notes never come into another thread, and \
             another thread reading it asks the owner first.",
            t.slug
        );
        for w in theseus_protocol::threads::crossed_words(&r.crossed) {
            note.push_str(&format!(" {w}."));
        }
        if let Err(e) = self.write_harness_note(session, &note) {
            tracing::warn!(thread = %t.slug, error = %format!("{e:#}"),
                "the seal's note was not left in the session");
        }
    }
}

/// The dispatch for `thread.seal`, `.unseal` and `.seal_dismiss`.
pub(super) fn route_seal(
    core: &Core,
    name: &str,
    params: serde_json::Value,
    conn: super::server::Conn<'_>,
) -> Result<serde_json::Value, RpcFailure> {
    use super::server::route;
    match name {
        method::THREAD_SEAL => route(params, |p: ThreadSealParams| {
            core.thread_seal(&p, conn.answerer(p.author.clone(), p.discord.clone()))
        }),
        method::THREAD_UNSEAL => route(params, |p: ThreadNameParams| {
            core.thread_unseal(&p.slug, conn.answerer(p.author, p.discord))
        }),
        _ => route(params, |p: ThreadNameParams| {
            core.thread_seal_dismiss(&p.slug, conn.answerer(p.author, p.discord))
        }),
    }
}

/// The proposed seal's card (theseus-x8b9k).
pub(crate) fn seal_text(title: &str, slug: &str, reason: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &format!("Seal thread «{title}»?"),
        &format!("This looks embargoed: {reason}"),
        &format!("Thread «{title}» ({slug})."),
    )
}
