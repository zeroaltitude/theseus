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

#[path = "support/herdr_schema.rs"]
mod herdr_schema;
use herdr_schema::Schema;

#[derive(Clone, Debug)]
struct Pane {
    id: String,
    tab: String,
    ws: String,
    tokens: serde_json::Map<String, Value>,
}

/// herdr's state as the follower sees it. Every request is checked against herdr's own schema first
/// (`support/herdr_schema.rs`), and every result against its success schema: a shape herdr would
/// refuse is refused here with its `invalid_request`, and noted in `bad` so the test fails.
#[derive(Default)]
struct Model {
    workspaces: Vec<(String, String)>,
    panes: Vec<Pane>,
    labels: std::collections::HashMap<String, String>,
    n: u32,
}

type Reply = Result<Value, (&'static str, String)>;

impl Model {
    fn ws_of(&mut self, label: &str) -> String {
        if let Some((id, _)) = self.workspaces.iter().find(|(_, l)| l == label) {
            return id.clone();
        }
        self.make_ws(label)
    }

    fn make_ws(&mut self, label: &str) -> String {
        self.n += 1;
        let id = format!("w{}", self.n);
        self.workspaces.push((id.clone(), label.to_string()));
        id
    }

    /// A new tab with one pane, in `ws`.
    fn make_tab(&mut self, ws: &str, label: &str, tokens: serde_json::Map<String, Value>) -> Pane {
        self.n += 1;
        let tab = format!("{ws}:t{}", self.n);
        self.n += 1;
        let pane = Pane {
            id: format!("{ws}:p{}", self.n),
            tab: tab.clone(),
            ws: ws.to_string(),
            tokens,
        };
        self.labels.insert(tab, label.to_string());
        self.panes.push(pane.clone());
        pane
    }

    fn tab_ids(&self, ws: &str) -> Vec<String> {
        let mut tabs: Vec<String> = Vec::new();
        for p in self.panes.iter().filter(|p| p.ws == ws) {
            if !tabs.contains(&p.tab) {
                tabs.push(p.tab.clone());
            }
        }
        tabs
    }

    fn pane_info(p: &Pane) -> Value {
        json!({"pane_id": p.id, "tab_id": p.tab, "workspace_id": p.ws,
               "terminal_id": format!("term-{}", p.id), "focused": false,
               "agent_status": "idle", "revision": 1, "tokens": p.tokens})
    }

    fn tab_info(&self, tab: &str) -> Value {
        let ws = self
            .panes
            .iter()
            .find(|p| p.tab == tab)
            .map(|p| p.ws.clone());
        let ws = ws.unwrap_or_default();
        let number = self.tab_ids(&ws).iter().position(|t| t == tab).unwrap_or(0) + 1;
        json!({"tab_id": tab, "workspace_id": ws, "number": number,
               "label": self.labels.get(tab).cloned().unwrap_or_default(), "focused": false,
               "pane_count": self.panes.iter().filter(|p| p.tab == tab).count(),
               "agent_status": "idle"})
    }

    fn ws_info(&self, number: usize, id: &str, label: &str) -> Value {
        let tabs = self.tab_ids(id);
        json!({"workspace_id": id, "number": number, "label": label, "focused": false,
               "pane_count": self.panes.iter().filter(|p| p.ws == id).count(),
               "tab_count": tabs.len(), "active_tab_id": tabs.first().cloned().unwrap_or_default(),
               "agent_status": "idle"})
    }

    /// A pane closes; a tab or a workspace left without a pane goes with it, as herdr's do.
    fn close_panes(&mut self, keep: impl Fn(&Pane) -> bool) {
        self.panes.retain(keep);
        let live: Vec<String> = self.panes.iter().map(|p| p.ws.clone()).collect();
        self.workspaces.retain(|(id, _)| live.contains(id));
    }

    fn answer(&mut self, m: &str, p: &Value) -> Reply {
        Ok(match m {
            "workspace.list" => {
                let ws: Vec<Value> = self
                    .workspaces
                    .iter()
                    .enumerate()
                    .map(|(i, (id, l))| self.ws_info(i + 1, id, l))
                    .collect();
                json!({"type": "workspace_list", "workspaces": ws})
            }
            "workspace.create" => {
                let label = p["label"].as_str().unwrap_or("workspace").to_string();
                let ws = self.make_ws(&label);
                let pane = self.make_tab(&ws, "shell", Default::default());
                let number = self.workspaces.len();
                json!({"type": "workspace_created", "workspace": self.ws_info(number, &ws, &label),
                       "tab": self.tab_info(&pane.tab), "root_pane": Self::pane_info(&pane)})
            }
            "pane.list" => {
                let panes: Vec<Value> = self.panes.iter().map(Self::pane_info).collect();
                json!({"type": "pane_list", "panes": panes})
            }
            "layout.apply" => self.layout_apply(p)?,
            "tab.close" => {
                let tab = p["tab_id"].as_str().unwrap_or_default().to_string();
                self.close_panes(|q| q.tab != tab);
                json!({"type": "ok"})
            }
            "pane.close" => {
                let id = p["pane_id"].as_str().unwrap_or_default().to_string();
                self.close_panes(|q| q.id != id);
                json!({"type": "ok"})
            }
            "pane.report_metadata" => {
                let id = p["pane_id"].as_str().unwrap_or_default();
                let Some(q) = self.panes.iter_mut().find(|q| q.id == id) else {
                    return Err(("pane_not_found", format!("pane {id} not found")));
                };
                for (k, v) in p["tokens"].as_object().into_iter().flatten() {
                    if v.is_null() {
                        q.tokens.remove(k);
                    } else {
                        q.tokens.insert(k.clone(), v.clone());
                    }
                }
                json!({"type": "ok"})
            }
            "agent.view.set" => json!({"type": "agent_view", "active": true}),
            "notification.show" => {
                json!({"type": "notification_show", "shown": true, "reason": "shown"})
            }
            _ => json!({"type": "ok"}),
        })
    }

    /// herdr's `layout.apply`: one new tab with the layout's pane, in the workspace or in place of
    /// the tab named; the answer describes the layout.
    fn layout_apply(&mut self, p: &Value) -> Reply {
        let (target_ws, replace) = match (p["tab_id"].as_str(), p["workspace_id"].as_str()) {
            (Some(_), Some(_)) => {
                return Err(("invalid_target", "use either tab_id or workspace_id".into()))
            }
            (Some(tab), None) => match self.panes.iter().find(|q| q.tab == tab) {
                Some(q) => (q.ws.clone(), Some(tab.to_string())),
                None => return Err(("tab_not_found", format!("tab {tab} not found"))),
            },
            (None, Some(ws)) => match self.workspaces.iter().any(|(id, _)| id == ws) {
                true => (ws.to_string(), None),
                false => return Err(("workspace_not_found", format!("workspace {ws} not found"))),
            },
            (None, None) => match self.workspaces.first() {
                Some((id, _)) => (id.clone(), None),
                None => return Err(("workspace_not_found", "no active workspace".into())),
            },
        };
        let mut node = &p["root"];
        while node["type"] == "split" {
            node = &node["first"];
        }
        let old_label = replace.as_ref().and_then(|t| self.labels.get(t)).cloned();
        let label = p["tab_label"].as_str().map(str::to_string).or(old_label);
        let pane = self.make_tab(
            &target_ws,
            label.as_deref().unwrap_or("tab"),
            Default::default(),
        );
        if let Some(old) = replace {
            self.close_panes(|q| q.tab != old);
        }
        Ok(json!({"type": "layout_apply", "layout": {
            "workspace_id": target_ws, "tab_id": pane.tab, "zoomed": false,
            "focused_pane_id": pane.id, "root": {"type": "pane", "pane_id": pane.id}}}))
    }
}

struct FakeHerdr {
    sock: PathBuf,
    model: Arc<Mutex<Model>>,
    got: Arc<Mutex<Vec<Value>>>,
    subs: Arc<Mutex<Vec<UnixStream>>>,
    /// What herdr's schema refused: a request it would not take, a result it would not give.
    bad: Arc<Mutex<Vec<String>>>,
}

impl Drop for FakeHerdr {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let bad = self.bad.lock().unwrap();
            assert!(bad.is_empty(), "herdr's schema refused: {bad:#?}");
        }
    }
}

impl FakeHerdr {
    fn start(dir: &Path, model: Model) -> Self {
        let sock = dir.join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let model = Arc::new(Mutex::new(model));
        let got = Arc::new(Mutex::new(Vec::new()));
        let subs = Arc::new(Mutex::new(Vec::new()));
        let bad = Arc::new(Mutex::new(Vec::new()));
        let schema = Arc::new(Schema::load());
        let (m, g, s, b) = (model.clone(), got.clone(), subs.clone(), bad.clone());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let Ok(c) = c else { break };
                let (m, g, s, b, schema) =
                    (m.clone(), g.clone(), s.clone(), b.clone(), schema.clone());
                std::thread::spawn(move || {
                    let mut line = String::new();
                    if BufReader::new(&c).read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let method = req["method"].as_str().unwrap().to_string();
                    g.lock().unwrap().push(req.clone());
                    let reply = match schema.check_request(&method, &req["params"]) {
                        Err(e) => {
                            b.lock().unwrap().push(format!("{method}: {e}"));
                            json!({"id": req["id"], "error": {"code": "invalid_request", "message": e}})
                        }
                        Ok(()) => {
                            let result = if method == "events.subscribe" {
                                s.lock().unwrap().push(c.try_clone().unwrap());
                                Ok(json!({"type": "subscription_started"}))
                            } else {
                                m.lock().unwrap().answer(&method, &req["params"])
                            };
                            match result {
                                Ok(r) => {
                                    if let Err(e) = schema.check_result(&r) {
                                        b.lock().unwrap().push(format!("{method} result: {e}"));
                                    }
                                    json!({"id": req["id"], "result": r})
                                }
                                Err((code, message)) => json!({"id": req["id"],
                                    "error": {"code": code, "message": message}}),
                            }
                        }
                    };
                    let _ = writeln!(&c, "{reply}");
                });
            }
        });
        Self {
            sock,
            model,
            got,
            subs,
            bad,
        }
    }

    /// An event as herdr pushes it: its name with underscores and the typed data (`pane.exited`
    /// becomes `pane_exited`), checked against the event schema.
    fn push(&self, event: &str, mut data: Value) {
        let name = event.replace('.', "_");
        data["type"] = json!(name);
        if data["workspace_id"].is_null() {
            data["workspace_id"] = json!("w1");
        }
        let line = json!({"event": name, "data": data});
        if let Err(e) = Schema::load().check_event(&line) {
            self.bad.lock().unwrap().push(format!("event {event}: {e}"));
        }
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

    /// The labels of the `theseus` workspace's tabs, in order.
    fn tab_labels(&self) -> Vec<String> {
        let m = self.model.lock().unwrap();
        m.workspaces
            .iter()
            .filter(|(_, l)| l == "theseus")
            .flat_map(|(id, _)| m.tab_ids(id))
            .map(|t| m.labels[&t].clone())
            .collect()
    }

    /// The `kind` tokens of the panes now in the model, in tab order.
    fn kinds(&self) -> Vec<String> {
        let m = self.model.lock().unwrap();
        m.panes
            .iter()
            .map(|p| {
                p.tokens
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string()
            })
            .collect()
    }
}

struct FakeDaemon {
    sock: PathBuf,
    views: Arc<Mutex<Vec<Value>>>,
    subs: Arc<Mutex<Vec<UnixStream>>>,
    down: Arc<AtomicBool>,
    pos: Arc<Mutex<u64>>,
    /// The digest's lines `work.digest` answers with.
    digest: Arc<Mutex<Vec<Value>>>,
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
        let digest: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let (v, s, d, dg) = (views.clone(), subs.clone(), down.clone(), digest.clone());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let Ok(c) = c else { break };
                if d.load(Ordering::SeqCst) {
                    continue;
                }
                let (v, s, dg) = (v.clone(), s.clone(), dg.clone());
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
                            Some("work.digest") => json!({
                                "from": req["params"]["since"], "to": 99,
                                "lines": dg.lock().unwrap().clone(), "from_ledger": false,
                                "now": {"needs_you": 0, "working": 0, "done": 0, "failed": 0,
                                        "total": 0},
                                "questions": []}),
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
            digest,
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
        .env("XDG_STATE_HOME", daemon.parent().unwrap().join("state"))
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

/// The argv a `layout.apply` request's pane runs.
fn argv_of(call: &Value) -> Vec<String> {
    call["params"]["root"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// The session a `layout.apply` request's pane watches.
fn watched(call: &Value) -> String {
    let argv = argv_of(call);
    let i = argv.iter().position(|a| a == "watch").unwrap();
    assert_eq!(argv[i + 2], "--interactive");
    argv[i + 1].clone()
}

/// The sessions the `layout.apply` requests from `from` on watch, in order.
fn watched_from(herdr: &FakeHerdr, from: usize) -> Vec<String> {
    herdr.requests("layout.apply")[from..]
        .iter()
        .map(watched)
        .collect()
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
/// needs you), a `layout.apply` each (herdr makes one tab a call), each an argv pane running
/// the session's watch: nothing is typed into a shell. The first replaces the new workspace's
/// blank tab. A new task is one more `layout.apply`.
#[test]
fn a_snapshot_makes_a_tab_per_task_and_a_new_task_one_more() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    until(10, "the first layouts", || {
        herdr.requests("layout.apply").len() == 3
    });
    let first = herdr.requests("layout.apply");
    let mut sids = watched_from(&herdr, 0);
    sids.sort();
    assert_eq!(sids, ["ses_ask002", "ses_conv03", "ses_work01"]);
    assert_eq!(herdr.requests("workspace.create").len(), 1);
    assert!(
        first[0]["params"]["tab_id"].is_string(),
        "the blank tab is replaced"
    );
    assert!(
        first[1]["params"]["workspace_id"].is_string(),
        "then new tabs"
    );
    assert_eq!(
        herdr.tab_labels().len(),
        3,
        "no blank tab is left: {:?}",
        herdr.tab_labels()
    );
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
        herdr.requests("layout.apply").len() == 4
    });
    assert_eq!(watched_from(&herdr, 3), ["ses_new009"]);
    assert_eq!(herdr.requests("workspace.create").len(), 1);
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
        herdr.requests("layout.apply").len() == 3
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
        herdr.requests("layout.apply").len() == 3
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
        herdr.requests("layout.apply").len() == 4
    });
    assert_eq!(watched_from(&herdr, 3), ["ses_work01"]);
    assert_eq!(
        herdr.requests("layout.apply").len(),
        4,
        "one tab, made again"
    );
}

/// A herdr restart restores a pane as a shell with no tokens: `pane`, in a tab of its own.
fn restored(model: &mut Model, ws: &str, i: u32, tokens: &[(&str, &str)]) {
    let tokens = tokens
        .iter()
        .map(|(k, v)| ((*k).to_string(), json!(v)))
        .collect();
    model.panes.push(Pane {
        id: format!("{ws}:r{i}"),
        tab: format!("{ws}:rt{i}"),
        ws: ws.to_string(),
        tokens,
    });
    model
        .labels
        .insert(format!("{ws}:rt{i}"), format!("restored {i}"));
}

/// `--boot` over what a herdr restart left: the restored shells (no `kind` token) are
/// closed first, the tabs rebuilt, and a pane of ours is left alone.
#[test]
fn boot_closes_restored_shells_and_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = Model::default();
    let ws = model.ws_of("theseus");
    restored(&mut model, &ws, 1, &[]);
    restored(&mut model, &ws, 2, &[]);
    restored(
        &mut model,
        &ws,
        3,
        &[("kind", "task"), ("session", "ses_work01")],
    );
    let herdr = FakeHerdr::start(dir.path(), model);
    let daemon = FakeDaemon::start(dir.path(), three_tasks());
    let _f = follow(&daemon.sock, &herdr.sock, &["--boot"]);
    until(10, "the rebuild", || {
        herdr.requests("layout.apply").len() == 2
    });
    let methods = herdr.methods();
    let closes = herdr.requests("pane.close");
    assert_eq!(closes.len(), 2, "{methods:?}");
    let first_layout = methods.iter().position(|m| m == "layout.apply").unwrap();
    let last_close = methods.iter().rposition(|m| m == "pane.close").unwrap();
    assert!(last_close < first_layout, "closes come first: {methods:?}");
    // The kept pane already watched ses_work01, so only the other two are made.
    let mut sids = watched_from(&herdr, 0);
    sids.sort();
    assert_eq!(sids, ["ses_ask002", "ses_conv03"]);
    assert!(
        herdr.requests("workspace.create").is_empty(),
        "the workspace stood"
    );
}

/// Run `theseus herdr up` with `args` and wait for it to exit well.
fn up_with(daemon: &FakeDaemon, herdr: &FakeHerdr, args: &[&str]) -> String {
    let out = theseus(&daemon.sock, &herdr.sock)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The startup hook, from anywhere but the board tab, with no task at all: the `theseus`
/// workspace has a `main` tab (the home conversation's chat) and a `board` tab (the follower),
/// the new workspace's blank shell tab is gone, and the command exits. Run again, it sends herdr
/// only reads (theseus-6ifz1: later tasks found no follower, because none ran when idle).
#[test]
fn boot_makes_the_board_tab_and_exits() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), vec![]);
    up_with(&daemon, &herdr, &["--boot"]);
    assert_eq!(herdr.requests("workspace.create").len(), 1);
    let layouts = herdr.requests("layout.apply");
    assert_eq!(layouts.len(), 2);
    let (main, board) = (argv_of(&layouts[0]), argv_of(&layouts[1]));
    assert_eq!(main[main.len() - 1], "chat", "{main:?}");
    assert!(main.iter().any(|a| a == "--socket"), "{main:?}");
    assert!(
        board
            .windows(4)
            .any(|w| w == ["herdr", "up", "--follow", "--here"]),
        "{board:?}"
    );
    assert_eq!(
        herdr.tab_labels(),
        ["main", "board"],
        "and no blank shell tab"
    );
    assert_eq!(herdr.kinds(), ["main", "board"]);
    // A second boot finds both and makes nothing.
    herdr.clear();
    up_with(&daemon, &herdr, &["--boot"]);
    let methods = herdr.methods();
    assert!(
        methods
            .iter()
            .all(|m| m == "workspace.list" || m == "pane.list"),
        "a second boot wrote: {methods:?}"
    );
    assert_eq!(herdr.tab_labels(), ["main", "board"]);
}

/// After a herdr restart the workspace is back as shells without tokens: boot closes them, and
/// makes the workspace and both tabs again, once. With only the board gone it makes only that.
#[test]
fn boot_after_a_herdr_restart_remakes_the_base_tabs_without_doubling_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut model = Model::default();
    let ws = model.ws_of("theseus");
    restored(&mut model, &ws, 1, &[]);
    restored(&mut model, &ws, 2, &[]);
    let herdr = FakeHerdr::start(dir.path(), model);
    let daemon = FakeDaemon::start(dir.path(), vec![]);
    up_with(&daemon, &herdr, &["--boot"]);
    assert_eq!(herdr.requests("pane.close").len(), 2);
    assert_eq!(herdr.tab_labels(), ["main", "board"]);
    assert_eq!(herdr.kinds(), ["main", "board"]);
    // The board's pane goes (the user closed it): the next boot makes it again, and only it.
    let board = herdr
        .model
        .lock()
        .unwrap()
        .panes
        .iter()
        .find(|p| p.tokens["kind"] == "board")
        .unwrap()
        .id
        .clone();
    herdr.model.lock().unwrap().panes.retain(|p| p.id != board);
    herdr.clear();
    up_with(&daemon, &herdr, &["--boot"]);
    assert_eq!(herdr.requests("layout.apply").len(), 1);
    assert!(herdr.requests("workspace.create").is_empty());
    assert_eq!(herdr.kinds(), ["main", "board"]);
}

/// `--follow` from a shell is the same: it makes what is missing and exits, and the board tab's
/// follower (the `--here` process) then lays out tasks as they start.
#[test]
fn follow_from_a_shell_makes_the_base_tabs_too() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let daemon = FakeDaemon::start(dir.path(), vec![]);
    let out = up_with(&daemon, &herdr, &["--follow"]);
    assert!(
        out.contains("main tab") && out.contains("board tab"),
        "{out}"
    );
    assert_eq!(herdr.tab_labels(), ["main", "board"]);
    // The follower in the board pane starts with no task and then gets one: the tab appears.
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    daemon.change(vec![view(
        "exe_9",
        "ses_new009",
        "task",
        "running",
        "working",
        1,
    )]);
    until(10, "the task's tab", || {
        herdr.tabs_of("ses_new009").len() == 1
    });
    assert_eq!(herdr.tab_labels().len(), 3);
    assert_eq!(herdr.requests("workspace.create").len(), 1);
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
    assert_eq!(herdr.requests("layout.apply").len(), 3);
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
    assert_eq!(herdr.requests("layout.apply").len(), 1);
    let tokens = herdr.requests("pane.report_metadata")[0]["params"]["tokens"].clone();
    assert_eq!(tokens["pinned"], "1");
    assert_eq!(tokens["session"], "ses_pinned1");
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
        herdr.requests("layout.apply").len() == 3
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
        3,
        "nothing laid out again"
    );
}

/// A digest line: one thing that happened (`kind`) under the daemon's board.
fn digest_line(kind: &str, n: u64) -> Value {
    json!({"at_ms": n, "kind": kind, "words": format!("{kind} {n}"), "count": 1,
           "work": {"id": format!("exe_{n}"), "kind": "task", "title": format!("job {n}")}})
}

/// The Space row's tokens as the last `workspace.report_metadata` left them.
fn space_row(herdr: &FakeHerdr) -> serde_json::Map<String, Value> {
    herdr
        .requests("workspace.report_metadata")
        .last()
        .map(|r| {
            r["params"]["tokens"]
                .as_object()
                .cloned()
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// The Space row says what happened since the owner last looked (the digest from the
/// seen position, not counting the owner's own answers) and the horizon's two numbers:
/// the next reminder and the routines due. It says it again after the daemon was away
/// and things happened meanwhile, and a reminder or a routine gets no tab.
#[test]
fn the_space_row_carries_the_digest_and_the_horizon_after_a_disconnect() {
    let dir = tempfile::tempdir().unwrap();
    let seen_file = dir.path().join("state/theseus/seen.json");
    theseus_client::seen::record_looked(&seen_file, 7, 1).unwrap();
    let herdr = FakeHerdr::start(dir.path(), Model::default());
    let mut views = three_tasks();
    let mut reminder = view("wake_1", "", "wake", "waiting", "idle", 1);
    reminder["purpose"] = json!("remind");
    reminder["next_at_ms"] = json!(4_000_000_000_000u64);
    let mut routine = view("rtn_1", "ses_rtn001", "routine", "waiting", "idle", 1);
    routine["next_at_ms"] = json!(1);
    views.extend([reminder, routine]);
    let daemon = FakeDaemon::start(dir.path(), views);
    let _f = follow(&daemon.sock, &herdr.sock, &[]);
    until(10, "the Space row", || !space_row(&herdr).is_empty());
    let row = space_row(&herdr);
    assert_eq!(row["since"], "", "nothing happened since the owner looked");
    assert_eq!(row["routines"], "🔁1", "the routine's time has come");
    assert!(row["next"].as_str().unwrap().starts_with('⏰'), "{row:?}");
    until(10, "the tabs", || herdr.requests("layout.apply").len() == 3);
    assert_eq!(
        herdr.requests("layout.apply").len(),
        3,
        "the horizon's kinds get no tab"
    );
    assert_eq!(
        herdr.tab_labels().len(),
        3,
        "and no blank tab: {:?}",
        herdr.tab_labels()
    );

    daemon.go_down();
    until(10, "the loss notice", || {
        herdr.requests("notification.show").len() == 1
    });
    *daemon.digest.lock().unwrap() = vec![
        digest_line("reported", 1),
        digest_line("failed", 2),
        digest_line("answered", 3),
    ];
    daemon.come_back();
    until(10, "the digest on the Space row", || {
        space_row(&herdr).get("since").and_then(Value::as_str) == Some("↺2")
    });
}
