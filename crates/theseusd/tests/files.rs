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
        Self::start_with(|_| vec![], |_| {})
    }

    /// A daemon whose model calls `calls` gives for a turn's prompt, its
    /// config changed by `tweak`.
    fn start_with(
        calls: impl Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync + 'static,
        tweak: impl FnOnce(&mut toml::Table),
    ) -> Self {
        let model = FakeModel::start(calls);
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
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        tweak(&mut t);
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
        self.submit(
            s["session_id"].as_str().unwrap(),
            profile,
            "Who is the harbour master?",
            Some((name, file)),
        )
    }

    /// A turn in session `sid` on `profile`, with `file` attached when given.
    fn submit(&self, sid: &str, profile: &str, input: &str, file: Option<(&str, &[u8])>) -> Value {
        let attachments: Vec<Value> = file
            .into_iter()
            .map(|(name, bytes)| {
                json!({"name": name, "media_type": "application/octet-stream", "size": bytes.len(),
                       "data": theseus_core::blobs::encode(bytes)})
            })
            .collect();
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": input, "profile": profile, "author": "test",
                   "attachments": attachments}),
        )
        .unwrap()
    }
}

/// The content of the last tool result the model was sent.
fn last_tool_result(model: &FakeModel) -> String {
    let reqs = model.requests();
    let last = reqs.last().expect("a request");
    let blocks = last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_array().cloned())
        .unwrap_or_default();
    let r = blocks
        .iter()
        .find(|b| b["type"] == "tool_result")
        .unwrap_or_else(|| panic!("no tool result: {blocks:?}"));
    match &r["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

/// What the stand-in Deepgram was sent: each request's path, type, key, and
/// length.
type Heard = std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, usize)>>>;

/// A stand-in for Deepgram's pre-recorded endpoint: every request answered
/// with one transcript of a 42-second recording.
fn fake_deepgram(transcript: &'static str) -> (String, Heard) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Heard = Default::default();
    let kept = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let (mut kind, mut key, mut len) = (String::new(), String::new(), 0usize);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() == 0 || h == "\r\n" {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap_or(("", ""));
                match k.to_ascii_lowercase().as_str() {
                    "content-type" => kind = v.trim().into(),
                    "authorization" => key = v.trim().into(),
                    "content-length" => len = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            let mut body = vec![0u8; len];
            std::io::Read::read_exact(&mut r, &mut body).unwrap();
            kept.lock().unwrap().push((path, kind, key, len));
            let answer = json!({"metadata": {"duration": 42.0},
                "results": {"channels": [{"alternatives": [{"transcript": transcript}]}]}})
            .to_string();
            let mut w = stream;
            let _ = write!(
                w,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (base, seen)
}

/// Join 2 (theseus-c9l6): `file.read` reads a document's sections, reads an
/// archive's member out under the working directory, and hears a recording
/// through Deepgram once, booked as a voice call's speech is; the second
/// read of it is the kept transcript, with no new request.
#[test]
fn file_read_reads_sections_a_member_and_a_recording_heard_once() {
    let (base, heard) = fake_deepgram("Moor at the outer mark after six.");
    let r = Rig::start_with(
        // The prompt is the turn's last user text, its files' lines included.
        |prompt| {
            let p = prompt.lines().last().unwrap_or("");
            match p {
                "Read the rules." => {
                    vec![("file_read", json!({"name": "rules.docx", "pages": "1"}))]
                }
                "Read the log." => vec![(
                    "file_read",
                    json!({"name": "logs.zip", "member": "logs/tide.log"}),
                )],
                "Hear the memo." | "Hear it again." => {
                    vec![("file_read", json!({"name": "memo.ogg"}))]
                }
                _ => vec![],
            }
        },
        move |t| {
            fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
                t.entry(key)
                    .or_insert_with(|| toml::Value::Table(Default::default()))
                    .as_table_mut()
                    .unwrap()
            }
            table(t, "voice").insert("api_base".into(), base.into());
            table(t, "secrets").insert(
                "deepgram_api_key".into(),
                "op://Test/deepgram_api_key/credential".into(),
            );
        },
    );
    let s = r.call("session.open", json!({"label": "files"})).unwrap();
    let sid = s["session_id"].as_str().unwrap();
    let docx = theseus_files::doc::sample_zip(&[
        ("[Content_Types].xml", "<Types/>"),
        (
            "word/document.xml",
            r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Harbour rules</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ]);
    r.submit(
        sid,
        "sonnet",
        "Read the rules.",
        Some(("rules.docx", &docx)),
    );
    let got = last_tool_result(&r.model);
    assert!(got.contains("--- text ---\n# Harbour rules"), "{got}");

    let zip = theseus_files::doc::sample_zip(&[("logs/tide.log", "high water 06:12")]);
    r.submit(sid, "sonnet", "Read the log.", Some(("logs.zip", &zip)));
    let got = last_tool_result(&r.model);
    assert!(got.contains("high water 06:12"), "{got}");
    let short: String = sid.chars().skip(sid.chars().count() - 8).collect();
    let saved = r.dir.path().join(format!(
        "projects/.theseus-files/{short}/logs.zip.d/logs/tide.log"
    ));
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "high water 06:12");

    let ogg = b"OggS\x00\x02 a recording, by its first bytes".to_vec();
    r.submit(sid, "sonnet", "Hear the memo.", Some(("memo.ogg", &ogg)));
    let got = last_tool_result(&r.model);
    assert!(
        got.contains("Transcript of memo.ogg (0:42), by Deepgram nova-3")
            && got.contains("Moor at the outer mark"),
        "{got}"
    );
    r.submit(sid, "sonnet", "Hear it again.", None);
    let got = last_tool_result(&r.model);
    assert!(got.contains("heard before; no new charge"), "{got}");
    let seen = heard.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "heard once: {seen:?}");
    let (path, kind, key, len) = &seen[0];
    assert!(path.starts_with("/v1/listen?model=nova-3"), "{path}");
    assert_eq!(
        (kind.as_str(), key.as_str(), *len),
        ("audio/ogg", "Token test-secret-value-0000", ogg.len())
    );
    // The transcript is spend, booked as a voice call's is.
    let rows = r
        .call("ledger.tail", json!({"kind": "speech.transcribed", "n": 5}))
        .unwrap();
    let rows = rows["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (&rows[0]["data"]["tool"], &rows[0]["data"]["seconds"]),
        (&json!("file.read"), &json!(42.0))
    );
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
