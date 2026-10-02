//! The TUI over a scripted daemon (design `stage2` §2.9's tests): a JSON-RPC
//! stream inside the test, through `Conn::over` on a duplex, and ratatui's
//! `TestBackend`, whose buffer each test reads as text.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use serde_json::{json, Value};
use theseus_client::Conn;
use theseus_protocol::{
    attention, utc_hm, Attention, ConfirmRequest, ExecutionView, Level, PendingConfirm,
    SessionInfo, SessionKind, WaitingOn,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::app::App;
use crate::board::{short_label, Board, Only};
use crate::run::{Connector, Runner};
use crate::ui;

/// 2026-09-21 14:13:20 UTC, the tests' epoch.
pub const T0: u64 = 1_790_000_000_000;

thread_local! {
    /// The tests' wall clock: the loop runs on the test's thread.
    pub static NOW: Cell<u64> = const { Cell::new(T0) };
}

pub fn clock() -> u64 {
    NOW.with(Cell::get)
}

// ---------------------------------------------------------------- the daemon

/// What a scripted daemon answers a request: a result, or an error's code and
/// message.
pub type Answer = Result<Value, (i64, String)>;

/// The daemon's script: a method and its params, to an answer.
pub type Script = Arc<dyn Fn(&str, &Value) -> Answer + Send + Sync>;

/// A scripted daemon on the other end of one connection.
#[derive(Clone)]
pub struct Daemon {
    /// Every request it read, in order.
    pub requests: Arc<Mutex<Vec<Value>>>,
    /// Lines for it to write; `Value::Null` closes the connection.
    pub push: mpsc::UnboundedSender<Value>,
}

impl Daemon {
    /// The methods it was asked, in order.
    pub fn methods(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["method"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// The params of the requests for `method`, in order.
    pub fn asked(&self, method: &str) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == method)
            .map(|r| r["params"].clone())
            .collect()
    }

    pub fn notify(&self, method: &str, params: Value) {
        let _ = self
            .push
            .send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn close(&self) {
        let _ = self.push.send(Value::Null);
    }
}

/// A connection to a new scripted daemon.
pub fn daemon(script: Script) -> (Conn, Daemon) {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    let (r, w) = tokio::io::split(ours);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (push, mut pushed) = mpsc::unbounded_channel::<Value>();
    let seen = requests.clone();
    tokio::spawn(async move {
        let (dr, mut dw) = tokio::io::split(theirs);
        let mut lines = BufReader::new(dr).lines();
        loop {
            let line = tokio::select! {
                l = lines.next_line() => {
                    let Ok(Some(l)) = l else { break };
                    let req: Value = serde_json::from_str(&l).expect("a request");
                    seen.lock().unwrap().push(req.clone());
                    let m = req["method"].as_str().unwrap_or_default();
                    match script(m, &req["params"]) {
                        Ok(result) => json!({"jsonrpc": "2.0", "id": req["id"], "result": result}),
                        Err((code, message)) => json!({
                            "jsonrpc": "2.0", "id": req["id"],
                            "error": {"code": code, "message": message}
                        }),
                    }
                }
                p = pushed.recv() => match p {
                    Some(Value::Null) | None => break,
                    Some(v) => v,
                },
            };
            let mut text = line.to_string();
            text.push('\n');
            if dw.write_all(text.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    (Conn::over(r, w), Daemon { requests, push })
}

/// A daemon that knows a board: `executions.watch` answers `board`, and
/// `session.list` the titles in `titles`; every other request answers `{}`.
pub fn script(board: Arc<Mutex<Value>>, titles: Arc<Mutex<HashMap<String, Value>>>) -> Script {
    Arc::new(move |m, p| match m {
        "executions.watch" => Ok(board.lock().unwrap().clone()),
        "session.list" => {
            let titles = titles.lock().unwrap();
            let ids: Vec<String> = p["ids"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let sessions: Vec<Value> = ids
                .iter()
                .filter_map(|id| titles.get(id).cloned())
                .collect();
            Ok(json!({ "sessions": sessions }))
        }
        "confirm.list" => Ok(json!({ "confirms": [] })),
        _ => Ok(json!({})),
    })
}

// ---------------------------------------------------------------- the rig

/// The TUI on a `TestBackend`, its keys, and the daemons it connected to.
pub struct Rig {
    pub runner: Runner<TestBackend>,
    pub keys: mpsc::UnboundedSender<TermEvent>,
    pub daemons: Arc<Mutex<Vec<Daemon>>>,
    /// While false, a connect fails.
    pub up: Arc<AtomicBool>,
}

impl Rig {
    pub fn new(width: u16, height: u16, script: Script) -> Self {
        let daemons = Arc::new(Mutex::new(Vec::new()));
        let up = Arc::new(AtomicBool::new(true));
        let (d, u) = (daemons.clone(), up.clone());
        let connect: Connector = Box::new(move || {
            let made = if u.load(Ordering::SeqCst) {
                let (conn, daemon) = daemon(script.clone());
                d.lock().unwrap().push(daemon);
                Ok(conn)
            } else {
                Err(anyhow::anyhow!("nothing listens"))
            };
            Box::pin(async move { made })
        });
        let (keys, events) = mpsc::unbounded_channel();
        let term = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut runner = Runner::new(App::new(utc_hm), term, connect, events, clock);
        runner.frame = Duration::ZERO;
        Self {
            runner,
            keys,
            daemons,
            up,
        }
    }

    /// The newest daemon.
    pub fn daemon(&self) -> Daemon {
        self.daemons
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a daemon")
    }

    pub fn screen(&self) -> Vec<String> {
        ui::text(self.runner.term.backend().buffer())
    }

    /// Step the loop until `cond` holds, for at most 5 s.
    pub async fn until(&mut self, what: &str, cond: impl Fn(&Rig) -> bool) {
        let ok = tokio::time::timeout(Duration::from_secs(5), async {
            while !cond(self) {
                self.runner.step().await.unwrap();
            }
        })
        .await;
        if ok.is_err() {
            panic!(
                "timed out waiting for {what}; the screen:\n{}",
                self.screen().join("\n")
            );
        }
    }

    /// Step until the screen shows `text`.
    pub async fn shows(&mut self, text: &str) {
        let t = text.to_string();
        self.until(&format!("the screen to show {text:?}"), move |r| {
            r.screen().iter().any(|l| l.contains(&t))
        })
        .await;
    }

    /// Step until the newest daemon has read `n` requests for `method`.
    pub async fn asked(&mut self, method: &str, n: usize) {
        let m = method.to_string();
        self.until(&format!("{n} {method} request(s)"), move |r| {
            r.daemons
                .lock()
                .unwrap()
                .last()
                .is_some_and(|d| d.asked(&m).len() >= n)
        })
        .await;
    }

    /// Press keys, each handled before the next.
    pub async fn press(&mut self, keys: &[KeyCode]) {
        for k in keys {
            self.key(KeyEvent::new(*k, KeyModifiers::NONE)).await;
        }
    }

    /// One key event, handled before this returns.
    pub async fn key(&mut self, k: KeyEvent) {
        let n = self.runner.terminal_events + 1;
        self.keys.send(TermEvent::Key(k)).unwrap();
        self.until("a key to be handled", move |r| {
            r.runner.terminal_events >= n
        })
        .await;
    }

    /// Type a text, one key a character.
    pub async fn type_text(&mut self, text: &str) {
        let keys: Vec<KeyCode> = text.chars().map(KeyCode::Char).collect();
        self.press(&keys).await;
    }

    /// Let the loop take everything already sent to it: one step per message
    /// written so far, with a short bound on each.
    pub async fn settle(&mut self) {
        while tokio::time::timeout(Duration::from_millis(50), self.runner.step())
            .await
            .is_ok()
        {}
    }
}

// ---------------------------------------------------------------- fixtures

/// A view of `sid` at `position` (its time `T0 + position` seconds), with the
/// attention the server's function gives it.
#[allow(clippy::too_many_arguments)]
pub fn view(
    position: u64,
    sid: &str,
    kind: SessionKind,
    parent: Option<&str>,
    state: &str,
    waiting_on: Option<WaitingOn>,
    pending: Vec<PendingConfirm>,
    turns: u64,
    spent_usd: f64,
) -> ExecutionView {
    let mut v = ExecutionView {
        position,
        at_ms: T0 + position * 1000,
        execution_id: format!("exe_{sid}"),
        session_id: sid.to_string(),
        kind,
        parent_session_id: parent.map(String::from),
        state: state.to_string(),
        previous: None,
        waiting_on,
        pending,
        turns,
        outstanding: 0,
        spent_usd,
        limit_usd: 25.0,
        ended_reason: None,
        why: None,
        wake_at_ms: None,
        attention: Attention {
            level: Level::Idle,
            label: String::new(),
            since_ms: 0,
        },
    };
    v.attention = attention(&v, &utc_hm);
    v
}

pub fn conv(
    position: u64,
    sid: &str,
    state: &str,
    waiting: Option<WaitingOn>,
    spent: f64,
) -> ExecutionView {
    view(
        position,
        sid,
        SessionKind::Conversation,
        None,
        state,
        waiting,
        vec![],
        3,
        spent,
    )
}

pub fn task(position: u64, sid: &str, parent: &str, state: &str, spent: f64) -> ExecutionView {
    view(
        position,
        sid,
        SessionKind::Task,
        Some(parent),
        state,
        None,
        vec![],
        2,
        spent,
    )
}

pub fn pending(correlation_id: &str, reason: &str) -> PendingConfirm {
    PendingConfirm {
        correlation_id: correlation_id.to_string(),
        tool: "proc.run".to_string(),
        reason: reason.to_string(),
        floor: false,
        budget: false,
        expires_at_ms: 0,
    }
}

/// The whole question behind `pending(correlation_id, reason)`.
pub fn question(sid: &str, correlation_id: &str, reason: &str, at: u64) -> ConfirmRequest {
    ConfirmRequest {
        correlation_id: correlation_id.to_string(),
        session_id: sid.to_string(),
        execution_id: format!("exe_{sid}"),
        tool: "proc.run".to_string(),
        input: json!({"argv": ["scripts/gate.sh"]}),
        resource: None,
        reason: reason.to_string(),
        by: "policy".to_string(),
        requested_at_ms: T0 + at * 1000,
        expires_at_ms: 0,
        floor: false,
        budget: None,
        task: None,
        external_text: None,
    }
}

pub fn info(sid: &str, kind: SessionKind, label: Option<&str>, title: Option<&str>) -> SessionInfo {
    serde_json::from_value(json!({
        "session_id": sid,
        "kind": kind.as_str(),
        "label": label,
        "title": title,
        "created_at_unix_ms": T0,
        "turns": 1,
    }))
    .unwrap()
}

fn input() -> Option<WaitingOn> {
    Some(WaitingOn::Input)
}

/// The harbour's board: a DM with two tasks (one working, one done and
/// folded), a spec review waiting on a question, and old notes, cancelled.
pub fn harbour() -> (Value, HashMap<String, Value>) {
    let spec_q = vec![pending("cor_spec01", "run the gate")];
    let views = vec![
        conv(90, "ses_dm0001", "waiting", input(), 0.42),
        task(95, "ses_tide01", "ses_dm0001", "running", 0.10),
        task(80, "ses_sum001", "ses_dm0001", "complete", 0.05),
        view(
            98,
            "ses_spec01",
            SessionKind::Conversation,
            None,
            "waiting",
            Some(WaitingOn::Confirm {
                confirm_id: "cor_spec01".into(),
            }),
            spec_q,
            1,
            1.20,
        ),
        conv(50, "ses_old001", "cancelled", None, 0.30),
    ];
    let board = json!({
        "position": 100,
        "executions": views,
        "confirms": [question("ses_spec01", "cor_spec01", "run the gate", 98)],
        "total": 5,
    });
    let mut titles = HashMap::new();
    for (sid, kind, label, title) in [
        ("ses_dm0001", SessionKind::Conversation, Some("DM"), None),
        (
            "ses_tide01",
            SessionKind::Task,
            None,
            Some("check the tide tables"),
        ),
        (
            "ses_sum001",
            SessionKind::Task,
            None,
            Some("sum the ledger"),
        ),
        (
            "ses_spec01",
            SessionKind::Conversation,
            None,
            Some("spec review"),
        ),
        (
            "ses_old001",
            SessionKind::Conversation,
            None,
            Some("old notes"),
        ),
        (
            "ses_new001",
            SessionKind::Conversation,
            None,
            Some("fresh start"),
        ),
    ] {
        titles.insert(
            sid.to_string(),
            serde_json::to_value(info(sid, kind, label, title)).unwrap(),
        );
    }
    (board, titles)
}

pub fn harbour_rig(width: u16, height: u16) -> Rig {
    let (board, titles) = harbour();
    Rig::new(
        width,
        height,
        script(Arc::new(Mutex::new(board)), Arc::new(Mutex::new(titles))),
    )
}

fn changed(v: &ExecutionView) -> Value {
    serde_json::to_value(v).unwrap()
}

/// A sidebar row on an 80-column screen: its text, and its cost at the right
/// edge (one column of margin after it).
fn row80(text: &str, cost: &str) -> String {
    format!("{text:<74}{cost}")
}

/// The header on an 80-column screen, with the tests' clock and a live
/// connection at its right.
fn head80(left: &str) -> String {
    format!("{left:<62}14:13 · socket ok")
}

// ---------------------------------------------------------------- 10b

/// §3.2's test for 10b: a snapshot plus three events give the expected
/// buffer. A stale fourth event changes nothing (the position rule).
#[tokio::test]
async fn a_snapshot_and_three_events_draw_the_sidebar_and_its_tree() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("spec review").await;
    rig.shows("check the tide tables").await;
    assert_eq!(
        rig.screen()[..6],
        [
            head80(" theseus · 1 needs you"),
            row80(" ● confirm proc.run  spec review", "$1.20"),
            row80(" ○ ready  DM +1", "$0.42"),
            row80("   └ ◐ turn 2  check the tide tables", "$0.10"),
            row80(" · cancelled  old notes", "$0.30"),
            String::new(),
        ]
    );
    let d = rig.daemon();
    // One: the DM's turn starts.
    let mut dm = conv(101, "ses_dm0001", "running", None, 0.43);
    dm.turns = 4;
    dm.attention = attention(&dm, &utc_hm);
    d.notify("execution.changed", changed(&dm));
    // Two: the tide task asks a question.
    let tide = view(
        102,
        "ses_tide01",
        SessionKind::Task,
        Some("ses_dm0001"),
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: "cor_tide01".into(),
        }),
        vec![pending("cor_tide01", "push the branch")],
        2,
        0.11,
    );
    d.notify("execution.changed", changed(&tide));
    d.notify(
        "confirm.requested",
        serde_json::to_value(question("ses_tide01", "cor_tide01", "push the branch", 102)).unwrap(),
    );
    // Three: a session the board has not seen; its title is asked for.
    d.notify(
        "execution.changed",
        changed(&conv(103, "ses_new001", "waiting", input(), 0.0)),
    );
    rig.shows("fresh start").await;
    rig.shows("turn 4").await;
    rig.shows("2 need you").await;
    // A stale view of the DM, from before its turn: dropped.
    d.notify(
        "execution.changed",
        changed(&conv(99, "ses_dm0001", "waiting", input(), 0.40)),
    );
    rig.settle().await;
    assert_eq!(
        rig.screen()[..7],
        [
            head80(" theseus · 2 need you"),
            row80(" ● confirm proc.run  spec review", "$1.20"),
            row80(" ◐ turn 4  DM +1", "$0.43"),
            row80("   └ ● confirm proc.run  check the tide tables", "$0.11"),
            row80(" ○ ready  fresh start", "$0.00"),
            row80(" · cancelled  old notes", "$0.30"),
            String::new(),
        ]
    );
    assert_eq!(
        d.asked("session.list").last().unwrap()["ids"],
        json!(["ses_new001"]),
        "a new session's title is asked for by its id"
    );
}

/// At 160 columns the sidebar sits beside the detail pane; below 100 the list
/// takes the whole width.
#[tokio::test]
async fn a_wide_screen_puts_the_sidebar_beside_the_detail_pane() {
    let mut rig = harbour_rig(160, 12);
    rig.shows("spec review").await;
    rig.shows("check the tide tables").await;
    let screen = rig.screen();
    let side = ui::sidebar_width(160) as usize;
    assert_eq!(side, 48);
    for line in &screen[1..11] {
        assert_eq!(
            line.chars().nth(side),
            Some('│'),
            "the separator at column {side}: {line:?}"
        );
    }
    assert!(
        screen[1].starts_with(" ● confirm proc.run  spec review"),
        "{screen:?}"
    );
    assert!(screen[1].contains("$1.20 │ no session open"), "{screen:?}");
}

/// A daemon restart closes the socket: the header says so, the TUI tries
/// again with backoff, and reads the board again from the new daemon.
#[tokio::test(start_paused = true)]
async fn a_closed_connection_is_tried_again_and_the_board_read_again() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("spec review").await;
    rig.up.store(false, Ordering::SeqCst);
    rig.daemon().close();
    rig.shows("reconnecting (1)").await;
    rig.shows("reconnecting (2)").await;
    assert_eq!(rig.daemons.lock().unwrap().len(), 1, "no daemon answered");
    rig.up.store(true, Ordering::SeqCst);
    rig.shows("socket ok").await;
    rig.asked("executions.watch", 1).await;
    assert_eq!(rig.daemons.lock().unwrap().len(), 2);
    assert_eq!(
        rig.daemon().methods()[0],
        "executions.watch",
        "the board is read again first"
    );
    // The new daemon's board: the spec review was answered while it was down.
    let d = rig.daemon();
    let mut spec = conv(120, "ses_spec01", "running", None, 1.25);
    spec.attention = attention(&spec, &utc_hm);
    d.notify("execution.changed", changed(&spec));
    rig.shows("turn 3").await;
    assert!(!rig.screen()[0].contains("needs you"), "{:?}", rig.screen());
}

/// The first snapshot after a reconnect is the truth: a daemon restarted on
/// another store has lower positions, and sessions it does not hold go.
#[tokio::test(start_paused = true)]
async fn a_reconnect_takes_the_new_daemons_board_whole() {
    let (board, titles) = harbour();
    let board = Arc::new(Mutex::new(board));
    let mut rig = Rig::new(80, 24, script(board.clone(), Arc::new(Mutex::new(titles))));
    rig.shows("spec review").await;
    *board.lock().unwrap() = json!({
        "position": 7,
        "executions": [conv(7, "ses_new001", "waiting", input(), 0.0)],
        "confirms": [],
        "total": 1,
    });
    rig.daemon().close();
    rig.shows("fresh start").await;
    let screen = rig.screen();
    assert!(
        !screen
            .iter()
            .any(|l| l.contains("spec review") || l.contains("DM")),
        "{screen:?}"
    );
    assert!(screen[0].starts_with(" theseus · 1 session "), "{screen:?}");
}

/// `events.lost` (design §2.5): the TUI reads the board again, and the
/// position rule keeps what it already had.
#[tokio::test]
async fn events_lost_reads_the_board_again() {
    let (board, titles) = harbour();
    let board = Arc::new(Mutex::new(board));
    let mut rig = Rig::new(80, 24, script(board.clone(), Arc::new(Mutex::new(titles))));
    rig.shows("spec review").await;
    // While the TUI was behind, the spec review's question was answered.
    {
        let mut b = board.lock().unwrap();
        let spec = conv(130, "ses_spec01", "running", None, 1.30);
        let execs = b["executions"].as_array_mut().unwrap();
        for e in execs.iter_mut() {
            if e["session_id"] == "ses_spec01" {
                *e = changed(&spec);
            }
        }
        b["confirms"] = json!([]);
        b["position"] = json!(130);
    }
    rig.daemon().notify(
        "events.lost",
        json!({"dropped": 865, "streams": ["executions"]}),
    );
    rig.asked("executions.watch", 2).await;
    rig.shows("turn 3").await;
    let screen = rig.screen();
    assert!(
        screen[23].contains("lost 865 notifications"),
        "the footer says what was lost: {screen:?}"
    );
    assert!(!screen[0].contains("need"), "{screen:?}");
}

/// The keys of the list: move, filter by level, filter by text, and clear.
#[tokio::test]
async fn the_list_keys_move_and_filter() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("check the tide tables").await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('j')]).await;
    assert_eq!(
        rig.runner.app.selected.as_deref(),
        Some("ses_dm0001"),
        "j from nothing selects the first row, then the next"
    );
    rig.press(&[KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Up])
        .await;
    assert_eq!(
        rig.runner.app.selected.as_deref(),
        Some("ses_tide01"),
        "the cursor stops at the last row"
    );
    rig.press(&[KeyCode::Char('b')]).await;
    assert_eq!(rig.runner.app.only, Only::NeedsYou);
    let screen = rig.screen();
    assert!(screen[0].contains("only needs you"), "{screen:?}");
    assert!(
        screen[1].contains("spec review") && screen[2].is_empty(),
        "{screen:?}"
    );
    rig.press(&[KeyCode::Char('w')]).await;
    let screen = rig.screen();
    assert!(
        screen[1].contains("check the tide tables"),
        "a task shows flat: {screen:?}"
    );
    rig.press(&[KeyCode::Char('a'), KeyCode::Char('/')]).await;
    rig.type_text("tide").await;
    let screen = rig.screen();
    assert_eq!(screen[23], " /tide", "the filter is typed in the footer");
    assert!(
        screen[1].contains("check the tide tables") && screen[2].is_empty(),
        "{screen:?}"
    );
    rig.press(&[KeyCode::Enter]).await;
    assert!(rig.screen()[0].contains("· /tide"));
    rig.press(&[KeyCode::Esc]).await;
    assert!(
        rig.screen()[2].contains("DM"),
        "esc clears the filter: {:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('?')]).await;
    assert!(rig.screen().iter().any(|l| l.contains("b w r d a")));
    rig.press(&[KeyCode::Char('x')]).await;
    assert!(
        !rig.screen().iter().any(|l| l.contains("b w r d a")),
        "any key closes the help"
    );
    rig.press(&[KeyCode::Char('q')]).await;
    assert!(rig.runner.quitting());
}

// ---------------------------------------------------------------- the board

#[test]
fn the_position_rule_applies_only_a_newer_view() {
    let mut b = Board::default();
    assert!(b.apply(conv(10, "ses_a", "running", None, 0.0)));
    assert!(
        !b.apply(conv(10, "ses_a", "waiting", input(), 0.0)),
        "the same position"
    );
    assert!(
        !b.apply(conv(9, "ses_a", "waiting", input(), 0.0)),
        "an older one"
    );
    assert!(b.apply(conv(11, "ses_a", "waiting", input(), 0.0)));
    let rows = b.rows(Only::All, "", &|_| false);
    assert_eq!(
        (rows[0].level, rows[0].label.as_str()),
        (Level::Ready, "ready")
    );
}

/// A tree sorts by the most urgent session in it (the rollup), and a task
/// that needs you lifts its parent's tree above a working one.
#[test]
fn a_tree_sorts_by_its_most_urgent_session() {
    let mut b = Board::default();
    b.apply(conv(10, "ses_busy", "running", None, 0.0));
    b.apply(conv(5, "ses_parent", "waiting", input(), 0.0));
    b.apply(view(
        12,
        "ses_child",
        SessionKind::Task,
        Some("ses_parent"),
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: "c".into(),
        }),
        vec![pending("c", "x")],
        1,
        0.0,
    ));
    let order: Vec<String> = b
        .rows(Only::All, "", &|_| false)
        .into_iter()
        .map(|r| r.session_id)
        .collect();
    assert_eq!(order, ["ses_parent", "ses_child", "ses_busy"]);
}

#[test]
fn the_sidebar_says_a_question_by_its_tool() {
    let a = |label: &str| Attention {
        level: Level::NeedsYou,
        label: label.to_string(),
        since_ms: 0,
    };
    assert_eq!(
        short_label(&a("confirm proc.run: run cargo test · floor")),
        "confirm proc.run"
    );
    assert_eq!(short_label(&a("confirm fs.write")), "confirm fs.write");
    assert_eq!(short_label(&a("budget: $10.02 of $10")), "budget");
    assert_eq!(short_label(&a("failed: the provider refused")), "failed");
    assert_eq!(
        short_label(&a("sleeping until 16:00")),
        "sleeping until 16:00"
    );
}

/// A question asked before its view arrives is kept; a view newer than the
/// question that no longer lists it closes it.
#[test]
fn a_question_follows_its_sessions_view() {
    let mut b = Board::default();
    b.apply(conv(10, "ses_a", "running", None, 0.0));
    b.confirm_requested(question("ses_a", "c1", "run it", 11));
    assert_eq!(b.confirm_ids("ses_a"), ["c1"], "asked after the view: kept");
    let mut asking = view(
        11,
        "ses_a",
        SessionKind::Conversation,
        None,
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: "c1".into(),
        }),
        vec![pending("c1", "run it")],
        1,
        0.0,
    );
    asking.at_ms = T0 + 11_000;
    b.apply(asking);
    assert_eq!(b.confirm_ids("ses_a"), ["c1"]);
    b.apply(conv(12, "ses_a", "running", None, 0.0));
    assert!(
        b.confirm_ids("ses_a").is_empty(),
        "a newer view without it closed it"
    );
    b.confirm_requested(question("ses_a", "c1", "run it", 11));
    assert!(
        b.confirm_ids("ses_a").is_empty(),
        "a late copy of a closed question stays closed"
    );
}
