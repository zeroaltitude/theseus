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
    attention, utc_hm, Attention, BudgetAsk, ConfirmRequest, ExecutionStopResult, ExecutionView,
    Level, PendingConfirm, SessionInfo, SessionKind, TurnSubmitResult, WaitingOn,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::app::App;
use crate::board::{short_label, Board, Only};
use crate::notice::{Delivery, Kind, Notices};
use crate::run::{Connector, Runner};
use crate::seen::Seen;
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

/// What a scripted daemon knows, and how it answers.
#[derive(Default)]
pub struct World {
    /// `executions.watch`'s answer.
    pub board: Value,
    /// `session.list`'s rows, by session.
    pub titles: HashMap<String, Value>,
    /// `session.history`'s answers, by session.
    pub histories: HashMap<String, Value>,
    /// A method's answer, in place of `{}`.
    pub answers: HashMap<String, Answer>,
}

/// A daemon that answers from `world`: `executions.watch` its board,
/// `session.list` and `session.history` what it holds for the sessions asked,
/// and any other method its canned answer, else `{}`.
pub fn script(world: Arc<Mutex<World>>) -> Script {
    Arc::new(move |m, p| {
        let w = world.lock().unwrap();
        if let Some(a) = w.answers.get(m) {
            return a.clone();
        }
        match m {
            "executions.watch" => Ok(w.board.clone()),
            "session.list" => {
                let ids: Vec<&str> = p["ids"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let sessions: Vec<Value> = ids
                    .iter()
                    .filter_map(|id| w.titles.get(*id).cloned())
                    .collect();
                Ok(json!({ "sessions": sessions }))
            }
            "session.history" => {
                let sid = p["session_id"].as_str().unwrap_or_default();
                Ok(w.histories.get(sid).cloned().unwrap_or_else(|| {
                    json!({
                        "session": w.titles.get(sid).cloned().unwrap_or(json!({
                            "session_id": sid, "kind": "conversation", "label": null,
                            "created_at_unix_ms": T0, "turns": 0,
                        })),
                        "nodes": [],
                    })
                }))
            }
            "confirm.list" => Ok(json!({ "confirms": [] })),
            _ => Ok(json!({})),
        }
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

/// A conversation just opened: waiting on its first input, no turn taken.
pub fn opened(position: u64, sid: &str) -> ExecutionView {
    view(
        position,
        sid,
        SessionKind::Conversation,
        None,
        "waiting",
        input(),
        vec![],
        0,
        0.0,
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

/// The harbour's world: its board, its titles, and the DM's history.
pub fn harbour_world() -> Arc<Mutex<World>> {
    let (board, titles) = harbour();
    let mut histories = HashMap::new();
    histories.insert("ses_dm0001".to_string(), dm_history());
    Arc::new(Mutex::new(World {
        board,
        titles,
        histories,
        answers: HashMap::new(),
    }))
}

pub fn harbour_rig(width: u16, height: u16) -> Rig {
    Rig::new(width, height, script(harbour_world()))
}

/// The DM's recorded history: the operator's question from Discord, a call to
/// read the tide table and its result, and the answer.
pub fn dm_history() -> Value {
    let node = |id: &str, kind: &str, pos: u64, author: Option<&str>, text: &str, detail: Value| {
        json!({
            "node_id": id, "kind": kind, "session_id": "ses_dm0001", "position": pos,
            "at_unix_ms": T0 + pos * 1000, "author": author, "text": text, "detail": detail,
            "bytes": text.len(),
        })
    };
    let mut session = info("ses_dm0001", SessionKind::Conversation, Some("DM"), None);
    session.model = Some("glm-5.3-flash".to_string());
    json!({
        "session": session,
        "nodes": [
            node("nod_a1", "user_message", 10, Some("discord:eddie"), "when is low water?", json!({})),
            node("nod_a2", "assistant_message", 11, None, "", json!({
                "model": "glm-5.3-flash", "stop_reason": "tool_use",
                "usage": {"input_tokens": 1200, "output_tokens": 40}, "cost_usd": 0.0004,
                "tool_calls": [{"name": "fs_read"}],
            })),
            node("nod_a3", "tool_call", 11, None, "", json!({
                "tool": "fs.read", "input": {"path": "tides.txt"},
                "decision": {"posture": "open", "reason": "a read"},
            })),
            node("nod_a4", "tool_result", 12, None, "low 14:10, high 20:30", json!({
                "tool": "fs.read", "status": "ok", "duration_ms": 3,
            })),
            node("nod_a5", "assistant_message", 14, None, "Low water is at 14:10.", json!({
                "model": "glm-5.3-flash", "stop_reason": "end_turn",
                "usage": {"input_tokens": 1300, "output_tokens": 12}, "cost_usd": 0.0003,
            })),
        ],
        "pending_confirms": [],
    })
}

fn changed(v: &ExecutionView) -> Value {
    serde_json::to_value(v).unwrap()
}

/// Put `v` in a board's snapshot, in place of its session's row.
pub fn replace_view(board: &mut Value, v: &ExecutionView) {
    let execs = board["executions"].as_array_mut().unwrap();
    match execs
        .iter_mut()
        .find(|e| e["session_id"] == v.session_id.as_str())
    {
        Some(e) => *e = changed(v),
        None => execs.push(changed(v)),
    }
}

/// The detail pane's rows on a wide screen: each from the column after the
/// separator.
pub fn pane(screen: &[String], width: u16) -> Vec<String> {
    let side = ui::sidebar_width(width) as usize;
    screen
        .iter()
        .map(|l| l.chars().skip(side + 1).collect::<String>())
        .collect()
}

/// Focus the DM: down to its row (the second), then enter.
pub async fn open_dm(rig: &mut Rig) {
    // The titles are in: the DM's row has its name and its folded task.
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Enter])
        .await;
    rig.shows("Low water is at 14:10.").await;
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
    d.notify("execution.changed", changed(&opened(103, "ses_new001")));
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
    let world = harbour_world();
    let mut rig = Rig::new(80, 24, script(world.clone()));
    rig.shows("spec review").await;
    world.lock().unwrap().board = json!({
        "position": 7,
        "executions": [opened(7, "ses_new001")],
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
    let world = harbour_world();
    let mut rig = Rig::new(80, 24, script(world.clone()));
    rig.shows("spec review").await;
    // While the TUI was behind, the spec review's question was answered.
    {
        let mut w = world.lock().unwrap();
        replace_view(
            &mut w.board,
            &conv(130, "ses_spec01", "running", None, 1.30),
        );
        w.board["confirms"] = json!([]);
        w.board["position"] = json!(130);
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
    let first = b.apply(conv(10, "ses_a", "running", None, 0.0));
    assert_eq!(first.map(|m| m.before), Some(None), "a session first seen");
    assert!(
        b.apply(conv(10, "ses_a", "waiting", input(), 0.0))
            .is_none(),
        "the same position"
    );
    assert!(
        b.apply(conv(9, "ses_a", "waiting", input(), 0.0)).is_none(),
        "an older one"
    );
    let moved = b.apply(conv(11, "ses_a", "waiting", input(), 0.0));
    assert_eq!(moved.map(|m| m.before), Some(Some(Level::Working)));
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

// ---------------------------------------------------------------- 10c

/// §3.2's test for 10c: the detail pane from a recorded history, then the
/// session's events as they come; the reply streams into its line.
#[tokio::test]
async fn the_detail_pane_shows_a_recorded_history_then_its_events() {
    let mut rig = harbour_rig(120, 20);
    open_dm(&mut rig).await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("session.history")[0],
        json!({"session_id": "ses_dm0001", "n": 200})
    );
    assert_eq!(
        d.asked("session.watch")[0],
        json!({"session_id": "ses_dm0001"})
    );
    d.notify(
        "turn.started",
        json!({"session_id": "ses_dm0001", "turn_id": "turn_b2", "execution_id": "exe_ses_dm0001"}),
    );
    for piece in [
        "High water",
        " is at 20:30.\nThe tide turns",
        " at dusk.",
        "",
    ] {
        d.notify(
            "model.delta",
            json!({"turn_id": "turn_b2", "loop_index": 0, "text": piece}),
        );
    }
    d.notify(
        "tool.started",
        json!({"session_id": "ses_dm0001", "turn_id": "turn_b2", "tool_use_id": "tu_1",
               "tool": "fs.list", "correlation_id": "cor_l1", "backend": "in_process"}),
    );
    d.notify(
        "tool.ended",
        json!({"session_id": "ses_dm0001", "turn_id": "turn_b2", "tool_use_id": "tu_1",
               "tool": "fs.list", "status": "ok", "duration_ms": 2, "bytes": 80}),
    );
    let ended = TurnSubmitResult {
        session_id: "ses_dm0001".into(),
        turn_id: "turn_b2".into(),
        loops: 2,
        stop_reason: "end_turn".into(),
        model: "glm-5.3-flash".into(),
        provider: "zai".into(),
        profile: "glm".into(),
        elapsed_ms: 1840,
        tool_calls: 1,
        ..Default::default()
    };
    d.notify("turn.ended", serde_json::to_value(&ended).unwrap());
    rig.shows("1840 ms").await;
    let rows = pane(&rig.screen(), 120);
    assert_eq!(
        rows[1..16],
        [
            " ses …dm0001 · DM · glm-5.3-flash · ○ ready",
            " [14:13:30.000Z] operator (discord:eddie): when is low water?",
            " [14:13:31.000Z] glm-5.3-flash: (1 tool call(s))",
            "       ↳ tool_use · in 1200 out 40 · $0.0004",
            "       ⚙ fs.read {\"path\":\"tides.txt\"} [open: a read]",
            "       ← fs.read ok · 3 ms · 21 B: low 14:10, high 20:30",
            " [14:13:34.000Z] glm-5.3-flash: Low water is at 14:10.",
            "       ↳ end_turn · in 1300 out 12 · $0.0003",
            " ── turn turn_b2",
            // The reply streamed in three pieces, one with a newline.
            " High water is at 20:30.",
            " The tide turns at dusk.",
            "   → fs.list",
            "   ← fs.list ok · 2 ms · 80 B",
            // The status line, wrapped at a space to the pane's width.
            " [glm → zai/glm-5.3-flash · 2 loop(s) · 1 tool call(s) · end_turn · tokens in 0 out",
            " 0 · 1840 ms · session ses_dm0001]",
        ]
    );
    // Another session's turn, which this connection still hears (it asked for
    // it before the focus moved), stays out of the pane.
    d.notify(
        "turn.started",
        json!({"session_id": "ses_spec01", "turn_id": "turn_x9"}),
    );
    d.notify(
        "model.delta",
        json!({"turn_id": "turn_x9", "loop_index": 0, "text": "not for this pane"}),
    );
    rig.settle().await;
    assert!(
        !rig.screen().iter().any(|l| l.contains("not for this pane")),
        "{:?}",
        rig.screen()
    );
}

/// A turn a refusal's fallback answered says so in the pane, above its status
/// line, in the words every surface uses (theseus-7gir.18).
#[tokio::test]
async fn the_detail_pane_says_a_refusals_fallback_answered() {
    let mut rig = harbour_rig(120, 20);
    open_dm(&mut rig).await;
    let d = rig.daemon();
    d.notify(
        "turn.started",
        json!({"session_id": "ses_dm0001", "turn_id": "turn_f1", "execution_id": "exe_ses_dm0001"}),
    );
    let ended = TurnSubmitResult {
        session_id: "ses_dm0001".into(),
        turn_id: "turn_f1".into(),
        loops: 2,
        stop_reason: "no_tool_calls".into(),
        model: "claude-sonnet-5".into(),
        provider: "anthropic".into(),
        profile: "sonnet".into(),
        elapsed_ms: 1840,
        fallback: Some(theseus_protocol::route::TurnFallback {
            from: "claude-sonnet-5-5".into(),
            to: "claude-sonnet-5".into(),
            category: Some("cyber".into()),
            answered: true,
        }),
        ..Default::default()
    };
    d.notify("turn.ended", serde_json::to_value(&ended).unwrap());
    let line = "Sonnet 5.5 declined (cyber); Sonnet 5 answered.";
    rig.shows(line).await;
    let rows = pane(&rig.screen(), 120);
    let at = rows.iter().position(|r| r.contains(line)).unwrap();
    assert!(
        rows[at + 1].starts_with(" [sonnet → anthropic/claude-sonnet-5 · 2 loop(s)"),
        "{rows:?}"
    );
}

#[test]
fn a_long_line_wraps_at_its_last_space_that_fits() {
    assert_eq!(ui::wrap("low water at 14:10", 9), ["low water", "at 14:10"]);
    assert_eq!(ui::wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(ui::wrap("", 4), [""]);
}

/// The input line sends `turn.submit` to the focused session, as the TUI;
/// below 100 columns the session's pane takes the screen, and `esc` goes
/// back to the list.
#[tokio::test]
async fn the_input_line_sends_a_turn_to_the_focused_session() {
    let mut rig = harbour_rig(80, 24);
    rig.shows("check the tide tables").await;
    rig.press(&[KeyCode::Char('i')]).await;
    assert!(
        rig.screen()[23].contains("open a session first"),
        "{:?}",
        rig.screen()
    );
    open_dm(&mut rig).await;
    assert!(
        !rig.screen().iter().any(|l| l.contains("spec review")),
        "the pane takes a narrow screen: {:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("and tomorrow?").await;
    assert_eq!(rig.screen()[23], " > and tomorrow?");
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("turn.submit")[0],
        json!({"session_id": "ses_dm0001", "input": "and tomorrow?", "author": "the TUI"})
    );
    assert!(
        rig.screen()
            .iter()
            .any(|l| l.trim() == "you: and tomorrow?"),
        "{:?}",
        rig.screen()
    );
    // Its own message is not read again; another surface's is.
    d.notify(
        "node.written",
        json!({"session_id": "ses_dm0001", "node_id": "nod_b1", "kind": "user_message"}),
    );
    rig.settle().await;
    assert_eq!(
        d.asked("session.history").len(),
        1,
        "its own message is not read again"
    );
    d.notify(
        "node.written",
        json!({"session_id": "ses_dm0001", "node_id": "nod_a1", "kind": "user_message"}),
    );
    rig.asked("session.history", 2).await;
    rig.press(&[KeyCode::Esc]).await;
    rig.shows("spec review").await;
}

/// `s` stops a conversation and `c` cancels a task; each asks for a second
/// key, and any other key says no.
#[tokio::test]
async fn stop_and_cancel_each_ask_for_a_second_key() {
    let world = harbour_world();
    let stopped = ExecutionStopResult {
        execution: serde_json::from_value(json!({
            "execution_id": "exe_ses_dm0001", "session_id": "ses_dm0001", "kind": "conversation",
            "state": "waiting", "turns": 3, "interrupted": 0, "outstanding": 0, "queued_results": 0,
            "budget": {"limit_usd": 25.0, "spent_usd": 0.42, "reserved_usd": 0.0,
                       "held_unknown_usd": 0.0, "available_usd": 24.58},
            "created_at_ms": T0, "updated_at_ms": T0,
        }))
        .unwrap(),
        stopped: true,
        stopped_actions: vec!["cor_job1".into()],
        verdicts: vec![],
        declined: vec![],
        turn_running: true,
        tasks_running: 1,
        wakes_pending: 0,
    };
    world.lock().unwrap().answers.insert(
        "execution.stop".into(),
        Ok(serde_json::to_value(&stopped).unwrap()),
    );
    let mut rig = Rig::new(120, 24, script(world));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('c')]).await;
    assert!(
        rig.screen()[23].contains("only a task is cancelled"),
        "{:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('s')]).await;
    assert!(
        rig.screen()[23].contains("stop DM (…dm0001)? s again"),
        "{:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('x')]).await;
    assert!(rig.screen()[23].contains("not stopped"));
    rig.press(&[KeyCode::Char('s'), KeyCode::Char('s')]).await;
    rig.asked("execution.stop", 1).await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("execution.stop"),
        [json!({"execution_id": "exe_ses_dm0001", "author": "the TUI"})]
    );
    rig.shows("stopped session ses_dm0001's work").await;
    // A task: `s` says how it ends; `c c` cancels it.
    rig.press(&[KeyCode::Char('j'), KeyCode::Enter]).await;
    rig.shows("ses …tide01").await;
    rig.press(&[KeyCode::Char('s')]).await;
    assert!(rig.screen()[23].contains("a task is not stopped but cancelled"));
    rig.press(&[KeyCode::Char('c'), KeyCode::Char('c')]).await;
    rig.asked("task.cancel", 1).await;
    assert_eq!(
        d.asked("task.cancel"),
        [json!({"task": "ses_tide01", "author": "the TUI"})]
    );
    assert_eq!(
        d.asked("session.unwatch"),
        [json!({"session_id": "ses_dm0001"})],
        "the focus moved: the DM is unwatched"
    );
}

// ---------------------------------------------------------------- 10d

/// `proc.run scripts/gate.sh`, asked at `T0 + at` seconds, expiring at
/// `T0 + expires` seconds.
pub fn gate_question(sid: &str, correlation_id: &str, at: u64, expires: u64) -> ConfirmRequest {
    let mut q = question(sid, correlation_id, "run the gate", at);
    q.expires_at_ms = T0 + expires * 1000;
    q
}

/// The harbour, with the tide task asking too (later than the spec review).
pub fn two_questions() -> Arc<Mutex<World>> {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let tide = view(
            99,
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
        replace_view(&mut w.board, &tide);
        w.board["confirms"].as_array_mut().unwrap().push(
            serde_json::to_value(question("ses_tide01", "cor_tide01", "push the branch", 99))
                .unwrap(),
        );
    }
    world
}

pub fn focused(rig: &Rig) -> Option<String> {
    rig.runner.app.detail.as_ref().map(|d| d.session_id.clone())
}

/// The queue's order (design §2.2): needs you, the longest waiting first.
/// `tab` walks it with each session's pane open, and wraps; `shift-tab`
/// walks it back.
#[tokio::test]
async fn tab_walks_the_queue_longest_waiting_first() {
    let mut rig = Rig::new(120, 20, script(two_questions()));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    assert_eq!(
        focused(&rig).as_deref(),
        Some("ses_spec01"),
        "the longest waiting first"
    );
    rig.press(&[KeyCode::Tab]).await;
    assert_eq!(focused(&rig).as_deref(), Some("ses_tide01"));
    rig.press(&[KeyCode::Tab]).await;
    assert_eq!(focused(&rig).as_deref(), Some("ses_spec01"), "it wraps");
    rig.press(&[KeyCode::BackTab]).await;
    assert_eq!(
        focused(&rig).as_deref(),
        Some("ses_tide01"),
        "shift-tab walks back"
    );
}

/// The card: the tool and its input, the reason, the floor, the countdown,
/// and the keys. `y` answers as the TUI, over the `cli` channel's method.
#[tokio::test]
async fn the_card_shows_the_question_and_y_approves_it_as_the_tui() {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
        q.floor = true;
        w.board["confirms"] = json!([q]);
    }
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("expires in 4:12").await;
    let rows = pane(&rig.screen(), 120);
    // The pane's foot: a rule as wide as the pane (83 columns), then the card.
    // The countdown and the floor come before the reason, which a long path
    // would cut at the pane's edge (the live check, 21:18).
    assert_eq!(
        rows[14..19],
        [
            "─".repeat(83),
            " ⏸ confirm proc.run: scripts/gate.sh".to_string(),
            "   expires in 4:12 · FLOOR".to_string(),
            "   why: run the gate".to_string(),
            "   [y] approve  [n] decline".to_string(),
        ]
    );
    // A second later, the countdown has moved.
    NOW.with(|n| n.set(T0 + 99_000));
    rig.runner.draw().unwrap();
    assert!(rig.screen().iter().any(|l| l.contains("expires in 4:11")));
    rig.press(&[KeyCode::Char('t')]).await;
    assert!(
        rig.screen()[19].contains("nothing to trust"),
        "{:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('y')]).await;
    rig.asked("action.confirm", 1).await;
    assert_eq!(
        rig.daemon().asked("action.confirm"),
        [json!({"correlation_id": "cor_spec01", "approve": true, "author": "the TUI"})]
    );
}

/// A long reason wraps at the pane's edge, its later rows under its text, so
/// it is read to its end, where a shared place's clause stands (theseus-94a6);
/// one row cut it at the edge before. A reason past five rows keeps its first
/// row, `…`, and its last three, which say why.
#[tokio::test]
async fn a_long_reason_wraps_and_is_read_to_its_end() {
    let reason =
        "fetch http://127.0.0.1:7455/notes: http.fetch — approve (127.0.0.1 is a loopback \
                  address, and a private address waits for approval; this is a shared place, so \
                  the page joins a conversation others can read)";
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut q = gate_question("ses_spec01", "cor_spec01", 98, 98 + 252);
        q.reason = reason.into();
        w.board["confirms"] = json!([q]);
    }
    NOW.with(|n| n.set(T0 + 98_000));
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("expires in 4:12").await;
    let rows = pane(&rig.screen(), 120);
    let why = rows.iter().position(|r| r.starts_with("   why: ")).unwrap();
    let keys = rows
        .iter()
        .position(|r| r.starts_with("   [y] approve"))
        .unwrap();
    let card: Vec<&str> = rows[why..keys].iter().map(|r| r.trim_end()).collect();
    assert_eq!(card.len(), 3, "{card:?}");
    assert!(
        card[1..]
            .iter()
            .all(|r| r.starts_with("   ") && !r.starts_with("    ")),
        "the later rows are as indented as the line: {card:?}"
    );
    assert_eq!(
        card.iter().map(|r| r.trim()).collect::<Vec<_>>().join(" "),
        format!("why: {reason}"),
        "the whole reason, in order"
    );

    let long = format!(
        "  why: run `{}` in /harbour: proc.run — approve (`bash` matches the approve list entry `bash`)",
        "tide ".repeat(80)
    );
    let shown = ui::card_rows(&long, 82);
    assert_eq!(shown.len(), 5, "{shown:?}");
    assert!(shown[0].starts_with("  why: run `tide tide"), "{shown:?}");
    assert_eq!(shown[1], "  …");
    let end: Vec<&str> = shown[2..].iter().map(|r| r.trim()).collect();
    assert!(
        end.join(" ")
            .ends_with("proc.run — approve (`bash` matches the approve list entry `bash`)"),
        "the end, which says why: {shown:?}"
    );
}

/// `n` asks for a note, and the decline carries it; `t` approves and trusts a
/// session that holds external text.
#[tokio::test]
async fn n_declines_with_a_note_and_t_trusts_external_text() {
    let world = two_questions();
    {
        let mut w = world.lock().unwrap();
        let confirms = w.board["confirms"].as_array_mut().unwrap();
        confirms[1]["external_text"] = json!({
            "since_ms": T0, "tool": "web.search", "url": "", "node_id": "nod_s1",
            "query": "harbour tide tables",
        });
    }
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab, KeyCode::Char('n')]).await;
    rig.type_text("not before the review").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("action.confirm", 1).await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("[t] approve + trust").await;
    assert!(rig
        .screen()
        .iter()
        .any(|l| l.contains("web.search \"harbour tide tables\"")));
    rig.press(&[KeyCode::Char('t')]).await;
    rig.asked("action.confirm", 2).await;
    assert_eq!(
        rig.daemon().asked("action.confirm"),
        [
            json!({"correlation_id": "cor_spec01", "approve": false, "author": "the TUI",
                   "note": "not before the review"}),
            json!({"correlation_id": "cor_tide01", "approve": true, "author": "the TUI",
                   "trust": true}),
        ]
    );
}

/// An answer that does not count (-32005) shows its reason on the card, and
/// the question stays.
#[tokio::test]
async fn a_refusal_shows_its_reason_on_the_card() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "action.confirm".into(),
        Err((
            -32005,
            "it came from a shared place, and only a private one counts".into(),
        )),
    );
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab, KeyCode::Char('y')]).await;
    rig.shows("refused: it came from a shared place").await;
    assert!(
        rig.screen()
            .iter()
            .any(|l| l.contains("⏸ confirm proc.run")),
        "the question stays"
    );
}

/// A budget question's card: spent, limit, needed, lifetime, and what an
/// approval does.
#[tokio::test]
async fn a_budget_question_says_what_approving_does() {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut q = question(
            "ses_spec01",
            "cor_spec01",
            "the session reached its limit",
            98,
        );
        q.tool = "budget.reset".into();
        q.budget = Some(BudgetAsk {
            spent_usd: 10.02,
            limit_usd: 10.0,
            needed_usd: 0.5,
            lifetime_usd: 42.0,
        });
        w.board["confirms"] = json!([q]);
        let mut spec = view(
            98,
            "ses_spec01",
            SessionKind::Conversation,
            None,
            "waiting",
            Some(WaitingOn::Budget {
                correlation_id: "cor_spec01".into(),
            }),
            vec![PendingConfirm {
                budget: true,
                tool: "budget.reset".into(),
                ..pending("cor_spec01", "the session reached its limit")
            }],
            1,
            10.02,
        );
        spec.limit_usd = 10.0;
        spec.attention = attention(&spec, &utc_hm);
        replace_view(&mut w.board, &spec);
    }
    let mut rig = Rig::new(120, 20, script(world));
    rig.shows("ready  DM +1").await;
    rig.press(&[KeyCode::Tab]).await;
    rig.shows("$ budget: spent $10.02 of $10 · needs $0.50 · lifetime $42")
        .await;
    assert!(rig
        .screen()
        .iter()
        .any(|l| l.contains("approve resets its spend to $0")));
    assert!(rig
        .screen()
        .iter()
        .any(|l| l.contains("[y] reset and continue  [n] keep waiting")));
}

// ---------------------------------------------------------------- 10e

/// A notice waits out its second and is checked against the latest view: a
/// session that finished and went back to work within the second gives
/// none; one that stays finished gives one, once.
#[test]
fn a_notice_is_debounced_and_checked_again_before_it_fires() {
    let mut n = Notices::default();
    let ready = conv(11, "ses_a", "waiting", input(), 0.0);
    let again = conv(12, "ses_a", "running", None, 0.0);
    n.moved("ses_a", Some(Level::Working), &ready, 1_000);
    assert_eq!(n.next_due(), Some(2_000));
    // Back to work at 1.5 s: at 2 s the latest view no longer holds it.
    n.moved("ses_a", Some(Level::Ready), &again, 1_500);
    let latest = again.clone();
    assert!(n
        .take_due(2_000, &|_| Some(latest.clone()), &|_| false)
        .is_empty());
    // Finished again, and it stays: one notice, once.
    n.moved("ses_a", Some(Level::Working), &ready, 3_000);
    let latest = ready.clone();
    assert!(
        n.take_due(3_999, &|_| Some(latest.clone()), &|_| false)
            .is_empty(),
        "not before its second"
    );
    assert_eq!(
        n.take_due(4_000, &|_| Some(latest.clone()), &|_| false),
        [("ses_a".to_string(), Kind::Finished)]
    );
    assert!(
        n.take_due(9_000, &|_| Some(latest.clone()), &|_| false)
            .is_empty(),
        "once"
    );
}

/// No notice for the session in focus while the terminal has focus; one when
/// the terminal lost it.
#[test]
fn the_focused_session_gets_no_notice_while_the_terminal_has_focus() {
    let mut n = Notices::default();
    let ready = conv(11, "ses_a", "waiting", input(), 0.0);
    n.moved("ses_a", Some(Level::Working), &ready, 0);
    let latest = ready.clone();
    assert!(n
        .take_due(1_000, &|_| Some(latest.clone()), &|_| true)
        .is_empty());
    n.moved("ses_a", Some(Level::Working), &ready, 2_000);
    assert_eq!(
        n.take_due(3_000, &|_| Some(latest.clone()), &|_| false)
            .len(),
        1
    );
}

#[test]
fn a_notice_reaches_the_terminal_as_its_delivery_says() {
    assert_eq!(Delivery::Bell.bytes("DM finished"), b"\x07");
    assert_eq!(
        Delivery::Osc9.bytes("DM finished"),
        b"\x1b]9;theseus: DM finished\x07"
    );
    assert_eq!(
        Delivery::Osc777.bytes("DM\x1b finished"),
        b"\x1b]777;notify;theseus;DM finished\x07",
        "no control character reaches the sequence"
    );
    assert!(Delivery::Off.bytes("DM finished").is_empty());
    assert_eq!(Delivery::parse("osc777"), Some(Delivery::Osc777));
    assert_eq!(Delivery::parse("loud"), None);
}

/// A writer the tests read back: the terminal's own sequences (the bell, a
/// notice, the title).
#[derive(Clone, Default)]
pub struct Captured(pub Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// §3.2's live check for 10e, in a test: a background task finishes; its row
/// says ◆ done, the header counts it, the title carries the count, and the
/// notice waits out its second, then rings once.
#[tokio::test]
async fn a_finished_task_is_done_counted_and_rung_once() {
    let mut rig = harbour_rig(80, 24);
    let out = Captured::default();
    rig.runner.out = Box::new(out.clone());
    rig.shows("ready  DM +1").await;
    assert!(
        out.text().contains("\x1b]0;theseus (1)\x07"),
        "{:?}",
        out.text()
    );
    let mut tide = task(140, "ses_tide01", "ses_dm0001", "complete", 0.12);
    tide.previous = Some("running".into());
    rig.daemon().notify("execution.changed", changed(&tide));
    rig.shows("◆ done  check the tide tables").await;
    assert!(
        rig.screen()[0].contains("1 needs you · 1 done"),
        "{:?}",
        rig.screen()
    );
    let title = "\x1b]0;theseus (2)\x07".to_string();
    rig.until("the title's count", |_| out.text().contains(&title))
        .await;
    // Each title ends in a BEL too: a bell is a BEL that ends no title.
    let bells =
        |o: &Captured| o.text().matches('\x07').count() - o.text().matches("\x1b]0;").count();
    assert_eq!(bells(&out), 0, "not before its second");
    NOW.with(|n| n.set(T0 + 1_000));
    rig.until("the bell", |_| bells(&out) == 1).await;
    assert!(
        rig.screen()[23].contains("check the tide tables finished"),
        "{:?}",
        rig.screen()
    );
    NOW.with(|n| n.set(T0 + 5_000));
    rig.settle().await;
    assert_eq!(bells(&out), 1, "once");
}

/// A rig whose seen file is `path`.
pub fn harbour_rig_seen(
    width: u16,
    height: u16,
    world: Arc<Mutex<World>>,
    path: &std::path::Path,
) -> Rig {
    let mut rig = Rig::new(width, height, script(world));
    rig.runner.app.seen = Seen::open(Some(path.to_path_buf()));
    rig
}

/// §3.2's test for 10e: a background task finishes, and its row says done
/// (◆) until the operator focuses it; the seen file keeps that across a
/// restart of the TUI, both before and after it is seen.
#[tokio::test]
async fn done_until_seen_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theseus").join("tui-seen.json");
    let world = harbour_world();
    // Run 1: a first start sees the board as it is, then the tide task ends.
    let mut rig = harbour_rig_seen(80, 24, world.clone(), &path);
    rig.shows("check the tide tables").await;
    let mut tide = task(140, "ses_tide01", "ses_dm0001", "complete", 0.12);
    tide.previous = Some("running".into());
    rig.daemon().notify("execution.changed", changed(&tide));
    rig.shows("◆ done  check the tide tables").await;
    assert!(rig.screen()[0].contains("1 done"), "{:?}", rig.screen());
    rig.press(&[KeyCode::Char('q')]).await;
    assert!(path.exists(), "the seen file is written on exit");
    // Run 2: the daemon's board now holds the ended task; it is still done.
    {
        let mut w = world.lock().unwrap();
        replace_view(&mut w.board, &tide);
    }
    let mut rig = harbour_rig_seen(80, 24, world.clone(), &path);
    rig.shows("◆ done  check the tide tables").await;
    // Focus it: seen.
    let row = rig
        .screen()
        .iter()
        .position(|l| l.contains("check the tide tables"))
        .unwrap();
    for _ in 1..row {
        rig.press(&[KeyCode::Char('j')]).await;
    }
    rig.press(&[KeyCode::Char('j'), KeyCode::Enter]).await;
    rig.press(&[KeyCode::Esc]).await;
    rig.settle().await;
    assert!(
        !rig.screen().iter().any(|l| l.contains('◆')),
        "{:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('q')]).await;
    // Run 3: seen stays seen.
    let mut rig = harbour_rig_seen(80, 24, world, &path);
    rig.shows("socket ok").await;
    rig.settle().await;
    assert!(
        !rig.screen().iter().any(|l| l.contains('◆')),
        "{:?}",
        rig.screen()
    );
}

/// The seen rule (design §2.2): done means it finished after it was last
/// displayed. A first start's board is seen as it is; a session the TUI never
/// knew counts as done once it has worked, unless it was cancelled; looking
/// at it clears it.
#[test]
fn the_seen_rule_marks_what_finished_unseen() {
    let mut s = Seen::open(None);
    let old = conv(10, "ses_old", "waiting", input(), 0.0);
    s.applied(&old, false);
    s.first_board_read();
    assert!(!s.done(&old), "a first start's board is seen as it is");
    let worked = conv(20, "ses_new", "waiting", input(), 0.0);
    s.applied(&worked, false);
    assert!(s.done(&worked), "new, and it worked: done");
    let idle = opened(21, "ses_idle");
    s.applied(&idle, false);
    assert!(!s.done(&idle), "new, and it never worked");
    let gone = conv(22, "ses_gone", "cancelled", None, 0.0);
    s.applied(&gone, false);
    assert!(!s.done(&gone), "whoever cancelled it was there");
    s.display(&worked);
    assert!(!s.done(&worked), "displayed: seen");
    let again = conv(30, "ses_new", "running", None, 0.0);
    s.applied(&again, false);
    let back = conv(31, "ses_new", "waiting", input(), 0.0);
    s.applied(&back, true);
    assert!(
        !s.done(&back),
        "it finished while the operator looked at it"
    );
}
