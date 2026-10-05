//! Situations (M6 step 35a, theseus-3nk.1) through the whole core:
//! - each compile records its situation on `context.compiled`, and a new
//!   compilation stores it: a conversation's first compile, then
//!   continuations;
//! - the precedence line follows the persona in the system's header, and a
//!   compilation made before it recompiles once (`system_changed`), then
//!   appends;
//! - a resume (a new core on the same store, a turn that brings nothing)
//!   renders the manifest's prefix byte for byte and recalls nothing;
//! - a recalled item's header names its source's place;
//! - a recalled item that shows a volatile value says as of when, and
//!   unverified, and names its source's place;
//! - a compilation whose assembled recall section is gone does not close:
//!   the turn fails as `context_unadmitted`, naming it, and nothing is sent.
//!
//! The index is `tests_recall`'s stand-in.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_store::{kinds, NewRecord};

use crate::bus::EventSink;
use crate::compiler::situation::PRECEDENCE;
use crate::compiler::Compilation;
use crate::config::MemoryMode;
use crate::node::{Body, Node};
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_recall::index_of;
use crate::turn::situation_step::UNADMITTED_CLASS;
use crate::turn::{TurnError, TurnRequest, ASSEMBLY, PERSONA};
use crate::{Config, Core};

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";
const GAUGE: &str = "The Pellworth harbor gauge read 3.2 m at 14:05 today, on firmware v2.4.1.";

/// A core over the store in `dir`, recall in front of the model for every
/// session.
fn build(dir: &Path, model: Arc<FakeProvider>) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let root = dir.join("w");
    if root.exists() {
        let root = root.canonicalize().unwrap();
        cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    }
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Canary;
    cfg.memory.canary_fraction = 1.0;
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap()
}

fn session(core: &Core, said: &[&str]) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    for s in said {
        let n = Node::user(&rec.session_id, None, "test", s);
        core.store.append(&[n.record().unwrap()]).unwrap();
    }
    rec.session_id
}

/// A turn on `sid`: `input`, or a continuation with none.
async fn run(core: &Arc<Core>, sid: &str, input: Option<&str>) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: input.map(str::to_string),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: input.map_or("harness", |_| "test").into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
}

/// The ledger rows of `kind`, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(2000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// The session's current compilation.
fn current(core: &Core, sid: &str) -> Compilation {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

/// Write `c` again, as a store an older build wrote would hold it.
fn rewrite(core: &Core, c: &Compilation) {
    let r = NewRecord::json(kinds::COMPILATION, Some(&c.id), c)
        .unwrap()
        .scoped(&c.session_id);
    core.store.append(&[r]).unwrap();
}

/// The first compile is a conversation's start, stored on its compilation;
/// the next turns append as continuations.
#[tokio::test]
async fn each_compile_records_its_situation() {
    let dir = tempfile::tempdir().unwrap();
    let core = build(dir.path(), Arc::new(FakeProvider::default()));
    let sid = session(&core, &[]);
    run(&core, &sid, Some("Is the lamp lit?")).await.unwrap();
    run(&core, &sid, Some("And the fog bell?")).await.unwrap();
    let compiled = rows(&core, "context.compiled");
    assert_eq!(compiled.len(), 2);
    assert_eq!(compiled[0]["situation"]["kind"], "conversation_start");
    assert_eq!(compiled[0]["decision"], "recompile");
    assert_eq!(compiled[1]["situation"]["kind"], "continuation");
    assert_eq!(compiled[1]["decision"], "append");
    let c = current(&core, &sid);
    assert_eq!(
        c.situation,
        Some(crate::compiler::situation::Situation::ConversationStart)
    );
}

/// The precedence line sits after the persona, in the header every session
/// of the profile shares. A compilation made before it (its system digest
/// another) recompiles once, `system_changed`, and the next turn appends.
#[tokio::test]
async fn the_precedence_line_costs_one_system_changed_recompile_then_appends() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &[]);
    run(&core, &sid, Some("Is the lamp lit?")).await.unwrap();
    let header = model.requests()[0].system[0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        header.starts_with(&format!("{PERSONA}\n\n{ASSEMBLY}\n\n{PRECEDENCE}\n\n")),
        "after the persona: {header}"
    );
    assert!(PRECEDENCE.starts_with("When sources disagree, trust them in this order: "));
    assert!(PRECEDENCE.ends_with("recalled notes, which are dated testimony."));

    // As the build before it wrote the compilation: another header.
    let mut old = current(&core, &sid);
    old.manifest.system_digest = "0123456789abcdef".into();
    old.situation = None;
    rewrite(&core, &old);
    run(&core, &sid, Some("And the fog bell?")).await.unwrap();
    run(&core, &sid, Some("And the tide?")).await.unwrap();
    let compiled = rows(&core, "context.compiled");
    assert_eq!(compiled[1]["decision"], "recompile");
    assert_eq!(compiled[1]["trigger"], "system_changed");
    assert_eq!(compiled[1]["situation"]["kind"], "recompile");
    assert_eq!(compiled[1]["situation"]["trigger"], "system_changed");
    assert_eq!(
        compiled[2]["decision"], "append",
        "one recompile, then appends"
    );
    assert_eq!(compiled[2]["situation"]["kind"], "continuation");
}

/// A turn that faults with a call unanswered, a restart, and its
/// continuation, which brings nothing new (tests_m3's fault, theseus-l6y): a
/// resume. Its request begins with the last request's bytes, its prefix
/// rebuilt from the manifest, and it recalls nothing.
#[tokio::test]
async fn a_resume_rebuilds_its_prefix_byte_for_byte_and_recalls_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("weir.txt"), "the heron fishes at dawn\n").unwrap();
    let read = Scripted::tools(
        "Reading it.",
        &[("t1", "fs_read", json!({"path": "weir.txt"}))],
    );
    let model = Arc::new(FakeProvider::scripted(vec![read]));
    let core = build(dir.path(), model.clone());
    let src = session(&core, &[HERON]);
    let sid = session(&core, &[]);
    core.runner
        .memory
        .set_ask(index_of(&core, vec![src.clone()]));
    core.store.fail_turn_frame(|records| {
        records.iter().any(|r| {
            r.kind == kinds::ACTION
                && serde_json::from_slice::<Value>(&r.payload)
                    .is_ok_and(|a| a["tool"].as_str().is_some_and(|t| t.starts_with("fs")))
        })
    });
    run(&core, &sid, Some("Where does the grey heron nest?"))
        .await
        .expect_err("the turn faults");
    let before = model.requests().pop().unwrap();
    let recalls = |core: &Core| {
        core.store
            .session_nodes(&sid)
            .unwrap()
            .iter()
            .filter(|(_, n)| matches!(n.body, Body::Recall { .. }))
            .count()
    };
    assert_eq!(recalls(&core), 1, "the first turn recalled");
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.clone().unwrap();
    drop(core);

    let model = Arc::new(FakeProvider::scripted(vec![Scripted::text(
        "By the old weir.",
    )]));
    let core = build(dir.path(), model.clone());
    core.runner
        .memory
        .set_ask(index_of(&core, vec![src.clone()]));
    let ran = rows(&core, "recall.ran").len();
    let res = core
        .continue_execution(&exec)
        .await
        .unwrap()
        .expect("the continuation runs");
    assert_eq!(res.recalled, 0);
    let compiled = rows(&core, "context.compiled");
    let last = compiled.last().unwrap();
    assert_eq!(last["situation"]["kind"], "resume", "{last}");
    assert_eq!(last["decision"], "append");
    let after = model.requests().pop().unwrap();
    assert_eq!(after.system, before.system);
    assert_eq!(
        &after.messages[..before.messages.len()],
        &before.messages[..],
        "the prefix, byte for byte"
    );
    assert!(
        after.messages.len() > before.messages.len(),
        "and the call's result"
    );
    assert_eq!(recalls(&core), 1, "no new recall");
    assert_eq!(rows(&core, "recall.ran").len(), ran, "nothing asked");

    // New inbound: a continuation.
    run(&core, &sid, Some("Where is the weir?")).await.unwrap();
    let compiled = rows(&core, "context.compiled");
    assert_eq!(
        compiled.last().unwrap()["situation"]["kind"],
        "continuation"
    );
}

/// A recalled item's frozen header names its origin: whose it is, and the
/// place its session speaks in, by its bound name (35a).
#[tokio::test]
async fn a_recalled_items_header_names_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let core = build(dir.path(), Arc::new(FakeProvider::default()));
    core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:channel:271000000000000009".into(),
        name: "#harbor".into(),
        private: true,
        ..Default::default()
    }]);
    let harbor = session(&core, &[HERON]);
    core.outbox
        .bind_place("channel:271000000000000009", &harbor)
        .unwrap();
    let sid = session(&core, &[]);
    core.runner
        .memory
        .set_ask(index_of(&core, vec![harbor.clone()]));
    let res = run(&core, &sid, Some("Where does the grey heron nest?"))
        .await
        .unwrap();
    assert_eq!(res.recalled, 1);
    let nodes = core.store.session_nodes(&sid).unwrap();
    let header = nodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { items, .. } => Some(items[0].header.clone()),
            _ => None,
        })
        .expect("a Recall node");
    assert!(
        header.starts_with("a message from test in #harbor, "),
        "{header}"
    );
    assert!(header.contains(" UTC (as of @"), "{header}");
}

/// A recalled item whose shown text holds a volatile value ends its
/// frozen header `volatile: as of <date>, unverified`; one that holds none
/// does not. The header names the source's place (35a).
#[tokio::test]
async fn a_volatile_item_is_as_of_and_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = build(dir.path(), model.clone());
    let gauge = session(&core, &[GAUGE]);
    let heron = session(&core, &[HERON]);
    let sid = session(&core, &[]);
    core.runner
        .memory
        .set_ask(index_of(&core, vec![gauge.clone(), heron.clone()]));
    let res = run(
        &core,
        &sid,
        Some("What did the Pellworth gauge read, and where is the heron?"),
    )
    .await
    .unwrap();
    assert_eq!(res.recalled, 2);
    let nodes = core.store.session_nodes(&sid).unwrap();
    let Some(Body::Recall { items, .. }) = nodes
        .iter()
        .map(|(_, n)| &n.body)
        .find(|b| matches!(b, Body::Recall { .. }))
    else {
        panic!("a Recall node");
    };
    let source = |sid: &str| core.store.session_nodes(sid).unwrap()[0].1.clone();
    let (g, h) = (source(&gauge), source(&heron));
    let day = {
        let (y, m, d) = crate::wake::civil_from_days((g.created_at_ms / 86_400_000) as i64);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let of = |id: &str| {
        items
            .iter()
            .find(|r| r.node_id == id)
            .unwrap()
            .header
            .clone()
    };
    let gh = of(&g.id);
    assert!(
        gh.ends_with(&format!(", volatile: as of {day}, unverified")),
        "{gh}"
    );
    assert!(
        gh.starts_with(&format!(
            "a message from test in {gauge} on the CLI or the web UI, "
        )),
        "{gh}"
    );
    assert!(!of(&h.id).contains("volatile"), "{}", of(&h.id));
    let q = model.requests().pop().unwrap();
    let sent = serde_json::to_string(&q.messages).unwrap();
    assert!(
        sent.contains(&format!("volatile: as of {day}, unverified")),
        "{sent}"
    );
}

/// A compilation whose assembled recall section names a node the session
/// does not hold does not close: the turn fails as `context_unadmitted`,
/// naming the section, and sends nothing.
#[tokio::test]
async fn a_set_that_does_not_close_fails_naming_the_piece() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &[]);
    run(&core, &sid, Some("Is the lamp lit?")).await.unwrap();
    let mut c = current(&core, &sid);
    c.recall_id = Some("rcn_gone".into());
    rewrite(&core, &c);
    let err = run(&core, &sid, Some("And the fog bell?"))
        .await
        .expect_err("it does not close");
    let te = err.downcast_ref::<TurnError>().unwrap();
    assert_eq!(te.class, UNADMITTED_CLASS);
    let message = format!("{:#}", te.source);
    assert!(message.contains("recall section rcn_gone"), "{message}");
    assert!(message.contains("Nothing was sent."), "{message}");
    assert_eq!(model.requests().len(), 1, "no provider call for it");
    let row = rows(&core, "context.unadmitted");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0]["why"], "unclosed");
    assert_eq!(row[0]["piece"], "recall_section rcn_gone");
    assert_eq!(row[0]["situation"]["kind"], "continuation");
}
