//! The cached header is one header (13b, theseus-ev1). Anthropic caches a
//! request's prefix in the order tools, system, messages, so the sessions of
//! a profile and their tasks share one cache entry for the tools and the
//! system blocks only while those bytes are the same. For each profile of
//! the template, with the real `theseusd`, a context file at each level (the
//! system's and the persona's), and a profile `system`:
//! - the first requests of two sessions carry byte-identical tools and
//!   system, and differ in nothing but their messages;
//! - so do the first requests of a session and of the task it starts;
//! - the system is three blocks (13c; theseus-aab7): the header (the
//!   persona, the tools note, the profile's `system`), the context files,
//!   and the session's directory, each with its breakpoint, and the
//!   conversation's is in the messages, on the block before the harness's
//!   tail blocks: the date and the git state ride the tail of every request
//!   (theseus-wadu), and a task's view already did (theseus-aab7), so the
//!   top-level breakpoint is moved onto the last block the conversation owns
//!   and the provider's four still hold;
//! - after an edit to a context file, a new session's header block is the
//!   same bytes, and only its context block changed.
//!
//! It guards against a change that puts a session's own byte (a date, an id)
//! into the header. The stand-in for the Messages API keeps each request's
//! body as the bytes that came. With `THESEUS_HEADER_DUMP=<dir>`, the test
//! also writes those first requests there (the Z.ai probe replays one).

mod common;

use common::{table, Fixture, FAKE_OP_FLAT};

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// The first session's prompt, which the stand-in answers with a task.
const START: &str = "Start the task in the background";
/// The second session's prompt.
const OTHER: &str = "A second session";
/// The third session's prompt, after the persona's context file is edited.
const EDITED: &str = "A third session, after an edit";
/// The line the edit adds to the persona's context file.
const EDIT: &str = "Keep a log of each change.";
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

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    /// The daemon, once its secrets are ready and its start step has
    /// accepted the context files.
    fn start() -> Self {
        let r = Self::spawned(&[]);
        r.wait("the secrets", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] == "ready")
        });
        // The start step accepts the context files once
        // (`Core::held_first_start`); a request compiled before it carries
        // none of their text.
        r.wait("the context files accepted", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h.get("held_files").is_none())
        });
        r
    }

    /// The daemon spawned with `env` added, waiting for nothing.
    fn spawned(env: &[(&str, &str)]) -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.starts_with(START) {
                vec![(
                    "task_create",
                    json!({"brief": BRIEF, "arrangement":
                           {"pieces": [{"quote": START, "role": "objective"}]}}),
                )]
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
        std::fs::write(&op, FAKE_OP_FLAT).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(path("context/system.md"), SYSTEM_FILE).unwrap();
        std::fs::write(path("context/persona.md"), PERSONA_FILE).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
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
        // `zai` is only the second provider here (theseus-a1wvq.3).
        common::unrestricted_glm(&mut t);
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        let file = |p: &str| toml::Value::Array(vec![path(p).display().to_string().into()]);
        let context = table(&mut t, "context");
        context.insert("files".into(), file("context/system.md"));
        context.insert("default_persona".into(), "theseus".into());
        table(table(&mut t, "personas"), "theseus")
            .insert("files".into(), file("context/persona.md"));
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let mut cmd = Command::new(&theseusd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let daemon = Daemon::spawn(
            cmd.arg("--config")
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
        Self { dir, daemon, model }
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

/// Everything but the messages; the system blocks without their breakpoints.
fn header_fields(req: &Value) -> Value {
    let mut m = req.as_object().unwrap().clone();
    m.remove("messages");
    if let Some(system) = m.get_mut("system") {
        *system = unmarked(system);
    }
    Value::Object(m)
}

/// The system blocks without their `cache_control`: a session whose first request lays the place's earlier
/// exchanges (issue 44) spends one of the four breakpoints on them, so its system blocks carry one fewer.
/// The blocks' bytes are the same; where the breakpoints fall is the request's own.
fn unmarked(system: &Value) -> Value {
    let mut blocks = system.clone();
    for b in blocks.as_array_mut().into_iter().flatten() {
        if let Some(o) = b.as_object_mut() {
            o.remove("cache_control");
        }
    }
    blocks
}

/// Everything but the messages and the conversation's breakpoint, which a
/// task's request carries in its messages, on the block before its task
/// view (moved, so the four breakpoints hold: theseus-aab7). Every request
/// carries it that way since theseus-wadu's tail block, so this now differs
/// from [`header_fields`] only for a request with no tail block at all
/// (`[project] tail_facts = false`).
fn header_fields_of_a_task(req: &Value) -> Value {
    let mut m = header_fields(req);
    m.as_object_mut().unwrap().remove("cache_control");
    m
}

/// How a session's first request is picked: which user message is read, and what it must say.
type Pick<'a> = (fn(&Value) -> String, &'a dyn Fn(&str) -> bool);

/// The first request among `raw` whose `text` (its first or its last user message) `pick` takes.
fn first_request(raw: &[Vec<u8>], what: &str, (text, pick): Pick<'_>) -> (Vec<u8>, Value) {
    raw.iter()
        .map(|b| (b.clone(), parsed(b)))
        .find(|(_, v)| pick(&text(v)))
        .unwrap_or_else(|| panic!("no first request of {what} among {}", raw.len()))
}

/// The last user message's text: a session's own message. The view lays the place's earlier exchanges, of
/// another session too, before it (issue 44), so a session's first request is known by its last message.
fn last_text(req: &Value) -> String {
    let last = (req["messages"].as_array().into_iter().flatten()).rfind(|m| m["role"] == "user");
    match last.map(|m| &m["content"]) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn text(block: &Value) -> &str {
    block["text"].as_str().unwrap_or_default()
}

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
    let first = |what: &str, pick: Pick<'_>| first_request(&raw, what, pick);
    let (a_raw, a) = first("the first session", (last_text, &|t| t.contains(START)));
    let (b_raw, b) = first("the second session", (last_text, &|t| t.contains(OTHER)));
    let (t_raw, t) = first("the task", (first_text, &|t| t.contains(BRIEF)));
    assert_ne!(a_raw, b_raw);
    assert_ne!(a_raw, t_raw);

    // The header is all there: the persona, both context files, the
    // profile's system, and the tools.
    let system = bytes(&a["system"]);
    for part in [SYSTEM_FILE, PERSONA_FILE, PROFILE_SYSTEM] {
        assert!(system.contains(part), "{part:?} is not in {system}");
    }
    // The owner's persona file replaces the built-in default whole
    // (theseus-slvl.2): none of it is in the request.
    assert!(
        !system.contains("You are Theseus"),
        "the default is merged in: {system}"
    );
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
            bytes(&unmarked(&a["system"])) == bytes(&unmarked(&other["system"])),
            "{profile}: the system blocks differ between the first session and {what}:\n{}\n{}",
            bytes(&a["system"]),
            bytes(&other["system"])
        );
        let (mine, theirs) = match what {
            "the task" => (header_fields_of_a_task(&a), header_fields_of_a_task(other)),
            _ => (header_fields(&a), header_fields(other)),
        };
        assert_eq!(
            mine, theirs,
            "{profile}: a field other than the messages differs from {what}"
        );
    }
    assert!(
        bytes(&t["messages"]).contains("cache_control"),
        "{profile}: the task's conversation breakpoint is in its messages"
    );
    assert_eq!(task["state"], "complete");

    // Four blocks (theseus-h4a7): the header, the persona's files, the
    // project layer (the system level's files), then the session's
    // directory; the first three carry a breakpoint, the directory none, and
    // the conversation's is in the messages (below).
    let blocks = a["system"].as_array().unwrap();
    assert_eq!(blocks.len(), 4, "{profile}: {}", bytes(&a["system"]));
    let (header, persona, context) = (&blocks[0], &blocks[1], &blocks[2]);
    assert!(
        text(&blocks[3]).starts_with("Directory: "),
        "{profile}: block 4"
    );
    // The owner's persona file replaced the built-in default (theseus-slvl.2),
    // so the header opens with the assembly note.
    assert!(
        !text(header).contains("You are Theseus"),
        "{profile}: the default is merged in"
    );
    for part in ["The harness assembles each request", PROFILE_SYSTEM] {
        assert!(
            text(header).contains(part),
            "{part:?} is not in the header block"
        );
        assert!(
            !text(context).contains(part),
            "{part:?} is in the context block"
        );
    }
    for (part, layer) in [(SYSTEM_FILE, context), (PERSONA_FILE, persona)] {
        assert!(text(layer).contains(part), "{part:?} is not in its layer");
        assert!(
            !text(header).contains(part),
            "{part:?} is in the header block"
        );
    }
    assert!(
        !text(context).contains(PERSONA_FILE) && !text(persona).contains(SYSTEM_FILE),
        "each file is in its own layer alone"
    );
    let five = json!({"type": "ephemeral"});
    for (i, b) in blocks.iter().enumerate().take(3) {
        assert_eq!(b["cache_control"], five, "{profile}: system block {i}");
    }
    assert!(
        blocks[3].get("cache_control").is_none(),
        "{profile}: the directory block carries no breakpoint"
    );
    // The conversation's breakpoint is the request's own, at the top level. A tail block once rode the
    // last message (the date and the git state, theseus-wadu) and `attach_text` moved this breakpoint onto
    // the block before it; what the harness says is now one block beside the message, frozen for the turn
    // (issue 46), so nothing is laid after the messages and the breakpoint stays where the compile put it.
    // The provider's four still hold (theseus-aab7).
    assert_eq!(
        a["cache_control"],
        serde_json::json!({"type": "ephemeral"}),
        "{profile}: the top-level breakpoint is the request's own"
    );
    let said = a["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_array())
        .unwrap();
    let marked: Vec<usize> = said
        .iter()
        .enumerate()
        .filter(|(_, b)| b.get("cache_control").is_some())
        .map(|(i, _)| i)
        .collect();
    assert!(
        marked.is_empty(),
        "{profile}: no breakpoint in the turn's own message, the request's own is top-level: {}",
        bytes(&json!(said))
    );
    assert!(
        said.first()
            .is_some_and(|b| theseus_core::task_graph::view::is_head(text(b))),
        "{profile}: the harness's block opens the turn's message: {}",
        bytes(&json!(said))
    );
    // The header block alone, byte for byte, in the second session and the task.
    for (what, other) in [("the second session", &b), ("the task", &t)] {
        assert!(
            bytes(header) == bytes(&other["system"][0]),
            "{profile}: the header block differs between the first session and {what}"
        );
    }

    // An edit to the persona's context file: a new session's header block is
    // the same bytes, so it and the tools still read from the cache, and only
    // the context block changed.
    std::fs::write(
        r.path("context/persona.md"),
        format!("{PERSONA_FILE}\n{EDIT}"),
    )
    .unwrap();
    r.turn(profile, "third", EDITED);
    let raw = r.model.raw_requests();
    let (_, e) = first_request(
        &raw,
        "the session after the edit",
        (last_text, &|t| t.contains(EDITED)),
    );
    let edited = e["system"].as_array().unwrap();
    assert_eq!(edited.len(), 4, "{profile}: {}", bytes(&e["system"]));
    assert!(
        bytes(&edited[0]) == bytes(header),
        "{profile}: the header block changed with a context file:\n{}\n{}",
        bytes(header),
        bytes(&edited[0])
    );
    assert!(
        bytes(&e["tools"]) == bytes(&a["tools"]),
        "{profile}: the tools changed"
    );
    assert_ne!(
        edited[1], *persona,
        "{profile}: the persona layer did not change"
    );
    assert!(
        bytes(&edited[2]) == bytes(context),
        "{profile}: the project layer changed with a persona file"
    );
    assert!(
        text(&edited[1]).contains(EDIT),
        "{profile}: the edit is not in the block"
    );
    // The edit's session is a later session at the same place: its first request lays the place's earlier
    // exchanges, whose cache mark takes the breakpoint the persona block had (issue 44).
    assert!(
        edited[1]["cache_control"] == five
            || (edited[1]["cache_control"].is_null()
                && bytes(&e["messages"]).contains("cache_control")),
        "{profile}: the edited persona block's breakpoint, or the window's that took it"
    );

    eprintln!(
        "{profile} ({provider}): model {}; tools {} bytes ({tools} tools); system {} bytes in {} \
         blocks (header {}, persona {}, project {}); first requests {}, {}, and {} bytes",
        a["model"],
        bytes(&a["tools"]).len(),
        system.len(),
        blocks.len(),
        bytes(header).len(),
        bytes(persona).len(),
        bytes(context).len(),
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

/// A turn sent the moment the socket answers, with no wait on health, while
/// the daemon's start step that accepts the context files is held back (a
/// debug build's `THESEUS_TEST_SLOW_AFTER_SERVING_MS`): the turn waits for
/// that step, so its request to the model carries the system level's file.
#[test]
fn a_turn_sent_before_the_context_files_are_accepted_waits_for_them() {
    let r = Rig::spawned(&[("THESEUS_TEST_SLOW_AFTER_SERVING_MS", "2000")]);
    let s = r.wait("the socket", || {
        r.call("session.open", json!({"label": "early"})).ok()
    });
    r.call(
        "turn.submit",
        json!({"session_id": s["session_id"], "input": OTHER, "profile": "sonnet",
               "author": "test", "attachments": []}),
    )
    .unwrap();
    let raw = r.model.raw_requests();
    let (_, first) = first_request(&raw, "the early turn", (last_text, &|t| t.contains(OTHER)));
    let system = bytes(&first["system"]);
    assert!(system.contains(SYSTEM_FILE), "{system}");
}
