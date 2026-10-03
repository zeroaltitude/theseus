//! The place rule through the whole core (theseus-nbsh): a shared place's
//! model is offered only the public tools and the gate refuses any other
//! call; its system block carries only the public context files; a task
//! takes its parent's class; a session whose place moved on answers its wake
//! as that place; and a channel bound private, or a DM with the owner, gets
//! everything.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{PlaceClass, SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::context_files::{ContextEntry, ContextFileEntry, ContextReaders};
use crate::places::BoundPlace;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The owner on Discord, and someone else.
const OWNER: u64 = 271_828_182_845_904_523;
const ALICE: u64 = 222_222_222_222_222_222;
/// A guild channel the bindings file binds.
const LAB: u64 = 314_159_265_358_979_323;

/// What the owner's notes say; no shared place's request may carry it.
const SECRET: &str = "the vault code is 4417";
/// What the public tree's README says.
const OPEN: &str = "the open tide table";

/// A stand-in model that answers each request from the request itself.
struct Model {
    script: Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let f = FakeProvider::scripted(vec![(self.script)(req)]);
            f.stream_message(req, on_delta).await
        })
    }
}

fn text_of(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn first_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .find(|m| m["role"] == "user")
        .map(text_of)
        .unwrap_or_default()
}

fn last_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .map(text_of)
        .unwrap_or_default()
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// The wire names of the tools a request offered.
fn offered(req: &ProviderRequest) -> Vec<String> {
    req.tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

fn system_text(req: &ProviderRequest) -> String {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A call's result, as its session stores it.
fn result_of(core: &Core, sid: &str, id: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult {
                tool_use_id,
                content,
                ..
            } if tool_use_id == id => Some(content),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// Why the gate refused each call it refused, by call.
fn refused(core: &Core) -> Vec<(String, String)> {
    rows(core, "tool.invalid_input")
        .iter()
        .map(|row| {
            let s = |k: &str| row[k].as_str().unwrap_or_default().to_string();
            (s("tool_use_id"), s("reason"))
        })
        .collect()
}

/// What a scripted session does: `look around` makes three calls (a
/// program, the owner's notes, the public README); `start a task` starts
/// one whose brief runs a program; `WAKE` sets a wake for a second from now.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let (first, last) = (first_user(req), last_user(req));
    if first.contains("count the files") {
        return Scripted::tools(
            "Counting.",
            &[("k1", "proc_run", json!({"argv": ["cat", "notes.md"]}))],
        );
    }
    if last.contains("look around") {
        return Scripted::tools(
            "Looking.",
            &[
                ("p1", "proc_run", json!({"argv": ["cat", "notes.md"]})),
                ("p2", "fs_read", json!({"path": "notes.md"})),
                ("p3", "fs_read", json!({"path": "open/README.md"})),
            ],
        );
    }
    if last.contains("start a task") {
        return Scripted::tools(
            "Starting it.",
            &[("t1", "task_create", json!({"brief": "count the files"}))],
        );
    }
    if last.starts_with("WAKE") {
        return Scripted::tools(
            "Setting it.",
            &[(
                "w1",
                "wake_at",
                json!({"after": "1s", "note": "check again"}),
            )],
        );
    }
    Scripted::text("Hello.")
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

/// A core over a work tree with the owner's notes, a public tree, and both
/// as context files (the README marked public); the driver runs, as the
/// daemon's does, so tasks and wakes take their turns.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("open")).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("notes.md"), format!("{SECRET}\n")).unwrap();
    std::fs::write(root.join("open/README.md"), format!("{OPEN}\n")).unwrap();
    let cfg = config(&root, dir.path());
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        script: Box::new(script),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    // Every call runs at once: a call the rule let through would run, not wait.
    cfg.policy.enforcement = Posture::Notify;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.places.public_paths = vec![path(&root.join("open"))];
    cfg.context.files = vec![
        ContextEntry::Path(path(&root.join("notes.md"))),
        ContextEntry::Table(ContextFileEntry {
            path: path(&root.join("open/README.md")),
            readers: ContextReaders::Public,
        }),
    ];
    cfg
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A session, posting to `place` (`channel:<id>`, `dm:<user>`) when given.
fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
        })
        .await
        .unwrap()
}

impl Rig {
    fn requests(&self) -> Vec<ProviderRequest> {
        self.model.requests.lock().unwrap().clone()
    }

    /// The requests whose first user message says `what`.
    fn asked(&self, what: &str) -> Vec<ProviderRequest> {
        self.requests()
            .into_iter()
            .filter(|r| first_user(r).contains(what) || last_user(r).contains(what))
            .collect()
    }

    async fn until(&self, what: &str, secs: u64, f: impl Fn(&Rig) -> bool) {
        let t0 = Instant::now();
        while !f(self) {
            assert!(
                t0.elapsed() < Duration::from_secs(secs),
                "no {what} in {secs} s"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// What a shared place is offered: the public tools, and the file tools
/// for the public tree.
const SHARED_TOOLS: &[&str] = &[
    "fs_edit",
    "fs_glob",
    "fs_grep",
    "fs_list",
    "fs_patch",
    "fs_read",
    "fs_write",
    "git_diff",
    "git_log",
    "http_fetch",
    "task_create",
    "text_diff",
    "wake_at",
    "web_search",
];

/// The brief's first test: in a guild channel nobody bound private, the
/// model is offered only the public tools, and the gate refuses a program
/// and a file outside the public tree, which never run; the public file is
/// read. Its system block carries the public context file alone.
#[tokio::test]
async fn a_shared_place_is_offered_and_allowed_only_the_public_tools() {
    let r = rig();
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    let res = turn(&r.core, &shared, "look around").await;
    assert_eq!(res.loops, 2, "{res:?}");
    let reqs = r.requests();
    assert_eq!(offered(&reqs[0]), SHARED_TOOLS, "the catalog");
    let system = system_text(&reqs[0]);
    assert!(system.contains("This place is shared"), "{system}");
    assert!(system.contains(OPEN), "the public context file: {system}");
    assert!(
        !system.contains(SECRET),
        "the owner's context file: {system}"
    );
    assert!(
        system.contains("withheld: this place is shared"),
        "{system}"
    );

    let program = result_of(&r.core, &shared, "p1");
    assert!(
        program.starts_with("Not run: proc.run is not offered in a shared place"),
        "{program}"
    );
    let notes = result_of(&r.core, &shared, "p2");
    assert!(notes.starts_with("Not run: "), "{notes}");
    assert!(notes.contains("outside the public paths"), "{notes}");
    assert!(result_of(&r.core, &shared, "p3").contains(OPEN));
    for req in &reqs {
        assert!(
            !serde_json::to_string(&req.messages)
                .unwrap()
                .contains(SECRET),
            "nothing of the owner's reached the shared place"
        );
    }
    let refused = refused(&r.core);
    let calls: Vec<&str> = refused.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(calls, ["p1", "p2"], "{refused:?}");
    assert!(
        refused.iter().all(|(_, why)| why.starts_with("place: ")),
        "{refused:?}"
    );
}

/// A DM with the owner, the CLI, and a guild channel bound `private = true`
/// get everything; a DM with anyone else is a shared place.
#[tokio::test]
async fn a_private_place_gets_everything_and_anyone_elses_dm_is_shared() {
    let r = rig();
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: true,
    }]);
    for place in [
        Some(format!("dm:{OWNER}")),
        None,
        Some(format!("channel:{LAB}")),
    ] {
        let sid = session(&r.core, place.as_deref());
        assert_eq!(
            r.core.runner.class_of(&sid),
            PlaceClass::Private,
            "{place:?}"
        );
        let before = r.requests().len();
        turn(&r.core, &sid, "hello").await;
        let req = &r.requests()[before];
        assert!(offered(req).contains(&"proc_run".to_string()), "{place:?}");
        assert!(system_text(req).contains(SECRET), "{place:?}");
    }
    let alice = session(&r.core, Some(&format!("dm:{ALICE}")));
    assert_eq!(r.core.runner.class_of(&alice), PlaceClass::Shared);
    let before = r.requests().len();
    turn(&r.core, &alice, "hello").await;
    assert_eq!(offered(&r.requests()[before]), SHARED_TOOLS);
}

/// A task takes its parent's class: started in a shared place, it is
/// offered the public tools alone, and its program is refused.
#[tokio::test]
async fn a_task_takes_its_parents_class() {
    let r = rig();
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &shared, "start a task").await;
    r.until("the task's answer", 15, |r| {
        r.asked("count the files").iter().any(answers_a_call)
    })
    .await;
    let task = r.asked("count the files");
    assert_eq!(offered(&task[0]), SHARED_TOOLS, "the task's catalog");
    assert!(!system_text(&task[0]).contains(SECRET));
    let refused = refused(&r.core);
    assert!(
        refused
            .iter()
            .any(|(id, why)| id == "k1" && why.starts_with("place: proc.run is not offered")),
        "the task's program was refused: {refused:?}"
    );
}

/// A session whose place has moved on (`/new`) answers its wake in the
/// place the wake was set from (theseus-4lx), so its turn is that place's:
/// shared here, though the session itself posts nowhere now.
#[tokio::test]
async fn a_wake_answered_in_a_shared_place_is_that_places_turn() {
    let r = rig();
    let old = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &old, "WAKE in a second").await;
    // `/new`: the channel moves on to a new session.
    let _new = session(&r.core, Some(&format!("channel:{LAB}")));
    assert_eq!(
        r.core.outbox.target(&old),
        None,
        "the old session posts nowhere"
    );
    r.until("the wake's turn", 15, |r| {
        !r.asked("check again").is_empty()
    })
    .await;
    let wake = &r.asked("check again")[0];
    assert_eq!(
        offered(wake),
        SHARED_TOOLS,
        "the wake's turn is the channel's"
    );
    assert!(!system_text(wake).contains(SECRET));
}

/// Health names each place with its class, and warns of a channel bound
/// private that others can view, as the binding read it at its start; the
/// read is recorded.
#[tokio::test]
async fn health_names_each_place_and_a_private_channel_others_can_view() {
    let r = rig();
    r.core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
        },
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @owner".into(),
            private: false,
        },
    ]);
    r.core.private_place_viewed(
        LAB,
        "#lab",
        Ok(vec![(OWNER, "owner".into()), (ALICE, "alice".into())]),
    );
    let h = r.core.health().places.unwrap();
    let named: Vec<(&str, PlaceClass, Option<&Vec<String>>)> = h
        .places
        .iter()
        .map(|p| (p.name.as_str(), p.class, p.others.as_ref()))
        .collect();
    let alice = vec!["alice".to_string()];
    assert_eq!(
        named,
        [
            ("CLI", PlaceClass::Private, None),
            ("web", PlaceClass::Private, None),
            ("#lab", PlaceClass::Private, Some(&alice)),
            ("DM @owner", PlaceClass::Private, None),
        ]
    );
    let row = &rows(&r.core, "place.viewed")[0];
    assert_eq!(row["others"], json!(["alice"]), "{row}");
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}
