//! `theseus status` and `theseus wait --any` (theseus-lweh): status without
//! asking for prompts, tmux, and the terminal's tab.
//!
//! One read feeds every form: `executions.watch {limit: 0}` answers exactly the
//! executions that need you or work, with their questions. `--short` is a
//! prompt's segment (`●1 ◐3 ✗1`), empty when nothing works or waits;
//! `--watch` keeps the connection and prints the line again when a count or
//! the first item that needs you changes, driven by the push and never by a
//! timer; `--watch --tab` also writes the terminal tab's progress (OSC 9;4).
//! The long form is a header and a row for each task.
//!
//! The pure parts (`Board`, the forms, `tab_seq`) take their clock and zone
//! as arguments, so their tests pin both.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::time::Duration;

use crate::status_notify::{read_name, Alerts};
use anyhow::{bail, Context, Result};
use clap::Args;
use serde::Serialize;
use serde_json::Value;
use theseus_client::notify::Delivery;
use theseus_client::{names, render, Conn};
use theseus_protocol::{
    error_code, method, ConfirmRequest, Event, ExecutionView, ExecutionsWatchParams,
    ExecutionsWatchResult, Level, SessionListParams, SessionListResult, SessionWaitResult,
    StatusShortResult,
};

/// How long `theseus status` waits to connect: a prompt cannot afford more.
const CONNECT_WITHIN: Duration = Duration::from_millis(100);
/// How long a one-shot `status` waits for the daemon's answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);
/// How long `--short` waits for it: a prompt or a status bar runs it on every
/// draw, and a wedged daemon must not hold them (theseus-lweh's review: 300 to
/// 500 ms; a healthy daemon answers in a few).
const SHORT_ANSWER_WITHIN: Duration = Duration::from_millis(400);
/// The rows the long form shows before it says how many more there are.
const MAX_ROWS: usize = 20;
/// The width of a terminal that says nothing of it.
const DEFAULT_WIDTH: usize = 100;

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// One line for a prompt or a status bar: `●1 ◐3 ✗1 ◆2` (needs you, working, failed, new),
    /// each count only when it is not zero; a failure counts until `theseus status` (or
    /// `history`, `watch`, the TUI) has shown it. It prints nothing when nothing works or waits,
    /// nothing, exit 3, within 100 ms when the daemon is down, and nothing, exit 1, when it does
    /// not answer within 400 ms.
    #[arg(long)]
    short: bool,
    /// Keep the connection and print the line again when a count or the first item that needs
    /// you changes, never on a timer. For tmux: `set -g status-right '#(theseus status --short
    /// --watch)'`. Reconnects quietly if the daemon restarts.
    #[arg(long)]
    watch: bool,
    /// With --watch: the terminal tab's progress (Windows Terminal's OSC 9;4): a ring while
    /// something works, the error state while something needs you, cleared when idle and on
    /// exit. Written to the controlling terminal, through tmux's passthrough inside tmux.
    #[arg(long, requires = "watch")]
    tab: bool,
    /// With --watch: ping when something needs you or finishes, as the TUI does (the one
    /// notification policy): auto (a Windows toast under WSL, OSC 9 in a terminal that has
    /// it, else the bell), toast, bell, osc9, osc777, or off (the default: a watch runs in a
    /// status bar). A bell or OSC goes to the controlling terminal, never to stdout.
    #[arg(long, requires = "watch", default_value = "off", value_parser = parse_notify)]
    notify: Delivery,
}

fn parse_notify(s: &str) -> Result<Delivery, String> {
    Delivery::parse(s).ok_or_else(|| format!("takes {}", Delivery::NAMES))
}

impl StatusArgs {
    /// The prompt's one-shot line: `--short` without `--watch`.
    pub fn is_short_once(&self) -> bool {
        self.short && !self.watch
    }
}

/// Run `theseus status`. It connects for itself (within 100 ms), so `--short`
/// can stay silent when the daemon is down.
pub async fn run(socket: &str, spawn: Option<&str>, json: bool, a: &StatusArgs) -> Result<()> {
    if a.watch {
        return watch(socket, spawn, json, a).await;
    }
    let connect = async {
        match spawn {
            Some(bin) => Conn::spawn(bin),
            None => Conn::socket_within(socket, Some(CONNECT_WITHIN)).await,
        }
    };
    let mut conn = match connect.await {
        Ok(c) => c,
        Err(_) if a.short => std::process::exit(3),
        Err(e) => return Err(e),
    };
    let within = if a.short {
        SHORT_ANSWER_WITHIN
    } else {
        ANSWER_WITHIN
    };
    let answer = tokio::time::timeout(within, async {
        // `--short` asks for the board's counts alone (theseus-8kn12): no view,
        // no subscription. A daemon that predates the method answers the full
        // read below, as it always did.
        if a.short && !json {
            match conn.request(method::STATUS_SHORT, ()).await {
                Ok(v) => return anyhow::Ok(Answer::Short(serde_json::from_value(v)?)),
                Err(e) if is_method_not_found(&e) => {}
                Err(e) => return Err(e),
            }
        }
        let mut board = Board::default();
        board.reload(&mut conn).await?;
        let titles = if a.short || json {
            HashMap::new()
        } else {
            board.progress = crate::status_progress::lines(&mut conn).await;
            titles(&mut conn, &board).await.unwrap_or_default()
        };
        anyhow::Ok(Answer::Full(board, titles))
    })
    .await;
    let (board, titles) = match answer {
        Ok(Ok(Answer::Short(r))) => {
            let line = short_line(&short_summary(&r));
            if !line.is_empty() {
                writeln!(std::io::stdout().lock(), "{line}")?;
            }
            return conn.close().await;
        }
        Ok(Ok(Answer::Full(b, t))) => (b, t),
        Ok(Err(_)) | Err(_) if a.short => std::process::exit(1),
        Ok(Err(e)) => return Err(e),
        Err(_) => bail!("the daemon did not answer within {within:?}"),
    };
    let now = now_ms();
    let mut sum = board.summary(now, new_since_seen(), &displayed());
    // The long form's first line says the daemon's own jobs running, from
    // the work board (theseus-jn17m); `--short` and `--json` ask nothing more.
    if !a.short && !json {
        sum.system = system_words(&mut conn).await;
    }
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{}", serde_json::to_string(&board.json(&sum))?)?;
    } else if a.short {
        let line = short_line(&sum);
        if !line.is_empty() {
            writeln!(out, "{line}")?;
        }
    } else {
        // A running job's time against its usual, one more read (theseus-e1hu4):
        // without it the rows read as before.
        let jobs = tokio::time::timeout(ANSWER_WITHIN, job_times(&mut conn))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        for line in long_lines_with(&board, &sum, &titles, &jobs, (now, width()), &local_hm) {
            writeln!(out, "{line}")?;
        }
    }
    drop(out);
    // The long form shows the sessions that need you or work: recorded as seen
    // after its output, as `history` does. `--short` and `--json` are a status
    // bar's and a script's polls, and show no one anything.
    if !a.short && !json {
        let shown: Vec<String> = board
            .ordered()
            .iter()
            .map(|v| v.session_id.clone())
            .collect();
        theseus_client::shown::shown(&mut conn, &shown).await;
    }
    conn.close().await
}

/// The daemon's own jobs running (`⚙ importing 41/120`), from the work board,
/// asked for within a second; none when it does not answer (a daemon that
/// predates the board, or a busy one). The watch it opens is closed at once.
async fn system_words(conn: &mut Conn) -> Option<String> {
    use theseus_protocol::{WorkWatchParams, WorkWatchResult};
    // The daemon's own work alone (`system`), not the whole board.
    let p = WorkWatchParams {
        limit: Some(0),
        roots: Some(vec![theseus_protocol::work_system::ROOT.to_string()]),
        interactive: None,
    };
    let ask = async {
        let v = conn
            .request(method::WORK_WATCH, serde_json::to_value(p).ok()?)
            .await
            .ok()?;
        let _ = conn
            .request(method::WORK_UNWATCH, serde_json::json!({}))
            .await;
        serde_json::from_value::<WorkWatchResult>(v).ok()
    };
    let snap = tokio::time::timeout(std::time::Duration::from_secs(1), ask)
        .await
        .ok()??;
    theseus_protocol::work_system::status_words(&snap.views)
}

/// What the one-shot read brought back.
enum Answer {
    Short(StatusShortResult),
    Full(Board, HashMap<String, String>),
}

/// Whether the daemon said it does not know the method.
fn is_method_not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<theseus_client::CallError>()
        .is_some_and(|c| c.code == error_code::METHOD_NOT_FOUND)
}

/// The counts of `--short` from the daemon's: a failure whose view the seen
/// file has reached is not counted (theseus-xnff's (a)), as `Board::summary`
/// does.
pub fn short_summary(r: &StatusShortResult) -> Summary {
    let (new, shown) = short_seen();
    short_summary_with(r, new, &shown)
}

fn short_summary_with(r: &StatusShortResult, new: u32, shown: &HashMap<String, u64>) -> Summary {
    let unseen = r
        .failed
        .iter()
        .filter(|f| !shown.get(&f.execution_id).is_some_and(|&d| d >= f.position))
        .count() as u32;
    Summary {
        needs_you: r.needs_you,
        working: r.working,
        failed: unseen,
        failed_seen: r.failed.len() as u32 - unseen,
        new,
        wake_at_ms: None,
        first: r.first.clone(),
        system: None,
    }
}

/// The seen file's `◆` count and displayed positions, from one read of it.
fn short_seen() -> (u32, HashMap<String, u64>) {
    if cfg!(test) {
        return (0, HashMap::new());
    }
    theseus_client::seen::Seen::default_path()
        .map_or_else(|| (0, HashMap::new()), |p| theseus_client::seen::counts(&p))
}

// ------------------------------------------------------------------ board

/// The executions that need you or work, as `executions.watch` and
/// `execution.changed` tell them, and the questions waiting.
#[derive(Default)]
pub struct Board {
    views: HashMap<String, ExecutionView>,
    /// The last position applied for each execution since the snapshot.
    last: HashMap<String, u64>,
    /// The snapshot's position: a view at or before it is in the snapshot.
    floor: u64,
    confirms: Vec<ConfirmRequest>,
    /// A watch that pings keeps the snapshot's executions, whole, for its notices.
    track: bool,
    snapshot: Vec<ExecutionView>,
    /// Each session's progress line from the work board, when the long form
    /// read it (theseus-xlias).
    pub progress: HashMap<String, String>,
}

impl Board {
    /// Read the board again: `executions.watch {limit: 0}`, then the changes
    /// that came before its answer, by the position rule.
    pub async fn reload(&mut self, conn: &mut Conn) -> Result<()> {
        let mut early: Vec<(String, Value)> = Vec::new();
        let v = conn
            .call(
                method::EXECUTIONS_WATCH,
                serde_json::to_value(ExecutionsWatchParams { limit: Some(0) })?,
                |m, p| early.push((m.to_string(), p.clone())),
            )
            .await?;
        self.load(serde_json::from_value(v)?);
        for (m, p) in early {
            if let Some(Event::ExecutionChanged(view)) = Event::from_notification(&m, &p)? {
                self.apply(view);
            }
        }
        Ok(())
    }

    fn load(&mut self, snap: ExecutionsWatchResult) {
        self.views.clear();
        self.last.clear();
        self.floor = snap.position;
        self.confirms = snap.confirms;
        if self.track {
            self.snapshot = snap.executions.clone();
        }
        for v in snap.executions {
            self.last.insert(v.execution_id.clone(), v.position);
            if matches!(v.attention.level, Level::NeedsYou | Level::Working) {
                self.views.insert(v.execution_id.clone(), v);
            }
        }
    }

    /// Apply a change if it is newer than what the board knows of that
    /// execution; whether it changed the board.
    pub fn apply(&mut self, v: ExecutionView) -> bool {
        let known = self
            .last
            .get(&v.execution_id)
            .copied()
            .unwrap_or(self.floor);
        if v.position <= known {
            return false;
        }
        self.last.insert(v.execution_id.clone(), v.position);
        if matches!(v.attention.level, Level::NeedsYou | Level::Working) {
            self.views.insert(v.execution_id.clone(), v);
        } else {
            self.views.remove(&v.execution_id);
        }
        true
    }

    /// The views, those that need you first, the longest waiting first; then
    /// the failed; then the working.
    fn ordered(&self) -> Vec<&ExecutionView> {
        let mut rows: Vec<&ExecutionView> = self.views.values().collect();
        rows.sort_by_key(|v| (group(v), v.attention.since_ms, v.execution_id.clone()));
        rows
    }

    /// The counts as of `now_ms`, with `new` finished since seen; a failure
    /// whose view `shown` (each execution's position last displayed) has
    /// reached is `failed_seen`, never `failed`: a failed execution stays on
    /// the board for good, so ✗ counted it for good (theseus-lweh's review).
    pub fn summary(&self, now_ms: u64, new: u32, shown: &HashMap<String, u64>) -> Summary {
        let rows = self.ordered();
        let count = |g: u8| rows.iter().filter(|v| group(v) == g).count() as u32;
        let seen =
            |v: &&&ExecutionView| shown.get(&v.execution_id).is_some_and(|&d| d >= v.position);
        let failed_seen = rows.iter().filter(|v| group(v) == 1).filter(seen).count() as u32;
        Summary {
            needs_you: count(0),
            working: count(2),
            failed: count(1) - failed_seen,
            failed_seen,
            new,
            wake_at_ms: rows
                .iter()
                .filter_map(|v| v.wake_at_ms)
                .filter(|&w| w > now_ms)
                .min(),
            first: rows
                .iter()
                .find(|v| v.attention.level == Level::NeedsYou)
                .map(|v| v.execution_id.clone()),
            system: None,
        }
    }

    fn json(&self, sum: &Summary) -> Value {
        serde_json::json!({"summary": sum, "executions": self.ordered()})
    }
}

/// 0 needs you, 1 failed, 2 working.
fn group(v: &ExecutionView) -> u8 {
    match (v.attention.level, v.state.as_str()) {
        (Level::NeedsYou, "failed") => 1,
        (Level::NeedsYou, _) => 0,
        _ => 2,
    }
}

/// Each execution's position last displayed, from the machine's seen file
/// (theseus-yus0), which the ✗ count reads. As `new_since_seen`, a unit test's
/// board is its own.
fn displayed() -> HashMap<String, u64> {
    if cfg!(test) {
        return HashMap::new();
    }
    theseus_client::seen::Seen::default_path()
        .map_or_else(HashMap::new, |p| theseus_client::seen::displayed(&p))
}

/// What finished since the owner last looked (◆), from the machine's seen file
/// (theseus-yus0): the TUI's account of the finishes, less what a command
/// showed since. It asks no daemon, and is 0 with no file. A unit test's board
/// is its own: it never reads the machine's file (the binary's tests point
/// `XDG_STATE_HOME` at their own directory instead).
fn new_since_seen() -> u32 {
    if cfg!(test) {
        return 0;
    }
    theseus_client::seen::Seen::default_path().map_or(0, |p| theseus_client::seen::done_count(&p))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub needs_you: u32,
    pub working: u32,
    /// Failures not shown since they failed: the ✗.
    pub failed: u32,
    /// Failures a client has shown since (theseus-lweh's review): still on the
    /// board and in the long form's rows, never in ✗ or the tab's state.
    pub failed_seen: u32,
    pub new: u32,
    /// The next wake due, if one is.
    pub wake_at_ms: Option<u64>,
    /// The execution that has needed you longest.
    pub first: Option<String>,
    /// The daemon's own jobs running, in a few words (`⚙ importing 41/120`):
    /// the long form asks the work board for them (theseus-jn17m).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

impl Summary {
    /// Nothing works or waits.
    fn quiet(&self) -> bool {
        self.needs_you + self.working + self.failed == 0
    }
}

// ------------------------------------------------------------------ forms

/// `●1 ◐3 ✗1 ◆2`: each count only when it is not zero; empty when nothing
/// works or waits and nothing is new.
pub fn short_line(s: &Summary) -> String {
    [
        ('●', s.needs_you),
        ('◐', s.working),
        ('✗', s.failed),
        ('◆', s.new),
    ]
    .iter()
    .filter(|(_, n)| *n > 0)
    .map(|(g, n)| format!("{g}{n}"))
    .collect::<Vec<_>>()
    .join(" ")
}

/// The long form's first line: `theseus 18:16 · ● 1 needs you · ◐ 3 working ·
/// ✗ 1 failed · ◆ 2 new · ⏰ 18:20`.
pub fn header_line(s: &Summary, now_ms: u64, hm: &dyn Fn(u64) -> String) -> String {
    let mut parts = vec![format!("theseus {}", hm(now_ms))];
    parts.extend(s.system.clone());
    for (text, n) in [
        ("● {} needs you", s.needs_you),
        ("◐ {} working", s.working),
        ("✗ {} failed", s.failed),
        ("◆ {} new", s.new),
    ] {
        if n > 0 {
            parts.push(text.replace("{}", &n.to_string()));
        }
    }
    if s.failed_seen > 0 {
        parts.push(format!("{} failed, seen", s.failed_seen));
    }
    if s.quiet() && s.new == 0 && s.failed_seen == 0 {
        parts.push("nothing needs you or works".into());
    }
    if let Some(w) = s.wake_at_ms {
        parts.push(format!("⏰ {}", hm(w)));
    }
    parts.join(" · ")
}

/// The long form: the header, then a row for each task (a line under each
/// that needs you, with the command that answers it). `at` is the time now
/// and the terminal's width; `titles` maps a session to its name.
#[cfg(test)]
pub fn long_lines(
    board: &Board,
    sum: &Summary,
    titles: &HashMap<String, String>,
    at: (u64, usize),
    hm: &dyn Fn(u64) -> String,
) -> Vec<String> {
    long_lines_with(board, sum, titles, &JobTimes::new(), at, hm)
}

/// When a session's oldest running job started, and its program's usual time.
pub type JobTimes = HashMap<String, (u64, Option<u64>)>;

/// The running jobs' times from the work board (`work.watch`, once): each
/// session's oldest. The watch is dropped as it is read.
async fn job_times(conn: &mut Conn) -> Result<JobTimes> {
    use theseus_protocol::work::{WorkKind, WorkState};
    let v = conn
        .request(
            method::WORK_WATCH,
            theseus_protocol::WorkWatchParams {
                limit: Some(0),
                ..Default::default()
            },
        )
        .await?;
    let _ = conn
        .request(method::WORK_UNWATCH, serde_json::json!({}))
        .await;
    let snap: theseus_protocol::WorkWatchResult = serde_json::from_value(v)?;
    let mut out = JobTimes::new();
    for j in snap
        .views
        .iter()
        .filter(|v| v.kind == WorkKind::Job && v.state == WorkState::Running)
    {
        let Some(s) = &j.session_id else { continue };
        let at = (j.started_at_ms, j.eta.map(|e| e.usual_ms));
        out.entry(s.clone())
            .and_modify(|old| {
                if at.0 < old.0 {
                    *old = at;
                }
            })
            .or_insert(at);
    }
    Ok(out)
}

/// `long_lines`, with the running jobs' times: a row whose session runs a job
/// says `job 2:25 of usual 0:31` after its time.
pub fn long_lines_with(
    board: &Board,
    sum: &Summary,
    titles: &HashMap<String, String>,
    jobs: &JobTimes,
    at: (u64, usize),
    hm: &dyn Fn(u64) -> String,
) -> Vec<String> {
    let (now, width) = at;
    let mut lines = vec![header_line(sum, now, hm)];
    let rows = board.ordered();
    let shown = &rows[..rows.len().min(MAX_ROWS)];
    // A long label gives way before the title does: a row whose title is cut to
    // eight characters tells which task it is no better than its id.
    let tail_cap = (width * 11 / 20).max(24);
    let cells: Vec<(String, String, String)> = shown
        .iter()
        .map(|v| {
            let title = titles
                .get(&v.session_id)
                .filter(|t| !t.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("({})", v.kind.as_str()));
            let mut tail = tail(v, now, hm, jobs.get(&v.session_id));
            if let Some(p) = board.progress.get(&v.session_id).filter(|_| group(v) != 1) {
                tail = format!("{tail} · {p}");
            }
            (
                format!("{} {}", mark(v), short_id(&v.session_id)),
                first_line(&title),
                cut(&tail, tail_cap),
            )
        })
        .collect();
    let widest_tail = cells.iter().map(|c| c.2.chars().count()).max().unwrap_or(0);
    let widest_title = cells.iter().map(|c| c.1.chars().count()).max().unwrap_or(0);
    // The id column is a mark, a space, and six characters; two spaces
    // separate the columns. The title gives way first.
    let title_w = widest_title
        .min(width.saturating_sub(8 + 4 + widest_tail))
        .max(8);
    for (v, (id, title, tail)) in shown.iter().zip(&cells) {
        let head = format!("{id}  {:<title_w$}  ", cut(title, title_w));
        let room = width.saturating_sub(head.chars().count()).max(10);
        lines.push(format!("{head}{}", cut(tail, room)));
        if group(v) == 0 {
            lines.push(format!("         {}", command(v)));
        }
    }
    if rows.len() > shown.len() {
        lines.push(format!(
            "… and {} more (theseus tasks)",
            rows.len() - shown.len()
        ));
    }
    lines
}

fn mark(v: &ExecutionView) -> char {
    match group(v) {
        0 => '●',
        1 => '✗',
        _ => '◐',
    }
}

/// The last six characters of a session's id: enough for every command that
/// takes one.
fn short_id(session_id: &str) -> String {
    let n = session_id.chars().count();
    session_id.chars().skip(n.saturating_sub(6)).collect()
}

/// What the row says: the attention label and how long, or for a failure
/// when and why.
fn tail(
    v: &ExecutionView,
    now: u64,
    hm: &dyn Fn(u64) -> String,
    job: Option<&(u64, Option<u64>)>,
) -> String {
    let label = &v.attention.label;
    if group(v) == 1 {
        return match label.strip_prefix("failed: ") {
            Some(why) => format!("failed {}: {why}", hm(v.attention.since_ms)),
            None => format!("failed {}", hm(v.attention.since_ms)),
        };
    }
    let mut s = format!(
        "{label} · {}",
        clock(now.saturating_sub(v.attention.since_ms))
    );
    if let Some((started, usual)) = job {
        s.push_str(&format!(
            " · {}",
            theseus_protocol::job_follow::job_words(now.saturating_sub(*started), *usual)
        ));
    }
    s
}

/// The one command that answers a question: the first one pending, else the
/// execution explained.
fn command(v: &ExecutionView) -> String {
    match v.pending.first() {
        Some(p) => format!("theseus confirm {}", p.correlation_id),
        None => format!("theseus executions explain {}", short_id(&v.session_id)),
    }
}

/// `2:14`, or `1:02:03` past an hour.
fn clock(ms: u64) -> String {
    theseus_protocol::job_follow::clock(ms)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// `s` in at most `max` characters, the last one `…` when it is cut.
fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

// ------------------------------------------------------------------ titles

/// The names of the sessions the long form shows, until the view carries
/// them: one `session.list {ids}`. A task is named by its title, a
/// conversation by its label or title (`theseus_client::names`,
/// theseus-0n1v); one with neither is left out, for its kind.
async fn titles(conn: &mut Conn, board: &Board) -> Result<HashMap<String, String>> {
    let ids: Vec<String> = board
        .ordered()
        .iter()
        .take(MAX_ROWS)
        .map(|v| v.session_id.clone())
        .collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let v = conn
        .request(
            method::SESSION_LIST,
            SessionListParams {
                ids: Some(ids),
                ..Default::default()
            },
        )
        .await?;
    let r: SessionListResult = serde_json::from_value(v)?;
    Ok(r.sessions
        .iter()
        .filter(|s| names::words(s.kind, s.label.as_deref(), s.title.as_deref()).is_some())
        .map(|s| (s.session_id.clone(), names::of(s)))
        .collect())
}

// ------------------------------------------------------------------- watch

/// What the terminal tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabState {
    Idle,
    Working,
    NeedsYou,
}

impl TabState {
    fn of(s: &Summary) -> Self {
        if s.needs_you + s.failed > 0 {
            TabState::NeedsYou
        } else if s.working > 0 {
            TabState::Working
        } else {
            TabState::Idle
        }
    }
}

/// Windows Terminal's tab progress, OSC 9;4: the indeterminate ring (3), the
/// error state (2), or cleared (0); inside tmux (`tmux`), wrapped in its
/// passthrough, the escape inside it doubled.
pub fn tab_seq(state: TabState, tmux: bool) -> String {
    let osc = match state {
        TabState::Working => "\x1b]9;4;3;0\x07",
        TabState::NeedsYou => "\x1b]9;4;2;100\x07",
        TabState::Idle => "\x1b]9;4;0;0\x07",
    };
    if tmux {
        format!("\x1bPtmux;{}\x1b\\", osc.replace('\x1b', "\x1b\x1b"))
    } else {
        osc.to_string()
    }
}

/// What `--watch` prints a change as: the board's counts and its first item.
#[derive(PartialEq)]
enum Key {
    Down,
    Up(Summary),
}

/// Where `--watch` writes, and what it wrote last: a line only when the
/// board's key changes, the tab only when its state does.
pub struct Sink<'a> {
    out: &'a mut dyn Write,
    tab: Option<(&'a mut dyn Write, bool)>,
    short: bool,
    json: bool,
    last: Option<Key>,
    last_tab: Option<TabState>,
    /// `--notify`: the pings this watch makes (theseus-f0bi9); none when off.
    pub alerts: Option<Alerts>,
}

impl<'a> Sink<'a> {
    pub fn new(
        out: &'a mut dyn Write,
        tab: Option<(&'a mut dyn Write, bool)>,
        short: bool,
        json: bool,
    ) -> Self {
        Self {
            out,
            tab,
            short,
            json,
            last: None,
            last_tab: None,
            alerts: None,
        }
    }

    /// Show the board if what a line says of it changed.
    pub fn show(&mut self, board: &Board) -> Result<()> {
        let now = now_ms();
        let sum = board.summary(now, new_since_seen(), &displayed());
        // The short line has no wake in it, so a wake alone does not print one.
        let mut key = sum.clone();
        if self.short {
            key.wake_at_ms = None;
        }
        if self.last.as_ref() == Some(&Key::Up(key.clone())) {
            return Ok(());
        }
        let line = if self.json {
            serde_json::to_string(&board.json(&sum))?
        } else if self.short {
            short_line(&sum)
        } else {
            header_line(&sum, now, &local_hm)
        };
        writeln!(self.out, "{line}")?;
        self.out.flush()?;
        self.last = Some(Key::Up(key));
        self.set_tab(TabState::of(&sum))
    }

    /// The daemon is not there: an empty line, once, so a status bar does not
    /// keep the last thing it said.
    pub fn down(&mut self) -> Result<()> {
        if self.last.as_ref() == Some(&Key::Down) {
            return Ok(());
        }
        writeln!(self.out)?;
        self.out.flush()?;
        self.last = Some(Key::Down);
        self.set_tab(TabState::Idle)
    }

    /// Clear the tab, at exit.
    pub fn finish(&mut self) -> Result<()> {
        self.set_tab(TabState::Idle)
    }

    fn set_tab(&mut self, state: TabState) -> Result<()> {
        if self.last_tab == Some(state) {
            return Ok(());
        }
        if let Some((w, tmux)) = self.tab.as_mut() {
            w.write_all(tab_seq(state, *tmux).as_bytes())?;
            w.flush()?;
        }
        self.last_tab = Some(state);
        Ok(())
    }
}

/// One connection's watch: the board, then each change the push brings, until
/// the daemon closes the connection. With `--notify` it also wakes for the
/// soonest notice that is due, the one timer a watch has.
pub async fn watch_conn(conn: &mut Conn, sink: &mut Sink<'_>) -> Result<()> {
    let mut board = Board {
        track: sink.alerts.is_some(),
        ..Board::default()
    };
    board.reload(conn).await?;
    if let Some(a) = sink.alerts.as_mut() {
        a.saw(std::mem::take(&mut board.snapshot));
    }
    sink.show(&board)?;
    let mut queue: std::collections::VecDeque<(String, Value)> = Default::default();
    loop {
        let due = sink.alerts.as_ref().and_then(Alerts::next_due);
        let wake = async {
            match due {
                Some(at) => {
                    let wait = at.saturating_sub(now_ms());
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            msg = conn.next() => {
                let Some(msg) = msg? else { return Ok(()) };
                let theseus_protocol::Message::Notification(n) = msg else {
                    continue;
                };
                queue.push_back((n.method, n.params));
            }
            () = wake => {
                if let Some(a) = sink.alerts.as_mut() {
                    a.fire(now_ms(), &displayed(), &local_hm);
                }
            }
        }
        while let Some((m, p)) = queue.pop_front() {
            match Event::from_notification(&m, &p)? {
                Some(Event::ExecutionChanged(v)) => {
                    let newer = board.apply(v.clone());
                    if newer {
                        if let Some(a) = sink.alerts.as_mut() {
                            if a.needs_name(&v) {
                                let mut early = Vec::new();
                                let name = read_name(conn, &v.session_id, &mut early).await;
                                a.name(&v.session_id, name.unwrap_or_default());
                                queue.extend(early);
                            }
                            a.moved(v, now_ms(), &local_hm);
                        }
                        sink.show(&board)?;
                    }
                }
                // This client fell behind: read the board again.
                Some(Event::EventsLost(_)) => {
                    board.reload(conn).await?;
                    if let Some(a) = sink.alerts.as_mut() {
                        a.saw(std::mem::take(&mut board.snapshot));
                    }
                    sink.show(&board)?;
                }
                _ => {}
            }
        }
    }
}

async fn watch(socket: &str, spawn: Option<&str>, json: bool, a: &StatusArgs) -> Result<()> {
    let mut tty = if a.tab { tab_terminal() } else { None };
    let tmux = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty());
    let mut stdout = std::io::stdout();
    let mut sink = Sink::new(
        &mut stdout,
        tty.as_mut().map(|t| (t as &mut dyn Write, tmux)),
        a.short,
        json,
    );
    sink.alerts = Alerts::new(a.notify);
    let ended = tokio::select! {
        r = reconnecting(socket, spawn, &mut sink) => r,
        () = stop_signal() => Ok(()),
    };
    sink.finish()?;
    ended
}

/// Watch through the daemon's restarts: connect (within 100 ms), watch until
/// the connection ends, say it is down, wait a little, and again. A spawned
/// daemon is watched once.
async fn reconnecting(socket: &str, spawn: Option<&str>, sink: &mut Sink<'_>) -> Result<()> {
    let mut wait = Duration::from_millis(250);
    loop {
        let conn = match spawn {
            Some(bin) => Conn::spawn(bin),
            None => Conn::socket_within(socket, Some(CONNECT_WITHIN)).await,
        };
        match conn {
            Ok(mut conn) => {
                wait = Duration::from_millis(250);
                let ended = watch_conn(&mut conn, sink).await;
                if spawn.is_some() {
                    return ended;
                }
            }
            Err(e) if spawn.is_some() => return Err(e),
            Err(_) => {}
        }
        sink.down()?;
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(Duration::from_secs(5));
    }
}

/// The controlling terminal, for the tab's sequences: they are for the
/// terminal even when stdout is a pipe.
fn tab_terminal() -> Option<std::fs::File> {
    let tty = std::fs::OpenOptions::new().write(true).open("/dev/tty");
    if tty.is_err() {
        eprintln!("theseus: --tab found no terminal to write to; showing no tab progress");
    }
    tty.ok()
}

/// Until this process is asked to end, so the tab can be cleared.
async fn stop_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
        signal(SignalKind::hangup()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
        _ = hup.recv() => {}
    }
}

// ------------------------------------------------------------------ wait --any

/// `theseus wait --any --until blocked [--timeout D]`: return once any
/// execution needs you (a question, a block, a failure), over
/// `executions.watch`. One already waiting answers at once, a failure that was
/// already there excepted (it never clears). Exit 4 at the timeout, as `wait` does.
pub async fn wait_any(
    conn: &mut Conn,
    json: bool,
    until: &str,
    timeout_ms: Option<u64>,
) -> Result<()> {
    if until != "blocked" {
        bail!("`wait --any` waits for `--until blocked`: any session that needs you");
    }
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(600_000).min(86_400_000));
    let found = tokio::time::timeout(timeout, find_blocked(conn)).await;
    let result = match found {
        Ok(r) => r?,
        Err(_) => SessionWaitResult {
            reached: "timeout".into(),
            already: false,
            execution: None,
            confirms: vec![],
        },
    };
    if json {
        println!("{}", serde_json::to_value(&result)?);
    } else {
        println!("{}", render::waited_line(&result).text);
        let mut out = std::io::stdout().lock();
        for c in &result.confirms {
            crate::print::lines(&mut out, &render::confirm_lines(c))?;
        }
    }
    if result.reached == "timeout" {
        std::process::exit(4);
    }
    Ok(())
}

async fn find_blocked(conn: &mut Conn) -> Result<SessionWaitResult> {
    let mut board = Board::default();
    board.reload(conn).await?;
    // A failure that was there before this wait began is not news: it stays on
    // the board for good, and `until theseus wait --any; do :; done` would spin
    // on it. One that happens while waiting answers.
    let old: HashSet<String> = board.failed_ids();
    if let Some(first) = board.first_needing(&old) {
        return Ok(blocked(&board, first, true));
    }
    loop {
        let msg = conn
            .next()
            .await?
            .context("connection closed before anything needed you")?;
        let theseus_protocol::Message::Notification(n) = msg else {
            continue;
        };
        match Event::from_notification(&n.method, &n.params)? {
            Some(Event::ExecutionChanged(v)) => {
                if board.apply(v) {
                    if let Some(first) = board.first_needing(&old) {
                        return Ok(blocked(&board, first, false));
                    }
                }
            }
            Some(Event::EventsLost(_)) => {
                board.reload(conn).await?;
                if let Some(first) = board.first_needing(&old) {
                    return Ok(blocked(&board, first, false));
                }
            }
            _ => {}
        }
    }
}

fn blocked(board: &Board, view: ExecutionView, already: bool) -> SessionWaitResult {
    SessionWaitResult {
        reached: "blocked".into(),
        already,
        confirms: board
            .confirms
            .iter()
            .filter(|c| c.session_id == view.session_id)
            .cloned()
            .collect(),
        execution: Some(view),
    }
}

impl Board {
    /// The execution that has needed you longest, leaving out the failures in `old`.
    fn first_needing(&self, old: &HashSet<String>) -> Option<ExecutionView> {
        self.ordered()
            .into_iter()
            .find(|v| {
                v.attention.level == Level::NeedsYou
                    && !(group(v) == 1 && old.contains(&v.execution_id))
            })
            .cloned()
    }

    /// The executions that have failed, as the board holds them.
    fn failed_ids(&self) -> HashSet<String> {
        self.views
            .values()
            .filter(|v| group(v) == 1)
            .map(|v| v.execution_id.clone())
            .collect()
    }
}

// ------------------------------------------------------------------- clock

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A time of day on this machine's clock, `18:16`: the clients' one zone
/// reader (`render::time`, theseus-0n1v).
pub fn local_hm(unix_ms: u64) -> String {
    render::time::fmt_hm(unix_ms)
}

/// The terminal's width: its own, else `COLUMNS`, else 100.
fn width() -> usize {
    // SAFETY: `winsize` is plain data, and TIOCGWINSZ writes only it.
    let ws = unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        (libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0).then_some(ws)
    };
    match ws {
        Some(ws) if ws.ws_col > 0 => ws.ws_col as usize,
        _ => std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse().ok())
            .filter(|&c| c > 0)
            .unwrap_or(DEFAULT_WIDTH),
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
