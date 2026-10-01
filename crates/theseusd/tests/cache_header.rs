//! The cached header is one header (13b, theseus-ev1). Anthropic caches a
//! request's prefix in the order tools, system, messages, so the sessions of
//! a profile and their tasks share one cache entry for the tools and the
//! system blocks only while those bytes are the same. For each profile of
//! the template, with the real `theseusd`, a context file at each level (the
//! system's and the persona's), and a profile `system`:
//! - the first requests of two sessions carry byte-identical tools and
//!   system, and differ in nothing but their messages;
//! - so do the first requests of a session and of the task it starts.
//!
//! It guards against a change that puts a session's own byte (a date, an id)
//! into the header. The stand-in for the Messages API keeps each request's
//! body as the bytes that came. With `THESEUS_HEADER_DUMP=<dir>`, the test
//! also writes those first requests there (the Z.ai probe replays one).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`: every reference reads as the same test value.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
    \x20 read) printf '%s' test-secret-value-0000 ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

/// The first session's prompt, which the stand-in answers with a task.
const START: &str = "Start the task";
/// The second session's prompt.
const OTHER: &str = "A second session";
/// The task's brief.
const BRIEF: &str = "Say one word.";

/// The context files' text, so the test can find them in the system block.
const SYSTEM_FILE: &str = "The operator works from the lab in Lisbon.";
const PERSONA_FILE: &str = "You answer as the harness's quiet engineer.";
/// The profiles' own `system`.
const PROFILE_SYSTEM: &str = "Keep each answer short.";

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    model: FakeModel,
}

impl Rig {
    fn start() -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.starts_with(START) {
                vec![("task_create", json!({"brief": BRIEF}))]
            } else {
                vec![]
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "context"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let op = path("bin/op");
        std::fs::write(&op, FAKE_OP).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(path("context/system.md"), SYSTEM_FILE).unwrap();
        std::fs::write(path("context/persona.md"), PERSONA_FILE).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
            t.entry(key)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
        }
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        for (_, p) in table(&mut t, "profiles").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("system".into(), PROFILE_SYSTEM.into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        let file = |p: &str| toml::Value::Array(vec![path(p).display().to_string().into()]);
        let context = table(&mut t, "context");
        context.insert("files".into(), file("context/system.md"));
        context.insert("default_persona".into(), "theseus".into());
        table(table(&mut t, "personas"), "theseus")
            .insert("files".into(), file("context/persona.md"));
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("state"))
                .arg("--socket")
                .arg(path("sock"))
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
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let r = Self { dir, daemon, model };
        r.wait("the secrets", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] == "ready")
        });
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s (daemon pid {}):\n{}",
                self.daemon.id(),
                self.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| json!(e.to_string()))?;
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

    /// A turn in a new session, on `profile`: its result, once it ends.
    fn turn(&self, profile: &str, label: &str, prompt: &str) -> Value {
        let s = self.call("session.open", json!({"label": label})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "profile": profile,
                   "author": "test", "attachments": []}),
        )
        .unwrap()
    }
}

/// The first user message's text.
fn first_text(req: &Value) -> String {
    match &req["messages"][0]["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A body as the daemon sent it, parsed. The daemon writes its body with
/// serde_json, compact and keys in order, so a parsed body serializes back to
/// the very bytes that came; that is checked here, so a field's serialization
/// below is that field's bytes on the wire.
fn parsed(raw: &[u8]) -> Value {
    let v: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(
        serde_json::to_vec(&v).unwrap(),
        raw,
        "the body is not serde_json's compact form, so a field's serialization is not its wire bytes"
    );
    v
}

fn bytes(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

/// Everything but the messages.
fn header_fields(req: &Value) -> Value {
    let mut m = req.as_object().unwrap().clone();
    m.remove("messages");
    Value::Object(m)
}

fn the_first_requests_share_one_header(profile: &str, provider: &str) {
    let r = Rig::start();
    r.turn(profile, "first", START);
    r.turn(profile, "second", OTHER);
    let task = r.wait("the task complete", || {
        r.call("task.list", json!({})).ok()?["tasks"]
            .as_array()?
            .iter()
            .find(|t| t["state"] == "complete")
            .cloned()
    });
    let raw = r.model.raw_requests();
    let first = |what: &str, pick: &dyn Fn(&str) -> bool| -> (Vec<u8>, Value) {
        raw.iter()
            .map(|b| (b.clone(), parsed(b)))
            .find(|(_, v)| pick(&first_text(v)))
            .unwrap_or_else(|| panic!("no first request of {what} among {}", raw.len()))
    };
    let (a_raw, a) = first("the first session", &|t| t.starts_with(START));
    let (b_raw, b) = first("the second session", &|t| t.starts_with(OTHER));
    let (t_raw, t) = first("the task", &|t| t.contains(BRIEF));
    assert_ne!(a_raw, b_raw);
    assert_ne!(a_raw, t_raw);

    // The header is all there: the persona, both context files, the
    // profile's system, and the tools.
    let system = bytes(&a["system"]);
    for part in [SYSTEM_FILE, PERSONA_FILE, PROFILE_SYSTEM, "You are Theseus"] {
        assert!(system.contains(part), "{part:?} is not in {system}");
    }
    let tools = a["tools"].as_array().map_or(0, Vec::len);
    assert!(tools > 0, "no tools in {}", bytes(&a));
    assert!(
        a["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "task_create"),
        "no task_create in the tools"
    );

    for (what, other) in [("the second session", &b), ("the task", &t)] {
        assert_eq!(
            a["model"], other["model"],
            "{profile}: the model, against {what}"
        );
        assert!(
            bytes(&a["tools"]) == bytes(&other["tools"]),
            "{profile}: the tools differ between the first session and {what}"
        );
        assert!(
            bytes(&a["system"]) == bytes(&other["system"]),
            "{profile}: the system blocks differ between the first session and {what}:\n{}\n{}",
            bytes(&a["system"]),
            bytes(&other["system"])
        );
        assert_eq!(
            header_fields(&a),
            header_fields(other),
            "{profile}: a field other than the messages differs from {what}"
        );
    }
    assert_eq!(task["state"], "complete");

    eprintln!(
        "{profile} ({provider}): model {}; tools {} bytes ({tools} tools); system {} bytes ({} block); \
         first requests {}, {}, and {} bytes",
        a["model"],
        bytes(&a["tools"]).len(),
        system.len(),
        a["system"].as_array().map_or(0, Vec::len),
        a_raw.len(),
        b_raw.len(),
        t_raw.len()
    );
    if let Some(dir) = std::env::var_os("THESEUS_HEADER_DUMP") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        for (name, body) in [("first", &a_raw), ("second", &b_raw), ("task", &t_raw)] {
            std::fs::write(dir.join(format!("{profile}-{name}.json")), body).unwrap();
        }
    }
}

#[test]
fn the_sonnet_profiles_sessions_and_task_share_one_header() {
    the_first_requests_share_one_header("sonnet", "anthropic");
}

#[test]
fn the_glm_profiles_sessions_and_task_share_one_header() {
    the_first_requests_share_one_header("glm", "zai");
}
