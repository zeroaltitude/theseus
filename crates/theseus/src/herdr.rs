//! The herdr adapter (theseus-l1l, design `stage2` §2.10): what `theseus watch`
//! tells herdr, the terminal workspace manager, about its session. herdr shows
//! every pane as working, blocked, or idle; a program in a pane may report its
//! own state instead of herdr reading its screen, and theseusd knows the state
//! exactly. Nothing in theseusd knows herdr: this module is the whole adapter's
//! herdr side, and it speaks only herdr's documented socket API (its `herdr api
//! schema --json` is the contract).
//!
//! - [`Env`]: a watch inside a herdr pane knows its pane and herdr's socket from
//!   the environment herdr gives every pane.
//! - [`Reporter`]: the session's attention as herdr's state, sent only when it
//!   changes, with a `seq` from the clock, then released on exit.
//! - [`request`]: one request on a fresh connection. herdr's API is
//!   newline-delimited JSON with string ids and results tagged by `type`; it is
//!   not JSON-RPC.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use theseus_protocol::{ExecutionView, Level, SessionKind};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

/// The source of our reports. herdr keeps one status authority per pane, and
/// this names ours: a custom source, so it is in charge of the pane's state.
pub const SOURCE: &str = "custom:theseus";

/// The agent our reports name. herdr knows no agent by this name, so nothing
/// it reads on the screen competes with the report.
pub const AGENT: &str = "theseus";

/// How long one request to herdr may take, from the connect to the answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// A report's retries, after the first try: at 0.5 s, then 1.5 s, as herdr's
/// own reporters retry.
const RETRIES: [Duration; 2] = [Duration::from_millis(500), Duration::from_millis(1500)];

/// How long the exit waits for the release to reach herdr. A herdr that does
/// not answer leaves the pane's last state, which `theseus herdr sync` sweeps.
const RELEASE_WAIT: Duration = Duration::from_secs(4);

/// herdr's cap on a title, a display name, and a token's value.
const TEXT_CHARS: usize = 80;

/// Where a watch inside a herdr pane reports: its pane, and herdr's socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    pub pane_id: String,
    pub socket: PathBuf,
}

impl Env {
    /// The pane this process runs in, when herdr started it: `HERDR_ENV=1`,
    /// with `HERDR_PANE_ID` and `HERDR_SOCKET_PATH`. Anywhere else, none, and
    /// the watch reports nothing.
    pub fn from_env() -> Option<Self> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        if var("HERDR_ENV").as_deref() != Some("1") {
            return None;
        }
        let pane_id = var("HERDR_PANE_ID").filter(|v| !v.is_empty())?;
        let socket = var("HERDR_SOCKET_PATH").filter(|v| !v.is_empty())?;
        Some(Self {
            pane_id,
            socket: PathBuf::from(socket),
        })
    }
}

/// herdr's state for a level: needs you is `blocked`; working is `working`;
/// ready and idle are `idle`. herdr makes `done` itself, of an idle pane not
/// yet seen, so a conversation's finished turn shows as done until looked at.
pub fn state_of(level: Level) -> &'static str {
    match level {
        Level::NeedsYou => "blocked",
        Level::Working => "working",
        Level::Ready | Level::Idle => "idle",
    }
}

/// The last `seq` this process gave out.
static LAST_SEQ: AtomicU64 = AtomicU64::new(0);

/// The next `seq`: above the last, and at least the clock in microseconds.
/// herdr keeps a source's last `seq` for the pane's life, past a release, and
/// ignores any report at or below it, so a watch started later in the same
/// pane must count above every watch before it: the clock does that.
pub fn next_seq() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX));
    let mut last = LAST_SEQ.load(Ordering::Relaxed);
    loop {
        let next = (last + 1).max(now);
        match LAST_SEQ.compare_exchange_weak(last, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(seen) => last = seen,
        }
    }
}

/// An answer herdr gave as an error: its code (`pane_not_found`, `not_agent`,
/// `duplicate_name`, …) and its message.
#[derive(Debug)]
pub struct HerdrError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for HerdrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "herdr: {} ({})", self.message, self.code)
    }
}

impl std::error::Error for HerdrError {}

/// One request to herdr on a fresh connection: a line out, its answer back,
/// within `REQUEST_TIMEOUT`. A fresh connection each time survives a herdr
/// restart between two reports. The answer is the result, its `type` tag
/// included; an error answer is a `HerdrError`.
pub async fn request(socket: &Path, id: &str, method: &str, params: Value) -> Result<Value> {
    let exchange = async {
        let mut stream = tokio::net::UnixStream::connect(socket)
            .await
            .with_context(|| format!("connecting to herdr at {}", socket.display()))?;
        let mut line = json!({"id": id, "method": method, "params": params}).to_string();
        line.push('\n');
        stream.write_all(line.as_bytes()).await?;
        let mut answer = String::new();
        if BufReader::new(stream).read_line(&mut answer).await? == 0 {
            anyhow::bail!("herdr closed the connection without answering {method}");
        }
        let v: Value = serde_json::from_str(answer.trim())
            .with_context(|| format!("herdr's answer to {method} is not JSON"))?;
        if let Some(e) = v.get("error") {
            let text = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            return Err(HerdrError {
                code: text("code"),
                message: text("message"),
            }
            .into());
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    };
    tokio::time::timeout(REQUEST_TIMEOUT, exchange)
        .await
        .map_err(|_| anyhow!("herdr did not answer {method} within 2 s"))?
}

/// herdr's name for an agent: `[a-z][a-z0-9_-]{0,31}`. A session's is
/// `theseus-`, the end of its id, then the words of its name
/// (`theseus_client::names`: a task's title, a conversation's label or
/// title): `theseus-q7f3k2-tide-notes`.
pub fn agent_name(session_id: &str, label: Option<&str>) -> String {
    let word = |s: &str| -> String {
        let mut out = String::new();
        for c in s.chars().flat_map(char::to_lowercase) {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                out.push(c);
            } else if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_end_matches('-').to_string()
    };
    let id = word(session_id.rsplit('_').next().unwrap_or(session_id));
    let tail: String = id
        .chars()
        .skip(id.chars().count().saturating_sub(6))
        .collect();
    let mut name = format!("{AGENT}-{tail}");
    if let Some(l) = label.map(word).filter(|l| !l.is_empty()) {
        name.push('-');
        name.push_str(&l);
    }
    name.truncate(32);
    name.trim_end_matches('-').to_string()
}

/// A text as herdr keeps it: one line, at most `TEXT_CHARS` characters.
fn fit(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= TEXT_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(TEXT_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// `pane.report_agent`'s params.
pub fn report_params(pane_id: &str, state: &str, message: &str, seq: u64) -> Value {
    json!({"pane_id": pane_id, "source": SOURCE, "agent": AGENT, "state": state,
           "message": message, "seq": seq})
}

/// `pane.release_agent`'s params.
pub fn release_params(pane_id: &str, seq: u64) -> Value {
    json!({"pane_id": pane_id, "source": SOURCE, "agent": AGENT, "seq": seq})
}

/// What the pane shows beside its state, for display only (herdr's
/// `pane.report_metadata`): the session's title, `theseus: <label>` as the
/// agent's name, and three tokens. herdr renders a token as `$name` in its
/// sidebar, and `theseus herdr sync` finds a session's pane by `$session`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub title: Option<String>,
    pub display_agent: String,
    /// The session's id, whole: sync matches on it.
    pub session: String,
    /// Spend since the last reset: `$0.42`.
    pub cost: String,
    /// The question waiting, by its correlation id: `theseus confirm $confirm`.
    pub confirm: Option<String>,
}

impl Meta {
    /// Its params: every field each time, so a report never depends on an
    /// earlier one (a token is a patch: a string sets it, null clears it).
    pub fn params(&self, pane_id: &str, seq: u64) -> Value {
        let mut p = json!({
            "pane_id": pane_id, "source": SOURCE, "agent": AGENT,
            "display_agent": fit(&self.display_agent),
            "tokens": {"session": self.session, "cost": self.cost,
                       "confirm": self.confirm.as_deref().map(fit)},
            "seq": seq,
        });
        if let Some(t) = self.title.as_deref().map(fit).filter(|t| !t.is_empty()) {
            p["title"] = json!(t);
        }
        p
    }
}

/// One request the reporter's task sends, in order.
#[derive(Debug, Clone, PartialEq)]
struct Outgoing {
    method: &'static str,
    params: Value,
}

enum Job {
    Send(Outgoing),
    /// Answered once everything queued before it was sent, or given up on.
    Flush(oneshot::Sender<()>),
}

/// The session's state as herdr shows it, kept by `theseus watch`: it sends a
/// report only when the state or its message changes, metadata only when it
/// changes, and the release on exit. A task sends them in order, each on a
/// fresh connection with its retries, so a slow or missing herdr never holds
/// the watch.
pub struct Reporter {
    env: Env,
    session_id: String,
    kind: SessionKind,
    label: Option<String>,
    title: Option<String>,
    tx: mpsc::UnboundedSender<Job>,
    /// The position of the last view applied: a view at or below it is old.
    position: u64,
    /// The state and message last sent.
    sent: Option<(&'static str, String)>,
    /// The state and message of the last view: what a failed submit goes
    /// back to.
    viewed: Option<(&'static str, String)>,
    meta: Option<Meta>,
    /// The last view's spend and question, for the metadata.
    cost: String,
    confirm: Option<String>,
    named: bool,
}

impl Reporter {
    /// A reporter for `session_id` in the pane `env` names, and its task.
    pub fn start(
        env: Env,
        session_id: &str,
        kind: SessionKind,
        label: Option<String>,
        title: Option<String>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(send_all(env.socket.clone(), rx));
        Self {
            env,
            session_id: session_id.to_string(),
            kind,
            label,
            title,
            tx,
            position: 0,
            sent: None,
            viewed: None,
            meta: None,
            cost: theseus_protocol::usd(0.0),
            confirm: None,
            named: false,
        }
    }

    pub fn pane_id(&self) -> &str {
        &self.env.pane_id
    }

    /// The session's title, once it has one (a fresh session gets it with its
    /// first turn).
    pub fn set_title(&mut self, title: Option<String>) {
        if title.is_some() && title != self.title {
            self.title = title;
            self.send_meta();
        }
    }

    /// Whether the pane's title still waits for the session's: a
    /// conversation the operator labelled is titled by its label, and needs no
    /// other; a task, by its title alone (theseus-0n1v).
    pub fn wants_title(&self) -> bool {
        self.named().is_none()
    }

    /// The words that name the session (`theseus_client::names`).
    fn named(&self) -> Option<&str> {
        theseus_client::names::words(self.kind, self.label.as_deref(), self.title.as_deref())
    }

    /// A view of the session, from the first read or `execution.changed`.
    /// One at or below the last position applied is old, and changes nothing.
    pub fn observe(&mut self, view: &ExecutionView) {
        if view.position <= self.position && self.position > 0 {
            return;
        }
        self.position = view.position;
        self.cost = theseus_protocol::usd(view.spent_usd);
        self.confirm = view.pending.first().map(|p| p.correlation_id.clone());
        let now = (state_of(view.attention.level), view.attention.label.clone());
        self.viewed = Some(now.clone());
        self.report(now);
    }

    /// A session with no execution yet: ready for its first message.
    pub fn observe_none(&mut self) {
        let now = ("idle", "ready".to_string());
        self.viewed = Some(now.clone());
        self.report(now);
    }

    /// A message was just sent from this terminal: working, at once, inside
    /// herdr's stall window, before the daemon's first event of the turn.
    pub fn working_now(&mut self) {
        self.report(("working", "sending your message".to_string()));
    }

    /// The message was refused: back to what the last view said.
    pub fn restore(&mut self) {
        if let Some(v) = self.viewed.clone() {
            self.report(v);
        }
    }

    fn report(&mut self, now: (&'static str, String)) {
        if self.sent.as_ref() != Some(&now) {
            self.push(
                "pane.report_agent",
                report_params(&self.env.pane_id, now.0, &now.1, next_seq()),
            );
            self.sent = Some(now);
            if !self.named {
                // herdr names only a pane that is an agent, so after the first
                // report; a release clears the name, so every watch names it.
                self.named = true;
                let name = agent_name(&self.session_id, self.named());
                self.push(
                    "agent.rename",
                    json!({"target": self.env.pane_id, "name": name}),
                );
            }
        }
        self.send_meta();
    }

    fn send_meta(&mut self) {
        let Some((_, label)) = &self.sent else {
            return;
        };
        let meta = Meta {
            title: self.named().map(String::from),
            display_agent: format!("{AGENT}: {label}"),
            session: self.session_id.clone(),
            cost: self.cost.clone(),
            confirm: self.confirm.clone(),
        };
        if self.meta.as_ref() != Some(&meta) {
            self.push(
                "pane.report_metadata",
                meta.params(&self.env.pane_id, next_seq()),
            );
            self.meta = Some(meta);
        }
    }

    fn push(&self, method: &'static str, params: Value) {
        let _ = self.tx.send(Job::Send(Outgoing { method, params }));
    }

    /// On exit: release the pane, so it stops showing this session's state,
    /// and clear the question's token. The pane keeps `$session`, so a later
    /// `theseus herdr sync` finds it and can start a watch in it again.
    /// Waits for herdr at most `RELEASE_WAIT`.
    pub async fn release(mut self) {
        self.push(
            "pane.release_agent",
            release_params(&self.env.pane_id, next_seq()),
        );
        if let Some(meta) = self.meta.take() {
            let meta = Meta {
                confirm: None,
                ..meta
            };
            self.push(
                "pane.report_metadata",
                meta.params(&self.env.pane_id, next_seq()),
            );
        }
        let (done, sent) = oneshot::channel();
        let _ = self.tx.send(Job::Flush(done));
        let _ = tokio::time::timeout(RELEASE_WAIT, sent).await;
    }
}

/// The reporter's task: sends each job in order, a batch at a time, dropping a
/// report or metadata that a later one in the same batch replaces. A failure
/// is said once on stderr, and once more when herdr answers again.
async fn send_all(socket: PathBuf, mut rx: mpsc::UnboundedReceiver<Job>) {
    let mut n = 0u64;
    let mut failing: Option<String> = None;
    while let Some(first) = rx.recv().await {
        let mut batch = vec![first];
        while let Ok(more) = rx.try_recv() {
            batch.push(more);
        }
        for job in coalesce(batch) {
            let o = match job {
                Job::Flush(done) => {
                    let _ = done.send(());
                    continue;
                }
                Job::Send(o) => o,
            };
            n += 1;
            let id = format!("theseus:{}:{n}", std::process::id());
            match send_one(&socket, &id, &o).await {
                Ok(()) => {
                    if failing.take().is_some() {
                        eprintln!("theseus: herdr answers again; reporting goes on");
                    }
                }
                Err(e) => {
                    let said = e.to_string();
                    // A name another pane holds is herdr's to keep.
                    if o.method == "agent.rename" {
                        eprintln!("theseus: herdr kept the pane's name: {said}");
                    } else if failing.as_deref() != Some(&said) {
                        eprintln!(
                            "theseus: {said}; the pane's state is sent again when herdr answers"
                        );
                        failing = Some(said);
                    }
                }
            }
        }
    }
}

/// One job, with its retries. An error answer is not retried: herdr heard it.
async fn send_one(socket: &Path, id: &str, o: &Outgoing) -> Result<()> {
    let mut waits = RETRIES.iter();
    loop {
        match request(socket, id, o.method, o.params.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) if e.downcast_ref::<HerdrError>().is_some() => return Err(e),
            Err(e) => match waits.next() {
                Some(w) => tokio::time::sleep(*w).await,
                None => return Err(e),
            },
        }
    }
}

/// A batch without the reports and metadata a later one replaces. Each
/// metadata report carries every field, so the last says it all. A report
/// is kept when a rename follows it before the next report: herdr names only
/// a pane that is already an agent.
fn coalesce(batch: Vec<Job>) -> Vec<Job> {
    let method = |j: &Job| match j {
        Job::Send(o) => Some(o.method),
        Job::Flush(_) => None,
    };
    let methods: Vec<Option<&str>> = batch.iter().map(method).collect();
    batch
        .into_iter()
        .enumerate()
        .filter(|(i, j)| {
            let Some(m @ ("pane.report_agent" | "pane.report_metadata")) = method(j) else {
                return true;
            };
            let later = &methods[i + 1..];
            match later.iter().position(|l| *l == Some(m)) {
                None => true,
                Some(next) => {
                    m == "pane.report_agent" && later[..next].contains(&Some("agent.rename"))
                }
            }
        })
        .map(|(_, j)| j)
        .collect()
}

/// On a panic, release the pane before the process dies: herdr's custom
/// authority never expires, so a crashed watch would leave its last state on
/// the pane until a `theseus herdr sync` swept it.
pub fn release_on_panic(env: &Env) {
    let env = env.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        release_blocking(&env);
        previous(info);
    }));
}

/// The release over a blocking socket with short timeouts: a panic hook
/// cannot count on the runtime.
pub fn release_blocking(env: &Env) {
    use std::io::{BufRead, Write};
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(&env.socket) else {
        return;
    };
    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    let line = json!({"id": format!("theseus:{}:panic", std::process::id()),
                      "method": "pane.release_agent",
                      "params": release_params(&env.pane_id, next_seq())});
    if writeln!(s, "{line}").is_ok() {
        let mut answer = String::new();
        let _ = std::io::BufReader::new(&s).read_line(&mut answer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::Attention;

    fn view(position: u64, level: Level, label: &str, spent: f64) -> ExecutionView {
        serde_json::from_value(json!({
            "position": position, "execution_id": "exe_q7f3k2", "session_id": "ses_q7f3k2",
            "kind": "conversation", "state": "running", "spent_usd": spent,
            "attention": Attention { level, label: label.into(), since_ms: 0 },
        }))
        .unwrap()
    }

    /// A reporter whose task is replaced by the test's receiver.
    fn reporter() -> (Reporter, mpsc::UnboundedReceiver<Job>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let r = Reporter {
            env: Env {
                pane_id: "w1:p2".into(),
                socket: PathBuf::from("/nowhere"),
            },
            session_id: "ses_q7f3k2".into(),
            kind: SessionKind::Conversation,
            label: Some("Tide notes".into()),
            title: None,
            tx,
            position: 0,
            sent: None,
            viewed: None,
            meta: None,
            cost: "$0".into(),
            confirm: None,
            named: false,
        };
        (r, rx)
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Job>) -> Vec<Outgoing> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|j| match j {
                Job::Send(o) => Some(o),
                Job::Flush(_) => None,
            })
            .collect()
    }

    /// The reports alone, as (state, message).
    fn reports(sent: &[Outgoing]) -> Vec<(String, String)> {
        sent.iter()
            .filter(|o| o.method == "pane.report_agent")
            .map(|o| {
                (
                    o.params["state"].as_str().unwrap().to_string(),
                    o.params["message"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    /// The design's mapping (§2.10): needs you is blocked, working is
    /// working, and ready and idle are idle, which herdr turns into done.
    #[test]
    fn each_level_maps_to_its_herdr_state() {
        assert_eq!(state_of(Level::NeedsYou), "blocked");
        assert_eq!(state_of(Level::Working), "working");
        assert_eq!(state_of(Level::Ready), "idle");
        assert_eq!(state_of(Level::Idle), "idle");
    }

    /// The pane reports only when the state or its message changes: a view
    /// again at the same position is old, and a spend-only view changes the
    /// metadata alone. The first report is followed by the pane's name.
    #[test]
    fn only_changes_are_reported() {
        let (mut r, mut rx) = reporter();
        r.observe(&view(10, Level::Working, "turn 1", 0.0));
        let first = drain(&mut rx);
        let methods: Vec<&str> = first.iter().map(|o| o.method).collect();
        assert_eq!(
            methods,
            ["pane.report_agent", "agent.rename", "pane.report_metadata"]
        );
        assert_eq!(
            first[1].params,
            json!({"target": "w1:p2", "name": "theseus-q7f3k2-tide-notes"})
        );
        r.observe(&view(10, Level::Working, "turn 1", 0.0));
        r.observe(&view(9, Level::NeedsYou, "an old view", 0.0));
        assert!(drain(&mut rx).is_empty(), "old views change nothing");
        r.observe(&view(11, Level::Working, "turn 1", 0.0123));
        let spend = drain(&mut rx);
        assert_eq!(spend.len(), 1, "{spend:?}");
        assert_eq!(spend[0].method, "pane.report_metadata");
        assert_eq!(spend[0].params["tokens"]["cost"], "$0.01");
        r.observe(&view(
            12,
            Level::NeedsYou,
            "confirm proc.run: run the tide",
            0.0123,
        ));
        r.observe(&view(
            13,
            Level::NeedsYou,
            "confirm proc.run: run the tide",
            0.0123,
        ));
        r.observe(&view(14, Level::Ready, "ready", 0.02));
        assert_eq!(
            reports(&drain(&mut rx)),
            [
                ("blocked".into(), "confirm proc.run: run the tide".into()),
                ("idle".into(), "ready".into())
            ]
        );
    }

    /// `seq` only grows, within a process and across processes: it is at
    /// least the clock in microseconds, so a later watch in the same pane
    /// counts above every earlier one.
    #[test]
    fn seq_only_grows_and_follows_the_clock() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        let seqs: Vec<u64> = (0..1000).map(|_| next_seq()).collect();
        assert!(seqs.windows(2).all(|w| w[1] > w[0]), "strictly increasing");
        assert!(seqs[0] >= before, "from the clock: {} < {before}", seqs[0]);
        let (mut r, mut rx) = reporter();
        r.observe(&view(1, Level::Working, "turn 1", 0.0));
        r.observe(&view(2, Level::NeedsYou, "blocked: a reason", 0.0));
        let sent = drain(&mut rx);
        let in_order: Vec<u64> = sent
            .iter()
            .filter_map(|o| o.params["seq"].as_u64())
            .collect();
        assert!(in_order.windows(2).all(|w| w[1] > w[0]), "{in_order:?}");
    }

    /// A message sent from the terminal says working at once; a refusal goes
    /// back to what the last view said.
    #[test]
    fn a_submit_says_working_at_once_and_a_refusal_restores() {
        let (mut r, mut rx) = reporter();
        r.observe(&view(5, Level::Ready, "ready", 0.0));
        drain(&mut rx);
        r.working_now();
        r.restore();
        assert_eq!(
            reports(&drain(&mut rx)),
            [
                ("working".into(), "sending your message".into()),
                ("idle".into(), "ready".into())
            ]
        );
    }

    /// The metadata: the title (or the label), `theseus: <label>`, and the
    /// session, cost, and question tokens, each within herdr's 80 characters.
    #[test]
    fn metadata_carries_every_field_within_herdrs_limits() {
        let m = Meta {
            title: Some("x".repeat(120)),
            display_agent: "theseus: confirm proc.run: run the tide".into(),
            session: "ses_q7f3k2".into(),
            cost: "$0.42".into(),
            confirm: None,
        };
        let p = m.params("w1:p2", 7);
        assert_eq!(p["title"].as_str().unwrap().chars().count(), 80);
        assert_eq!(
            p["tokens"],
            json!({"session": "ses_q7f3k2", "cost": "$0.42", "confirm": null})
        );
        assert_eq!(
            (p["source"].as_str(), p["agent"].as_str(), p["seq"].as_u64()),
            (Some(SOURCE), Some(AGENT), Some(7))
        );
    }

    /// herdr's names: `[a-z][a-z0-9_-]{0,31}`.
    #[test]
    fn agent_names_fit_herdrs_rule() {
        assert_eq!(
            agent_name("ses_q7f3k2", Some("Tide notes")),
            "theseus-q7f3k2-tide-notes"
        );
        assert_eq!(agent_name("ses_Q7F3K2", None), "theseus-q7f3k2");
        let long = agent_name("ses_q7f3k2", Some("A very long label: with *punctuation*"));
        assert!(long.len() <= 32, "{long}");
        assert!(!long.ends_with('-'), "{long}");
        for name in [long, agent_name("x", Some("--"))] {
            let mut chars = name.chars();
            assert!(chars.next().unwrap().is_ascii_lowercase());
            assert!(chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        }
    }

    /// The environment herdr gives a pane turns the reporter on; anything
    /// less leaves it off.
    #[test]
    fn the_reporter_needs_herdrs_whole_environment() {
        fn vars<'a>(env: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
            move |k| {
                env.iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        }
        let all = [
            ("HERDR_ENV", "1"),
            ("HERDR_PANE_ID", "w1:p2"),
            ("HERDR_SOCKET_PATH", "/run/herdr.sock"),
        ];
        assert_eq!(
            Env::from_vars(vars(&all)),
            Some(Env {
                pane_id: "w1:p2".into(),
                socket: "/run/herdr.sock".into()
            })
        );
        assert_eq!(Env::from_vars(vars(&all[1..])), None);
        assert_eq!(Env::from_vars(vars(&all[..2])), None);
        assert_eq!(
            Env::from_vars(vars(&[
                ("HERDR_ENV", "0"),
                ("HERDR_PANE_ID", "w1:p2"),
                ("HERDR_SOCKET_PATH", "/s")
            ])),
            None
        );
    }

    /// A batch keeps the last report and the last metadata, but never drops a
    /// report a rename depends on.
    #[test]
    fn a_batch_drops_what_a_later_job_replaces() {
        let o = |m: &'static str, n: u64| {
            Job::Send(Outgoing {
                method: m,
                params: json!({ "n": n }),
            })
        };
        let kept = |b: Vec<Job>| -> Vec<(String, u64)> {
            coalesce(b)
                .into_iter()
                .filter_map(|j| match j {
                    Job::Send(o) => Some((o.method.to_string(), o.params["n"].as_u64().unwrap())),
                    Job::Flush(_) => None,
                })
                .collect()
        };
        assert_eq!(
            kept(vec![
                o("pane.report_agent", 1),
                o("agent.rename", 2),
                o("pane.report_metadata", 3),
                o("pane.report_agent", 4),
                o("pane.report_metadata", 5),
                o("pane.report_agent", 6),
                o("pane.release_agent", 7),
            ]),
            [
                ("pane.report_agent".into(), 1),
                ("agent.rename".into(), 2),
                ("pane.report_metadata".into(), 5),
                ("pane.report_agent".into(), 6),
                ("pane.release_agent".into(), 7),
            ]
        );
    }

    /// A panic releases the pane before the process dies, over a blocking
    /// socket: herdr's custom authority never expires.
    #[test]
    fn a_panic_releases_the_pane() {
        use std::io::{BufRead, Write};
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("herdr.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&s).read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            writeln!(&s, "{}", json!({"id": req["id"], "result": {"type": "ok"}})).unwrap();
            let _ = tx.send(req);
        });
        let env = Env {
            pane_id: "w1:p2".into(),
            socket: sock,
        };
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        release_on_panic(&env);
        let caught = std::panic::catch_unwind(|| panic!("a reef"));
        let _ = std::panic::take_hook();
        std::panic::set_hook(before);
        assert!(caught.is_err());
        let req = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("herdr heard the release");
        assert_eq!(req["method"], "pane.release_agent");
        assert_eq!(
            (
                req["params"]["pane_id"].as_str(),
                req["params"]["source"].as_str(),
                req["params"]["agent"].as_str()
            ),
            (Some("w1:p2"), Some(SOURCE), Some(AGENT))
        );
        assert!(req["params"]["seq"].as_u64().unwrap() > 0);
    }
}
