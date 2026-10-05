//! Files people give the model (theseus-c9l6), with the real `theseusd`:
//! - its converter role runs a PDF's conversion in a capped child, and a
//!   child that passes its time or its memory is stopped, with words that
//!   name the cap it met;
//! - a PDF attached to a message reaches a model that reads PDFs as a
//!   `document` block of its bytes, and a model that reads none (GLM) as its
//!   text by page, and its `file.read` row says it ran in the capped child.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_files::convert::{self, Limits};
use theseus_files::pdf;

fn theseusd() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_theseusd"))
}

/// Conversions from now on in this test's process run in the real binary's
/// role, under `limits`. Each test runs in a process of its own.
fn in_child(limits: Limits) {
    fn spawn(c: &mut Command) -> std::io::Result<std::process::Child> {
        c.spawn()
    }
    convert::use_child(theseusd(), spawn, limits);
    assert!(convert::in_child());
}

fn text_of(bytes: &[u8]) -> (Result<pdf::Read, String>, convert::Ran) {
    convert::pdf(
        bytes,
        &pdf::Ask {
            text: true,
            ..pdf::Ask::default()
        },
    )
}

#[test]
fn a_pdf_is_read_in_the_capped_child() {
    in_child(Limits::DEFAULT);
    let (r, ran) = text_of(&pdf::sample(&["The pilot boards at the outer mark.", ""]));
    let r = r.unwrap();
    assert_eq!(
        (r.pages, r.texts),
        (
            2,
            vec![
                "The pilot boards at the outer mark.".to_string(),
                String::new()
            ]
        )
    );
    assert!(ran.capped, "it ran in the child");
    // A part comes back across the pipe whole.
    let (part, _) = convert::pdf(
        &pdf::sample(&["one", "two", "three"]),
        &pdf::Ask {
            pages: Some(pdf::Pages::new(2, 3)),
            part: true,
            text: false,
        },
    );
    let part = part.unwrap().part.expect("a part");
    assert_eq!(pdf::read(&part, &pdf::Ask::default()).unwrap().pages, 2);
    // What is not a PDF says so, from the child.
    assert_eq!(
        text_of(b"PK\x03\x04 a zip").0.unwrap_err(),
        "it is not a PDF (no %PDF- header)"
    );
}

/// A child that passes its memory cap dies, and its words say which cap it
/// met. The debug build's probe allocates past it before it converts.
#[test]
fn a_conversion_past_its_memory_is_stopped_and_says_so() {
    in_child(Limits {
        memory_bytes: 768 * 1024 * 1024,
        ..Limits::DEFAULT
    });
    std::env::set_var(convert::PROBE_ENV, "alloc:1073741824");
    let t0 = Instant::now();
    let (r, ran) = text_of(&pdf::sample(&["one"]));
    assert_eq!(
        r.unwrap_err(),
        "its conversion ran out of its 768 MiB of memory and was stopped"
    );
    assert!(ran.capped && t0.elapsed() < Duration::from_secs(10));
}

/// A child that runs past its time is killed at it, and says so.
#[test]
fn a_conversion_past_its_time_is_killed_and_says_so() {
    in_child(Limits {
        timeout: Duration::from_millis(1500),
        ..Limits::DEFAULT
    });
    std::env::set_var(convert::PROBE_ENV, "sleep:20000");
    let t0 = Instant::now();
    let (r, _) = text_of(&pdf::sample(&["one"]));
    assert_eq!(
        r.unwrap_err(),
        "its conversion took longer than 1.5 s and was stopped"
    );
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_millis(1500) && took < Duration::from_secs(5),
        "{took:?}"
    );
}

/// A fake `op`: every reference reads as the same test value.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
    \x20 read) printf '%s' test-secret-value-0000 ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    model: FakeModel,
}

impl Rig {
    fn start() -> Self {
        let model = FakeModel::start(|_| vec![]);
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        for d in ["bin", "projects"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let op = path("bin/op");
        std::fs::write(&op, FAKE_OP).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd(), &path("projects"), 100.0)
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
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(theseusd())
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
        let t0 = Instant::now();
        while r
            .call("health", Value::Null)
            .ok()
            .is_none_or(|h| h["secrets"]["state"] != "ready")
        {
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no secrets in 30 s (daemon pid {}):\n{}",
                r.daemon.id(),
                std::fs::read_to_string(r.dir.path().join("theseusd.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        r
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s =
            UnixStream::connect(self.dir.path().join("sock")).map_err(|e| json!(e.to_string()))?;
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

    /// A turn in a new session on `profile`, with `file` attached.
    fn turn_with(&self, profile: &str, name: &str, file: &[u8]) -> Value {
        let s = self
            .call("session.open", json!({"label": profile}))
            .unwrap();
        let data = theseus_core::blobs::encode(file);
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": "Who is the harbour master?",
                   "profile": profile, "author": "test",
                   "attachments": [{"name": name, "media_type": "application/pdf",
                                    "size": file.len(), "data": data}]}),
        )
        .unwrap()
    }
}

/// The blocks of the last user message of the last request the model got.
fn last_user_blocks(model: &FakeModel) -> Vec<Value> {
    let reqs = model.requests();
    let last = reqs.last().expect("a request");
    let user = last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap();
    user["content"].as_array().unwrap().clone()
}

#[test]
fn an_attached_pdf_reaches_each_model_as_it_reads_pdfs() {
    let r = Rig::start();
    let file = pdf::sample(&["The harbour master is Odile Varnack.", "Buoy B-2 is green."]);
    // Sonnet reads PDFs: a document block of the file's own bytes, after its
    // line.
    r.turn_with("sonnet", "orders.pdf", &file);
    let blocks = last_user_blocks(&r.model);
    let doc = blocks
        .iter()
        .find(|b| b["type"] == "document")
        .unwrap_or_else(|| panic!("no document block: {blocks:?}"));
    assert_eq!(doc["source"]["media_type"], "application/pdf");
    assert_eq!(
        theseus_core::blobs::decode(doc["source"]["data"].as_str().unwrap()).unwrap(),
        file
    );
    assert!(blocks[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("[PDF orders.pdf from test, "));
    // GLM reads none: the text, page by page.
    r.turn_with("glm", "orders.pdf", &file);
    let blocks = last_user_blocks(&r.model);
    assert!(blocks.iter().all(|b| b["type"] != "document"), "{blocks:?}");
    let text = blocks[0]["text"].as_str().unwrap();
    assert!(
        text.contains("since this model reads no PDFs")
            && text.contains("--- page 1 ---\nThe harbour master is Odile Varnack.")
            && text.contains("--- page 2 ---\nBuoy B-2 is green."),
        "{text}"
    );
    // Each file read is a row, and the daemon read it in the capped child.
    let rows = r
        .call("ledger.tail", json!({"kind": "file.read", "n": 10}))
        .unwrap();
    let rows = rows["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        let d = &row["data"];
        assert_eq!(
            (&d["via"], &d["outcome"], &d["pages"], &d["capped"]),
            (
                &json!("attachment"),
                &json!("read"),
                &json!(2),
                &json!(true)
            ),
            "{row}"
        );
    }
}
