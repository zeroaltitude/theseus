//! The web surface carries the agent guard (theseus-iie8e), with the real
//! `theseusd` and real `/ws` connections. The UI's port is open to every
//! process of the owner's own user, and `Host` and `Origin` are headers any
//! of them can send, so the admit (theseus-3qf) and the origin rule
//! (theseus-70f) together say only "some process of this user claims to be
//! the cockpit's page". What tells an agent's own WebSocket from the
//! cockpit's is the process holding the client's end of the connection, read
//! as the Unix socket's peer is (theseus-5rvss).
//!
//! Each client here is a small relay that holds its own connection: run
//! under a program `[policy] agent_processes` lists, it is an agent's; run
//! as a plain child of this test, it is the owner's, as the browser that
//! holds the cockpit's page is. The test daemon lists its own
//! `example-agent` (`common::safe_note`), never `claude`, so where the tests
//! run decides nothing.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};

/// The agent host the test daemon lists (`common::safe_note`).
const HOST: &str = "example-agent";

/// A WebSocket client that relays one JSON-RPC line at a time: a line in on
/// its stdin, every text frame that comes back out on its stdout. The
/// process that runs it is the one holding the client's end of the
/// connection, which is what the guard reads.
const RELAY: &str = r#"
import base64, os, socket, struct, sys

port = int(sys.argv[1])
s = socket.create_connection(("127.0.0.1", port), 10)
key = base64.b64encode(os.urandom(16)).decode()
s.sendall((
    "GET /ws HTTP/1.1\r\n"
    "Host: 127.0.0.1:%d\r\n"
    "Origin: http://127.0.0.1:%d\r\n"
    "Upgrade: websocket\r\nConnection: Upgrade\r\n"
    "Sec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n" % (port, port, key)
).encode())
buf = b""
while b"\r\n\r\n" not in buf:
    d = s.recv(4096)
    if not d:
        sys.exit("the upgrade was closed")
    buf += d
head, rest = buf.split(b"\r\n\r\n", 1)
status = head.split(b"\r\n")[0].decode()
if " 101 " not in status + " ":
    sys.exit("the upgrade was refused: " + status)

def send(text):
    b = text.encode()
    mask = os.urandom(4)
    n = len(b)
    hdr = bytes([0x81, 0x80 | n]) if n < 126 else bytes([0x81, 0xFE]) + struct.pack(">H", n)
    s.sendall(hdr + mask + bytes(c ^ mask[i % 4] for i, c in enumerate(b)))

def take(n):
    global rest
    while len(rest) < n:
        d = s.recv(65536)
        if not d:
            raise EOFError
        rest += d
    out, rest = rest[:n], rest[n:]
    return out

def text():
    while True:
        h = take(2)
        op, n = h[0] & 0x0F, h[1] & 0x7F
        if n == 126:
            n = struct.unpack(">H", take(2))[0]
        elif n == 127:
            n = struct.unpack(">Q", take(8))[0]
        body = take(n) if n else b""
        if op == 8:
            return None
        if op == 1:
            return body.decode()

for line in iter(sys.stdin.readline, ""):
    line = line.strip()
    if not line:
        continue
    send(line)
    while True:
        m = text()
        if m is None:
            sys.exit(0)
        sys.stdout.write(m + "\n")
        sys.stdout.flush()
        if '"id"' in m:
            break
"#;

/// One `/ws` connection, held by the process that opened it.
struct Ws {
    child: Child,
    to: ChildStdin,
    from: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Ws {
    /// A connection whose holder is a plain child of this test, as the
    /// owner's browser is a plain process of their own session; with `under`
    /// set, a child of that program, as an agent's client is.
    fn open(script: &std::path::Path, port: u16, under: Option<&str>) -> Self {
        let py = format!("python3 {} {port}", script.display());
        let mut c = match under {
            Some(host) => {
                let mut c = Command::new(host);
                c.args(["-c", &py]);
                c
            }
            None => {
                let mut c = Command::new("python3");
                c.args([script.display().to_string(), port.to_string()]);
                c
            }
        };
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("python3 runs the relay");
        let to = child.stdin.take().unwrap();
        let from = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            to,
            from,
            next: 1,
        }
    }

    /// One request over this connection: its result, or its error.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        self.next += 1;
        let id = self.next;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.to, "{req}").unwrap();
        self.to.flush().unwrap();
        loop {
            let mut line = String::new();
            let n = self.from.read_line(&mut line).unwrap_or(0);
            assert!(n > 0, "the relay closed before answering {method}");
            let m: Value = serde_json::from_str(line.trim()).unwrap();
            if m["id"] == json!(id) {
                return match (m.get("result"), m.get("error")) {
                    (Some(r), _) => Ok(r.clone()),
                    (None, Some(e)) => Err(e.clone()),
                    _ => panic!("neither a result nor an error: {m}"),
                };
            }
        }
    }
}

impl Drop for Ws {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Rig {
    s: Served,
    port: u16,
    script: PathBuf,
    host: String,
    _model: FakeModel,
    _keys: tempfile::TempDir,
}

impl Rig {
    /// A daemon serving its web UI on a free port, whose spend limit is
    /// `limit_usd`: a tiny one makes the first model call a budget question.
    fn start(limit_usd: f64) -> Self {
        let model = FakeModel::start(|_| vec![]);
        let base = model.base.clone();
        // Every secret is a file holding a stand-in value: no vault to ask.
        let keys = tempfile::tempdir().unwrap();
        let key = keys.path().join("key");
        std::fs::write(&key, "test-secret-value-0000").unwrap();
        std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let s = Served::start(
            |_| {},
            move |t| {
                for (_, v) in table(t, "secrets").iter_mut() {
                    *v = format!("file:{}", key.display()).into();
                }
                table(t, "model").insert("api_base".into(), base.clone().into());
                for (_, p) in table(t, "providers").iter_mut() {
                    p.as_table_mut()
                        .unwrap()
                        .insert("api_base".into(), base.clone().into());
                }
                let w = table(t, "web");
                w.insert("enabled".into(), true.into());
                w.insert("bind".into(), "127.0.0.1".into());
                w.insert("port".into(), i64::from(port).into());
                let kernel = table(t, "kernel");
                kernel.insert("spend_limit_usd".into(), limit_usd.into());
                kernel.insert("spend_limit_mode".into(), "ask".into());
            },
        );
        let script = s.path("relay.py");
        std::fs::write(&script, RELAY).unwrap();
        // A program named for an agent host: a copy of `sh`.
        std::fs::create_dir_all(s.path("bin")).unwrap();
        let host = s.path(&format!("bin/{HOST}"));
        std::fs::copy("/bin/sh", &host).unwrap();
        // The UI answers before the test's first connection.
        let ui = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let t0 = std::time::Instant::now();
        while std::net::TcpStream::connect_timeout(&ui, std::time::Duration::from_secs(1)).is_err()
        {
            assert!(
                t0.elapsed() < std::time::Duration::from_secs(30),
                "no web UI"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Self {
            s,
            port,
            script,
            host: host.display().to_string(),
            _model: model,
            _keys: keys,
        }
    }

    fn agent(&self) -> Ws {
        Ws::open(&self.script, self.port, Some(&self.host))
    }

    fn owner(&self) -> Ws {
        Ws::open(&self.script, self.port, None)
    }

    /// A turn in a new session, over the socket: its result.
    fn turn(&self, prompt: &str) -> Value {
        let s = self
            .s
            .call("session.open", json!({"label": prompt}))
            .unwrap();
        self.s
            .call(
                "turn.submit",
                json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
            )
            .unwrap()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.s.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }
}

/// The red one. A budget question answered over `/ws` by a client an agent
/// holds is refused with the words, ledgered as the agent's with `via` web,
/// and the question stays open; the same answer from the owner's own client
/// resets the spend, and is the owner's.
#[test]
fn a_web_answer_from_an_agents_process_is_refused_for_a_spend_limit_and_the_owners_resets_it() {
    let r = Rig::start(0.001);
    let res = r.turn("hello");
    assert_eq!(res["stop_reason"], "budget", "{res}\n{}", r.s.log());
    let q = res["awaiting_confirm"]
        .as_str()
        .expect("the question")
        .to_string();

    let mut agent = r.agent();
    // `confirm.list` draws no button for it either.
    let list = agent.call("confirm.list", json!({})).unwrap();
    let asked = list["confirms"].as_array().unwrap();
    assert_eq!(asked.len(), 1, "{list}");
    assert_eq!(asked[0]["may_answer"], json!(false), "{list}");

    let e = agent
        .call(
            "action.confirm",
            json!({"correlation_id": q, "approve": true, "option": "reset"}),
        )
        .expect_err("an agent's web client answers no budget question");
    assert!(
        e["message"]
            .as_str()
            .unwrap()
            .contains("this spends money or changes AWS, so only the owner answers it"),
        "{e}"
    );
    let refused = r.ledger("approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}\n{}", r.s.log());
    assert_eq!(refused[0]["by"], format!("agent:{HOST}"));
    assert_eq!(refused[0]["via"], "web");
    assert!(r.ledger("budget.reset").is_empty());

    // The owner's own browser answers it.
    let mut owner = r.owner();
    let list = owner.call("confirm.list", json!({})).unwrap();
    assert_eq!(
        list["confirms"][0]["may_answer"],
        json!(true),
        "{list}\n{}",
        r.s.log()
    );
    owner
        .call(
            "action.confirm",
            json!({"correlation_id": q, "approve": true, "option": "reset"}),
        )
        .expect("the owner's own web client");
    let reset = r.ledger("budget.reset");
    assert_eq!(reset.len(), 1, "{reset:?}\n{}", r.s.log());
    assert_eq!(reset[0]["by"], "the web UI");
    assert_eq!(r.ledger("approval.refused").len(), 1);
}
