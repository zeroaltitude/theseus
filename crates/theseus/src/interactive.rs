//! `theseus watch` inside herdr, and `theseus watch --interactive`
//! (theseus-l1l, design `stage2` §2.10). Both follow one session and print
//! what `theseus watch` prints. Inside a herdr pane the watch also reports the
//! session's state to its pane (`herdr::Reporter`), and releases it on exit:
//! Ctrl-C, SIGTERM, a hangup, the end of
//! stdin, or a panic. With `--interactive` it reads lines: a question waiting
//! asks `approve? [y/N/t/note]`, and any other line is sent to the session as a
//! message. Line mode, no raw terminal, so it works in any pane.
//!
//! A plain `theseus watch` outside herdr is `cmd::watch`, unchanged.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_client::{render, resolve, CallError, Conn};
use theseus_protocol::{
    method, notify, ConfirmRequest, Event, Id, Message, Notification, Response, SessionInfo,
    SessionListResult, SessionWaitResult,
};
use tokio::signal::unix::{signal, SignalKind};

mod thread;

use crate::herdr::{self, Reporter};
use crate::keys::{Input, Source};
use crate::print::{Mode, Printer};

/// What answers and messages sent from the terminal carry as their author: the
/// ledger names it. The answer counts as the CLI's (the `cli` channel), as
/// `theseus confirm`'s does.
pub const AUTHOR: &str = "theseus watch";

/// An answer to a question, as a line says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub approve: bool,
    pub trust: bool,
    pub note: Option<String>,
}

/// `y` approves; `t` approves and trusts the session again (it read external
/// text); `n`, or an empty line, declines. Words after any of them are the
/// note. Any other line declines, with the line as its note, so the model is
/// told why.
pub fn parse_answer(line: &str) -> Answer {
    let line = line.trim();
    let (head, rest) = match line.split_once(char::is_whitespace) {
        Some((h, r)) => (h, Some(r.trim().to_string()).filter(|r| !r.is_empty())),
        None => (line, None),
    };
    let (approve, trust) = match head.to_ascii_lowercase().as_str() {
        "y" | "yes" => (true, false),
        "t" | "trust" => (true, true),
        "" | "n" | "no" => (false, false),
        _ => {
            return Answer {
                approve: false,
                trust: false,
                note: Some(line.to_string()),
            }
        }
    };
    Answer {
        approve,
        trust,
        note: rest,
    }
}

/// A line that is only an answer: with no question waiting (it was answered
/// elsewhere a moment ago), it is not sent as a message.
fn only_an_answer(line: &str) -> bool {
    matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "t" | "trust" | "n" | "no"
    )
}

/// What a request this watch sent is for, until its answer comes.
enum Purpose {
    /// An answer to this question.
    Answer(Box<ConfirmRequest>),
    /// A message, as `turn.submit`.
    Turn,
    /// The session again: its title, which a fresh session gets with its
    /// first turn, and the profile its last turn ran on.
    Info,
    /// The view and questions again, after `events.lost`.
    Reread,
    /// `session.typing`: a notice, whose answer says nothing a person needs.
    Typing,
    /// The current thread, read again for the status line (theseus-j0yh).
    ThreadRow,
}

/// How long one try to reach the daemon again may take.
const RECONNECT_WITHIN: Duration = Duration::from_secs(1);

/// The first try again after a lost daemon, doubling to `RECONNECT_MAX`.
const RECONNECT_FIRST: Duration = Duration::from_millis(100);
const RECONNECT_MAX: Duration = Duration::from_secs(2);

/// What the loop does after one step.
enum Step {
    Go,
    /// The daemon went away: the pane keeps its place and tries again.
    Lost,
    /// The terminal's end (stdin, a signal): release the pane and go.
    Stop,
}

/// The session as a watch reads it, at its start and again at each
/// reconnect: subscribe first, then read, so no change falls between.
async fn read_session(
    conn: &mut Conn,
    sid: &str,
    early: &mut Vec<Notification>,
) -> Result<(SessionWaitResult, Option<SessionInfo>)> {
    call(
        conn,
        method::SESSION_WATCH,
        json!({ "session_id": sid }),
        early,
    )
    .await?;
    // The session's view and its questions, whole, in one read. The watch
    // above seeds the daemon's push itself (theseus-tq04), so this read no
    // longer brings `execution.changed`; it stays as the cheapest way to
    // get the state this watch starts from, and `events.lost` repeats it.
    let first: SessionWaitResult =
        serde_json::from_value(call(conn, method::SESSION_WAIT, reread(sid), early).await?)?;
    let info = serde_json::from_value::<SessionListResult>(
        call(conn, method::SESSION_LIST, json!({ "ids": [sid] }), early).await?,
    )?
    .sessions
    .into_iter()
    .find(|s| s.session_id == sid);
    Ok((first, info))
}

/// `theseus watch [SESSION] [--interactive] [--no-herdr]`. A watch in a herdr
/// pane, or an interactive one, over a daemon's socket keeps its place
/// through the daemon's restart (theseus-qb1es).
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub async fn watch(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    thinking: bool,
    interactive: bool,
    no_herdr: bool,
) -> Result<()> {
    let pane = if no_herdr {
        None
    } else {
        herdr::Env::from_env()
    };
    if !interactive && pane.is_none() {
        return crate::cmd::watch(conn, json, session, thinking).await;
    }
    // Handled from here on, so an exit always releases the pane.
    let mut term = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut early = Vec::new();
    // The thread this terminal chose, when no session is named (theseus-j0yh).
    let thread = match (interactive, &session) {
        (true, None) => thread::at_start(conn, &mut early).await,
        _ => None,
    };
    let session = session.or_else(|| thread.as_ref().and_then(|t| t.thread.session_id.clone()));
    let sid = match session {
        // Named whole: the read below finds it, or refuses it; named by an
        // end: by the one rule (theseus-0n1v).
        Some(s) if resolve::whole_session(&s).is_some() => resolve::trimmed(&s).to_string(),
        Some(s) => resolve::existing_session(conn, &s).await?,
        None => serde_json::from_value::<SessionListResult>(
            call(conn, method::SESSION_LIST, json!({ "n": 1 }), &mut early).await?,
        )?
        .sessions
        .first()
        .map(|s| s.session_id.clone())
        .ok_or_else(|| anyhow!("there are no sessions yet"))?,
    };
    let (first, info) = read_session(conn, &sid, &mut early).await?;
    // An unknown session is refused, never watched (theseus-0n1v).
    let Some(info) = info else {
        anyhow::bail!("no session is named `{sid}`");
    };
    let parent = match &info.parent_session_id {
        Some(p) => parent_name(conn, p, &mut early).await,
        None => None,
    };
    let (turns, kind) = (info.turns, info.kind);
    let facts = (
        info.model.clone().or(info.profile.clone()),
        info.created_at_unix_ms,
    );
    let (label, title, profile) = (info.label, info.title, info.profile);

    let mut w = Watch {
        sid: sid.clone(),
        json,
        interactive,
        printer: Printer::new(Mode::Watch, thinking),
        reporter: pane.map(|env| Reporter::start(env, &sid, kind, label, title)),
        questions: Vec::new(),
        asked: None,
        resolved: HashSet::new(),
        pending: HashMap::new(),
        profile,
        title_asked_at: 0,
        profile_read_at: turns,
        typist: Default::default(),
        open_thread: thread
            .as_ref()
            .filter(|t| t.thread.session_id.is_none())
            .map(|t| t.thread.slug.clone()),
        home_sid: thread.is_none().then(|| sid.clone()),
        thread,
        listed: Vec::new(),
        rebind_to: None,
        status_due: false,
    };
    if let Some(r) = &mut w.reporter {
        r.set_facts(facts.0, facts.1, parent);
    }
    eprintln!("watching {sid} (Ctrl-C to stop)");
    if let Some(r) = &w.reporter {
        eprintln!("reporting its state to herdr's pane {}", r.pane_id());
    }
    if interactive {
        eprintln!("a line you type is sent to the session; a question asks approve? [y/N/t/note]");
    }
    // A session that already failed says why, then the watch waits on it.
    w.say_failed(first.execution.as_ref());
    w.apply_read(first);
    for n in early {
        w.on_note(conn, n).await?;
    }
    if interactive {
        let mut out = std::io::stderr().lock();
        for c in &w.questions {
            crate::print::lines(&mut out, &render::confirm_lines(c))?;
        }
    }
    w.ask();
    if w.thread.is_some() {
        w.print_status();
    }

    let mut stdin = interactive.then(Source::open);
    // Where to come back to; a spawned daemon has no socket, and its end is the watch's.
    let socket = conn.socket_path().map(std::path::Path::to_path_buf);
    loop {
        let step: Result<Step> = tokio::select! {
            msg = conn.next() => match msg {
                // The daemon closed the connection, or it broke.
                Ok(None) | Err(_) => Ok(Step::Lost),
                Ok(Some(Message::Notification(n))) => w.on_note(conn, n).await.map(|()| Step::Go),
                Ok(Some(Message::Response(r))) => {
                    w.on_response(r);
                    w.follow(conn).await.map(|()| Step::Go)
                }
                Ok(Some(Message::Request(_))) => Ok(Step::Go),
            },
            input = next_input(&mut stdin) => match input {
                Ok(Input::Eof) => Ok(Step::Stop),
                Ok(Input::Typing) => w.on_typing(conn).await.map(|()| Step::Go),
                Ok(Input::Line(l)) => w.on_line(conn, &l).await.map(|()| Step::Go),
                Err(e) => return Err(e.into()),
            },
            _ = reminder(w.reporter.as_ref().and_then(Reporter::next_due)) => {
                if let Some(r) = &mut w.reporter {
                    r.remind(herdr::now_ms());
                }
                Ok(Step::Go)
            }
            _ = term.recv() => Ok(Step::Stop),
            _ = hangup.recv() => Ok(Step::Stop),
            _ = interrupt.recv() => Ok(Step::Stop),
        };
        let step = match (step, &socket) {
            (Ok(s), _) => s,
            // A write to a daemon that went away fails: the same as its close.
            (Err(_), Some(_)) => Step::Lost,
            (Err(e), _) => return Err(e),
        };
        match step {
            Step::Go => {}
            Step::Stop => break,
            Step::Lost => {
                let Some(socket) = &socket else { break };
                let back = tokio::select! {
                    r = w.reconnect(conn, socket) => r,
                    _ = term.recv() => Ok(false),
                    _ = hangup.recv() => Ok(false),
                    _ = interrupt.recv() => Ok(false),
                };
                if !back? {
                    break;
                }
            }
        }
    }
    w.printer.settle();
    if let Some(r) = w.reporter.take() {
        r.release().await;
    }
    Ok(())
}

/// The name of the session a task was opened under, for the pane's `parent`
/// token: one read, and none if it fails.
async fn parent_name(
    conn: &mut Conn,
    parent: &str,
    early: &mut Vec<Notification>,
) -> Option<String> {
    let list = call(
        conn,
        method::SESSION_LIST,
        json!({ "ids": [parent] }),
        early,
    )
    .await
    .ok()?;
    let list = serde_json::from_value::<SessionListResult>(list).ok()?;
    list.sessions
        .iter()
        .find(|s| s.session_id == parent)
        .map(theseus_client::names::of)
}

/// Until the wall clock reaches `at_ms`: a question's reminder for herdr's
/// pane. None waits forever.
async fn reminder(at_ms: Option<u64>) {
    match at_ms {
        Some(at) => {
            let wait = at.saturating_sub(herdr::now_ms());
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        }
        None => std::future::pending().await,
    }
}

/// `session.wait` as a read: settled or not, it answers at once with the
/// session's view and its questions.
fn reread(sid: &str) -> Value {
    json!({ "session_id": sid, "until": "settled", "timeout_ms": 0 })
}

/// A request and its answer; the notifications that come first are kept, to
/// be shown after it.
async fn call(
    conn: &mut Conn,
    m: &str,
    params: Value,
    early: &mut Vec<Notification>,
) -> Result<Value> {
    conn.call(m, params, |m, p| early.push(Notification::new(m, p)))
        .await
}

/// The next key or line on stdin; never, without `--interactive`.
async fn next_input(source: &mut Option<Source>) -> std::io::Result<Input> {
    match source {
        Some(s) => s.next().await,
        None => std::future::pending().await,
    }
}

/// One watch's state.
struct Watch {
    sid: String,
    json: bool,
    interactive: bool,
    printer: Printer,
    reporter: Option<Reporter>,
    /// The session's questions, the first asked first.
    questions: Vec<ConfirmRequest>,
    /// The question the prompt asks.
    asked: Option<String>,
    /// Questions answered or withdrawn: a refused answer never brings one back.
    resolved: HashSet<String>,
    pending: HashMap<Id, Purpose>,
    /// The profile the session's last turn ran on: a message continues the
    /// session on it, and not on the daemon's live profile (theseus-nu3z). None
    /// for a session that has not run a turn yet.
    profile: Option<String>,
    /// The turns the session had when its title was last asked for.
    title_asked_at: u64,
    /// The turns it had when `profile` was last read.
    profile_read_at: u64,
    /// Whether the daemon has been told someone types (theseus-tnky).
    typist: theseus_protocol::warm::Typist,
    /// The current thread (theseus-j0yh): none is the home conversation.
    thread: Option<theseus_protocol::threads::ThreadRow>,
    /// The slug of a thread with no session yet: the next message opens it.
    open_thread: Option<String>,
    /// The session the watch left when a thread was chosen, to go back to.
    home_sid: Option<String>,
    /// The last `/t` listing, which `/t N` counts in.
    listed: Vec<theseus_protocol::threads::ThreadRow>,
    /// A session to move to once the response in hand is dealt with.
    rebind_to: Option<String>,
    /// The thread's row is due again for the status line.
    status_due: bool,
}

impl Watch {
    /// A session that already failed says why in words, once, at the start.
    fn say_failed(&self, view: Option<&theseus_protocol::ExecutionView>) {
        let Some(v) = view.filter(|v| v.state == "failed") else {
            return;
        };
        let why = v
            .ended_reason
            .as_deref()
            .filter(|r| !r.trim().is_empty())
            .unwrap_or("no reason was recorded");
        eprintln!("{} failed: {why}", self.sid);
    }

    /// The daemon went away. The pane keeps its place: its label reads
    /// `· reconnecting` and it is never released. It tries again at 100 ms,
    /// doubling to 2 s, with nothing but a timer between tries; then it
    /// reads the session once, and says its state and tokens again. False if
    /// it was ended meanwhile.
    async fn reconnect(&mut self, conn: &mut Conn, socket: &std::path::Path) -> Result<bool> {
        self.printer.settle();
        eprintln!("theseus: the daemon went away; this pane waits for it and keeps its place");
        if let Some(r) = &mut self.reporter {
            r.set_reconnecting(true);
        }
        // Answers that were due on the old connection never come.
        self.pending.clear();
        self.asked = None;
        let mut wait = RECONNECT_FIRST;
        let (mut fresh, first, info, mut early) = loop {
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(RECONNECT_MAX);
            let Ok(mut c) =
                Conn::socket_within(&socket.to_string_lossy(), Some(RECONNECT_WITHIN)).await
            else {
                continue;
            };
            let mut early = Vec::new();
            // A daemon that dies again mid-read is tried again.
            if let Ok((first, info)) = read_session(&mut c, &self.sid, &mut early).await {
                break (c, first, info, early);
            }
        };
        std::mem::swap(conn, &mut fresh);
        eprintln!("theseus: the daemon is back; watching {} again", self.sid);
        self.typist = Default::default();
        if let Some(r) = &mut self.reporter {
            r.set_reconnecting(false);
        }
        if let Some(info) = info {
            self.profile = info.profile.clone().or(self.profile.take());
            self.title_asked_at = self.title_asked_at.max(info.turns);
            self.profile_read_at = self.profile_read_at.max(info.turns);
            if let Some(rep) = &mut self.reporter {
                rep.set_facts(info.model.or(info.profile), info.created_at_unix_ms, None);
                rep.set_title(info.title);
            }
        }
        self.apply_read(first);
        for n in early.drain(..) {
            self.on_note(conn, n).await?;
        }
        if self.interactive {
            let mut out = std::io::stderr().lock();
            for c in &self.questions {
                crate::print::lines(&mut out, &render::confirm_lines(c))?;
            }
        }
        self.ask();
        Ok(true)
    }

    /// A read of the view and questions: the first, or one after
    /// `events.lost`.
    fn apply_read(&mut self, r: SessionWaitResult) {
        if let Some(rep) = &mut self.reporter {
            match &r.execution {
                Some(v) => rep.observe(v),
                None => rep.observe_none(),
            }
        }
        self.questions = r
            .confirms
            .into_iter()
            .filter(|c| c.session_id == self.sid && !self.resolved.contains(&c.correlation_id))
            .collect();
        if self
            .asked
            .as_ref()
            .is_some_and(|a| !self.questions.iter().any(|q| q.correlation_id == *a))
        {
            self.asked = None;
        }
    }

    /// One notification: shown as `theseus watch` shows it, then read for the
    /// session's state and questions.
    async fn on_note(&mut self, conn: &mut Conn, n: Notification) -> Result<()> {
        let (printed, was_asked) = (self.printer.lines_printed(), self.asked.clone());
        if self.json {
            println!("{}", serde_json::to_string(&n)?);
        } else if n.method == notify::EVENTS_LOST {
            let lost: theseus_protocol::EventsLost = serde_json::from_value(n.params.clone())?;
            self.printer.settle();
            eprintln!(
                "{} · `theseus history {}` shows what happened",
                render::lost_line(&lost),
                self.sid
            );
        } else {
            self.printer.on(&n.method, &n.params);
        }
        match Event::from_notification(&n.method, &n.params)
            .ok()
            .flatten()
        {
            Some(Event::ExecutionChanged(v)) if v.session_id == self.sid => {
                let wants_title = self.reporter.as_mut().is_some_and(|rep| {
                    rep.observe(&v);
                    rep.wants_title()
                });
                // Read again at each new turn: for the pane, until the session
                // has a title (it may not have one yet at its first turn's
                // start); and for a watch that sends messages, the profile the
                // turn ran on, which the next message continues on, whoever
                // ran it (theseus-nu3z).
                let new_title = wants_title && v.turns > self.title_asked_at;
                let new_profile = self.interactive && v.turns > self.profile_read_at;
                if new_title || new_profile {
                    self.title_asked_at = self.title_asked_at.max(v.turns);
                    self.profile_read_at = self.profile_read_at.max(v.turns);
                    let id = conn
                        .send(method::SESSION_LIST, json!({ "ids": [self.sid] }))
                        .await?;
                    self.pending.insert(id, Purpose::Info);
                }
            }
            Some(Event::ConfirmRequested(c)) if c.session_id == self.sid => {
                if !self
                    .questions
                    .iter()
                    .any(|q| q.correlation_id == c.correlation_id)
                {
                    self.questions.push(c);
                }
                self.ask();
            }
            Some(Event::ConfirmResolved(r)) if r.session_id == self.sid => {
                self.resolved.insert(r.correlation_id.clone());
                self.questions
                    .retain(|q| q.correlation_id != r.correlation_id);
                if self.asked.as_deref() == Some(r.correlation_id.as_str()) {
                    self.asked = None;
                }
                self.ask();
            }
            Some(Event::EventsLost(_)) => {
                let id = conn.send(method::SESSION_WAIT, reread(&self.sid)).await?;
                self.pending.insert(id, Purpose::Reread);
            }
            // A call's events say what the pane's session is doing now.
            Some(ev) => {
                if let Some(rep) = &mut self.reporter {
                    rep.on_event(&ev);
                }
            }
            None => {}
        }
        // A question still waits and this event printed below its prompt (a
        // parked turn's footer comes after its question): the prompt goes
        // last again, so the line to answer on is the one the terminal ends on.
        let below = self.printer.lines_printed() != printed;
        if self.interactive && self.asked.is_some() && self.asked == was_asked && below {
            self.say_prompt();
        }
        Ok(())
    }

    /// Ask the first question waiting, if none is asked yet.
    fn ask(&mut self) {
        if !self.interactive || self.asked.is_some() {
            return;
        }
        let Some(q) = self.questions.first() else {
            return;
        };
        self.asked = Some(q.correlation_id.clone());
        self.say_prompt();
    }

    /// The prompt of the question asked, as a whole line below whatever was
    /// printed last: the prompt is what the terminal ends on while a question
    /// waits (theseus-rn7s).
    fn say_prompt(&mut self) {
        let Some(q) = self
            .questions
            .iter()
            .find(|q| Some(&q.correlation_id) == self.asked.as_ref())
        else {
            return;
        };
        self.printer.settle();
        let prompt = if q.budget.is_some() {
            "reset and continue? [y/N/note]".to_string()
        } else if q.external_text.is_some() {
            "approve? [y/N/t/note] (t: approve, and trust the session again)".to_string()
        } else if let Some(o) = &q.grant_offer {
            // "Yes, and don't ask again" (theseus-drjs).
            format!("approve? [y/N/{k}/note] ({k}: {})", o.words, k = o.key())
        } else {
            "approve? [y/N/t/note]".to_string()
        };
        // Under the card, in a herdr pane: links that open the answer card, never answer.
        if let Some(l) =
            crate::herdr_keys::links_under(q, std::env::var("HERDR_ENV").ok().as_deref())
        {
            eprintln!("  {l}");
        }
        // A whole line: an event that comes while it waits starts below it.
        eprintln!("  {prompt}");
    }

    /// The first key of a line: one `session.typing` per idle spell, so the
    /// daemon warms what the message will wait for. Not while a question
    /// waits (the line is an answer), and never waited on (theseus-tnky).
    async fn on_typing(&mut self, conn: &mut Conn) -> Result<()> {
        if self.asked.is_some() || !self.typist.keystroke(&self.sid, herdr::now_ms()) {
            return Ok(());
        }
        let params = json!({"session_id": self.sid, "author": AUTHOR});
        let id = conn.send(method::SESSION_TYPING, params).await?;
        self.pending.insert(id, Purpose::Typing);
        Ok(())
    }

    /// A line from the terminal: the answer to the question asked, or a
    /// message.
    async fn on_line(&mut self, conn: &mut Conn, line: &str) -> Result<()> {
        if let Some(cid) = self.asked.take() {
            if let Some(i) = self.questions.iter().position(|q| q.correlation_id == cid) {
                let q = self.questions.remove(i);
                let a = parse_answer(line);
                // `a` or `e`: approve, and take the offer, echoed (theseus-drjs).
                let grant = grant_answer(&q, line);
                let params = match &grant {
                    Some(pattern) => json!({"correlation_id": cid, "approve": true,
                                            "grant": {"pattern": pattern}, "author": AUTHOR}),
                    None => json!({"correlation_id": cid, "approve": a.approve, "trust": a.trust,
                                   "note": a.note, "author": AUTHOR}),
                };
                let id = conn.send(method::ACTION_CONFIRM, params).await?;
                self.pending.insert(id, Purpose::Answer(Box::new(q)));
                return Ok(());
            }
        }
        let text = line.trim();
        if text.is_empty() {
            return Ok(());
        }
        if let Some(arg) = thread::command(text) {
            return self.thread_cmd(conn, arg).await;
        }
        if only_an_answer(text) {
            self.printer.settle();
            eprintln!("  nothing waits for an answer now (it was answered or withdrawn): `{text}` was not sent");
            return Ok(());
        }
        // On the profile the session's last turn ran on, as a continuation
        // does: a message sent without one takes the daemon's live profile,
        // and a session started on another model (`ask -P glm`) then changed
        // models under the pane without a word (theseus-nu3z).
        let mut params = json!({"session_id": self.sid, "input": text, "author": AUTHOR});
        if let Some(slug) = &self.open_thread {
            // A thread with no session: this message opens it.
            params = json!({"thread": slug, "input": text, "author": AUTHOR});
        }
        if let (Some(profile), None) = (&self.profile, &self.open_thread) {
            params["profile"] = json!(profile);
            // Carried, not chosen: routing may still move the turn (25e).
            params["carried"] = json!(true);
        }
        // Inside a job, its session (theseus-b5cl).
        if let Some(from) = theseus_client::client::job_session() {
            params["opened_from"] = json!(from);
        }
        let id = conn.send(method::TURN_SUBMIT, params).await?;
        self.pending.insert(id, Purpose::Turn);
        self.print_tag();
        if let Some(rep) = &mut self.reporter {
            rep.working_now();
        }
        Ok(())
    }

    /// The answer to a request this watch sent. A turn's events, its end
    /// included, came before it, so a success shows nothing more.
    fn on_response(&mut self, r: Response) {
        let Some(purpose) = self.pending.remove(&r.id) else {
            return;
        };
        let refused = |e: theseus_protocol::RpcError| {
            CallError {
                code: e.code,
                message: e.message,
                data: e.data,
            }
            .to_string()
        };
        match (purpose, r.error) {
            (Purpose::Answer(q), Some(e)) => {
                self.printer.settle();
                eprintln!(
                    "  the answer to {} was refused: {}",
                    q.correlation_id,
                    refused(e)
                );
                if !self.resolved.contains(&q.correlation_id) {
                    self.questions.insert(0, *q);
                }
                self.ask();
            }
            (Purpose::Turn, Some(e)) => {
                self.printer.settle();
                eprintln!("theseus: the message was not sent: {}", refused(e));
                if let Some(rep) = &mut self.reporter {
                    rep.restore();
                }
            }
            (Purpose::Turn, None) => self.opened(r.result),
            (Purpose::ThreadRow, None) => self.thread_row(r.result),
            (Purpose::Info, None) => {
                let info = r
                    .result
                    .and_then(|v| serde_json::from_value::<SessionListResult>(v).ok())
                    .and_then(|l| l.sessions.into_iter().find(|s| s.session_id == self.sid));
                if let Some(profile) = info.as_ref().and_then(|s| s.profile.clone()) {
                    self.profile = Some(profile);
                }
                if let Some(rep) = &mut self.reporter {
                    if let Some(i) = &info {
                        rep.set_facts(
                            i.model.clone().or(i.profile.clone()),
                            i.created_at_unix_ms,
                            None,
                        );
                    }
                    rep.set_title(info.and_then(|s| s.title));
                }
                self.status_due = self.thread.is_some();
            }
            (Purpose::Reread, None) => {
                if let Some(read) = r
                    .result
                    .and_then(|v| serde_json::from_value::<SessionWaitResult>(v).ok())
                {
                    self.apply_read(read);
                    self.ask();
                }
            }
            _ => {}
        }
    }
}

/// The pattern an answer line takes: the offer's, when the line is its key
/// (`a` for a command, `e` for edits) and the question offers one.
pub fn grant_answer(q: &ConfirmRequest, line: &str) -> Option<String> {
    let o = q.grant_offer.as_ref()?;
    let head = line.trim().to_ascii_lowercase();
    (head.len() == 1 && head.starts_with(o.key())).then(|| o.pattern.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a` takes a command's offer, `e` an edit's, and nothing else does.
    #[test]
    fn a_grant_key_takes_the_offer_it_names() {
        use theseus_protocol::grants::GrantOffer;
        let mut q = ConfirmRequest::default();
        assert_eq!(grant_answer(&q, "a"), None, "no offer, no grant");
        q.grant_offer = Some(GrantOffer::prefix(&[
            "python3".into(),
            "-m".into(),
            "unittest".into(),
        ]));
        assert_eq!(grant_answer(&q, " a "), Some("python3 -m unittest".into()));
        assert_eq!(grant_answer(&q, "e"), None);
        assert_eq!(grant_answer(&q, "y"), None);
        assert_eq!(grant_answer(&q, "a lot"), None);
        q.grant_offer = Some(GrantOffer::edits("/work/app"));
        assert_eq!(grant_answer(&q, "e"), Some("/work/app".into()));
    }

    /// The answers a line gives (design §2.10's `approve? [y/N/t/note]`).
    #[test]
    fn a_line_answers_as_it_says() {
        let a = |approve, trust, note: Option<&str>| Answer {
            approve,
            trust,
            note: note.map(str::to_string),
        };
        assert_eq!(parse_answer("y"), a(true, false, None));
        assert_eq!(parse_answer(" YES "), a(true, false, None));
        assert_eq!(
            parse_answer("y the tide is low"),
            a(true, false, Some("the tide is low"))
        );
        assert_eq!(parse_answer("t"), a(true, true, None));
        assert_eq!(parse_answer(""), a(false, false, None));
        assert_eq!(parse_answer("n"), a(false, false, None));
        assert_eq!(
            parse_answer("no, not today"),
            a(false, false, Some("no, not today"))
        );
        assert_eq!(
            parse_answer("n not today"),
            a(false, false, Some("not today"))
        );
        assert_eq!(
            parse_answer("use the staging port"),
            a(false, false, Some("use the staging port"))
        );
    }

    /// With nothing asked, a bare answer is held back, and anything longer is
    /// a message.
    #[test]
    fn a_bare_answer_is_never_sent_as_a_message() {
        for l in ["y", "N", " t ", "yes", "no", "trust"] {
            assert!(only_an_answer(l), "{l}");
        }
        for l in ["yes, run it", "y?", "tide", "n2"] {
            assert!(!only_an_answer(l), "{l}");
        }
    }
}
