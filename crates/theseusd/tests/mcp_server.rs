//! Theseus's own MCP server, wired in (step 41b, M7 §2.5), with the real
//! `theseusd`, its real listener on 127.0.0.1, its real job wrappers, and
//! the real `theseus` CLI; `theseus-mcp`'s client is the MCP client. A
//! stand-in for the Messages API asks for the tool calls, and a stand-in
//! `op` gives every secret, the server's key among them.
//! - a client opens a conversation, sends it text, and gets the reply; the
//!   session is labelled `mcp <client> <label>`; each call is a
//!   `mcp_server.call` row; a turn into a session the client did not open is
//!   refused;
//! - a wrong key gets 401 and one `mcp_server.refused` row; a foreign
//!   `Origin` gets 403;
//! - an acting call in its turn waits, under `notify` everywhere else, and
//!   the operator's `theseus confirm` lets it run; the client then reads the
//!   reply;
//! - a job's process that opens a session through MCP passes on its
//!   session's hold of external text.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_mcp::client::{Client, HttpTarget, Options, Transport};
use theseus_mcp::types::Implementation;

/// What the stand-in `op` gives every secret: the server's key too.
const KEY: &str = "test-secret-value-0000";

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    rt: tokio::runtime::Runtime,
    _model: FakeModel,
}

/// Where a job writes what it saw: inside the workspace root, so each
/// `proc.run` there runs under `notify` but for the floor.
const OUT: &str = "projects/out";

impl Rig {
    fn start() -> Self {
        let script = Script::default();
        let asks = script.clone();
        let model = FakeModel::start(move |prompt| {
            asks.lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.starts_with(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects/bin", OUT] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let cli = theseusd.with_file_name("theseus");
        assert!(
            cli.exists(),
            "no {} to run: build the CLI (cargo build -p theseus)",
            cli.display()
        );
        std::fs::copy(&cli, path("projects/bin/theseus")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let exe = |p: &PathBuf, body: &str| {
            std::fs::write(p, body).unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exe(
            &path("bin/op"),
            &format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/{KEY}/g' ;;\n\
                 \x20 read) printf '%s' {KEY} ;;\n\
                 \x20 *) exit 1 ;;\n\
                 esac\n"
            ),
        );
        // `[policy] external_programs` lists `gh`: what it prints is outside text.
        exe(
            &path("projects/bin/gh"),
            "#!/bin/sh\necho 'issue 7: a stranger wrote this'\n",
        );
        let t = config(&theseusd, &path("projects"), &model.base);
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("state"))
                .arg("--socket")
                .arg(path("projects/sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .env_remove("THESEUS_SESSION")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let mut r = Self {
            dir,
            daemon,
            script,
            rt: tokio::runtime::Runtime::new().unwrap(),
            _model: model,
        };
        r.until("the MCP server listening", |h| {
            h["mcp_server"]["state"] == "listening"
        });
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn asks(&self, prompt: &str, calls: Vec<(&'static str, Value)>) {
        self.script.lock().unwrap().push((prompt.into(), calls));
    }

    fn until(&mut self, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Ok(h) = self.call("health", Value::Null) {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(status) = self.daemon.try_wait() {
                panic!("theseusd exited ({status}) before {what}:\n{}", self.log());
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s =
            UnixStream::connect(self.path("projects/sock")).map_err(|e| json!(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| json!(e.to_string()))?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line.map_err(|e| json!(e.to_string()))?)
                .map_err(|e| json!(e.to_string()))?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.clone()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err(json!("the connection closed"))
    }

    /// The operator's own `theseus`, from outside every job.
    fn cli(&self, args: &[&str]) -> std::process::Output {
        Command::new(self.path("projects/bin/theseus"))
            .arg("--socket")
            .arg(self.path("projects/sock"))
            .args(args)
            .env_remove("THESEUS_SESSION")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .cloned()
            .collect()
    }

    fn port(&self) -> u16 {
        let h = self.call("health", Value::Null).unwrap();
        u16::try_from(h["mcp_server"]["port"].as_u64().unwrap()).unwrap()
    }

    fn target(&self, key: &str) -> HttpTarget {
        let mut t = HttpTarget::new(format!("http://127.0.0.1:{}/mcp", self.port()));
        t.connect_timeout = Duration::from_secs(2);
        t.bearer = Some(key.into());
        t
    }

    /// An MCP client named `name`, with the key.
    fn client(&self, name: &str) -> Client {
        let opts = Options {
            client_info: Implementation::new(name, "1.0"),
            request_timeout: Duration::from_secs(30),
            call_timeout: Duration::from_secs(30),
            ..Options::default()
        };
        self.rt
            .block_on(Client::connect(Transport::Http(self.target(KEY)), opts))
            .unwrap_or_else(|e| panic!("a handshake: {e}\n{}", self.log()))
            .0
    }

    /// A tool's answer: its JSON, and whether it was an error.
    fn tool(&self, c: &Client, name: &str, args: Value) -> (Value, bool) {
        let r = self
            .rt
            .block_on(c.call_tool(name, args))
            .unwrap_or_else(|e| panic!("{name}: {e}\n{}", self.log()));
        let v = r
            .structured_content
            .clone()
            .unwrap_or_else(|| json!(r.text_for_model()));
        (v, r.is_error)
    }

    fn ok(&self, c: &Client, name: &str, args: Value) -> Value {
        let (v, err) = self.tool(c, name, args);
        assert!(!err, "{name}: {v}\n{}", self.log());
        v
    }

    /// A request written by hand: its status line and body.
    fn raw(&self, headers: &str, body: &str) -> (String, String) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port())).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\ncontent-type: application/json\r\n\
             {headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            self.port(),
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let status = out.lines().next().unwrap_or("").to_string();
        let body = out.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s
            .lines()
            .filter(|l| !l.contains("client connected") && !l.contains("client disconnected"))
            .collect();
        lines[lines.len().saturating_sub(40)..].join("\n")
    }
}

/// The template made safe (`common::safe_note`), on the stand-in model, under
/// `notify`, with the MCP server on, on a port the kernel picks.
fn config(theseusd: &std::path::Path, projects: &std::path::Path, base: &str) -> toml::Table {
    let mut t: toml::Table = common::safe_note(theseusd, projects, 100.0)
        .parse()
        .unwrap();
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
    }
    table(&mut t, "model").insert("api_base".into(), base.into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        p.as_table_mut()
            .unwrap()
            .insert("api_base".into(), base.into());
    }
    table(&mut t, "policy").insert("enforcement".into(), "notify".into());
    table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
    table(&mut t, "secrets").insert(
        "mcp_server_key".into(),
        "op://Test/mcp_server_key/credential".into(),
    );
    let m = table(&mut t, "mcp_server");
    m.insert("enabled".into(), true.into());
    m.insert("port".into(), 0.into());
    m.insert("spend_limit_usd".into(), 2.0.into());
    t
}

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"raw","version":"0"}}}"#;

/// The session `sid` is labelled `label`, with `[mcp_server]`'s limit.
fn the_clients_session(r: &Rig, sid: &str, label: &str) {
    let list = r.call("session.list", json!({})).unwrap();
    let s = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["session_id"] == sid)
        .unwrap_or_else(|| panic!("{list}"));
    assert_eq!(s["label"], label);
    let execs = r.call("execution.list", json!({})).unwrap();
    let e = execs["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["session_id"] == sid)
        .unwrap_or_else(|| panic!("{execs}"));
    assert_eq!(e["budget"]["limit_usd"], 2.0, "{e}");
}

/// Open, send, and get the reply: the session is the client's, labelled
/// `mcp <client> <label>`, with `[mcp_server]`'s limit; each call is a row; a turn into a session the
/// client did not open is refused; a wrong key and a foreign Origin are
/// refused, the key's with one row.
#[test]
fn a_client_opens_a_conversation_sends_it_text_and_gets_the_reply() {
    let r = Rig::start();
    let c = r.client("lantern-agent");
    let tools = r.rt.block_on(c.list_tools()).unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "conversation_open",
            "conversation_send",
            "conversation_status",
            "task_list",
            "wake_list"
        ]
    );
    let opened = r.ok(&c, "conversation_open", json!({"label": "tide tables"}));
    let sid = opened["session_id"].as_str().unwrap().to_string();
    the_clients_session(&r, &sid, "mcp lantern-agent tide tables");

    let sent = r.ok(
        &c,
        "conversation_send",
        json!({"session_id": sid, "text": "hello there"}),
    );
    assert_eq!(sent["reply"], "Nothing to do.", "{sent}");
    assert!(
        sent["turn_id"].as_str().unwrap().starts_with("turn_"),
        "{sent}"
    );
    let st = r.ok(&c, "conversation_status", json!({"session_id": sid}));
    assert_eq!(st["last_reply"], "Nothing to do.", "{st}");
    assert_eq!(st["turns"], 1, "{st}");
    assert_eq!(st["turn_id"], sent["turn_id"], "{st}");
    let tasks = r.ok(&c, "task_list", json!({"session_id": sid}));
    assert!(tasks["tasks"].as_array().unwrap().is_empty(), "{tasks}");
    let wakes = r.ok(&c, "wake_list", json!({}));
    assert!(wakes["wakes"].is_array(), "{wakes}");

    // The operator's own conversation is not the client's to write in.
    let own = r
        .call("session.open", json!({"label": "the operator's"}))
        .unwrap();
    let (refused, err) = r.tool(
        &c,
        "conversation_send",
        json!({"session_id": own["session_id"], "text": "write here"}),
    );
    assert!(err, "{refused}");
    assert!(
        refused
            .as_str()
            .unwrap()
            .contains("is not a conversation an MCP client opened"),
        "{refused}"
    );

    // One row per call, each with its tool, session, client, and latency.
    let calls = r.wait("the calls' rows", || {
        let rows = r.ledger("mcp_server.call");
        (rows.len() >= 6).then_some(rows)
    });
    let tools: Vec<&str> = calls
        .iter()
        .map(|c| c["data"]["tool"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        [
            "conversation_open",
            "conversation_send",
            "conversation_status",
            "task_list",
            "wake_list",
            "conversation_send"
        ],
        "{calls:?}"
    );
    assert_eq!(calls[1]["session_id"], sid.as_str());
    assert_eq!(calls[1]["data"]["client"], "lantern-agent");
    assert!(calls[1]["data"]["latency_ms"].is_u64());
    assert_eq!(
        (
            calls[1]["data"]["ok"].clone(),
            calls[5]["data"]["ok"].clone()
        ),
        (json!(true), json!(false))
    );

    let h = r.call("health", Value::Null).unwrap();
    let m = &h["mcp_server"];
    assert_eq!(m["opened"], 1, "{m}");
    assert_eq!(m["last_client"], "lantern-agent", "{m}");
    assert_eq!(m["calls"], 6, "{m}");
    assert_eq!(m["errors"], 1, "{m}");
    // The CLI's health says it in a line.
    let out = r.cli(&["health"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("mcp server: listening on 127.0.0.1:"),
        "{text}"
    );
}

/// An acting call in an MCP client's turn waits for the operator, though
/// every other call here runs under `notify`: the send answers `running`
/// past its wait, the call has not run, and `conversation_status` says what
/// it waits on. The operator's `theseus confirm` lets it run, and the client
/// then reads the reply.
#[test]
fn an_acting_call_waits_and_the_cli_approves_it() {
    let r = Rig::start();
    let out = r.path(OUT).join("acted");
    r.asks(
        "act for me",
        vec![(
            "proc_run",
            json!({"argv": ["sh", "-c", format!("echo acted > {}", out.display())]}),
        )],
    );
    let c = r.client("lantern-agent");
    let sid = r.ok(&c, "conversation_open", json!({}))["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let sent = r.ok(
        &c,
        "conversation_send",
        json!({"session_id": sid, "text": "act for me", "wait_secs": 2}),
    );
    assert_eq!(sent["status"], "running", "{sent}");
    assert!(
        sent["turn_id"].as_str().unwrap().starts_with("turn_"),
        "{sent}"
    );
    assert!(!out.exists(), "the call ran before the operator answered");
    let st = r.ok(&c, "conversation_status", json!({"session_id": sid}));
    assert_eq!(st["waiting_on"], "confirm", "{st}");
    let pending = r.call("confirm.list", json!({})).unwrap();
    let id = pending["confirms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["session_id"] == sid.as_str())
        .unwrap_or_else(|| panic!("{pending}"))["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let why = pending.to_string();
    assert!(why.contains("an MCP client opened this session"), "{why}");
    let answered = r.cli(&["confirm", "--approve", &id, "--no-wait"]);
    assert!(
        answered.status.success(),
        "{}{}",
        String::from_utf8_lossy(&answered.stdout),
        String::from_utf8_lossy(&answered.stderr)
    );
    r.wait("the call's file", || out.exists().then_some(()));
    let st = r.wait("the reply", || {
        let (st, _) = r.tool(&c, "conversation_status", json!({"session_id": sid}));
        (st["state"] == "waiting" && st["waiting_on"] == "input").then_some(st)
    });
    assert_eq!(st["last_reply"], "Done.", "{st}");
}

/// A job's process that opens a session through MCP passes on its hold: a
/// session that read `gh`'s output holds external text, its next call
/// waits, the operator lets it run, and the job's `curl` opens a
/// conversation through MCP, which holds that text too, from that session,
/// by way of the job.
#[test]
fn a_jobs_process_that_opens_a_session_through_mcp_passes_on_its_hold() {
    let r = Rig::start();
    let gh = r.path("projects/bin/gh").display().to_string();
    r.asks(
        "read issue 7",
        vec![("proc_run", json!({"argv": [gh, "issue", "view", "7"]}))],
    );
    let port = r.port();
    let o = r.path(OUT);
    let script = format!(
        "u=http://127.0.0.1:{port}/mcp\n\
         curl -sS -D {o}/h -o /dev/null -H 'authorization: Bearer {KEY}' -H 'content-type: application/json' \
         -H 'accept: application/json, text/event-stream' --data '{INITIALIZE}' $u\n\
         m=$(grep -i '^mcp-session-id:' {o}/h | tr -d '\\r' | cut -d' ' -f2)\n\
         curl -sS -o {o}/open.json -H 'authorization: Bearer {KEY}' -H 'content-type: application/json' \
         -H \"mcp-session-id: $m\" \
         --data '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"conversation_open\",\"arguments\":{{}}}}}}' $u\n\
         touch {o}/open.done\n",
        o = o.display()
    );
    // The stand-in numbers each response's calls from 0, so a read comes
    // first: the job's call is `toolu_test_1`, which no earlier turn
    // answered.
    r.asks(
        "open through mcp",
        vec![
            ("fs_list", json!({"path": "."})),
            ("proc_run", json!({"argv": ["sh", "-c", script]})),
        ],
    );
    let s = r.call("session.open", json!({"label": "reader"})).unwrap();
    let reader = s["session_id"].as_str().unwrap().to_string();
    let read = r
        .call(
            "turn.submit",
            json!({"session_id": reader, "input": "read issue 7"}),
        )
        .unwrap();
    assert_eq!(read["stop_reason"], "no_tool_calls", "{read}");
    let held = r.call("health", Value::Null).unwrap()["external_text"].clone();
    assert!(held.to_string().contains(&reader), "{held}");
    let parked = r
        .call(
            "turn.submit",
            json!({"session_id": reader, "input": "open through mcp"}),
        )
        .unwrap();
    let id = parked["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the job waits: {parked}\n{}", r.log()))
        .to_string();
    let answered = r.cli(&["confirm", "--approve", &id, "--no-wait"]);
    assert!(answered.status.success(), "{answered:?}");
    let t0 = Instant::now();
    while !o.join("open.done").exists() {
        let actions = r.call("ledger.tail", json!({"n": 60})).unwrap()["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                format!("{} {}", r["kind"], r["data"])
                    .chars()
                    .take(300)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no open in 30 s: {actions}\n{}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let opened: Value =
        serde_json::from_str(&std::fs::read_to_string(o.join("open.json")).unwrap())
            .unwrap_or_else(|e| panic!("{e}\n{}", r.log()));
    let sid = opened["result"]["structuredContent"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{opened}"))
        .to_string();
    let h = r.call("health", Value::Null).unwrap();
    let it = h["external_text"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["session_id"] == sid.as_str())
        .unwrap_or_else(|| panic!("the MCP session holds nothing: {}", h["external_text"]));
    assert_eq!(it["held"]["via"], "job", "{it}");
    assert_eq!(it["held"]["from_session"], reader.as_str(), "{it}");
}

/// A wrong key gets 401, twice, with one `mcp_server.refused` row for the
/// minute; the right key from a page that is not on this machine gets 403;
/// health counts both.
#[test]
fn a_wrong_key_and_a_foreign_origin_are_refused() {
    let r = Rig::start();
    // A wrong key, twice: 401 each, and one row for the minute.
    for _ in 0..2 {
        let (status, _) = r.raw("authorization: Bearer not-the-key-at-all\r\n", INITIALIZE);
        assert!(status.contains(" 401 "), "{status}");
    }
    // The right key from a page that is not on this machine.
    let (status, body) = r.raw(
        &format!("authorization: Bearer {KEY}\r\norigin: http://lantern.example\r\n"),
        INITIALIZE,
    );
    assert!(status.contains(" 403 "), "{status}: {body}");
    assert!(body.contains("a foreign Origin"), "{body}");
    // Each row is written off the runtime's workers, after the answer.
    let refused: Vec<String> = r.wait("the refusals' rows", || {
        let rows = r.ledger("mcp_server.refused");
        (rows.len() >= 2).then(|| {
            rows.iter()
                .map(|row| row["data"]["why"].as_str().unwrap().to_string())
                .collect()
        })
    });
    assert_eq!(refused, ["key", "origin"]);
    let h = r.call("health", Value::Null).unwrap();
    let m = &h["mcp_server"]["refused"];
    assert_eq!(
        (m["key"].clone(), m["origin"].clone()),
        (json!(2), json!(1)),
        "{m}"
    );
}
