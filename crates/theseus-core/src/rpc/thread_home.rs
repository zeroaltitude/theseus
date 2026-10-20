//! `thread.split`, `thread.split_plan` and `thread.unsplit` (theseus-vt9k, T8; theseus-q9b85).
//!
//! A split quotes whole turns of a source into a thread, and the source's compile leaves them out:
//!
//! - **The source** is a thread (by its slug), or a place's own session, its home: by the whole
//!   session id (`ses_…`, `theseus places`), or `home@<place>`. A slug that is no thread is refused
//!   with what to type instead.
//! - **The target** is a new thread `slug`, opened with the turns quoted as its first compile, or an
//!   existing thread `into`, which takes the same brief and arrangement at its tail, so its cache
//!   prefix is unchanged. A sealed thread takes no turns from an unsealed source, nor the reverse.
//! - **The source's compile.** One `Body::Moved` node in the source (`moved.rs`) names every node that
//!   left: its next compile is rewritten once (one cache write), the range replaced by one line,
//!   `moved to thread «X»: <gist>`. The nodes stay in the store. The model's own tool (a thread
//!   session's, the turn running there) quotes only: its turn's compile is not rewritten mid-turn.
//! - **`thread.unsplit`** reads the split's record (META `thread.split.<id>`) and writes the undo:
//!   a second `Moved` node in the source that makes the range readable again in its own place, the
//!   quoted turns hidden from an existing target's compile by a quiet `Moved` node of its own, a new
//!   thread done. Nothing is deleted.
//! - **`thread.split_plan`** reads and refuses as a split does, writes nothing, and counts the next
//!   request of each side by `context.census`'s estimate (no model call).

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use theseus_protocol::threads::{
    place_of, slug_ok, SplitEstimate, ThreadInfo, ThreadSplitParams, ThreadSplitPlan,
    ThreadSplitResult, ThreadState, ThreadUnsplitParams, ThreadUnsplitResult,
};
use theseus_protocol::{error_code, method, SessionOpenParams};
use theseus_store::{kinds, NewRecord};

use super::server::RpcFailure;
use super::thread_join::{no_thread, refuse_taken};
use super::threads::line;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread::{ThreadSplit, ThreadUnsplit};
use crate::moved::{self, MovedTo, SplitRecord};
use crate::node::{Body, Node, Origin};
use crate::session::SessionRecord;
use crate::thread_join::{day, split_note};
use crate::thread_split::{self, Chosen, Pick};

/// What a split's source is, as named.
struct Source {
    /// As the owner typed it, for the notes and the record.
    label: String,
    session: String,
    /// Its thread, when it is one's session.
    thread: Option<ThreadInfo>,
}

/// Where the turns go.
enum Target {
    New(ThreadInfo),
    Into { thread: ThreadInfo, session: String },
}

impl Target {
    fn slug(&self) -> &str {
        match self {
            Target::New(t) | Target::Into { thread: t, .. } => &t.slug,
        }
    }
}

/// A split read and checked.
struct Planned {
    source: Source,
    target: Target,
    chosen: Chosen,
    /// The range's line in the source's compile.
    gist: String,
    title: String,
    split: String,
}

/// How a split is asked.
struct SplitAsk<'a> {
    from: &'a str,
    slug: &'a str,
    into: Option<&'a str>,
    title: Option<&'a str>,
    goal: Option<&'a str>,
    gist: Option<&'a str>,
    pick: Pick,
    who: &'a str,
    via: &'a str,
    note: Note,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Note {
    /// The source's compile is rewritten, a `Moved` node its record.
    Node,
    /// The tool's result is the note; the source's compile is the running turn's.
    Result,
}

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::invalid(anyhow!("{e}"))
}

impl Core {
    /// The pick a request names: a range, several, or a list of positions.
    fn pick_of(p: &ThreadSplitParams) -> Result<Pick, RpcFailure> {
        match (&p.range, &p.positions, p.ranges.is_empty()) {
            (Some([a, b]), None, true) => Ok(Pick::Range(*a, *b)),
            (None, Some(ps), true) if !ps.is_empty() => Ok(Pick::Positions(ps.clone())),
            (None, None, false) => Ok(Pick::Ranges(
                p.ranges.iter().map(|[a, b]| (*a, *b)).collect(),
            )),
            _ => Err(RpcFailure::invalid(anyhow!(
                "name the turns to split: a range [first, last] of positions, several ranges, or \
                 a list of positions (the `@` numbers of `theseus history`)"
            ))),
        }
    }

    fn split_ask<'a>(
        p: &'a ThreadSplitParams,
        pick: Pick,
        who: &'a str,
        via: &'a str,
    ) -> SplitAsk<'a> {
        SplitAsk {
            from: p.from.trim(),
            slug: &p.slug,
            into: p.into.as_deref(),
            title: p.title.as_deref(),
            goal: p.goal.as_deref(),
            gist: p.gist.as_deref(),
            pick,
            who,
            via,
            note: Note::Node,
        }
    }

    /// The thread an ask's names point at, for the owner's judgment.
    fn split_pair(p: &ThreadSplitParams) -> &str {
        p.into.as_deref().unwrap_or(&p.slug)
    }

    /// `thread.split`, from a person: whole turns of `from` by position.
    pub fn thread_split(
        &self,
        p: &ThreadSplitParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadSplitResult, RpcFailure> {
        let who = who.into();
        self.judge_thread_pair(&who, method::THREAD_SPLIT, &p.from, Self::split_pair(p))?;
        let pick = Self::pick_of(p)?;
        let (who_s, via) = (who.who(), who.via());
        let ask = Self::split_ask(p, pick, &who_s, &via);
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
            into: None,
            title,
            goal,
            gist: None,
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

    /// The session a split reads from: a thread's, or a place's own.
    fn split_source(&self, from: &str) -> Result<Source, RpcFailure> {
        let board = self.threads();
        let hint = "to split a place's own session (its home), type its session id (`ses_…`: \
                    `theseus places` lists them) or `home@<place>`";
        let of_session = |label: &str, sid: String| -> Result<Source, RpcFailure> {
            if let Some(t) = board.of_session(&sid) {
                return Ok(Source {
                    label: t.slug.clone(),
                    session: sid,
                    thread: Some(t.clone()),
                });
            }
            let rec: SessionRecord =
                self.store
                    .get_session(&sid)
                    .map_err(invalid)?
                    .ok_or_else(|| {
                        RpcFailure::new(error_code::NOT_FOUND, format!("no session {sid}"))
                    })?;
            if rec.task.is_some() || rec.imported.is_some() {
                return Err(invalid(format!(
                    "{sid} is a task's or an imported session: a split takes turns from a \
                     thread or a place's own session"
                )));
            }
            if rec.execution_id.is_none() {
                return Err(invalid(format!("{sid} has no turns yet: nothing to split")));
            }
            Ok(Source {
                label: label.to_string(),
                session: sid,
                thread: None,
            })
        };
        if let Some(place) = from.strip_prefix("home@") {
            let sid = self
                .outbox
                .place_session(place)
                .map_err(invalid)?
                .ok_or_else(|| {
                    RpcFailure::new(
                        error_code::NOT_FOUND,
                        format!("no place {place} runs a session: `theseus places` lists them"),
                    )
                })?;
            return of_session(from, sid);
        }
        if from.starts_with("ses_") {
            return of_session(from, from.to_string());
        }
        let Some(a) = board.get(from) else {
            return Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no thread {from}: `theseus thread list` names them; {hint}"),
            ));
        };
        if let Some(m) = &a.merged_into {
            return Err(invalid(format!(
                "the thread {} was merged into {m}: split from that one",
                a.slug
            )));
        }
        let session = a.session_id.clone().ok_or_else(|| {
            invalid(format!(
                "the thread {} has no turns yet: nothing to split",
                a.slug
            ))
        })?;
        Ok(Source {
            label: a.slug.clone(),
            session,
            thread: Some(a.clone()),
        })
    }

    /// The target of a split, read and refused before anything is written.
    fn split_target(
        &self,
        ask: &SplitAsk<'_>,
        src: &Source,
        chosen: &Chosen,
    ) -> Result<(Target, String), RpcFailure> {
        let board = self.threads();
        let max_live = self.cfg.threads.max_live as usize;
        if let Some(into) = ask.into {
            let t = board.get(into.trim()).ok_or_else(|| {
                RpcFailure::new(
                    error_code::NOT_FOUND,
                    format!(
                        "no thread {into} to split into: `theseus thread list` names them; \
                         without `--into` a new thread opens"
                    ),
                )
            })?;
            if t.state == ThreadState::Done {
                return Err(invalid(format!(
                    "the thread {} is done: `theseus thread reopen {}` first",
                    t.slug, t.slug
                )));
            }
            if let Some(m) = &t.merged_into {
                return Err(invalid(format!(
                    "the thread {} was merged into {m}",
                    t.slug
                )));
            }
            if src.thread.as_ref().is_some_and(|s| s.slug == t.slug) {
                return Err(invalid("a thread cannot be split into itself"));
            }
            let sealed = |t: &ThreadInfo| t.sealed.is_some() || t.seal_pending.is_some();
            if sealed(t) != src.thread.as_ref().is_some_and(sealed) {
                return Err(invalid(format!(
                    "the seal differs: {} and {} are not both sealed or both open, and turns \
                     never cross a seal",
                    src.label, t.slug
                )));
            }
            let session = t.session_id.clone().ok_or_else(|| {
                invalid(format!(
                    "the thread {} has no session yet: send it a message first",
                    t.slug
                ))
            })?;
            return Ok((
                Target::Into {
                    thread: t.clone(),
                    session,
                },
                t.title.clone(),
            ));
        }
        let slug = ask.slug.trim();
        if !slug_ok(slug) {
            return Err(invalid(format!(
                "{slug:?} is not a slug: lower-case letters, digits and hyphens, at most 40, \
                 starting with a letter or digit (or `--into` an existing thread)"
            )));
        }
        let title = match ask.title {
            Some(t) => line(t, "title")?,
            None => slug.to_string(),
        };
        let goal = line(ask.goal.unwrap_or(""), "goal")?;
        refuse_taken(&board, slug, max_live)?;
        let now = theseus_protocol::now_unix_ms();
        let quoted = chosen.pieces.len() as u64;
        let a = src.thread.as_ref();
        let new = ThreadInfo {
            slug: slug.to_string(),
            title: title.clone(),
            goal,
            topics: a.map(|a| a.topics.clone()).unwrap_or_default(),
            working_set: chosen.working_set.clone(),
            card: vec![format!(
                "split from «{}», {}: {} quoted at the head of this thread.",
                src.label,
                day(now),
                crate::narrative::count(quoted, "message", "messages")
            )],
            persona: a.and_then(|a| a.persona.clone()),
            state: ThreadState::Active,
            sealed: a.and_then(|a| a.sealed.clone()),
            seal_pending: a.and_then(|a| a.seal_pending.clone()),
            surfaces: vec![],
            parent: None,
            merged_into: None,
            split_from: a.map(|a| a.slug.clone()),
            session_id: None,
            last_active_ms: now,
            created_at_ms: now,
        };
        Ok((Target::New(new), title))
    }

    /// What a split will make, read and refused before anything is written.
    fn plan_split(&self, ask: &SplitAsk<'_>) -> Result<Planned, RpcFailure> {
        self.threads_on()?;
        // The cheap refusals before anything is read.
        let slug = ask.slug.trim();
        if ask.into.is_none() {
            if !slug_ok(slug) {
                return Err(invalid(format!(
                    "{slug:?} is not a slug: lower-case letters, digits and hyphens, at most 40, \
                     starting with a letter or digit (or `--into` an existing thread)"
                )));
            }
            refuse_taken(&self.threads(), slug, self.cfg.threads.max_live as usize)?;
        }
        let source = self.split_source(ask.from)?;
        let nodes = self
            .store
            .transcript(&source.session)
            .map_err(|e| invalid(format!("{e:#}")))?;
        let chosen =
            thread_split::choose(&nodes, &ask.pick, ask.note == Note::Node).map_err(invalid)?;
        self.refuse_moved(&chosen)?;
        let (target, title) = self.split_target(ask, &source, &chosen)?;
        let gist = match ask.gist {
            Some(g) => crate::split_turns::one_line(g, 200),
            None => chosen.gist.clone(),
        };
        let gist = if gist.is_empty() {
            "turns moved".to_string()
        } else {
            gist
        };
        Ok(Planned {
            source,
            target,
            chosen,
            gist,
            title,
            split: crate::new_id("spl"),
        })
    }

    /// A part that holds a node an earlier split moved is refused, with the split to undo.
    fn refuse_moved(&self, chosen: &Chosen) -> Result<(), RpcFailure> {
        let Some(closed) = &chosen.closed else {
            return Ok(());
        };
        for id in &closed.moved {
            let to: Option<Option<MovedTo>> = self
                .store
                .get_meta(&moved::moved_of_key(id))
                .map_err(invalid)?;
            if let Some(Some(m)) = to {
                return Err(invalid(format!(
                    "the part chosen holds {id}, which split {} moved to «{}» already: choose \
                     other turns, or `theseus thread unsplit {}` first",
                    m.split, m.thread, m.split
                )));
            }
        }
        Ok(())
    }

    /// Whether a turn runs in `sid`'s session (its tail is that turn's). Reads the kernel's view
    /// in a frame that stages nothing.
    fn turn_runs_in(&self, sid: &str) -> bool {
        let Some(exec) = self
            .store
            .get_session::<SessionRecord>(sid)
            .ok()
            .flatten()
            .and_then(|r| r.execution_id)
        else {
            return false;
        };
        let held = std::cell::Cell::new(false);
        let probe: Result<()> = self.kernel.frame(&[&exec], |k| {
            held.set(k.holds_turn(&exec));
            bail!("a probe: nothing is written")
        });
        drop(probe);
        held.get()
    }

    /// Open the target, quote the turns, and (for the owner's act) take them out of the source's
    /// compile.
    fn split_thread(&self, ask: &SplitAsk<'_>) -> Result<(ThreadSplitResult, String), RpcFailure> {
        let plan = self.plan_split(ask)?;
        if ask.note == Note::Node && self.turn_runs_in(&plan.source.session) {
            return Err(invalid(format!(
                "a turn is running in {}: split when it has ended, so its compile can leave \
                 the turns out",
                plan.source.label
            )));
        }
        let (r, text) = match &plan.target {
            Target::New(_) => self.split_into_new(ask, &plan)?,
            Target::Into { .. } => self.split_into_existing(ask, &plan)?,
        };
        Ok((r, text))
    }

    /// The note and the rows every split ends with.
    fn finish_split(
        &self,
        ask: &SplitAsk<'_>,
        plan: &Planned,
        thread: ThreadInfo,
        target_nodes: Vec<String>,
        opened: bool,
        extra: Vec<NewRecord>,
    ) -> Result<(ThreadSplitResult, String), RpcFailure> {
        let (c, slug) = (&plan.chosen, thread.slug.as_str());
        let moved = c.closed.as_ref().map_or(0, |c| c.moved.len() as u32);
        let text = split_note(
            &plan.source.label,
            slug,
            &plan.title,
            c.pieces.len() as u32,
            c.first_pos,
            c.last_pos,
        );
        let noted = match (ask.note, &c.closed) {
            (Note::Node, Some(closed)) => {
                let record = SplitRecord {
                    id: plan.split.clone(),
                    from: plan.source.label.clone(),
                    source_session: plan.source.session.clone(),
                    moved_node: String::new(),
                    thread: slug.to_string(),
                    target_session: thread.session_id.clone().unwrap_or_default(),
                    opened,
                    target_nodes,
                    moved,
                    first: closed.first,
                    last: closed.last,
                    gist: plan.gist.clone(),
                    at_ms: theseus_protocol::now_unix_ms(),
                    undone: false,
                };
                self.write_moved(&plan.source.session, closed, record, extra)
                    .inspect_err(|e| {
                        tracing::warn!(error = %e.message, "a split's range was not taken out of its source's compile");
                    })
                    .is_ok()
            }
            _ => true,
        };
        let into = !opened;
        self.rec(None).announce(&ThreadSplit {
            thread: &thread,
            from: &plan.source.label,
            quoted: c.pieces.len() as u32,
            first_pos: c.first_pos,
            last_pos: c.last_pos,
            split: &plan.split,
            moved,
            into,
            who: ask.who,
            via: ask.via,
        });
        let mut t = [thread];
        let row = self.thread_rows(&mut t).pop().expect("one row");
        Ok((
            ThreadSplitResult {
                thread: row,
                from: plan.source.label.clone(),
                quoted: c.pieces.len() as u32,
                first_pos: c.first_pos,
                last_pos: c.last_pos,
                noted,
                split: plan.split.clone(),
                moved,
                into,
            },
            text,
        ))
    }

    /// A new thread: its session opens holding what the source holds, with its brief and the
    /// quoted turns in place before any message can reach it.
    fn split_into_new(
        &self,
        ask: &SplitAsk<'_>,
        plan: &Planned,
    ) -> Result<(ThreadSplitResult, String), RpcFailure> {
        let Target::New(new) = &plan.target else {
            unreachable!("a new thread")
        };
        let slug = new.slug.as_str();
        let max_live = self.cfg.threads.max_live as usize;
        let c = &plan.chosen;
        let rec = self
            .open_session(SessionOpenParams {
                label: Some(place_of(slug)),
                opened_from: Some(plan.source.session.clone()),
                ..SessionOpenParams::default()
            })
            .map_err(RpcFailure::invalid)?;
        let sid = rec.session_id.clone();
        let brief = thread_split::brief(
            new,
            &plan.source.label,
            c.pieces.len() as u32,
            c.first_pos,
            c.last_pos,
        );
        let nodes = match self.write_quoted_turns(&rec, &plan.source.label, &brief, c, false) {
            Ok(n) => n,
            Err(e) => {
                self.drop_orphan(&sid);
                return Err(RpcFailure::invalid(e));
            }
        };
        let place = place_of(slug);
        let bound = self
            .outbox
            .place_record(&place, &sid)
            .map_err(RpcFailure::invalid)?;
        let (from_s, new_t) = (
            plan.source.thread.as_ref().map(|t| t.slug.clone()),
            new.clone(),
        );
        let moved = c.closed.as_ref().map_or(0, |c| c.moved.len() as u32);
        let written = self
            .runner
            .threads
            .write_bound(
                &self.store,
                |board| {
                    refuse_taken(board, slug, max_live).map_err(|e| anyhow!(e.message))?;
                    if let Some(f) = &from_s {
                        let a = board.get(f).ok_or_else(|| no_thread(f))?;
                        if a.merged_into.is_some() {
                            bail!("the thread {f} was merged away while the split opened");
                        }
                    }
                    Ok(vec![new_t.clone()])
                },
                |_, ts| {
                    Ok(vec![
                        bound,
                        fact::row(
                            &ThreadSplit {
                                thread: &ts[0],
                                from: &plan.source.label,
                                quoted: c.pieces.len() as u32,
                                first_pos: c.first_pos,
                                last_pos: c.last_pos,
                                split: &plan.split,
                                moved,
                                into: false,
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
        self.finish_split(ask, plan, t, nodes, true, vec![])
    }

    /// An existing thread: the brief and the quoted turns at its tail.
    fn split_into_existing(
        &self,
        ask: &SplitAsk<'_>,
        plan: &Planned,
    ) -> Result<(ThreadSplitResult, String), RpcFailure> {
        let Target::Into { thread, session } = &plan.target else {
            unreachable!("an existing thread")
        };
        let c = &plan.chosen;
        let rec: SessionRecord = self
            .store
            .get_session(session)
            .map_err(invalid)?
            .ok_or_else(|| invalid(format!("no session {session}")))?;
        let brief = thread_split::brief_into(
            &thread.slug,
            &plan.source.label,
            c.pieces.len() as u32,
            c.first_pos,
            c.last_pos,
        );
        let nodes = self
            .write_quoted_turns(&rec, &plan.source.label, &brief, c, true)
            .map_err(RpcFailure::invalid)?;
        let moved = c.closed.as_ref().map_or(0, |c| c.moved.len() as u32);
        // The ledger row rides the frame that takes the turns out of the source.
        let row = fact::row(
            &ThreadSplit {
                thread,
                from: &plan.source.label,
                quoted: c.pieces.len() as u32,
                first_pos: c.first_pos,
                last_pos: c.last_pos,
                split: &plan.split,
                moved,
                into: true,
                who: ask.who,
                via: ask.via,
            },
            None,
            None,
        )
        .map_err(RpcFailure::invalid)?;
        self.finish_split(ask, plan, thread.clone(), nodes, false, vec![row])
    }

    /// The brief and the arrangement in a session, one frame, and their ids. `guard`: refused
    /// while a turn runs there (an existing thread's tail is that turn's).
    fn write_quoted_turns(
        &self,
        rec: &SessionRecord,
        from: &str,
        brief: &str,
        chosen: &Chosen,
        guard: bool,
    ) -> Result<Vec<String>> {
        let sid = &rec.session_id;
        let exec = rec
            .execution_id
            .clone()
            .ok_or_else(|| anyhow!("session {sid} has no execution"))?;
        let author = format!("{}{from}", thread_split::AUTHOR_PREFIX);
        let brief = Node::relayed(sid, None, Origin::Harness, &author, brief);
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
            if guard && k.holds_turn(&exec) {
                bail!("a turn is running in {sid}: split when it has ended");
            }
            k.stage(&records)?;
            Ok(())
        })?;
        Ok(vec![brief.id, arrangement.id])
    }

    /// The `Moved` node in the source, with the split's record, where each node went, and `extra`
    /// rows: one frame, refused while a turn runs in the source.
    fn write_moved(
        &self,
        source: &str,
        closed: &crate::split_turns::Closed,
        mut record: SplitRecord,
        extra: Vec<NewRecord>,
    ) -> Result<(), RpcFailure> {
        let rec: SessionRecord = self
            .store
            .get_session(source)
            .map_err(invalid)?
            .ok_or_else(|| invalid(format!("no session {source}")))?;
        let exec = rec
            .execution_id
            .ok_or_else(|| invalid(format!("session {source} has no execution")))?;
        let node = Node::harness_body(
            "mov",
            source,
            moved::AUTHOR,
            Body::Moved {
                split: record.id.clone(),
                undoes: None,
                thread: record.thread.clone(),
                to_session: record.target_session.clone(),
                nodes: closed.moved.clone(),
                first: closed.first,
                last: closed.last,
                gist: record.gist.clone(),
                quiet: false,
            },
        );
        record.moved_node = node.id.clone();
        let to = MovedTo {
            thread: record.thread.clone(),
            split: record.id.clone(),
        };
        let mut records = vec![
            node.record().map_err(invalid)?,
            NewRecord::json(kinds::META, Some(&moved::split_key(&record.id)), &record)
                .map_err(invalid)?,
        ];
        for id in &closed.moved {
            records.push(
                NewRecord::json(kinds::META, Some(&moved::moved_of_key(id)), &Some(&to))
                    .map_err(invalid)?,
            );
        }
        records.extend(extra);
        self.kernel
            .frame(&[&exec], |k| {
                if k.holds_turn(&exec) {
                    bail!("a turn is running in {source}");
                }
                k.stage(&records)?;
                Ok(())
            })
            .map_err(invalid)
    }

    /// `thread.split_plan`: a split's reading and its counts, nothing written.
    pub fn thread_split_plan(
        &self,
        p: &ThreadSplitParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadSplitPlan, RpcFailure> {
        let who = who.into();
        self.judge_thread_pair(&who, method::THREAD_SPLIT, &p.from, Self::split_pair(p))?;
        let pick = Self::pick_of(p)?;
        let (who_s, via) = (who.who(), who.via());
        let ask = Self::split_ask(p, pick, &who_s, &via);
        let plan = self.plan_split(&ask)?;
        let closed = plan.chosen.closed.clone().unwrap_or_default();
        let hypothetical = Node::harness_body(
            "mov",
            &plan.source.session,
            moved::AUTHOR,
            Body::Moved {
                split: plan.split.clone(),
                undoes: None,
                thread: plan.target.slug().to_string(),
                to_session: String::new(),
                nodes: closed.moved.clone(),
                first: closed.first,
                last: closed.last,
                gist: plan.gist.clone(),
                quiet: false,
            },
        );
        let (live, _) = self.live_profile();
        let tokens = |sid: &str, extra: Option<&Node>| -> Result<u64, RpcFailure> {
            let rec: SessionRecord = self
                .store
                .get_session(sid)
                .map_err(invalid)?
                .ok_or_else(|| invalid(format!("no session {sid}")))?;
            let next = self
                .runner
                .census_request_with(Some(&rec), "cli", &live, extra)
                .map_err(invalid)?;
            Ok(next.compiled.estimate.tokens)
        };
        let before = tokens(&plan.source.session, None)?;
        let after = tokens(&plan.source.session, Some(&hypothetical))?;
        let quoted_bytes: usize = plan
            .chosen
            .pieces
            .iter()
            .filter_map(|p| p.text.as_ref())
            .map(String::len)
            .sum();
        let quoted_tokens = (quoted_bytes as f64 / 3.0).ceil() as u64;
        let (thread, into, target) = match &plan.target {
            Target::New(t) => (t.slug.clone(), false, None),
            Target::Into { thread, session } => {
                let b = tokens(session, None)?;
                (
                    thread.slug.clone(),
                    true,
                    Some(SplitEstimate {
                        before: b,
                        after: b + quoted_tokens,
                    }),
                )
            }
        };
        Ok(ThreadSplitPlan {
            from: plan.source.label,
            thread,
            into,
            turns: closed.turns,
            nodes: closed.moved.len() as u32,
            bytes: closed.bytes,
            quoted: plan.chosen.pieces.len() as u32,
            quoted_tokens,
            first_pos: plan.chosen.first_pos,
            last_pos: plan.chosen.last_pos,
            gist: plan.gist,
            source: SplitEstimate { before, after },
            target,
        })
    }

    /// `thread.unsplit`: the split `p.split` undone.
    pub fn thread_unsplit(
        &self,
        p: &ThreadUnsplitParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadUnsplitResult, RpcFailure> {
        let who = who.into();
        let id = p.split.trim();
        let rec: SplitRecord = self
            .store
            .get_meta(&moved::split_key(id))
            .map_err(invalid)?
            .ok_or_else(|| {
                RpcFailure::new(
                    error_code::NOT_FOUND,
                    format!("no split {id}: `thread split` printed its id (`spl_…`)"),
                )
            })?;
        self.judge_thread_pair(&who, method::THREAD_UNSPLIT, &rec.from, &rec.thread)?;
        if rec.undone {
            return Err(invalid(format!("the split {id} was undone already")));
        }
        let mut sessions = vec![&rec.source_session];
        if !rec.opened {
            sessions.push(&rec.target_session);
        }
        if let Some(sid) = sessions.into_iter().find(|s| self.turn_runs_in(s)) {
            return Err(invalid(format!(
                "a turn is running in {sid}: undo the split when it has ended"
            )));
        }
        // The target first: a refusal there changes nothing, and a retry is whole.
        if !rec.opened {
            self.hide_quoted(&rec).map_err(invalid)?;
        }
        self.write_undo(&rec, &who).map_err(invalid)?;
        let retired = rec.opened && self.thread_done(&rec.thread, who).is_ok();
        let found = self
            .threads()
            .get(&rec.thread)
            .cloned()
            .ok_or_else(|| invalid(no_thread(&rec.thread)))?;
        let thread = self.thread_rows(&mut [found]).pop().expect("one row");
        Ok(ThreadUnsplitResult {
            split: rec.id,
            from: rec.from,
            thread,
            restored: rec.moved,
            retired,
        })
    }

    /// The quoted turns out of an existing target's compile: a quiet `Moved` node in its session
    /// hides the brief and the arrangement, and says nothing in their place.
    fn hide_quoted(&self, rec: &SplitRecord) -> Result<()> {
        let sid = &rec.target_session;
        let session: SessionRecord = self
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let exec = session
            .execution_id
            .ok_or_else(|| anyhow!("session {sid} has no execution"))?;
        let node = Node::harness_body(
            "mov",
            sid,
            moved::AUTHOR,
            Body::Moved {
                split: rec.id.clone(),
                undoes: None,
                thread: rec.thread.clone(),
                to_session: sid.clone(),
                nodes: rec.target_nodes.clone(),
                first: 0,
                last: 0,
                gist: String::new(),
                quiet: true,
            },
        );
        self.kernel.frame(&[&exec], |k| {
            if k.holds_turn(&exec) {
                bail!("a turn is running in {sid}");
            }
            k.stage(&[node.record()?])?;
            Ok(())
        })?;
        Ok(())
    }

    /// The undo in the source: a `Moved` node that `undoes` the split, the record marked, each
    /// node's thread cleared, and the ledger row: one frame.
    fn write_undo(&self, rec: &SplitRecord, who: &Answerer) -> Result<()> {
        let sid = &rec.source_session;
        let session: SessionRecord = self
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let exec = session
            .execution_id
            .ok_or_else(|| anyhow!("session {sid} has no execution"))?;
        let node = Node::harness_body(
            "mov",
            sid,
            moved::AUTHOR,
            Body::Moved {
                split: rec.id.clone(),
                undoes: Some(rec.id.clone()),
                thread: rec.thread.clone(),
                to_session: rec.target_session.clone(),
                nodes: vec![],
                first: rec.first,
                last: rec.last,
                gist: rec.gist.clone(),
                quiet: false,
            },
        );
        let mut done = rec.clone();
        done.undone = true;
        let (who_s, via) = (who.who(), who.via());
        let mut records = vec![
            node.record()?,
            NewRecord::json(kinds::META, Some(&moved::split_key(&rec.id)), &done)?,
            fact::row(
                &ThreadUnsplit {
                    split: &rec.id,
                    from: &rec.from,
                    thread: &rec.thread,
                    restored: rec.moved,
                    retired: rec.opened,
                    who: &who_s,
                    via: &via,
                },
                None,
                None,
            )?,
        ];
        // Each moved node's mark: the nodes are the source's `Moved` node's list.
        let moved_ids = self.moved_ids(sid, &rec.moved_node)?;
        for id in moved_ids {
            records.push(NewRecord::json(
                kinds::META,
                Some(&moved::moved_of_key(&id)),
                &None::<MovedTo>,
            )?);
        }
        self.kernel.frame(&[&exec], |k| {
            if k.holds_turn(&exec) {
                bail!("a turn is running in {sid}");
            }
            k.stage(&records)?;
            Ok(())
        })?;
        Ok(())
    }

    /// The ids a `Moved` node of `sid` lists.
    fn moved_ids(&self, sid: &str, node: &str) -> Result<Vec<String>> {
        let nodes = self.store.transcript(sid)?;
        nodes
            .iter()
            .find(|(_, n)| n.id == node)
            .and_then(|(_, n)| match &n.body {
                Body::Moved { nodes, .. } => Some(nodes.clone()),
                _ => None,
            })
            .ok_or_else(|| anyhow!("the record {node} of the split is not in {sid}"))
    }
}
