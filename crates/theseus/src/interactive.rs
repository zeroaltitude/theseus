//! `theseus watch` inside herdr, and `theseus watch --interactive`
//! (theseus-l1l, design `stage2` §2.10). Both follow one session and print
//! what `theseus watch` prints. Inside a herdr pane the watch also reports the
//! session's state to its pane (`herdr::Reporter`), and releases it on exit:
//! Ctrl-C, SIGTERM, a hangup, the daemon closing the connection, the end of
//! stdin, or a panic. With `--interactive` it reads lines: a question waiting
//! asks `approve? [y/N/t/note]`, and any other line is sent to the session as a
//! message. Line mode, no raw terminal, so it works in any pane.
//!
//! A plain `theseus watch` outside herdr is `cmd::watch`, unchanged.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_client::{render, resolve, CallError, Conn};
use theseus_protocol::{
    method, notify, ConfirmRequest, Event, Id, Message, Notification, Response, SessionListResult,
    SessionWaitResult,
};
use tokio::io::{AsyncBufReadExt, BufReader, Lines, Stdin};
use tokio::signal::unix::{signal, SignalKind};

use crate::herdr::{self, Reporter};
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
}

/// `theseus watch [SESSION] [--interactive] [--no-herdr]`.
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
    // Subscribe first, then read, so no change falls between.
    call(
        conn,
        method::SESSION_WATCH,
        json!({ "session_id": sid }),
        &mut early,
    )
    .await?;
    // The session's view and its questions, whole, in one read. The watch
    // above seeds the daemon's push itself (theseus-tq04), so this read no
    // longer brings `execution.changed`; it stays as the cheapest way to
    // get the state this watch starts from, and `events.lost` repeats it.
    let first: SessionWaitResult =
        serde_json::from_value(call(conn, method::SESSION_WAIT, reread(&sid), &mut early).await?)?;
    let info = serde_json::from_value::<SessionListResult>(
        call(
            conn,
            method::SESSION_LIST,
            json!({ "ids": [sid] }),
            &mut early,
        )
        .await?,
    )?
    .sessions
    .into_iter()
    .find(|s| s.session_id == sid);
    // An unknown session is refused, never watched (theseus-0n1v).
    let Some(info) = info else {
        anyhow::bail!("no session is named `{sid}`");
    };
    let (turns, kind) = (info.turns, info.kind);
    let (label, title, profile) = (info.label, info.title, info.profile);

    let mut w = Watch {
        sid: sid.clone(),
        json,
        interactive,
        printer: Printer::new(Mode::Watch, thinking),
        reporter: pane.map(|env| {
            herdr::release_on_panic(&env);
            Reporter::start(env, &sid, kind, label, title)
        }),
        questions: Vec::new(),
        asked: None,
        resolved: HashSet::new(),
        pending: HashMap::new(),
        profile,
        title_asked_at: 0,
        profile_read_at: turns,
    };
    eprintln!("watching {sid} (Ctrl-C to stop)");
    if let Some(r) = &w.reporter {
        eprintln!("reporting its state to herdr's pane {}", r.pane_id());
    }
    if interactive {
        eprintln!("a line you type is sent to the session; a question asks approve? [y/N/t/note]");
    }
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

    let mut stdin = interactive.then(|| BufReader::new(tokio::io::stdin()).lines());
    loop {
        tokio::select! {
            msg = conn.next() => match msg? {
                // The daemon closed the connection.
                None => break,
                Some(Message::Notification(n)) => w.on_note(conn, n).await?,
                Some(Message::Response(r)) => w.on_response(r),
                Some(Message::Request(_)) => {}
            },
            line = next_line(&mut stdin) => match line? {
                None => break,
                Some(l) => w.on_line(conn, &l).await?,
            },
            _ = reminder(w.reporter.as_ref().and_then(Reporter::next_due)) => {
                if let Some(r) = &mut w.reporter {
                    r.remind(now_ms());
                }
            }
            _ = term.recv() => break,
            _ = hangup.recv() => break,
            _ = interrupt.recv() => break,
        }
    }
    w.printer.settle();
    if let Some(r) = w.reporter.take() {
        r.release().await;
    }
    Ok(())
}

/// Until the wall clock reaches `at_ms`: a question's reminder for herdr's
/// pane. None waits forever.
async fn reminder(at_ms: Option<u64>) {
    match at_ms {
        Some(at) => {
            let wait = at.saturating_sub(now_ms());
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        }
        None => std::future::pending().await,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
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

/// The next line on stdin; never, without `--interactive`.
async fn next_line(lines: &mut Option<Lines<BufReader<Stdin>>>) -> std::io::Result<Option<String>> {
    match lines {
        Some(l) => l.next_line().await,
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
}

impl Watch {
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
            _ => {}
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
        self.printer.settle();
        let prompt = if q.budget.is_some() {
            "reset and continue? [y/N/note]"
        } else if q.external_text.is_some() {
            "approve? [y/N/t/note] (t: approve, and trust the session again)"
        } else {
            "approve? [y/N/t/note]"
        };
        // A whole line: an event that comes while it waits starts below it.
        eprintln!("  {prompt}");
    }

    /// A line from the terminal: the answer to the question asked, or a
    /// message.
    async fn on_line(&mut self, conn: &mut Conn, line: &str) -> Result<()> {
        if let Some(cid) = self.asked.take() {
            if let Some(i) = self.questions.iter().position(|q| q.correlation_id == cid) {
                let q = self.questions.remove(i);
                let a = parse_answer(line);
                let id = conn
                    .send(
                        method::ACTION_CONFIRM,
                        json!({"correlation_id": cid, "approve": a.approve, "trust": a.trust,
                               "note": a.note, "author": AUTHOR}),
                    )
                    .await?;
                self.pending.insert(id, Purpose::Answer(Box::new(q)));
                return Ok(());
            }
        }
        let text = line.trim();
        if text.is_empty() {
            return Ok(());
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
        if let Some(profile) = &self.profile {
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
            (Purpose::Info, None) => {
                let info = r
                    .result
                    .and_then(|v| serde_json::from_value::<SessionListResult>(v).ok())
                    .and_then(|l| l.sessions.into_iter().find(|s| s.session_id == self.sid));
                if let Some(profile) = info.as_ref().and_then(|s| s.profile.clone()) {
                    self.profile = Some(profile);
                }
                if let Some(rep) = &mut self.reporter {
                    rep.set_title(info.and_then(|s| s.title));
                }
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

#[cfg(test)]
mod tests {
    use super::*;

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
