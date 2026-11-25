//! One JSON-RPC call on a daemon's socket, in the two shapes the tests read:
//! the error as the response's `error` value, and the error as text.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

/// The call, answered within 60 s; an error is the response's `error` value.
pub fn call(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    call_within(sock, 60, method, params)
}

/// `call`, with `secs` to read the answer.
pub fn call_within(sock: &Path, secs: u64, method: &str, params: Value) -> Result<Value, Value> {
    let s = UnixStream::connect(sock).map_err(|e| json!(e.to_string()))?;
    s.set_read_timeout(Some(Duration::from_secs(secs))).unwrap();
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

/// `call`, with an error as text.
pub fn call_text(sock: &Path, method: &str, params: Value) -> Result<Value, String> {
    let s = UnixStream::connect(sock).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    for line in BufReader::new(&s).lines() {
        let v: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if v["id"] == 1 {
            return match v.get("error") {
                Some(e) if !e.is_null() => Err(e.to_string()),
                _ => Ok(v["result"].clone()),
            };
        }
    }
    Err("the connection closed".into())
}

/// The one request `call_at_a_terminal`'s process sends, printing its answer on one line.
const AT_A_TERMINAL: &str = "import json, socket, sys\n\
    s = socket.socket(socket.AF_UNIX)\n\
    s.connect(sys.argv[1])\n\
    req = {'jsonrpc': '2.0', 'id': 1, 'method': sys.argv[2], 'params': json.loads(sys.argv[3])}\n\
    s.sendall((json.dumps(req) + '\\n').encode())\n\
    for line in s.makefile('rb'):\n\
    \x20   v = json.loads(line)\n\
    \x20   if v.get('id') == 1:\n\
    \x20       print(json.dumps(v))\n\
    \x20       break\n";

/// `call`, sent by a process at a terminal of its own, as the owner's shell is: the daemon reads
/// a CLI peer with none as an agent's (theseus-5rvss), and an agent's turn is not the owner's own
/// message, so capture hears nothing of it (issue 57).
pub fn call_at_a_terminal(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    let python = std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("python3"))
                .find(|f| f.is_file())
        })
        .expect("python3 on the PATH");
    let argv: Vec<String> = vec![
        python.display().to_string(),
        "-I".into(),
        "-c".into(),
        AT_A_TERMINAL.into(),
        sock.display().to_string(),
        method.into(),
        params.to_string(),
    ];
    let (code, out) = super::terminal::on_a_terminal(&argv, &[]);
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with('{'))
        .ok_or_else(|| json!(format!("no answer (exit {code}): {out}")))?;
    let v: Value = serde_json::from_str(line).map_err(|e| json!(e.to_string()))?;
    match v.get("error") {
        Some(e) if !e.is_null() => Err(e.clone()),
        _ => Ok(v["result"].clone()),
    }
}
