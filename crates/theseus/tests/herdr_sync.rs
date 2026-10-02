//! `theseus herdr sync` end to end (theseus-l1l, design `stage2` §2.10 and §3.2
//! row 11b): the real binary against a scripted daemon and a fake herdr that
//! keeps a small model of its workspaces and panes (their tokens, their agent,
//! and what runs in them), changes it as sync acts, and records every request.
//! A pane given `theseus watch <s> --interactive` becomes that session's watch,
//! as the real watch's reports would make it. These hold what sync makes,
//! restarts, sweeps, leaves alone, and closes, and that a sync with nothing
//! to do sends herdr only reads.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// What runs in a fake pane.
#[derive(Clone, Debug, PartialEq)]
enum Fg {
    /// Its shell, at the prompt.
    Shell,
    /// `theseus watch <session>`.
    Watch(String),
    /// Something else, by name.
    Other(&'static str),
}

#[derive(Clone, Debug)]
struct Pane {
    id: String,
    ws: String,
    session: Option<String>,
    agent: Option<String>,
    fg: Fg,
}

impl Pane {
    /// As `pane.list` and `pane.split` give it.
    fn info(&self) -> Value {
        let tokens = match &self.session {
            Some(s) => json!({ "session": s, "cost": "$0" }),
            None => json!({}),
        };
        json!({"pane_id": self.id, "workspace_id": self.ws, "tab_id": format!("{}:t1", self.ws),
               "terminal_id": format!("term-{}", self.id), "focused": false,
               "agent_status": "idle", "revision": 1, "agent": self.agent, "tokens": tokens})
    }
}

#[derive(Default)]
struct Model {
    workspaces: Vec<(String, String)>,
    panes: Vec<Pane>,
    next_ws: u32,
    next_pane: u32,
    /// What a new pane's shell runs first: none, its prompt.
    startup: Option<&'static str>,
}

impl Model {
    fn pane(&mut self, id: &str) -> &mut Pane {
        self.panes
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("no pane {id}"))
    }

    fn new_pane(&mut self, ws: &str) -> String {
        self.next_pane += 1;
        let id = format!("{ws}:p{}", self.next_pane);
        self.panes.push(Pane {
            id: id.clone(),
            ws: ws.to_string(),
            session: None,
            agent: None,
            fg: self.startup.map_or(Fg::Shell, Fg::Other),
        });
        id
    }

    /// One request, answered as herdr answers it.
    fn answer(&mut self, m: &str, p: &Value) -> Result<Value, Value> {
        let pane_id = p["pane_id"].as_str().unwrap_or_default().to_string();
        Ok(match m {
            "workspace.list" => {
                let ws: Vec<Value> = self
                    .workspaces
                    .iter()
                    .map(|(id, label)| json!({"workspace_id": id, "label": label}))
                    .collect();
                json!({"type": "workspace_list", "workspaces": ws})
            }
            "pane.list" => {
                let panes: Vec<Value> = self.panes.iter().map(Pane::info).collect();
                json!({"type": "pane_list", "panes": panes})
            }
            "pane.process_info" => {
                let fg = self.pane(&pane_id).fg.clone();
                let (group, procs) = match fg {
                    Fg::Shell => (
                        100,
                        json!([{"pid": 100, "name": "bash", "argv": ["-bash"]}]),
                    ),
                    Fg::Watch(s) => (
                        200,
                        json!([{"pid": 200, "name": "theseus",
                                 "argv": [THESEUS, "--socket", "/s", "watch", s, "--interactive"]}]),
                    ),
                    Fg::Other(name) => (300, json!([{"pid": 300, "name": name, "argv": [name]}])),
                };
                json!({"type": "pane_process_info", "process_info": {"pane_id": pane_id,
                       "shell_pid": 100, "foreground_process_group_id": group,
                       "foreground_processes": procs}})
            }
            "workspace.create" => {
                self.next_ws += 1;
                let ws = format!("w{}", self.next_ws);
                let label = p["label"].as_str().unwrap().to_string();
                self.workspaces.push((ws.clone(), label.clone()));
                let root = self.new_pane(&ws);
                let root = self.pane(&root).info();
                json!({"type": "workspace_created",
                       "workspace": {"workspace_id": ws, "label": label},
                       "tab": {"tab_id": format!("{ws}:t1"), "workspace_id": ws},
                       "root_pane": root})
            }
            "pane.layout" => {
                let ws = self.pane(&pane_id).ws.clone();
                let panes: Vec<Value> = self
                    .panes
                    .iter()
                    .filter(|p| p.ws == ws)
                    .map(|p| {
                        json!({"pane_id": p.id, "focused": false,
                                    "rect": {"x": 0, "y": 0, "width": 100, "height": 50}})
                    })
                    .collect();
                json!({"type": "pane_layout", "layout": {"workspace_id": ws, "panes": panes}})
            }
            "pane.split" => {
                let ws = p["workspace_id"].as_str().unwrap().to_string();
                let id = self.new_pane(&ws);
                json!({"type": "pane_info", "pane": self.pane(&id).info()})
            }
            "pane.send_input" => {
                let text = p["text"].as_str().unwrap();
                let words: Vec<&str> = text.split(' ').collect();
                let pane = self.pane(&pane_id);
                assert_eq!(
                    pane.fg,
                    Fg::Shell,
                    "typed into a pane its shell does not hold"
                );
                if let Some(i) = words.iter().position(|w| *w == "watch") {
                    // The watch's first reports: its state and its tokens.
                    let s = words[i + 1].to_string();
                    pane.fg = Fg::Watch(s.clone());
                    pane.session = Some(s);
                    pane.agent = Some("theseus".into());
                }
                json!({"type": "ok"})
            }
            "pane.clear_agent_authority" => {
                self.pane(&pane_id).agent = None;
                json!({"type": "ok"})
            }
            "pane.report_metadata" => {
                if let Some(s) = p["tokens"]["session"].as_str() {
                    self.pane(&pane_id).session = Some(s.to_string());
                }
                json!({"type": "ok"})
            }
            "pane.close" => {
                self.panes.retain(|p| p.id != pane_id);
                json!({"type": "ok"})
            }
            other => {
                return Err(json!({"code": "unknown_method", "message": format!("no {other}")}))
            }
        })
    }
}

struct FakeHerdr {
    sock: PathBuf,
    model: Arc<Mutex<Model>>,
    got: Arc<Mutex<Vec<Value>>>,
}

impl FakeHerdr {
    fn start(dir: &Path, model: Model) -> Self {
        let sock = dir.join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let model = Arc::new(Mutex::new(model));
        let got = Arc::new(Mutex::new(Vec::new()));
        let (m, g) = (model.clone(), got.clone());
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(s) = s else { break };
                let mut line = String::new();
                if BufReader::new(&s).read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let req: Value = serde_json::from_str(&line).unwrap();
                let out = match m
                    .lock()
                    .unwrap()
                    .answer(req["method"].as_str().unwrap(), &req["params"])
                {
                    Ok(r) => json!({"id": req["id"], "result": r}),
                    Err(e) => json!({"id": req["id"], "error": e}),
                };
                g.lock().unwrap().push(req);
                let _ = writeln!(&s, "{out}");
            }
        });
        Self { sock, model, got }
    }

    /// The requests since the last take.
    fn take(&self) -> Vec<Value> {
        std::mem::take(&mut *self.got.lock().unwrap())
    }
}

/// A scripted daemon: answers `session.list` with `sessions`, on every
/// connection.
fn daemon(dir: &Path, sessions: Value) -> PathBuf {
    let sock = dir.join("theseusd.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(s) = s else { break };
            let sessions = sessions.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(&s).lines() {
                    let Ok(line) = line else { break };
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let out = match req["method"].as_str() {
                        Some("session.list") => {
                            json!({"jsonrpc": "2.0", "id": req["id"], "result": {"sessions": sessions}})
                        }
                        _ => json!({"jsonrpc": "2.0", "id": req["id"],
                                    "error": {"code": -32601, "message": "unexpected"}}),
                    };
                    let _ = writeln!(&s, "{out}");
                }
            });
        }
    });
    sock
}

fn session(id: &str, level: &str, label: &str) -> Value {
    json!({"session_id": id, "kind": "conversation", "label": null,
           "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1,
           "attention": {"level": level, "label": label, "since_ms": 0}})
}

/// `theseus herdr sync ARGS`: its exit code, stdout, and stderr.
fn sync(daemon: &Path, herdr: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(daemon)
        .args(["herdr", "sync", "--herdr-socket"])
        .arg(herdr)
        .args(args)
        .env_remove("THESEUS_SOCKET")
        .env_remove("HERDR_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Each request as `method pane`, or `method` alone: what it did, and where.
fn brief(got: &[Value]) -> Vec<String> {
    got.iter()
        .map(|r| {
            let m = r["method"].as_str().unwrap();
            let p = &r["params"];
            match p["pane_id"].as_str().or(p["target_pane_id"].as_str()) {
                Some(pane) => format!("{m} {pane}"),
                None => m.to_string(),
            }
        })
        .collect()
}

/// A request that only reads.
fn reads(r: &str) -> bool {
    let method = r.split(' ').next().unwrap_or_default();
    ["workspace.list", "pane.list", "pane.process_info"].contains(&method)
}

/// The design's prove (§3.2 row 11b): sync against a scripted pane list sends
/// the expected requests, and a second run sends only reads. The scene:
/// sessions that need you and work, one ready and pinned, one finished whose
/// pane's watch died, one working whose watch died with its shell at the
/// prompt, one whose pane runs an editor, and a pane of another daemon's.
#[test]
fn sync_makes_mends_and_then_sends_only_reads() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = json!([
        session(
            "ses_needs1",
            "needs_you",
            "confirm proc.run: run the tide tables"
        ),
        session("ses_work02", "working", "turn 2"),
        session("ses_ready3", "ready", "ready"),
        session("ses_done04", "idle", "complete"),
        session("ses_crash5", "working", "turn 4"),
        session("ses_busy06", "needs_you", "blocked: the reef"),
    ]);
    let d = daemon(dir.path(), sessions);
    let mut model = Model {
        workspaces: vec![("w1".into(), "home".into())],
        next_ws: 1,
        next_pane: 9,
        ..Model::default()
    };
    let pane = |id: &str, s: Option<&str>, agent: Option<&str>, fg: Fg| Pane {
        id: id.into(),
        ws: "w1".into(),
        session: s.map(str::to_string),
        agent: agent.map(str::to_string),
        fg,
    };
    model.panes = vec![
        pane("w1:p1", None, None, Fg::Shell),
        pane("w1:p2", Some("ses_done04"), Some("theseus"), Fg::Shell),
        pane("w1:p3", Some("ses_crash5"), Some("theseus"), Fg::Shell),
        pane("w1:p4", Some("ses_busy06"), None, Fg::Other("vim")),
        pane(
            "w1:p5",
            Some("ses_other9"),
            Some("theseus"),
            Fg::Watch("ses_other9".into()),
        ),
    ];
    let herdr = FakeHerdr::start(dir.path(), model);

    // The first run makes, restarts, and sweeps, but closes nothing unasked.
    let (code, out, err) = sync(&d, &herdr.sock, &["--pin", "ses_ready3"]);
    assert_eq!(code, 0, "{out}{err}");
    let first = herdr.take();
    assert_eq!(
        brief(&first),
        [
            "workspace.list",
            "pane.list",
            "pane.process_info w1:p2",
            "pane.process_info w1:p3",
            "pane.process_info w1:p4",
            "pane.clear_agent_authority w1:p2",
            "pane.clear_agent_authority w1:p3",
            "workspace.create",
            "pane.process_info w2:p10",
            "pane.send_input w2:p10",
            "pane.layout w2:p10",
            "pane.split w2:p10",
            "pane.process_info w2:p11",
            "pane.send_input w2:p11",
            "pane.layout w2:p11",
            "pane.split w2:p11",
            "pane.process_info w2:p12",
            "pane.send_input w2:p12",
            "pane.send_input w1:p3",
        ],
        "{out}"
    );
    // The details: the workspace, the split, the sweep, and the command.
    let find = |m: &str| first.iter().find(|r| r["method"] == m).unwrap()["params"].clone();
    assert_eq!(
        find("workspace.create"),
        json!({"label": "theseus", "focus": false})
    );
    let split = find("pane.split");
    assert_eq!(
        (
            split["workspace_id"].as_str(),
            split["focus"].as_bool(),
            split["direction"].as_str()
        ),
        (Some("w2"), Some(false), Some("right"))
    );
    let sweep = find("pane.clear_agent_authority");
    assert_eq!(sweep["source"], "custom:theseus");
    assert!(
        sweep["seq"].as_u64().unwrap() > 1_759_000_000_000_000,
        "a seq from the clock: {sweep}"
    );
    let typed = find("pane.send_input");
    let text = typed["text"].as_str().unwrap();
    // The binary by its whole path, as the kernel names it (no symlinks).
    let exe = std::fs::canonicalize(THESEUS).unwrap();
    assert_eq!(
        text,
        format!(
            "{} --socket {} watch ses_needs1 --interactive",
            exe.display(),
            d.display()
        )
    );
    assert_eq!(typed["keys"], json!(["Enter"]));
    for line in [
        "made herdr's workspace `theseus` (w2)",
        "made      w2:p10   ses_needs1  needs you · confirm proc.run: run the tide tables",
        "restarted w1:p3    ses_crash5  working · turn 4",
        "swept     w1:p2    ses_done04  idle · complete (its watch had stopped: its state is cleared)",
        "busy      w1:p4    ses_busy06  needs you · blocked: the reef (vim runs there: nothing \
         typed; sync starts the watch there once its shell is back at a prompt)",
        "1 pane(s) watch sessions this daemon does not know (another daemon's?): left alone",
    ] {
        assert!(out.contains(line), "missing `{line}` in:\n{out}");
    }

    // The second, with nothing missing (and no pin: the pinned session's pane
    // stays), sends herdr only reads, and makes no second pane for the busy one.
    let (code, out, err) = sync(&d, &herdr.sock, &[]);
    assert_eq!(code, 0, "{out}{err}");
    let second = brief(&herdr.take());
    assert!(
        second.iter().all(|r| reads(r)),
        "a second sync sends only reads: {second:?}"
    );
    assert!(
        out.contains("nothing changed: a busy pane waits for its shell's prompt"),
        "{out}"
    );
    for line in [
        "watched   w2:p10   ses_needs1",
        // Panes whose sessions want none now are kept, and said.
        "kept      w2:p12   ses_ready3  ready · ready",
        "kept      w1:p2    ses_done04  idle · complete (no watch runs there)",
    ] {
        assert!(out.contains(line), "missing `{line}` in:\n{out}");
    }

    // The editor exits: the next sync starts the watch in that pane, and the
    // one after it sends only reads again.
    herdr.model.lock().unwrap().pane("w1:p4").fg = Fg::Shell;
    let (code, out, err) = sync(&d, &herdr.sock, &[]);
    assert_eq!(code, 0, "{out}{err}");
    let writes: Vec<String> = brief(&herdr.take())
        .into_iter()
        .filter(|r| !reads(r))
        .collect();
    assert_eq!(writes, ["pane.send_input w1:p4"], "{out}");
    let (code, out, err) = sync(&d, &herdr.sock, &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(brief(&herdr.take()).iter().all(|r| reads(r)), "{out}");
    assert!(out.contains("herdr is in sync: nothing changed"), "{out}");

    // Asked to, it closes the finished session's pane, and only that.
    let (code, out, err) = sync(&d, &herdr.sock, &["--close-finished"]);
    assert_eq!(code, 0, "{out}{err}");
    let changes: Vec<String> = brief(&herdr.take())
        .into_iter()
        .filter(|r| !reads(r))
        .collect();
    assert_eq!(changes, ["pane.close w1:p2"], "{out}");
    assert!(
        out.contains("closed    w1:p2    ses_done04  idle · complete"),
        "{out}"
    );
    let model = herdr.model.lock().unwrap();
    assert!(model.panes.iter().all(|p| p.id != "w1:p2"));
    assert!(
        model.panes.iter().any(|p| p.id == "w1:p5"),
        "another daemon's pane is left alone"
    );
}

/// A new pane whose shell never reaches its prompt (its startup files ask for
/// a key's passphrase) is never typed into. It gets the session's token, so
/// the next sync, once its shell is at the prompt, starts the watch there and
/// makes no second pane.
#[test]
fn a_new_pane_is_typed_into_only_at_its_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(
        dir.path(),
        json!([session(
            "ses_needs1",
            "needs_you",
            "confirm proc.run: run the tide tables"
        )]),
    );
    let model = Model {
        startup: Some("ssh-add"),
        ..Model::default()
    };
    let herdr = FakeHerdr::start(dir.path(), model);
    let started = Instant::now();
    let (code, out, err) = sync(&d, &herdr.sock, &[]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "it waits for the prompt"
    );
    let first = brief(&herdr.take());
    assert!(
        !first.iter().any(|r| r.starts_with("pane.send_input")),
        "{first:?}"
    );
    assert_eq!(first.last().unwrap(), "pane.report_metadata w1:p1");
    assert!(
        out.contains(
            "made      w1:p1    ses_needs1  needs you · confirm proc.run: run the tide tables \
             (ssh-add runs there, not its shell's prompt: nothing typed; sync again once it is)"
        ),
        "{out}"
    );
    // The passphrase is typed, and the shell reaches its prompt.
    herdr.model.lock().unwrap().pane("w1:p1").fg = Fg::Shell;
    let (code, out, err) = sync(&d, &herdr.sock, &[]);
    assert_eq!(code, 0, "{out}{err}");
    let writes: Vec<String> = brief(&herdr.take())
        .into_iter()
        .filter(|r| !reads(r))
        .collect();
    assert_eq!(writes, ["pane.send_input w1:p1"], "{out}");
    assert!(out.contains("restarted w1:p1    ses_needs1"), "{out}");
}

/// No herdr: sync says where it looked, and exits 1.
#[test]
fn sync_without_herdr_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let d = daemon(dir.path(), json!([]));
    let missing = dir.path().join("no-herdr.sock");
    let (code, out, err) = sync(&d, &missing, &[]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(err.contains("connecting to herdr at"), "{err}");
    assert!(err.contains("no-herdr.sock"), "{err}");
}
