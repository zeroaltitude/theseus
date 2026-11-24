//! `context.history` (issue 44; design §6, rung 3): the history tool, offered on every turn, by which the model
//! extends the conversation it can see when it thinks it is missing something. The interface is the view's
//! (`view::history`); this module is its implementation over the exchange ledger (`crate::exchange`).
//!
//! - **Bare**, it returns the [`PAGE_EXCHANGES`] exchanges of this place that come before the oldest exchange the
//!   model sees in full (`page::in_view`). A page's `next` cursor, passed back as `before`, gives the ones
//!   before those, with no horizon. One line an exchange: its date, the place, whose words, the person's words
//!   (cut by code at 300 characters), the `did` line or the reply's beginning, and a handle.
//! - **Open**: a handle gives the exchange in spoken form, cut at about 3,000 tokens (`open`); a session's id
//!   (`ses_…`, this harness's or an imported one) gives a page of its messages the same way, `before` paging
//!   back (`session`).
//! - **Narrowing**: `place` (any other place), `thing` (its timeline in the touch index), `words` (the words
//!   index, searched over the sessions on that timeline, then a filter on the walk), and a time span: `when`,
//!   or `from` / `to` ([`span_of`]: an explicit argument's reader, `recall::when::read_local`, never recall's
//!   reading of English in a person's message). A span's page holds exactly its exchanges, newest first,
//!   paged by `before`; an empty span says so; beside `words`, `place` or `thing` it narrows them.
//! - **Every place, for every reader** (anything goes, 2026-11-22): no presence, place class, seal or move
//!   keeps an exchange off a page or out of an open.
//! - **Retraction corrects; it does not hide**: a retracted statement still appears in history as said, and its
//!   correction is the note the retraction gave the sessions that used it. An erased import is gone (each node
//!   read by its newest record).
//! - **Another person's words are outside text** ([`marker`]): a result whose page holds another person's
//!   line, or whose open holds another person's message, is marked External on its node, as a fetched page
//!   is, so `external::read_any`, `external::first` and the settings acts' outside-text check see it.
//! - **Where** ([`anchor`]): a message's turn is anchored where the message arrived; any other turn where its
//!   reply is delivered (`Outbox::speaks_at`, a wake's or a report's target too); a route that cannot be read
//!   is no place, and its bare page is the unknown place's.
//! - **Bounded**: [`LOOKS_PER_TURN`] pages and [`OPENS_PER_TURN`] opens a turn, each told plainly when used;
//!   "history is exhausted" is told apart.
//! - **Offered everywhere**: a public tool (`places::public_tool`), so a shared place has it too. Its family,
//!   `history`, loads by default in a DM and a channel; a terminal's, a task's and a headless run's first
//!   request has no room for it under its budget (`first_request`), so there it is declared and deferred, one
//!   search away, and the self card names it. Costs nothing until called: the core is reached through a weak
//!   handle given at build (`Board::attach`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_memory::recall::Place;
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use crate::config::memory::MemoryArm;
use crate::exchange::{self, Exchange, Opener};
use crate::recall::when::When;
use crate::turn::TurnRunner;
use crate::view::history::{
    HistoryArgs, HistoryPage, PageEnd, HISTORY_TOOL, LOOKS_PER_TURN, PAGE_EXCHANGES,
};
use crate::view::timeline::Cursor;
use crate::view::{Anchor, PointOfView, Present};
use crate::Core;

pub(crate) mod imports;
pub(crate) mod mcp;
pub(crate) mod open;
pub(crate) mod page;
pub(crate) mod session;
#[cfg(test)]
mod tests;

/// Its name, as the config's `[policy.tools]` checks it.
pub const NAMES: [&str; 1] = [HISTORY_TOOL];
/// Exchanges a turn may open.
pub const OPENS_PER_TURN: u32 = 12;

/// The turns a words filter may name, at most.
const WORDS_K: usize = 100;
const DEADLINE: Duration = Duration::from_secs(2);
/// Turns whose counts are kept before finished ones are let go.
const TURNS_KEPT: usize = 256;
/// What a resume token's turn id begins with.
const RESUME: &str = "resume:";

fn parse(input: &Value) -> Result<HistoryArgs, String> {
    serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))
}

/// The time span a call names, read as of `now_ms` by the reader an explicit argument takes
/// (`recall::when::read_local`): `when`, or `from` and `to` (each read as a span, `from` giving its start and
/// `to` its end). None when none is given; a span that does not read is told in words, never guessed.
pub(crate) fn span_of(a: &HistoryArgs, now_ms: u64) -> Result<Option<When>, String> {
    let some = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let read = |s: &str| {
        crate::recall::when::read_local(s, now_ms).ok_or_else(|| {
            format!(
                "\"{}\" does not read as a time: name a day, a date or a span (\"yesterday\", \"2026-11-20\", \
                 \"last week\", \"from 3 to 5 November\")",
                page::flat(s)
            )
        })
    };
    match (some(&a.when), some(&a.from), some(&a.to)) {
        (None, None, None) => Ok(None),
        (Some(w), None, None) => Ok(Some(read(&w)?)),
        (Some(_), _, _) => Err("name a span with when, or with from and to, not both".into()),
        (None, from, to) => {
            let start = from.as_deref().map(read).transpose()?;
            let end = to.as_deref().map(read).transpose()?;
            let said = match (&from, &to) {
                (Some(f), Some(t)) => format!("from {f} to {t}"),
                (Some(f), None) => format!("since {f}"),
                (None, Some(t)) => format!("before the end of {t}"),
                (None, None) => unreachable!("one of them is given"),
            };
            Ok(Some(When {
                said,
                from_ms: start.and_then(|w| w.from_ms),
                to_ms: end.and_then(|w| w.to_ms),
            }))
        }
    }
}

/// The registry's `context.history`: its name, schema and plan, for the gate. The runtime runs it
/// ([`Board::run`]).
pub struct HistoryTool;

impl Tool for HistoryTool {
    fn name(&self) -> &'static str {
        HISTORY_TOOL
    }
    fn description(&self) -> &'static str {
        "This place's earlier history, 100 exchanges a page before those you see; before pages back; open reads one (or a ses_ session by its id); when (or from/to) a time span. place \"import\" (or \"import:<book>\") with words searches the imported conversations."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "before": {"type": "object"},
                "open": {"type": "string"},
                "words": {"type": "string"},
                "thing": {"type": "string"},
                "place": {"type": "string"},
                "when": {"type": "string"},
                "from": {"type": "string"},
                "to": {"type": "string"}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = parse(input)?;
        let summary = match (&a.open, &a.before) {
            (Some(h), _) if session::names_a_session(&h.0) => {
                format!("open session {}", page::flat(&h.0))
            }
            (Some(h), _) => format!("open exchange {}", page::flat(&h.0)),
            (None, Some(_)) => "read further back through history".into(),
            (None, None) => "read back through history".into(),
        };
        Ok(Plan {
            summary,
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "context.history runs through the runtime, which reads the ledger",
        ))))
    }
}

/// One open, whatever its handle names: a session's page by its `ses_…` id (`session::open`, `before` paging
/// back), or one exchange (`open::open_marked`). Behind the lens's seams (the fork's cut, the self loop's
/// scope); every reader of history opens through here (the model's tool, and any other face of it).
pub(crate) fn open_any(
    lens: &page::Lens<'_>,
    h: &crate::view::Handle,
    before: Option<&Cursor>,
) -> Result<OpenedAny, String> {
    if session::names_a_session(&h.0) {
        let s = session::open(lens, &h.0, before)?;
        return Ok(OpenedAny {
            text: s.text,
            outside: s.outside,
            session: true,
            next: s.next,
            sources: s.sources,
        });
    }
    let o = open::open_marked(lens, h)?;
    Ok(OpenedAny {
        text: o.text,
        outside: o.outside,
        session: false,
        next: None,
        sources: o.sources,
    })
}

/// What [`open_any`] gives: the text, whether it shows outside words (its result is marked External,
/// [`marker`]), whether it was a session's page, a session page's continuation, as data, and the nodes whose
/// words it shows.
pub(crate) struct OpenedAny {
    pub text: String,
    pub outside: bool,
    pub session: bool,
    pub next: Option<Cursor>,
    pub sources: Vec<String>,
}

/// The nodes a page's lines show the words of (`copies`): each line's message when its words are on it, and
/// its reply when its beginning is; a stored account (`did`) is written from both (`ladder::did::material`), so
/// a line that shows one links both, whatever the page could read of the message itself (D6 round 5). An
/// exchange that cannot be read again fails the call: no words go out whose sources are unknown.
fn page_sources(core: &Core, p: &HistoryPage) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for l in &p.lines {
        let gone = || {
            format!(
                "what the page shows could not be read again: {}",
                l.handle.0
            )
        };
        let ex = exchange::exchange(&core.store, &l.handle.0)
            .map_err(|e| format!("{}: {e:#}", gone()))?
            .ok_or_else(gone)?;
        if !l.his_words.is_empty() || l.did.is_some() {
            out.extend(ex.ask);
        }
        if l.did.is_some() || l.reply_begins.is_some() {
            out.extend(ex.reply);
        }
    }
    Ok(out)
}

/// The External marker of a call's result (`external`): set when what it shows holds another person's
/// words (a page's line, an opened exchange or session marked outside text), so every reader of the marker
/// (`external::read_any`, `external::first`, the settings acts' "outside text since the owner's message")
/// sees it as it sees a fetched page. Its `url` names what was read: `history:<place>`, `history:<handle>`.
pub(crate) fn marker(o: &ToolOutput) -> Option<theseus_tools::External> {
    o.meta["outside"]
        .as_str()
        .map(|url| theseus_tools::External {
            url: url.to_string(),
        })
}

/// What a page read, as its marker names it: the place or the thing it was asked for, or here.
fn read_of<'a>(a: &'a HistoryArgs, here: &'a str) -> &'a str {
    let some = |s: &'a Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty());
    some(&a.place).or(some(&a.thing)).unwrap_or(here)
}

/// The `outside` of a result's meta: what it read, when it shows another person's words.
fn outside_of(shows: bool, what: &str) -> Value {
    match shows {
        true => json!(format!("history:{}", page::flat(what))),
        false => Value::Null,
    }
}

/// One turn's use of the tool: its looks and opens, and the resume positions of walks stopped by their bound.
#[derive(Debug, Default, Clone)]
pub struct Use {
    pub looks: u32,
    pub opens: u32,
    resumes: Vec<Cursor>,
}

/// What `context.history` reads: the core, once built, and each turn's use.
#[derive(Default)]
pub struct Board {
    core: OnceLock<Weak<Core>>,
    turns: Mutex<HashMap<String, Use>>,
}

/// What a call does to its turn's counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Look,
    Open,
}

impl Board {
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// `turn`'s use before this call, and this call counted. Past [`TURNS_KEPT`] turns, only turns `done`
    /// says have finished are let go: a running turn's counts are never dropped.
    pub fn count(&self, turn: &str, what: Count, done: &dyn Fn(&str) -> bool) -> Use {
        let mut m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= TURNS_KEPT && !m.contains_key(turn) {
            m.retain(|t, _| !done(t));
        }
        let u = m.entry(turn.into()).or_default();
        let was = u.clone();
        match what {
            Count::Look => u.looks = u.looks.saturating_add(1),
            Count::Open => u.opens = u.opens.saturating_add(1),
        }
        was
    }

    /// An opaque token for `at`, kept for `turn`.
    fn keep(&self, turn: &str, at: Cursor) -> Cursor {
        let mut m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        let u = m.entry(turn.into()).or_default();
        u.resumes.push(at);
        Cursor {
            at_ms: 0,
            turn_id: format!("{RESUME}{}", u.resumes.len() - 1),
        }
    }

    /// The raw position a caller's `before` names: a token of this turn's, or the cursor itself.
    fn position(&self, turn: &str, before: Option<&Cursor>) -> Result<Option<Cursor>, String> {
        let Some(c) = before else { return Ok(None) };
        let Some(i) = c.turn_id.strip_prefix(RESUME).filter(|_| c.at_ms == 0) else {
            return Ok(Some(c.clone()));
        };
        let m = self.turns.lock().unwrap_or_else(|e| e.into_inner());
        i.parse::<usize>()
            .ok()
            .and_then(|i| m.get(turn)?.resumes.get(i).cloned())
            .map(Some)
            .ok_or_else(|| "that token belongs to another turn: call again without before".into())
    }

    /// The call, in its own task.
    pub fn run(self: &Arc<Self>, tc: &crate::toolrun::TurnCtx<'_>, input: &Value) -> AsyncMediaRun {
        let fail = |why: &str| -> AsyncMediaRun {
            Box::pin(std::future::ready(Err(ToolFailure::new(why))))
        };
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return fail("context.history is not ready: the core is not built");
        };
        let a = match parse(input) {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
        let pov = pov_of(&core, tc.session_id, tc.turn_id, tc.dir, tc.class);
        let (board, session, turn) = (
            self.clone(),
            tc.session_id.to_string(),
            tc.turn_id.to_string(),
        );
        Box::pin(async move {
            answer(&core, &board, (&session, &turn), &pov, &a)
                .await
                .map(|o| {
                    let external = marker(&o);
                    (o, external, None)
                })
                .map_err(ToolFailure::new)
        })
    }
}

/// Where a turn is anchored: where its message arrived; else where its reply is delivered
/// (`Outbox::speaks_at`); none when that route cannot be read.
pub fn anchor(
    core: &Core,
    session: &str,
    ex: Option<&Exchange>,
    dir: Option<&str>,
) -> Option<String> {
    if let Some(e) = ex.filter(|e| e.opened_by == Opener::Message) {
        return (e.place != exchange::rebuild::UNKNOWN).then(|| e.place.clone());
    }
    core.runner
        .outbox
        .speaks_at(session)
        .ok()
        .map(|t| exchange::place_of(t.as_deref(), dir))
}

/// The turn's point of view: where it is anchored, and who wrote its message. A task's or a self session's
/// anchor is the one its view is assembled under (`view_gate::Verified`). Every reader reads every place, so
/// its presence is the widest (`Open`); `_class` is read no more, and goes with the sweep.
pub(crate) fn pov_of(
    core: &Core,
    session: &str,
    turn: &str,
    dir: Option<&str>,
    _class: crate::places::PlaceClass,
) -> PointOfView {
    let ex = exchange::exchange(&core.store, turn).ok().flatten();
    if crate::view_gate::worker(&core.runner, session) {
        return worker_pov(core, session, ex.as_ref());
    }
    let here = anchor(core, session, ex.as_ref(), dir);
    let person = ex
        .as_ref()
        .filter(|e| e.opened_by == Opener::Message)
        .and_then(|e| e.person.clone());
    PointOfView {
        anchor: Anchor::Place(here.unwrap_or_else(|| exchange::rebuild::UNKNOWN.into())),
        person,
        present: Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    }
}

/// A task's or a self session's point of view: its view's anchor (`view_gate::Verified`).
fn worker_pov(core: &Core, session: &str, ex: Option<&Exchange>) -> PointOfView {
    let v = crate::view_gate::Verified::of_session(&core.runner, session);
    let v = v.pov();
    PointOfView {
        anchor: Anchor::Place(v.anchor.clone()),
        person: (ex.filter(|e| e.opened_by == Opener::Message)).and_then(|e| e.person.clone()),
        present: Present::Open,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    }
}

/// The self loop's scope of `session`'s history read (`view_gate::Verified`, as its view reads it): for a
/// session the self loop opened, its own place at its verified anchor and the run it was handed as a fork
/// (read fallibly: a run that cannot be read fails the call); none for any other session, which reads every
/// place. A scope that cannot be read fails the call plainly, as it fails a turn's view: never the widest.
fn self_scope(runner: &TurnRunner, session: &str) -> Result<Option<page::SelfScope>, String> {
    let v = crate::view_gate::Verified::of_session(runner, session);
    if v.scope_unread() {
        return Err(
            "context.history reads nothing here: whether the self loop opened this session \
                    cannot be read"
                .into(),
        );
    }
    if !v.own_place_only() {
        return Ok(None);
    }
    let handed =
        crate::fork::window::inherited(&runner.store, session).map_err(|e| format!("{e:#}"))?;
    Ok(Some(page::SelfScope {
        session: session.into(),
        anchor: v.pov().anchor.clone(),
        handed: handed.into_iter().map(|e| e.turn_id).collect(),
    }))
}

/// Whether `turn`'s exchange has ended.
fn ended(core: &Core, turn: &str) -> bool {
    exchange::exchange(&core.store, turn)
        .ok()
        .flatten()
        .is_some_and(|e| e.end_ms.is_some())
}

/// A call's answer, for `session`'s `turn` seen from `pov`.
pub(crate) async fn answer(
    core: &Arc<Core>,
    board: &Board,
    (session, turn): (&str, &str),
    pov: &PointOfView,
    a: &HistoryArgs,
) -> Result<ToolOutput, String> {
    let Anchor::Place(here) = &pov.anchor else {
        return Err("context.history reads a place's history, and this turn has none".into());
    };
    // The turn's allowance is counted first: a call whose lens cannot be built has used its look or open.
    let allowance = Allowance::Turn { board, turn };
    let used = count(core, &allowance, a);
    let in_view = page::in_view(&core.store, session, here).map_err(|e| format!("{e:#}"))?;
    let scope = self_scope(&core.runner, session)?;
    let lens = lens_for(core, here, in_view, (Some(session), scope), a, pov.now_ms)?;
    let span = lens.span.clone();
    Ok(match read(core, lens, &allowance, used, a).await? {
        Read::OpensUsed { opens } => ToolOutput {
            text: format!(
                "This turn's allowance of opens is used: {OPENS_PER_TURN} exchanges opened. Do not open \
                 more this turn: tell the person what you found and offer to read further."
            ),
            meta: json!({"open": null, "opens": opens, "of": OPENS_PER_TURN}),
        },
        Read::Opened {
            handle,
            opened,
            opens,
        } => {
            let mut meta = json!({
                "open": page::flat(&handle), "session": opened.session,
                "opens": opens, "of": OPENS_PER_TURN,
                "outside": outside_of(opened.outside, handle.trim()),
            });
            meta[crate::copies::KEY] = json!(opened.sources);
            ToolOutput {
                meta,
                text: opened.text,
            }
        }
        Read::Page {
            walked,
            looks,
            resume,
            imported,
        } => {
            let p = walked.page;
            // An import search's page holds its own size of hits.
            let size = match imported {
                Some(_) => imports::PAGE,
                None => PAGE_EXCHANGES as u64,
            };
            let mut meta = json!({
                "lines": p.lines.len(), "end": p.end, "next": p.next.as_ref().or(resume.as_ref()),
                "look": looks + 1, "of": LOOKS_PER_TURN, "page": size,
                "outside": outside_of(page::shows_another(&p), read_of(a, here)),
            });
            // An import search's page shows imported messages, which no exchange holds: its own sources.
            let (text, sources) = match imported {
                Some(f) => (
                    imports::render(&p, a, span.as_ref(), f.why.as_deref()),
                    f.nodes,
                ),
                None => (
                    page::render(&p, (here, span.as_ref()), looks, resume.as_ref()),
                    page_sources(core, &p)?,
                ),
            };
            meta[crate::copies::KEY] = json!(sources);
            ToolOutput { text, meta }
        }
    })
}

/// What a call may do: a turn's allowance (its looks and opens, counted on the board, which keeps its resume
/// tokens), or none for a caller with no turn (an MCP client: each call is one page or one open, bounded by the
/// page's size and the reading limit, and nothing is counted across calls).
pub(crate) enum Allowance<'a> {
    Turn { board: &'a Board, turn: &'a str },
    NoTurn,
}

/// The call counted against `allowance`, and the use before it: a turn's looks or opens; none with no turn.
pub(crate) fn count(core: &Core, allowance: &Allowance<'_>, a: &HistoryArgs) -> Use {
    match allowance {
        Allowance::Turn { board, turn } => {
            let done = |t: &str| ended(core, t);
            let what = match a.open.is_some() {
                true => Count::Open,
                false => Count::Look,
            };
            board.count(turn, what, &done)
        }
        Allowance::NoTurn => Use::default(),
    }
}

/// The lens a read takes, for both callers: `here`, where a bare page starts (`in_view`), the span `a` names
/// (as of `now_ms`, `span_of`), the cut of `session` when a session anchors the read (a fork's history holds
/// nothing an ancestor said at or after its point, `fork::cut`; a cut that cannot be read fails the call
/// plainly), and the asker's self-loop `scope` its caller read ([`self_scope`] for the model's call; none for
/// an MCP client, which is not the self loop). With no anchoring session the cut is empty.
pub(crate) fn lens_for<'a>(
    core: &'a Core,
    here: &str,
    in_view: Option<Cursor>,
    (session, scope): (Option<&str>, Option<page::SelfScope>),
    a: &HistoryArgs,
    now_ms: u64,
) -> Result<page::Lens<'a>, String> {
    let span = span_of(a, now_ms)?;
    let cut = match session {
        Some(s) => crate::fork::cut::Cut::of(&core.store, s).map_err(|e| format!("{e:#}"))?,
        None => Default::default(),
    };
    Ok(page::Lens {
        store: &core.store,
        here: here.to_string(),
        in_view,
        words: None,
        span,
        cut,
        scope,
    })
}

/// What `session`'s model opens through: its fork's cut and its self-loop scope ([`self_scope`]), as [`answer`]
/// reads them for an `open`; the rest of a lens only shapes a page or an open's heading. A recalled note names its
/// exchange's handle only where this lens opens it (`turn::view_step::recall`).
pub(crate) fn open_lens<'a>(
    runner: &'a TurnRunner,
    session: &str,
) -> Result<page::Lens<'a>, String> {
    Ok(page::Lens {
        store: &runner.store,
        here: String::new(),
        in_view: None,
        words: None,
        span: None,
        cut: crate::fork::cut::Cut::of(&runner.store, session).map_err(|e| format!("{e:#}"))?,
        scope: self_scope(runner, session)?,
    })
}

/// A call's answer before it is said: an opened exchange or session ([`open_any`]), a turn's opens used, or a
/// page. `resume` is a turn's opaque token, or with no turn the raw position a walk stopped by its
/// bound resumes at.
pub(crate) enum Read {
    Opened {
        handle: String,
        opened: OpenedAny,
        opens: u32,
    },
    OpensUsed {
        opens: u32,
    },
    Page {
        walked: page::Walked,
        looks: u32,
        resume: Option<Cursor>,
        /// An import search's page ([`imports`]): the imported nodes whose words its lines show, and why the
        /// search may be incomplete.
        imported: Option<imports::Found>,
    },
}

/// The history tool's one read, for every caller: `a` over `lens`, within `allowance`, `used` being the use
/// before this call ([`count`]).
pub(crate) async fn read(
    core: &Arc<Core>,
    mut lens: page::Lens<'_>,
    allowance: &Allowance<'_>,
    used: Use,
    a: &HistoryArgs,
) -> Result<Read, String> {
    if let Some(h) = &a.open {
        if used.opens >= OPENS_PER_TURN {
            return Ok(Read::OpensUsed { opens: used.opens });
        }
        // A `ses_…` id opens that session, a page of it, `before` paging back (`session`); any other handle
        // one exchange.
        return Ok(Read::Opened {
            handle: h.0.clone(),
            opened: open_any(&lens, h, a.before.as_ref())?,
            opens: used.opens + 1,
        });
    }
    let before = match allowance {
        Allowance::Turn { board, turn } => board.position(turn, a.before.as_ref())?,
        Allowance::NoTurn => a.before.clone(),
    };
    // The imported conversations, searched by words: not a timeline the walk reads (`imports`).
    if let Some(book) = imports::named(a) {
        let (page, found) =
            imports::read(core, a, (book, &lens), before.as_ref(), used.looks).await?;
        return Ok(Read::Page {
            walked: page::Walked { page, resume: None },
            looks: used.looks,
            resume: None,
            imported: Some(found),
        });
    }
    let mut stale = false;
    if let Some(w) = a.words.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        let (root, _) = lens.root(a).map_err(|e| format!("{e:#}"))?;
        let scope = lens.scope(&root).map_err(|e| format!("{e:#}"))?;
        let (turns, cut) = found(core, w, scope).await?;
        lens.words = Some(turns);
        stale = cut;
    }
    let mut walked = lens
        .walk(a, before, used.looks)
        .map_err(|e| format!("{e:#}"))?;
    // Hits no longer on record took places in the search's bound: what lay below them may be missing, so the
    // search is never told as the end of history (D6 round 3, F).
    if stale && walked.page.end == PageEnd::HistoryExhausted {
        walked.page.end = PageEnd::SearchStale;
    }
    let resume = match (allowance, walked.resume.take()) {
        (Allowance::Turn { board, turn }, Some(at)) => Some(board.keep(turn, at)),
        (_, at) => at,
    };
    Ok(Read::Page {
        walked,
        looks: used.looks,
        resume,
        imported: None,
    })
}

/// The turns the words index finds for `words` among `scope`, the sessions on the timeline walked: the search
/// is scoped to them before it ranks anything. And whether the search may be incomplete: a hit it admitted gave
/// no live turn (gone, erased, or outside the scope: a stale index entry) while its bound was reached (the most
/// it asks, or the pack's budget dropped some), so a live hit below the stale ones may have been left out. The
/// pack is bounded before each hit's node is read, and nothing refills it.
pub(crate) async fn found(
    core: &Arc<Core>,
    words: &str,
    scope: HashSet<String>,
) -> Result<(HashSet<String>, bool), String> {
    if scope.is_empty() {
        return Ok((HashSet::new(), false));
    }
    let memory = &core.runner.memory;
    let deadline = Duration::from_millis(memory.cfg().recall_deadline_ms).max(DEADLINE);
    let mut sessions: Vec<String> = scope.iter().cloned().collect();
    sessions.sort();
    let mut begun = memory.begin_within(
        words.to_string(),
        None,
        WORDS_K,
        MemoryArm::Baseline,
        deadline,
        None,
        sessions,
    );
    begun.max_items = Some(WORDS_K);
    let answer = begun.answer().await;
    let scene = crate::recall::Scene {
        mode: "context.history",
        session_id: None,
        turn_id: None,
        place: Place::Private,
        in_context: Default::default(),
        labeled: memory.labeled(&core.store).map_err(|e| format!("{e:#}"))?,
        budget_tokens: None,
        max_items: None,
        science: memory.science_for(MemoryArm::Baseline),
        persona: None,
        category: None,
    };
    let m = memory.manifest(
        &scene,
        &begun,
        answer,
        |s| core.runner.place_of(s),
        |ids| crate::recall::links(&core.store, ids),
        true,
    );
    if m.outcome != "ran" && m.outcome != crate::recall::WORDS_ONLY {
        return Err(format!(
            "the words index did not answer ({}): call again without words to read the history in order",
            m.outcome
        ));
    }
    let mut turns = HashSet::new();
    let mut discarded = 0usize;
    for i in &m.admitted {
        let live = scope
            .contains(&i.session_id)
            .then(|| core.store.get_node(&i.node_id).ok().flatten())
            .flatten()
            .filter(|(_, n)| !matches!(n.body, crate::node::Body::Erased { .. }))
            .and_then(|(_, n)| n.turn_id);
        match live {
            Some(t) => {
                turns.insert(t);
            }
            None => discarded += 1,
        }
    }
    let bound = m.admitted.len() >= WORDS_K
        || m.candidates >= WORDS_K as u64
        || m.drops.get("budget").is_some_and(|n| *n > 0);
    Ok((turns, discarded > 0 && bound))
}
