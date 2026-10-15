//! `theseus herdr up` end to end (theseus-fuw5d): the real binary against a scripted
//! daemon (`work.watch`, with changes pushed at the test's word) and a scripted herdr
//! that keeps a model of its workspaces, tabs and panes, records every request, and
//! pushes events to the follower's `events.subscribe` connection. These hold what the
//! follower makes, closes, rebuilds and leaves alone, and that a second `up` sends
//! only reads.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

#[derive(Clone, Debug)]
struct Pane {
    id: String,
    tab: String,
    ws: String,
    tokens: serde_json::Map<String, Value>,
}

#[derive(Default)]
struct Model {
    workspaces: Vec<(String, String)>,
    panes: Vec<Pane>,
    n: u32,
}

impl Model {
    fn ws_of(&mut self, label: &str) -> String {
        if let Some((id, _)) = self.workspaces.iter().find(|(_, l)| l == label) {
            return id.clone();
        }
        self.n += 1;
        let id = format!("w{}", self.n);
        self.workspaces.push((id.clone(), label.to_string()));
        id
    }

    fn answer(&mut self, m: &str, p: &Value) -> Value {
        match m {
            "workspace.list" => {
                let ws: Vec<Value> = self
                    .workspaces
                    .iter()
                    .map(|(id, l)| json!({"workspace_id": id, "label": l}))
                    .collect();
                json!({"type": "workspace_list", "workspaces": ws})
            }
            "pane.list" => {
                let panes: Vec<Value> = self
                    .panes
                    .iter()
                    .map(|p| {
                        json!({"pane_id": p.id, "tab_id": p.tab, "workspace_id": p.ws,
                               "focused": false, "tokens": p.tokens})
                    })
                    .collect();
                json!({"type": "pane_list", "panes": panes})
            }
            "layout.apply" => {
                let ws = self.ws_of(p["workspace"]["label"].as_str().unwrap_or("theseus"));
                let mut tabs = Vec::new();
                for t in p["tabs"].as_array().into_iter().flatten() {
                    self.n += 1;
                    let tab = format!("{ws}:t{}", self.n);
                    let mut panes = Vec::new();
                    for q in t["panes"].as_array().into_iter().flatten() {
                        self.n += 1;
                        let id = format!("{ws}:p{}", self.n);
                        self.panes.push(Pane {
                            id: id.clone(),
                            tab: tab.clone(),
                            ws: ws.clone(),
                            tokens: q["tokens"].as_object().cloned().unwrap_or_default(),
                        });
                        panes.push(json!({"pane_id": id}));
                    }
                    tabs.push(json!({"tab_id": tab, "panes": panes}));
                }
                json!({"type": "layout_applied", "workspace": {"workspace_id": ws}, "tabs": tabs})
            }
            "tab.close" => {
                let tab = p["tab_id"].as_str().unwrap_or_default();
                self.panes.retain(|q| q.tab != tab);
                json!({"type": "ok"})
            }
            "pane.close" => {
                let id = p["pane_id"].as_str().unwrap_or_default();
                self.panes.retain(|q| q.id != id);
                json!({"type": "ok"})
            }
            "pane.report_metadata" => {
                let id = p["pane_id"].as_str().unwrap_or_default();
                if let Some(q) = self.panes.iter_mut().find(|q| q.id == id) {
                    for (k, v) in p["tokens"].as_object().into_iter().flatten() {
                        q.tokens.insert(k.clone(), v.clone());
                    }
                }
                json!({"type": "ok"})
            }
            _ => json!({"type": "ok"}),
        }
    }
}

struct FakeHerdr {
    sock: PathBuf,
    model: Arc<Mutex<Model>>,
    got: Arc<Mutex<Vec<Value>>>,
    subs: Arc<Mutex<Vec<UnixStream>>>,
}

impl FakeHerdr {
    fn start(dir: &Path, model: Model) -> Self {
        let sock = dir.join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let model = Arc::new(Mutex::new(model));
        let got = Arc::new(Mutex::new(Vec::new()));
        let subs = Arc::new(Mutex::new(Vec::new()));
        let (m, g, s) = (model.clone(), got.clone(), subs.clone());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let Ok(c) = c else { break };
                let (m, g, s) = (m.clone(), g.clone(), s.clone());
                std::thread::spawn(move || {
                    let mut line = String::new();
                    if BufReader::new(&c).read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let method = req["method"].as_str().unwrap().to_string();
                    g.lock().unwrap().push(req.clone());
                    let result = if method == "events.subscribe" {
                        s.lock().unwrap().push(c.try_clone().unwrap());
                        json!({"type": "subscribed"})
                    } else {
                        m.lock().unwrap().answer(&method, &req["params"])
                    };
                    let _ = writeln!(&c, "{}", json!({"id": req["id"], "result": result}));
                });
            }
        });
        Self {
            sock,
            model,
            got,
            subs,
        }
    }

    fn push(&self, event: &str, data: Value) {
        let line = json!({"event": event, "data": data}).to_string();
        for mut s in self.subs.lock().unwrap().iter() {
            let _ = writeln!(s, "{line}");
        }
    }

    fn requests(&self, method: &str) -> Vec<Value> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == method)
            .cloned()
            .collect()
    }

    fn methods(&self) -> Vec<String> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["method"].as_str().unwrap().to_string())
            .collect()
    }

    fn clear(&self) {
        self.got.lock().unwrap().clear();
    }

    /// The tabs of tasks (by session token) now in the model.
    fn tabs_of(&self, sid: &str) -> Vec<String> {
        self.model
            .lock()
            .unwrap()
            .panes
            .iter()
            .filter(|p| p.tokens.get("session").and_then(Value::as_str) == Some(sid))
            .map(|p| p.tab.clone())
            .collect()
    }
}

struct FakeDaemon {
    sock: PathBuf,
    views: Arc<Mutex<Vec<Value>>>,
    subs: Arc<Mutex<Vec<UnixStream>>>,
    down: Arc<AtomicBool>,
    pos: Arc<Mutex<u64>>,
}

fn view(id: &str, sid: &str, kind: &str, state: &str, level: &str, pos: u64) -> Value {
    json!({"id": id, "kind": kind, "position": pos, "at_ms": pos, "root": id,
           "session_id": sid, "title": format!("title of {sid}"), "state": state,
           "attention": {"level": level, "label": state, "since_ms": 1},
           "started_at_ms": 1, "state_since_ms": 1})
}

impl FakeDaemon {
    fn start(dir: &Path, views: Vec<Value>) -> Self {
        let sock = dir.join("theseusd.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let views = Arc::new(Mutex::new(views));
        let subs: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));
        let down = Arc::new(AtomicBool::new(false));
        let (v, s, d) = (views.clone(), subs.clone(), down.clone());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let Ok(c) = c else { break };
                if d.load(Ordering::SeqCst) {
                    continue;
                }
                let (v, s) = (v.clone(), s.clone());
                std::thread::spawn(move || {
                    for line in BufReader::new(&c).lines() {
                        let Ok(line) = line else { break };
                        let req: Value = serde_json::from_str(&line).unwrap();
                        let result = match req["method"].as_str() {
                            Some("work.watch") => {
                                s.lock().unwrap().push(c.try_clone().unwrap());
                                let views = v.lock().unwrap().clone();
                                json!({"position": 1, "views": views, "total": views.len()})
                            }
                            Some("session.list") => json!({"sessions": [
                                {"session_id": "ses_pinned1", "kind": "conversation",
                                 "label": "Tide notes", "created_at_unix_ms": 1, "turns": 1}]}),
                            _ => json!({"watching": false}),
                        };
                        let _ = writeln!(
                            &c,
                            "{}",
                            json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
                        );
                    }
                });
            }
        });
        Self {
            sock,
            views,
            subs,
            down,
            pos: Arc::new(Mutex::new(10)),
        }
    }

    /// A change: the views, pushed to every watcher and kept for the next snapshot.
    fn change(&self, views: Vec<Value>) {
        let mut pos = self.pos.lock().unwrap();
        *pos += 1;
        let mut views = views;
        for v in &mut views {
            v["position"] = json!(*pos);
        }
        {
            let mut all = self.views.lock().unwrap();
            for v in &views {
                all.retain(|o| o["id"] != v["id"]);
                all.push(v.clone());
            }
        }
        let note = json!({"jsonrpc": "2.0", "method": "work.changed",
                          "params": {"position": *pos, "at_ms": *pos, "views": views}});
        for mut s in self.subs.lock().unwrap().iter() {
            let _ = writeln!(s, "{note}");
        }
    }

    /// The daemon goes away: its connections close and new ones are dropped.
    fn go_down(&self) {
        self.down.store(true, Ordering::SeqCst);
        for s in self.subs.lock().unwrap().drain(..) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    fn come_back(&self) {
        self.down.store(false, Ordering::SeqCst);
    }
}

fn theseus(daemon: &Path, herdr: &Path) -> Command {
    let mut c = Command::new(THESEUS);
    c.arg("--socket")
        .arg(daemon)
        .args(["herdr", "up", "--herdr-socket"])
        .arg(herdr)
        .env_remove("THESEUS_SOCKET")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH");
    c
}

/// A follower process, killed when the test ends.
struct Follower(Child);

impl Drop for Follower {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn follow(daemon: &Path, herdr: &Path, extra: &[&str]) -> Follower {
    Follower(
        theseus(daemon, herdr)
            .args(["--follow", "--here", "--finished-grace", "2"])
            .args(extra)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

/// Wait until `f` holds, up to `secs`.
fn until(secs: u64, what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

fn tabs_in(call: &Value) -> Vec<Value> {
    call["params"]["tabs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn watched(tab: &Value) -> String {
    let argv: Vec<&str> = tab["panes"][0]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let i = argv.iter().position(|a| *a == "watch").unwrap();
    assert_eq!(argv[i + 2], "--interactive");
    argv[i + 1].to_string()
}

fn three_tasks() -> Vec<Value> {
    vec![
        view("exe_1", "ses_work01", "task", "running", "working", 1),
        view("exe_2", "ses_ask002", "task", "needs_you", "needs_you", 1),
        view(
            "exe_3",
            "ses_conv03",
            "conversation",
            "needs_you",
            "needs_you",
            1,
        ),
        view(
            "exe_4",
            "ses_conv04",
            "conversation",
            "running",
            "working",
            1,
        ),
    ]
}

/// A snapshot becomes one tab per task that wants one (a conversation only while it
/// needs you), in one `layout.apply`, each an argv pane running the session's watch:
/// nothing is typed into a shell. A new task is one more `layout.apply`.
#[test]
fn a_snapshot_makes_a_tab_per_task_and_a_new_task_one_more() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    until(10, "the first layout", || {
        herdr.requests("layout.apply").len() == 1
    });
    let first = herdr.requests("layout.apply");
    let mut sids: Vec<String> = tabs_in(&first[0]).iter().map(watched).collect();
    sids.sort();
    assert_eq!(sids, ["ses_ask002", "ses_conv03", "ses_work01"]);
    assert!(herdr.requests("pane.send_input").is_empty());
    assert!(herdr.requests("pane.split").is_empty());
    daemon.change(vec![view(
        "exe_9",
        "ses_new009",
        "task",
        "running",
        "working",
        1,
    )]);
    until(10, "the new task's tab", || {
        herdr.requests("layout.apply").len() == 2
    });
    let second = herdr.requests("layout.apply");
    assert_eq!(tabs_in(&second[1]).len(), 1);
    assert_eq!(watched(&tabs_in(&second[1])[0]), "ses_new009");
    // The Space row's counts and the view's sort were sent once the layout stood.
    until(10, "the counts", || {
        herdr
            .requests("workspace.report_metadata")
            .last()
            .is_some_and(|c| c["params"]["tokens"]["working"] == "◐3")
    });
    let counts = herdr.requests("workspace.report_metadata");
    assert_eq!(counts[counts.len() - 1]["params"]["tokens"]["needs"], "●2");
    assert!(!herdr.requests("agent.view.set").is_empty());
}

/// A finished task's tab closes the grace after it was seen: not before, not unseen.
/// A cancelled task's closes at once.
#[test]
fn a_finished_tab_closes_a_grace_after_it_is_seen_and_a_cancelled_one_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    until(10, "the layout", || {
        herdr.requests("layout.apply").len() == 1
    });
    let tab_work = herdr.tabs_of("ses_work01")[0].clone();
    let tab_ask = herdr.tabs_of("ses_ask002")[0].clone();
    // One finishes unseen, one finishes and is looked at, one is cancelled.
    daemon.change(vec![
        view("exe_1", "ses_work01", "task", "done", "idle", 1),
        view("exe_2", "ses_ask002", "task", "cancelled", "idle", 1),
    ]);
    until(10, "the cancelled tab's close", || {
        herdr.requests("tab.close").len() == 1
    });
    assert_eq!(
        herdr.requests("tab.close")[0]["params"]["tab_id"],
        tab_ask.as_str()
    );
    std::thread::sleep(Duration::from_millis(3200));
    assert_eq!(
        herdr.requests("tab.close").len(),
        1,
        "unseen: the finished tab stays"
    );
    let seen_at = Instant::now();
    herdr.push("tab.focused", json!({"tab_id": tab_work}));
    until(10, "the finished tab's close", || {
        herdr.requests("tab.close").len() == 2
    });
    assert!(
        seen_at.elapsed() >= Duration::from_millis(1900),
        "closed {:?} after it was seen: the grace is 2 s",
        seen_at.elapsed()
    );
    assert_eq!(
        herdr.requests("tab.close")[1]["params"]["tab_id"],
        tab_work.as_str()
    );
}

/// A pane that exits (its watch ended) is made again while its task still wants a tab.
#[test]
fn a_pane_that_exits_is_swept_and_its_tab_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    until(10, "the layout", || {
        herdr.requests("layout.apply").len() == 1
    });
    // Wait out the reopen hold, then lose the pane as herdr would.
    std::thread::sleep(Duration::from_millis(5200));
    let pane = herdr
        .model
        .lock()
        .unwrap()
        .panes
        .iter()
        .find(|p| p.tokens["session"] == "ses_work01")
        .unwrap()
        .id
        .clone();
    herdr.model.lock().unwrap().panes.retain(|p| p.id != pane);
    herdr.push("pane.exited", json!({"pane_id": pane}));
    until(10, "the tab made again", || {
        herdr.requests("layout.apply").len() == 2
    });
    let again = herdr.requests("layout.apply");
    assert_eq!(tabs_in(&again[1]).len(), 1);
    assert_eq!(watched(&tabs_in(&again[1])[0]), "ses_work01");
}

/// `--boot` over what a herdr restart left: the restored shells (no `kind` token) are
/// closed first, the tabs rebuilt, and a pane of ours is left alone.
#[test]
fn boot_closes_restored_shells_and_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = Model::default();
    let ws = model.ws_of("theseus");
    for (i, kind) in [(1, None), (2, None), (3, Some("task"))] {
        let mut tokens = serde_json::Map::new();
        if let Some(k) = kind {
            tokens.insert("kind".into(), json!(k));
            tokens.insert("session".into(), json!("ses_work01"));
        }
        model.panes.push(Pane {
            id: format!("{ws}:r{i}"),
            tab: format!("{ws}:rt{i}"),
            ws: ws.clone(),
            tokens,
        });
    }
    let herdr = FakeHerdr::start(dir.path(), model);
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &["--boot"]);
    until(10, "the rebuild", || {
        !herdr.requests("layout.apply").is_empty()
    });
    let methods = herdr.methods();
    let closes = herdr.requests("pane.close");
    assert_eq!(closes.len(), 2, "{methods:?}");
    let first_layout = methods.iter().position(|m| m == "layout.apply").unwrap();
    let last_close = methods.iter().rposition(|m| m == "pane.close").unwrap();
    assert!(last_close < first_layout, "closes come first: {methods:?}");
    // The kept pane already watched ses_work01, so only the other two are made.
    let mut sids: Vec<String> = tabs_in(&herdr.requests("layout.apply")[0])
        .iter()
        .map(watched)
        .collect();
    sids.sort();
    assert_eq!(sids, ["ses_ask002", "ses_conv03"]);
}

/// From anywhere but the board tab, `--boot` closes the leftovers and makes the board
/// tab, whose pane runs the follower, and exits.
#[test]
fn boot_makes_the_board_tab_and_exits() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), vec![]);
    let out = theseus(&daemon.sock, &herdr.sock)
        .arg("--boot")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let layouts = herdr.requests("layout.apply");
    assert_eq!(layouts.len(), 1);
    let tab = &tabs_in(&layouts[0])[0];
    let argv: Vec<&str> = tab["panes"][0]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        argv.windows(4)
            .any(|w| w == ["herdr", "up", "--follow", "--here"]),
        "{argv:?}"
    );
    assert_eq!(tab["panes"][0]["tokens"]["kind"], "board");
    // A second boot finds the board tab and makes nothing.
    herdr.clear();
    let out = theseus(&daemon.sock, &herdr.sock)
        .arg("--boot")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(herdr.requests("layout.apply").is_empty());
}

/// `up` lays out and exits; a second `up` sends herdr only reads.
#[test]
fn a_second_up_sends_only_reads() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let out = theseus(&daemon.sock, &herdr.sock).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(herdr.requests("layout.apply").len(), 1);
    herdr.clear();
    let out = theseus(&daemon.sock, &herdr.sock).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let methods = herdr.methods();
    assert!(
        methods
            .iter()
            .all(|m| m == "workspace.list" || m == "pane.list"),
        "a second up wrote: {methods:?}"
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("nothing changed"));
}

/// A pin is remembered on the pane: a later `up` without `--pin` keeps the tab of a
/// session that would otherwise want none, and makes no second one.
#[test]
fn a_pin_survives_a_second_up() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), vec![]);
    let out = theseus(&daemon.sock, &herdr.sock)
        .args(["--pin", "pinned1"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let first = herdr.requests("layout.apply");
    assert_eq!(first.len(), 1);
    assert_eq!(tabs_in(&first[0])[0]["panes"][0]["tokens"]["pinned"], "1");
    herdr.clear();
    let out = theseus(&daemon.sock, &herdr.sock).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let methods = herdr.methods();
    assert!(
        methods
            .iter()
            .all(|m| m == "workspace.list" || m == "pane.list"),
        "{methods:?}"
    );
    assert_eq!(herdr.tabs_of("ses_pinned1").len(), 1);
}

/// The daemon's loss is one notice, its return another, and the tabs stand through it.
#[test]
fn a_lost_daemon_is_one_notice_and_its_return_another() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    until(10, "the layout", || {
        herdr.requests("layout.apply").len() == 1
    });
    daemon.go_down();
    until(10, "the loss notice", || {
        herdr.requests("notification.show").len() == 1
    });
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        herdr.requests("notification.show").len(),
        1,
        "once, however long it is away"
    );
    daemon.come_back();
    until(10, "the return notice", || {
        herdr.requests("notification.show").len() == 2
    });
    let n = herdr.requests("notification.show");
    assert_eq!(n[0]["params"]["body"], "daemon restarting");
    assert_eq!(n[1]["params"]["body"], "daemon is back");
    assert_eq!(n[0]["params"]["title"], "Theseus");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        herdr.requests("layout.apply").len(),
        1,
        "nothing laid out again"
    );
}
