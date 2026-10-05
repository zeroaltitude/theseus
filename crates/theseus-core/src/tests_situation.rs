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
//! - a compilation whose assembled recall section was never written (a
//!   compaction's call not dispatched) is sent without it, as the render
//!   leaves it out (theseus-783a);
//! - a piece its situation does not admit fails the turn as
//!   `context_unadmitted`, naming it, and nothing is sent.
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

/// A compilation whose assembled recall section was never written (its
/// call was never dispatched: the budget's refusal, a `/stop`, or a kernel
/// error at the dispatch) is sent without the section, as the render leaves
/// it out: nodes are never deleted, so a `recall_id` with no node is only
/// ever that (theseus-783a).
#[tokio::test]
async fn a_section_never_written_is_left_out_and_the_turn_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &[]);
    run(&core, &sid, Some("Is the lamp lit?")).await.unwrap();
    let mut c = current(&core, &sid);
    c.recall_id = Some("rcn_gone".into());
    rewrite(&core, &c);
    run(&core, &sid, Some("And the fog bell?"))
        .await
        .expect("it is sent");
    assert!(rows(&core, "context.unadmitted").is_empty());
    let compiled = rows(&core, "context.compiled");
    let last = compiled.last().unwrap();
    assert_eq!(last["decision"], "append", "{last}");
    assert_eq!(last["situation"]["kind"], "continuation");
    let reqs = model.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        reqs[1].messages[..reqs[0].messages.len()],
        reqs[0].messages[..],
        "the prefix, with no section"
    );
}

/// A piece its situation does not admit fails the turn as
/// `context_unadmitted`, naming the piece, and nothing is sent: here a
/// task's arrangement in a conversation's first compile, which no path
/// writes (an arrangement goes into its task's own session).
#[tokio::test]
async fn a_piece_its_situation_does_not_admit_fails_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = build(dir.path(), model.clone());
    let sid = session(&core, &["Draft a note to the harbor master."]);
    let arr = Node::arrangement(&sid, "test", vec![], false);
    core.store.append(&[arr.record().unwrap()]).unwrap();
    let err = run(&core, &sid, Some("Is the lamp lit?"))
        .await
        .expect_err("it is not admitted");
    let te = err.downcast_ref::<TurnError>().unwrap();
    assert_eq!(te.class, UNADMITTED_CLASS);
    let piece = format!("arrangement {}", arr.id);
    let message = format!("{:#}", te.source);
    assert!(
        message.contains(&format!(
            "a conversation_start compile does not admit {piece}"
        )),
        "{message}"
    );
    assert!(message.contains("Nothing was sent."), "{message}");
    assert!(model.requests().is_empty(), "no provider call");
    let row = rows(&core, "context.unadmitted");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0]["why"], "not_admitted");
    assert_eq!(row[0]["piece"], piece);
    assert_eq!(row[0]["situation"]["kind"], "conversation_start");
}

const WINDOW: u64 = 40_000;

/// A core whose model has a 40k window, summaries on glm, and recall in
/// front of the model, recalling `HERON`'s session for every query.
fn compacting(dir: &Path, model: Arc<FakeProvider>, glm: Arc<FakeProvider>) -> Arc<Core> {
    let root = dir.join("w");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Live;
    cfg.memory.summary_profile = "glm".into();
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(WINDOW),
            max_output_tokens: Some(2_000),
            ..Default::default()
        },
    );
    let store = Store::open(&dir.join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model, store);
    parts.providers.insert("zai".into(), glm);
    let core = Core::build(parts).unwrap();
    let src = crate::tests_recall::session(&core, None, &[HERON]);
    core.runner.memory.set_ask(index_of(&core, vec![src]));
    core
}

/// glm's summary, billed as a summary call is.
fn summary() -> Scripted {
    Scripted::Billed {
        usage: theseus_protocol::Usage {
            input_tokens: 9_000,
            output_tokens: 40,
            ..Default::default()
        },
        then: Box::new(Scripted::text("Six tides were logged.")),
    }
}

/// A message of about `tokens` tokens of prose.
fn long(tag: &str, tokens: usize) -> String {
    let mut s = format!("{tag} ");
    while s.len() < tokens * 4 {
        s.push_str("the keeper logged the tide and the wind before the lamp was lit. ");
    }
    s
}

/// A compaction whose call is never dispatched (its dispatch frame not
/// written, as on a full disk; the budget's refusal and a `/stop` at the
/// dispatch return before that frame too) leaves the session's
/// compilation naming its assembled section, never written. The next turn
/// is sent without it, as the render leaves it out (local reviewer R7's
/// probe, theseus-783a).
#[tokio::test]
async fn a_compactions_next_turn_after_its_undispatched_call_is_sent() {
    // A first core finds the turn that compacts.
    let dry = tempfile::tempdir().unwrap();
    let glm = || Arc::new(FakeProvider::scripted(vec![summary()]));
    let core = compacting(dry.path(), Arc::new(FakeProvider::default()), glm());
    let sid = session(&core, &[]);
    let mut at = None;
    for k in 0..12 {
        run(&core, &sid, Some(&long(&format!("turn{k}"), 6_000)))
            .await
            .unwrap();
        let last = rows(&core, "context.compiled").pop().unwrap_or_default();
        if last["strategy"] == "compaction" {
            at = Some(k);
            break;
        }
    }
    let at = at.expect("a compaction");
    drop(core);

    // The same turns again, the compacting turn's own call not dispatched.
    let dir = tempfile::tempdir().unwrap();
    let model = Arc::new(FakeProvider::default());
    let core = compacting(dir.path(), model.clone(), glm());
    let sid = session(&core, &[]);
    for k in 0..at {
        run(&core, &sid, Some(&long(&format!("turn{k}"), 6_000)))
            .await
            .unwrap();
    }
    // The turn's own call's dispatch frame: its provider action, with the
    // assembled section riding in it. The compaction's summary call is a
    // provider action too, and carries no recall node.
    core.store.fail_turn_frame(|records| {
        let call = records.iter().any(|r| {
            r.kind == kinds::ACTION
                && serde_json::from_slice::<Value>(&r.payload)
                    .is_ok_and(|a| a["tool"] == theseus_protocol::PROVIDER_TOOL)
        });
        let section = records.iter().any(|r| {
            r.kind == kinds::NODE && r.key.as_deref().is_some_and(|k| k.starts_with("rcn_"))
        });
        call && section
    });
    run(&core, &sid, Some(&long(&format!("turn{at}"), 6_000)))
        .await
        .expect_err("its call is not dispatched");
    let c = current(&core, &sid);
    assert_eq!(c.strategy, "compaction");
    let section = c.recall_id.clone().expect("an assembled section");
    let held = core.store.transcript(&sid).unwrap();
    assert!(
        !held.iter().any(|(_, n)| n.id == section),
        "its section was never written"
    );
    let sent = model.requests().len();
    run(&core, &sid, Some("and the wind?"))
        .await
        .expect("the next turn is sent");
    assert_eq!(model.requests().len(), sent + 1);
    assert!(rows(&core, "context.unadmitted").is_empty());
}
