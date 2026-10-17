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

struct FakeHerdr {
    sock: PathBuf,
    panes: Arc<Mutex<Vec<Value>>>,
    got: Arc<Mutex<Vec<Value>>>,
}

impl FakeHerdr {
    fn start(dir: &Path, panes: Vec<Value>) -> Self {
        let sock = dir.join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let panes = Arc::new(Mutex::new(panes));
        let got = Arc::new(Mutex::new(Vec::new()));
        let (p, g) = (panes.clone(), got.clone());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let Ok(c) = c else { break };
                let (p, g) = (p.clone(), g.clone());
                std::thread::spawn(move || {
                    let mut line = String::new();
                    if BufReader::new(&c).read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let req: Value = serde_json::from_str(&line).unwrap();
                    g.lock().unwrap().push(req.clone());
                    let mut panes = p.lock().unwrap();
                    let result = match req["method"].as_str().unwrap() {
                        "pane.list" => json!({"type": "pane_list", "panes": *panes}),
                        "pane.get" => {
                            let id = &req["params"]["pane_id"];
                            let pane = panes.iter().find(|q| &q["pane_id"] == id);
                            json!({"type": "pane_info", "pane": pane})
                        }
                        "layout.apply" => {
                            let n = panes.len() + 1;
                            let tokens = req["params"]["tabs"][0]["panes"][0]["tokens"].clone();
                            panes.push(json!({"pane_id": format!("p{n}"), "tokens": tokens}));
                            json!({"type": "layout_applied", "tabs": [
                                {"tab_id": format!("t{n}"), "panes": [{"pane_id": format!("p{n}")}]}]})
                        }
                        _ => json!({"type": "ok"}),
                    };
                    let _ = writeln!(&c, "{}", json!({"id": req["id"], "result": result}));
                });
            }
        });
        Self { sock, panes, got }
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
    let pane = &made[0]["params"]["tabs"][0]["panes"][0];
    let argv: Vec<&str> = pane["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(
        &argv[1..],
        ["--socket", "/nonexistent/theseusd.sock", "chat"]
    );
    assert_eq!(pane["tokens"]["kind"], "main");
    assert_eq!(herdr.panes.lock().unwrap().len(), 1);
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
