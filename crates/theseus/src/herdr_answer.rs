//! `theseus herdr answer [--pane P | --oldest | CID]` (theseus-pkaq): the answer card, a line-mode
//! screen for a herdr popup. It shows one waiting question whole (the task and its short id, where
//! it came from, the model, the spend against its limit, when it was asked and when it expires, the
//! call and why and its input) and takes the answer from the keys the question's own `options` give:
//! `y` approves, `n` declines (any other words decline
//! with the words as the note), `1`..`6` pick an `ask` question's choices. The answer is one
//! `work.answer` with author `herdr`.
//!
//! Which question: the one named (`CID`), else the one the focused pane's `confirm` token names
//! (`--pane`), else the one that has waited longest. A question answered somewhere else meanwhile
//! is said, and the card moves on or closes. Nothing here is resident: a popup starts on a key.
//!
//! The pure parts (`pick`, `keys`, `card`, `decide`) are tested in `herdr_answer_tests.rs`; the
//! whole command, against a scripted daemon and a fake herdr, in `tests/herdr_answer.rs`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::{json, Value};
use theseus_client::render::{human_secs, time};
use theseus_client::{names, Conn};
use theseus_protocol::answer::WorkAnswerParams;
use theseus_protocol::work::AnswerOption;
use theseus_protocol::{
    method, ConfirmListResult, ConfirmRequest, Event, Message, SessionInfo, SessionListResult,
};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::herdr;

/// What the card's answers carry as their author: the ledger names it.
pub const AUTHOR: &str = "herdr";

/// How long the card waits for the daemon at its start.
const CONNECT_WITHIN: Duration = Duration::from_millis(1500);

/// Which question the card opens on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Want {
    /// A question named by its id, whole or a unique end.
    pub cid: Option<String>,
    /// The focused pane's `confirm` token, if it has one.
    pub focused: Option<String>,
}

/// The result of choosing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The question at this index of the list.
    Found(usize),
    /// A named question that no longer waits.
    Gone(String),
    /// Nothing waits.
    Nothing,
}

/// A question's id matches a name: whole, or an end of at least four characters.
pub fn names_it(name: &str, cid: &str) -> bool {
    !name.is_empty() && (cid == name || (name.chars().count() >= 4 && cid.ends_with(name)))
}

/// The index of the question that has waited longest.
fn oldest(qs: &[ConfirmRequest]) -> Option<usize> {
    (0..qs.len()).min_by_key(|&i| (qs[i].requested_at_ms, qs[i].correlation_id.clone()))
}

/// Which question to show: the one named, else the focused pane's, else the oldest. A name that
/// matches nothing is `Gone` and never replaced by another question: the owner asked for that one.
pub fn pick(qs: &[ConfirmRequest], want: &Want) -> Pick {
    if let Some(name) = want.cid.as_deref() {
        return match qs.iter().position(|q| names_it(name, &q.correlation_id)) {
            Some(i) => Pick::Found(i),
            None => Pick::Gone(name.to_string()),
        };
    }
    let focused = want
        .focused
        .as_deref()
        .and_then(|name| qs.iter().position(|q| names_it(name, &q.correlation_id)));
    match focused.or_else(|| oldest(qs)) {
        Some(i) => Pick::Found(i),
        None => Pick::Nothing,
    }
}

/// The questions in the order `tab` walks them: longest waiting first.
pub fn queue(qs: &[ConfirmRequest]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..qs.len()).collect();
    order.sort_by_key(|&i| (qs[i].requested_at_ms, qs[i].correlation_id.clone()));
    order
}

/// The other questions in the order `tab` walks them from `shown`: the one after it first, the
/// card lists them so (theseus-dowi0.1).
pub fn others_after<'a>(
    qs: &'a [ConfirmRequest],
    shown: &ConfirmRequest,
) -> Vec<&'a ConfirmRequest> {
    let order = queue(qs);
    let at = order
        .iter()
        .position(|&n| qs[n].correlation_id == shown.correlation_id)
        .map_or(0, |p| p + 1);
    order
        .iter()
        .cycle()
        .skip(at)
        .take(order.len())
        .map(|&n| &qs[n])
        .filter(|q| q.correlation_id != shown.correlation_id)
        .collect()
}

/// One answer the card offers: the key that picks it and the option it sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub key: char,
    pub option: String,
    pub label: String,
    pub takes_note: bool,
}

/// The key of an option: `y` approves (or resets, accepts, acknowledges, is done), `n` declines, a
/// choice or a snooze is its number.
fn key_of(option: &str, position: usize) -> char {
    match option {
        "approve" | "reset" | "accept" | "ack" | "done" => 'y',
        "decline" => 'n',
        _ => char::from_digit((position + 1).min(9) as u32, 10).unwrap_or('1'),
    }
}

/// The question's own answers: its `options`, else (a daemon that does not say) the three a call
/// has, worded as `work::answer_options` words them.
fn options_of(c: &ConfirmRequest) -> Vec<AnswerOption> {
    if !c.options.is_empty() {
        return c.options.clone();
    }
    let option = |id: &str, label: &str, note: bool| AnswerOption {
        id: id.into(),
        label: label.into(),
        style: theseus_protocol::work::Style::Plain,
        takes_note: note,
        effect: String::new(),
    };
    let mut out = Vec::new();
    if c.budget.is_some() {
        out.push(option("reset", "Reset to $0", false));
    } else if c.change.is_some() {
        out.push(option("accept", "Accept", false));
    } else {
        out.push(option("approve", "Approve", false));
    }
    out.push(option("decline", "Decline", true));
    out
}

/// The keys of a question, one per option.
pub fn keys(c: &ConfirmRequest) -> Vec<Key> {
    options_of(c)
        .into_iter()
        .enumerate()
        .map(|(i, o)| Key {
            key: key_of(&o.id, i),
            option: o.id,
            label: o.label,
            takes_note: o.takes_note,
        })
        .collect()
}

/// What a typed line means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Answer with this option, and these words.
    Answer {
        option: String,
        note: Option<String>,
    },
    /// The next question.
    Next,
    /// Close the card, answering nothing.
    Quit,
    /// Nothing typed yet: ask again.
    Nothing,
    /// A line that is no answer.
    Unknown(String),
}

/// A line against a question's keys: a key and the words after it are the answer and its note;
/// `tab` or `next` is the next question; `q` closes; other words decline with themselves as the
/// note (the model reads why), as `watch --interactive`'s prompt does; a line of only Esc closes, and
/// one with an Esc among words is no answer. An empty line answers
/// nothing: a stray Enter never declines.
pub fn decide(line: &str, keys: &[Key]) -> Decision {
    let line = line.trim_matches(|c: char| c == '\n' || c == '\r' || c == ' ');
    if line.trim_matches('\t').is_empty() && line.contains('\t') {
        return Decision::Next;
    }
    let line = line.trim();
    if line.is_empty() {
        return Decision::Nothing;
    }
    // Esc closes the card; an Esc inside a line is a key, not words to decline with.
    if line.contains('\x1b') {
        return if line
            .trim_matches(|c: char| c == '\x1b' || c.is_whitespace())
            .is_empty()
        {
            Decision::Quit
        } else {
            Decision::Unknown(line.replace('\x1b', ""))
        };
    }
    let (head, rest) = match line.split_once(char::is_whitespace) {
        Some((h, r)) => (h, Some(r.trim().to_string()).filter(|r| !r.is_empty())),
        None => (line, None),
    };
    let lower = head.to_ascii_lowercase();
    match lower.as_str() {
        "tab" | "next" => return Decision::Next,
        "q" | "quit" | "esc" => return Decision::Quit,
        _ => {}
    }
    let hit = |word: &str| -> Option<&Key> {
        let mut chars = word.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return match word {
                "yes" => keys.iter().find(|k| k.key == 'y'),
                "no" => keys.iter().find(|k| k.key == 'n'),
                _ => None,
            };
        };
        keys.iter().find(|k| k.key == c.to_ascii_lowercase())
    };
    if let Some(k) = hit(&lower) {
        return Decision::Answer {
            option: k.option.clone(),
            note: rest,
        };
    }
    // Other words decline with themselves as the note, if the question can be declined.
    match keys.iter().find(|k| k.option == "decline") {
        Some(k) => Decision::Answer {
            option: k.option.clone(),
            note: Some(line.to_string()),
        },
        None => Decision::Unknown(line.to_string()),
    }
}

/// `work.answer`'s params for a decision: the author is always `herdr`.
pub fn answer_params(question_id: &str, option: &str, note: Option<String>) -> WorkAnswerParams {
    WorkAnswerParams {
        question_id: question_id.to_string(),
        option: option.to_string(),
        note,
        author: Some(AUTHOR.to_string()),
        ..Default::default()
    }
}

/// A clip at `n` characters, with the cut said.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n.saturating_sub(1)).collect();
    format!("{head}…")
}

/// What the card knows of the session that asks, besides the question.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub session: Option<SessionInfo>,
    /// The name of the session the task was opened under.
    pub parent: Option<String>,
    /// The other questions that wait: how many, and a word each.
    pub others: Vec<String>,
}

/// The card, as lines: no escape sequences, so a popup of any size shows it and a test compares it.
pub fn card(c: &ConfirmRequest, ctx: &Context, now_ms: u64) -> Vec<String> {
    let mut out = Vec::new();
    let title = match (&c.task, &ctx.session) {
        (Some(t), _) => t.title.clone().filter(|s| !s.is_empty()),
        (None, Some(s)) => Some(names::of(s)),
        (None, None) => None,
    }
    .unwrap_or_else(|| "a conversation".to_string());
    let short = c
        .task
        .as_ref()
        .map_or_else(|| tail(&c.session_id), |t| t.short.clone());
    out.push(format!("{}  ·  {short}", clip(&title, 60)));
    let from = match (&ctx.parent, &c.task) {
        (Some(p), _) => format!("from {}", clip(p, 40)),
        (None, Some(_)) => "a task of its own".to_string(),
        (None, None) => format!("in {}", tail(&c.session_id)),
    };
    let mut facts = vec![from];
    if let Some(s) = &ctx.session {
        if let Some(m) = s.model.as_ref().or(s.profile.as_ref()) {
            facts.push(m.clone());
        }
        facts.push(match s.limit_usd {
            Some(l) => format!("${:.2} of ${l:.2}", s.cost_usd),
            None => format!("${:.2}", s.cost_usd),
        });
    }
    let asked = now_ms.saturating_sub(c.requested_at_ms) / 1000;
    facts.push(format!("asked {} ago", human_secs(asked)));
    if c.expires_at_ms > 0 {
        facts.push(match c.expires_at_ms.checked_sub(now_ms) {
            Some(left) if left > 0 => format!("expires in {}", human_secs(left / 1000)),
            _ => "expired".to_string(),
        });
        facts.push(format!("at {}", time::fmt_hm(c.expires_at_ms)));
    }
    out.push(facts.join(" · "));
    out.push(String::new());
    match (&c.budget, &c.change) {
        (Some(_), _) => out.push(format!("? {}", c.reason)),
        (None, Some(ch)) => out.push(format!("? {}", ch.question())),
        _ => {
            out.push(format!("? {} waits for you", c.tool));
            out.push(format!("  why: {}", clip(&c.reason, 200)));
            out.push(format!("  input: {}", clip(&c.input.to_string(), 300)));
        }
    }
    if c.floor {
        out.push("  the floor asks: it touches Theseus's own state or the vault".to_string());
    }
    out.push(String::new());
    let ks = keys(c);
    let row: Vec<String> = ks
        .iter()
        .map(|k| format!("[{}] {}", k.key, k.label.to_lowercase()))
        .collect();
    out.push(row.join("  "));
    if ks.iter().any(|k| k.takes_note) {
        out.push("any other words decline, with the words as the note".to_string());
    }
    if !ctx.others.is_empty() {
        out.push(format!(
            "{} more wait: {} [tab] next",
            ctx.others.len(),
            ctx.others.join(", ")
        ));
    }
    out
}

/// The last six characters of an id, which is how the clients say one.
fn tail(id: &str) -> String {
    let id = id.rsplit('_').next().unwrap_or(id);
    let n = id.chars().count();
    id.chars().skip(n.saturating_sub(6)).collect()
}

/// A word for a question in "N more wait": its task's short id or its tool.
fn word_of(c: &ConfirmRequest) -> String {
    match &c.task {
        Some(t) => format!("{} {}", t.short, c.tool),
        None => format!("{} {}", tail(&c.session_id), c.tool),
    }
}

/// How the card learns the focused pane's question: its `confirm` token, from herdr.
pub async fn focused_confirm(socket: &std::path::Path, pane: &str) -> Option<String> {
    let v = herdr::request(
        socket,
        "theseus:answer:pane",
        "pane.get",
        json!({ "pane_id": pane }),
    )
    .await
    .ok()?;
    let tokens = v.pointer("/pane/tokens").or_else(|| v.get("tokens"))?;
    tokens
        .get("confirm")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The arguments of `theseus herdr answer`.
#[derive(Debug, Clone, Default)]
pub struct Args {
    pub pane: Option<String>,
    pub oldest: bool,
    pub cid: Option<String>,
    pub herdr_socket: Option<PathBuf>,
}

/// The questions that wait, whole.
async fn waiting(conn: &mut Conn) -> Result<Vec<ConfirmRequest>> {
    let v = conn.request(method::CONFIRM_LIST, Value::Null).await?;
    Ok(serde_json::from_value::<ConfirmListResult>(v)?.confirms)
}

/// The asking session and its parent, for the card.
async fn context(conn: &mut Conn, qs: &[ConfirmRequest], shown: &ConfirmRequest) -> Context {
    let mut ctx = Context {
        others: others_after(qs, shown).into_iter().map(word_of).collect(),
        ..Context::default()
    };
    let one = |id: String| json!({ "ids": [id] });
    if let Ok(v) = conn
        .request(method::SESSION_LIST, one(shown.session_id.clone()))
        .await
    {
        ctx.session = serde_json::from_value::<SessionListResult>(v)
            .ok()
            .and_then(|l| l.sessions.into_iter().next());
    }
    if let Some(p) = ctx
        .session
        .as_ref()
        .and_then(|s| s.parent_session_id.clone())
    {
        if let Ok(v) = conn.request(method::SESSION_LIST, one(p)).await {
            ctx.parent = serde_json::from_value::<SessionListResult>(v)
                .ok()
                .and_then(|l| l.sessions.into_iter().next())
                .map(|s| names::of(&s));
        }
    }
    ctx
}

/// Whether the card goes on.
enum Flow {
    Go,
    Done,
}

/// The card's state: the connection, the questions that wait, the one shown.
struct Card {
    conn: Conn,
    qs: Vec<ConfirmRequest>,
    at: String,
    shown: Option<String>,
}

impl Card {
    /// The connection, the questions and the first one to show; none to show says why and is done.
    async fn open(socket: &str, a: &Args) -> Result<Option<Self>> {
        let socket = theseus_client::client::tilde(socket, std::env::var("HOME").ok());
        let mut conn = Conn::socket_within(&socket, Some(CONNECT_WITHIN)).await?;
        let qs = waiting(&mut conn).await?;
        let focused = match (&a.pane, a.cid.is_some() || a.oldest) {
            (Some(p), false) => {
                let hs = match &a.herdr_socket {
                    Some(s) => Some(s.clone()),
                    None => crate::herdr_sync::default_socket().ok(),
                };
                match hs {
                    Some(hs) => focused_confirm(&hs, p).await,
                    None => None,
                }
            }
            _ => None,
        };
        let want = Want {
            cid: a.cid.clone(),
            focused,
        };
        let at = match pick(&qs, &want) {
            Pick::Found(i) => qs[i].correlation_id.clone(),
            Pick::Gone(name) => {
                println!("{name} no longer waits: it was answered or withdrawn");
                return Ok(None);
            }
            Pick::Nothing => {
                println!("nothing waits for you");
                return Ok(None);
            }
        };
        Ok(Some(Self {
            conn,
            qs,
            at,
            shown: None,
        }))
    }

    /// The longest-waiting question is the one shown next.
    fn move_on(&mut self) -> Flow {
        match queue(&self.qs).first() {
            Some(&n) => {
                self.at = self.qs[n].correlation_id.clone();
                Flow::Go
            }
            None => Flow::Done,
        }
    }

    /// The card of a question, once.
    async fn show(&mut self, q: &ConfirmRequest) {
        if self.shown.as_deref() == Some(q.correlation_id.as_str()) {
            return;
        }
        let ctx = context(&mut self.conn, &self.qs, q).await;
        // The card's events: this question's session, so its resolution is heard.
        let _ = self
            .conn
            .request(method::SESSION_WATCH, json!({ "session_id": q.session_id }))
            .await;
        println!();
        for l in card(q, &ctx, herdr::now_ms()) {
            println!("{l}");
        }
        if q.may_answer == Some(false) {
            println!("this connection may not answer it: answer where the daemon says");
        }
        self.shown = Some(q.correlation_id.clone());
    }

    /// A line from the popup.
    async fn on_line(&mut self, line: &str, q: &ConfirmRequest) -> Result<Flow> {
        match decide(line, &keys(q)) {
            Decision::Nothing => {}
            Decision::Quit => return Ok(Flow::Done),
            Decision::Unknown(w) => println!("`{w}` is no answer here"),
            Decision::Next => match others_after(&self.qs, q).first() {
                Some(next) => self.at = next.correlation_id.clone(),
                None => println!("no other question waits"),
            },
            Decision::Answer { option, note } => return self.answer(q, &option, note).await,
        }
        Ok(Flow::Go)
    }

    /// One `work.answer`; a refusal is said, and the list read again to see whether the question
    /// was answered elsewhere.
    async fn answer(
        &mut self,
        q: &ConfirmRequest,
        option: &str,
        note: Option<String>,
    ) -> Result<Flow> {
        let params = answer_params(&q.correlation_id, option, note);
        match self.conn.request(method::WORK_ANSWER, params).await {
            Ok(_) => {
                println!("answered {} · {option}", q.correlation_id);
                self.qs.retain(|x| x.correlation_id != q.correlation_id);
                // Others wait: the longest waiting is next, unless the owner closes.
                Ok(self.move_on())
            }
            Err(e) => {
                println!("{e}");
                self.qs = waiting(&mut self.conn).await?;
                if !self.qs.iter().any(|x| x.correlation_id == q.correlation_id) {
                    println!("{} was answered elsewhere meanwhile", q.correlation_id);
                }
                Ok(Flow::Go)
            }
        }
    }

    /// A notification: the shown question resolved elsewhere, or a new one asked.
    fn on_message(&mut self, msg: Result<Option<Message>>, q: &ConfirmRequest) -> Result<Flow> {
        let Ok(Some(Message::Notification(n))) = msg else {
            if matches!(msg, Ok(None) | Err(_)) {
                bail!("the daemon went away");
            }
            return Ok(Flow::Go);
        };
        match Event::from_notification(&n.method, &n.params) {
            Ok(Some(Event::ConfirmResolved(r))) => {
                self.qs.retain(|x| x.correlation_id != r.correlation_id);
                if r.correlation_id == q.correlation_id {
                    let by = r.by.map_or(String::new(), |b| format!(" by {b}"));
                    println!();
                    println!("{} was answered elsewhere{by}", r.correlation_id);
                    return Ok(self.move_on());
                }
            }
            Ok(Some(Event::ConfirmRequested(c)))
                if !self.qs.iter().any(|x| x.correlation_id == c.correlation_id) =>
            {
                self.qs.push(c);
            }
            _ => {}
        }
        Ok(Flow::Go)
    }
}

/// `theseus herdr answer`.
pub async fn run(socket: &str, a: Args) -> Result<()> {
    let Some(mut c) = Card::open(socket, &a).await? else {
        return Ok(());
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        // The shown question went (answered elsewhere): the next one, or the end.
        let Some(q) = c.qs.iter().find(|q| q.correlation_id == c.at).cloned() else {
            if matches!(c.move_on(), Flow::Done) {
                println!("nothing else waits for you");
                return Ok(());
            }
            continue;
        };
        c.show(&q).await;
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let flow = tokio::select! {
            line = lines.next_line() => match line? {
                Some(line) => c.on_line(&line, &q).await?,
                None => Flow::Done,
            },
            msg = c.conn.next() => c.on_message(msg, &q)?,
        };
        if matches!(flow, Flow::Done) {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "herdr_answer_tests.rs"]
mod tests;
