//! prefix+m and prefix+i end to end (theseus-m638c): the real binary against a scripted herdr
//! that keeps its panes and records every request. `herdr main` makes the `main` tab once and
//! focuses it after; `herdr popup inspect` opens the inspect popup on the focused task's session.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

#[path = "support/herdr_schema.rs"]
mod herdr_schema;

/// A fake herdr that refuses, as herdr's own schema does, a request of a shape herdr does not take
/// (`support/herdr_schema.rs`) and fails the test when dropped with any noted.
struct FakeHerdr {
    sock: PathBuf,
    panes: Arc<Mutex<Vec<Value>>>,
    got: Arc<Mutex<Vec<Value>>>,
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
    fn start(dir: &Path, panes: Vec<Value>) -> Self {
        let sock = dir.join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let panes = Arc::new(Mutex::new(panes));
        let got = Arc::new(Mutex::new(Vec::new()));
        let bad = Arc::new(Mutex::new(Vec::new()));
        let workspaces = Arc::new(Mutex::new(Vec::<Value>::new()));
        let schema = Arc::new(herdr_schema::Schema::load());
        let (p, g, b) = (panes.clone(), got.clone(), bad.clone());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let Ok(c) = c else { break };
                let (p, g, b) = (p.clone(), g.clone(), b.clone());
                let (schema, workspaces) = (schema.clone(), workspaces.clone());
                std::thread::spawn(move || {
                    let mut line = String::new();
                    if BufReader::new(&c).read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let req: Value = serde_json::from_str(&line).unwrap();
                    g.lock().unwrap().push(req.clone());
                    let method = req["method"].as_str().unwrap();
                    if let Err(e) = schema.check_request(method, &req["params"]) {
                        b.lock().unwrap().push(format!("{method}: {e}"));
                        let err = json!({"code": "invalid_request", "message": e});
                        let _ = writeln!(&c, "{}", json!({"id": req["id"], "error": err}));
                        return;
                    }
                    let mut panes = p.lock().unwrap();
                    let result = match method {
                        "pane.list" => json!({"type": "pane_list", "panes": *panes}),
                        "pane.get" => {
                            let id = &req["params"]["pane_id"];
                            let pane = panes.iter().find(|q| &q["pane_id"] == id);
                            json!({"type": "pane_info", "pane": pane})
                        }
                        "workspace.list" => {
                            json!({"type": "workspace_list", "workspaces": *workspaces.lock().unwrap()})
                        }
                        "workspace.create" => {
                            let ws = json!({"workspace_id": "w1", "label": req["params"]["label"]});
                            workspaces.lock().unwrap().push(ws.clone());
                            json!({"type": "workspace_created", "workspace": ws,
                                   "tab": {"tab_id": "w1:t0"}, "root_pane": {"pane_id": "p0"}})
                        }
                        "layout.apply" => {
                            let n = panes.len() + 1;
                            panes.push(json!({"pane_id": format!("p{n}"), "tokens": {}}));
                            json!({"type": "layout_apply", "layout": {
                                "workspace_id": "w1", "tab_id": format!("t{n}"), "zoomed": false,
                                "focused_pane_id": format!("p{n}"),
                                "root": {"type": "pane", "pane_id": format!("p{n}")}}})
                        }
                        "pane.report_metadata" => {
                            let id = &req["params"]["pane_id"];
                            if let Some(q) = panes.iter_mut().find(|q| &q["pane_id"] == id) {
                                q["tokens"] = req["params"]["tokens"].clone();
                            }
                            json!({"type": "ok"})
                        }
                        _ => json!({"type": "ok"}),
                    };
                    let _ = writeln!(&c, "{}", json!({"id": req["id"], "result": result}));
                });
            }
        });
        Self {
            sock,
            panes,
            got,
            bad,
        }
    }

    fn requests(&self, method: &str) -> Vec<Value> {
        let all = self.got.lock().unwrap();
        all.iter()
            .filter(|r| r["method"] == method)
            .cloned()
            .collect()
    }
}

fn theseus(herdr: &Path, args: &[&str]) -> std::process::Output {
    Command::new(THESEUS)
        .args(["--socket", "/nonexistent/theseusd.sock", "herdr"])
        .args(args)
        .arg("--herdr-socket")
        .arg(herdr)
        .env_remove("THESEUS_SOCKET")
        .env_remove("HERDR_SOCKET_PATH")
        .output()
        .unwrap()
}

/// The first prefix+m makes the `main` tab, running `theseus chat`; every later one only focuses it.
#[test]
fn prefix_m_makes_the_main_tab_once_and_then_focuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(dir.path(), Vec::new());
    for _ in 0..3 {
        let out = theseus(&herdr.sock, &["main"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let made = herdr.requests("layout.apply");
    assert_eq!(made.len(), 1, "the main tab is made once: {made:?}");
    let pane = &made[0]["params"]["root"];
    assert_eq!(made[0]["params"]["tab_label"], "main");
    assert_eq!(made[0]["params"]["focus"], true);
    let argv: Vec<&str> = pane["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(
        &argv[1..],
        ["--socket", "/nonexistent/theseusd.sock", "chat"]
    );
    let panes = herdr.panes.lock().unwrap().clone();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["tokens"]["kind"], "main", "the token is set after");
    assert_eq!(herdr.requests("workspace.create").len(), 1);
    let focused = herdr.requests("pane.focus");
    assert_eq!(focused.len(), 3, "each key focuses the tab: {focused:?}");
    assert!(focused.iter().all(|f| f["params"]["pane_id"] == "p1"));
}

/// prefix+i opens the inspect popup on the session the focused pane shows, and only for a task.
#[test]
fn prefix_i_opens_the_inspect_popup_on_the_focused_tasks_session() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(
        dir.path(),
        vec![
            json!({"pane_id": "p1", "tokens": {"kind": "task", "session": "ses_task9f2"}}),
            json!({"pane_id": "p2", "tokens": {"kind": "conversation", "session": "ses_chat1"}}),
        ],
    );
    let out = theseus(&herdr.sock, &["popup", "inspect", "--pane", "p1"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let open = herdr.requests("plugin.pane.open");
    assert_eq!(open.len(), 1);
    let p = &open[0]["params"];
    assert_eq!(
        (p["entrypoint"].as_str(), p["placement"].as_str()),
        (Some("inspect"), Some("popup"))
    );
    assert_eq!(p["env"]["THESEUS_SESSION_ARG"], "ses_task9f2");
    let out = theseus(&herdr.sock, &["popup", "inspect", "--pane", "p2"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not show a task"));
    assert_eq!(herdr.requests("plugin.pane.open").len(), 1);
}

/// A key's popup that needs the focused pane refuses without one, with a line that says so, and
/// opens nothing: `answer` with no pane must never open the question that has waited longest (a `y`
/// there would approve an unrelated request; theseus-us8e8). A link click names its question, so it
/// needs no pane; the popups that need nothing still open.
#[test]
fn a_popup_that_needs_the_focused_pane_refuses_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let herdr = FakeHerdr::start(
        dir.path(),
        vec![json!({"pane_id": "p1", "tokens": {"kind": "task", "session": "ses_task9f2"}})],
    );
    for name in ["answer", "stop", "inspect"] {
        let out = theseus(&herdr.sock, &["popup", name]);
        assert!(!out.status.success(), "popup {name} without a pane");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("needs the focused pane"),
            "popup {name}: the refusal says what is missing: {err}"
        );
        assert_eq!(
            herdr.requests("plugin.pane.open").len(),
            0,
            "popup {name} without a pane opened a pane"
        );
    }
    // An empty pane id is no pane: the manifest's `${P:+...}` never passes one, but a stray `--pane ""`
    // must not slip through.
    let out = theseus(&herdr.sock, &["popup", "answer", "--pane", ""]);
    assert!(!out.status.success());
    assert_eq!(herdr.requests("plugin.pane.open").len(), 0);
    // A question named by a link click needs no pane; a pane is fine; the popups with no pane need none.
    for args in [
        &["popup", "answer", "--cid", "act_0c7e527d2a"][..],
        &["popup", "answer", "--pane", "p1"][..],
        &["popup", "stop", "--pane", "p1"][..],
        &["popup", "tui"][..],
        &["popup", "new"][..],
    ] {
        let out = theseus(&herdr.sock, args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(herdr.requests("plugin.pane.open").len(), 5);
}
