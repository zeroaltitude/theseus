//! Jev files the session into the ontology (theseus-xzxqv), through a whole
//! core and the fake Jev: `onto.v1` in the inbound batch, its answers in the
//! three bands, the write in the judgment's frame, the reply's footer, the
//! owner's cards, the undo, and the naming call.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_protocol::OntologyUndoParams;

use crate::outbox::OPERATOR_TARGET;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Parts;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_inbound::inbound_only;
use crate::tests_judge::{judge_config, kinds, texts, warm};
use crate::{Config, Core};
use theseus_protocol::SessionKind;

const SAID: &str = "let's plan the Lantern Ledger migration";
const PROVIDER_MS: u64 = 400;

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) jev: FakeJev,
    /// The profile `[ontology] name_profile` names runs on this provider.
    namer: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig_with(jev: FakeJev, namer: Vec<Scripted>, more: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg: Config = judge_config(dir.path(), Some(&jev));
    inbound_only(&mut cfg);
    cfg.judge.packs.remove(crate::judge::onto::ONTO_PACK);
    cfg.ontology.judge = true;
    // The namer is only a second provider here; it reads the session's text,
    // so it runs an unrestricted model (theseus-a1wvq.3).
    cfg.ontology.name_profile = Some("glm".into());
    cfg.serve_glm_unrestricted();
    more(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut fake = FakeProvider::scripted(texts(8));
    fake.delay_ms = PROVIDER_MS;
    let mut p = Parts::for_tests(cfg, Arc::new(fake), store);
    let namer = Arc::new(FakeProvider::scripted(namer));
    p.providers.insert("zai".into(), namer.clone());
    p.secrets = crate::tests_judge::board();
    let core = Core::build(p).unwrap();
    warm(&core);
    Rig {
        core,
        jev,
        namer,
        _dir: dir,
    }
}

pub(crate) fn rig(jev: FakeJev) -> Rig {
    rig_with(jev, Vec::new(), |_| {})
}

/// A session bound to the owner's DM `dm:42`.
pub(crate) fn bound(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place("dm:42", &rec.session_id).unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    rec.session_id
}

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn operator_writes(core: &Core, records: Vec<Record>) {
    core.runner
        .ontology
        .write(&core.store, records, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
}

fn held(sid: &str, categories: &[&str]) -> Record {
    Record::Members(MemberList {
        session: sid.into(),
        kind: "topic".into(),
        members: categories
            .iter()
            .map(|c| Membership::operator(id(c), 1_000))
            .collect(),
    })
}

fn list_bytes(core: &Core, sid: &str) -> Vec<u8> {
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let list = o.member_list(sid, "topic").cloned();
    serde_json::to_vec(&list.map(|l| Record::Members(l).to_value())).unwrap()
}

fn frames_in(f: impl FnOnce()) -> u64 {
    let before = theseus_store::frames_written_here();
    f();
    theseus_store::frames_written_here() - before
}

/// A turn in `sid`, then the sink's frame written now, and how many frames
/// that was: the filing rides the judgments' own.
pub(crate) async fn said(r: &Rig, sid: &str, text: &str) -> u64 {
    crate::tests_route::turn(&r.core, Some(sid), text, None).await;
    let judge = r.core.runner.judge.clone();
    tokio::task::spawn_blocking(move || {
        frames_in(|| {
            judge.flush_sink();
        })
    })
    .await
    .unwrap()
}

pub(crate) fn cards(core: &Core) -> Vec<Value> {
    core.outbox
        .open_for(OPERATOR_TARGET)
        .iter()
        .map(|a| crate::outbox::body_of(a).clone())
        .filter(|b| b["kind"] == "onto_card")
        .collect()
}

fn reply(core: &Core) -> Value {
    let posts = core.outbox.open_for("discord:dm:42");
    let body = crate::outbox::body_of(posts.last().expect("a reply post")).clone();
    assert_eq!(body["kind"], "reply");
    body
}

fn onto_judgment(core: &Core) -> String {
    kinds(&core.store, "judge.call")
        .into_iter()
        .find(|r| r.data["pack"] == "onto.v1")
        .expect("onto.v1's row")
        .data["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// KEY: `topic = new` at 0.9 and `topic_name` a mined name at 0.9 mint the
/// category, file the session in it as Jev (the category and the row say
/// `jev:<judgment>`), in the judgment's own frame; the reply's footer says
/// so, and one card tells the owner, with its undo.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_topic_at_the_act_band_is_minted_in_the_judgments_frame_with_its_footer_and_card() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    let frames = said(&r, &sid, SAID).await;
    assert_eq!(frames, 1, "the filing rides the judgments' frame");
    let judgment = onto_judgment(&r.core);
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let list = o.member_list(&sid, "topic").expect("the session is filed");
    let [m] = list.members.as_slice() else {
        panic!("{list:?}")
    };
    assert_eq!((m.origin, m.confidence), (Origin::Jev, Some(0.9)));
    let c = o.category(&m.category).expect("the category is minted");
    assert_eq!(c.name, "Lantern Ledger");
    assert_eq!(c.added_by, format!("jev:{judgment}"));
    let row = kinds(&r.core.store, "ontology.membership")
        .pop()
        .expect("the membership's row");
    assert_eq!(row.data["who"], format!("jev:{judgment}"));
    assert_eq!(
        reply(&r.core)["result"]["filed"],
        "filed: topic › Lantern Ledger (new)"
    );
    let cards = cards(&r.core);
    assert_eq!(cards.len(), 1, "one mint card");
    assert_eq!(
        (&cards[0]["undo"], &cards[0]["write"]),
        (&json!(true), &json!(judgment))
    );
}

/// A confirm-band answer (0.7) writes nothing, says nothing in the footer,
/// and sends one card asking.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirm_band_answer_writes_nothing_and_sends_one_card() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.7);
    let r = rig(jev);
    operator_writes(
        &r.core,
        vec![Record::Category(Category::new(
            id("topic:ledger"),
            "Ledger",
            "ada",
        ))],
    );
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.member_list(&sid, "topic").is_none(), "nothing written");
    assert!(reply(&r.core)["result"].get("filed").is_none());
    let cards = cards(&r.core);
    assert_eq!(cards.len(), 1, "one confirm card");
    assert_eq!(cards[0]["undo"], false);
    assert!(cards[0]["text"].as_str().unwrap().contains("topic:ledger"));
    let filed = kinds(&r.core.store, "onto.filed");
    assert_eq!(filed[0].data["steps"][0]["outcome"], "asked");
}

/// Below the confirm band an answer is only a record: no row of its own, no
/// card.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_below_the_confirm_band_is_only_the_judgments_row() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.4);
    let r = rig(jev);
    operator_writes(
        &r.core,
        vec![Record::Category(Category::new(
            id("topic:ledger"),
            "Ledger",
            "ada",
        ))],
    );
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    assert!(cards(&r.core).is_empty());
    assert!(kinds(&r.core.store, "onto.filed").is_empty());
    assert!(!kinds(&r.core.store, "judge.call").is_empty());
}

/// `ontology.undo` through the RPC puts the member list back byte for byte
/// and retires the category the write minted, with one `ontology.undo` row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undo_through_the_rpc_restores_the_list_and_retires_the_mint() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    operator_writes(
        &r.core,
        vec![
            Record::Category(Category::new(id("topic:web"), "web", "ada")),
            held(&sid, &["topic:web"]),
        ],
    );
    let was = list_bytes(&r.core, &sid);
    said(&r, &sid, SAID).await;
    assert_ne!(list_bytes(&r.core, &sid), was, "Jev's entry was added");
    let write = onto_judgment(&r.core);
    let done = r
        .core
        .ontology_undo_rpc(
            &OntologyUndoParams {
                write: write.clone(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(done.write, write);
    assert_eq!(list_bytes(&r.core, &sid), was);
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.category(&id("topic:lantern-ledger")).is_none(), "retired");
    let undone = kinds(&r.core.store, "ontology.undo");
    assert_eq!(
        (undone.len(), &undone[0].data["label"]),
        (1, &json!("wrong"))
    );
}

/// The operator's memberships are never overwritten: a list the operator
/// filled to its limit refuses Jev's entry, and the refusal is a row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_operators_memberships_are_never_overwritten() {
    let jev = FakeJev::start().unwrap();
    // The session's own three come first, so the fourth is c4.
    jev.script_onto("topic", "c4", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    let names = ["a", "b", "c", "d"];
    let mut records: Vec<Record> = names
        .iter()
        .map(|n| Record::Category(Category::new(id(&format!("topic:{n}")), *n, "ada")))
        .collect();
    records.push(held(&sid, &["topic:a", "topic:b", "topic:c"]));
    operator_writes(&r.core, records);
    let was = list_bytes(&r.core, &sid);
    said(&r, &sid, SAID).await;
    assert_eq!(list_bytes(&r.core, &sid), was, "the operator's list stands");
    let filed = kinds(&r.core.store, "onto.filed");
    let step = &filed[0].data["steps"][0];
    assert_eq!(
        (&step["outcome"], &step["disagrees"]),
        (&json!("refused"), &json!(true))
    );
    assert!(reply(&r.core)["result"].get("filed").is_none());
}

fn batch_request(jev: &FakeJev) -> Value {
    jev.seen()
        .into_iter()
        .map(|s| s.body)
        .find(|b| b["questions"].to_string().contains("classify.v4/"))
        .expect("the batch's request")
}

/// With `[ontology] judge = false` the batch's request is byte for byte what
/// it was: no onto question and no ontology field in its state. With it on,
/// the state is that plus the fields, and `route.v6` never carries them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn off_the_batch_is_todays_and_on_its_state_is_todays_plus_the_fields() {
    let mut states = Vec::new();
    for on in [false, true] {
        let jev = FakeJev::start().unwrap();
        let r = rig_with(jev, Vec::new(), |c| {
            c.ontology.judge = on;
            c.judge.packs.remove(crate::judge::inbound::ROUTE_PACK);
        });
        let sid = bound(&r.core);
        crate::tests_route::turn(&r.core, Some(&sid), SAID, None).await;
        let body = batch_request(&r.jev);
        let asks_onto = body["questions"].to_string().contains("onto.v1/");
        assert_eq!(asks_onto, on, "onto.v1 rides the batch only when on");
        for route in r
            .jev
            .seen()
            .iter()
            .filter(|s| s.body["questions"].to_string().contains("route.v6/"))
        {
            assert!(
                !route.body["state"].to_string().contains("mined_names"),
                "route.v6's state never carries the ontology's fields"
            );
        }
        assert_eq!(
            r.jev.seen().iter().filter(|s| s.body == body).count(),
            1,
            "one batch request"
        );
        states.push(body["state"].clone());
    }
    let (off, on) = (
        states[0].as_object().unwrap(),
        states[1].as_object().unwrap(),
    );
    for (k, v) in off {
        assert_eq!(on.get(k), Some(v), "{k} is as it was");
    }
    let added: Vec<&String> = on.keys().filter(|k| !off.contains_key(*k)).collect();
    assert_eq!(
        added,
        ["mined_names"],
        "the ontology fields of this message"
    );
}

/// The turn waits for nothing of onto.v1: with the batch (and so onto.v1's
/// answer) held 2.5 s, the turn's route verdict, which rides a request of
/// its own, comes at once and the turn finishes as soon as it does without
/// onto.v1 (the batch, the sink and the filing are off its path).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turns_wait_is_unchanged_with_onto_in_a_slow_batch() {
    let mut took = Vec::new();
    for on in [false, true] {
        let jev = FakeJev::start().unwrap();
        jev.set_latency(Duration::ZERO, Duration::from_millis(2_500));
        let r = rig_with(jev, Vec::new(), |c| {
            c.ontology.judge = on;
            // route.v6 asks, and the turn waits for its verdict beside its
            // first compile (a slow one up to this long).
            c.judge.packs.remove(crate::judge::inbound::ROUTE_PACK);
            c.routing.max_wait_ms = 5_000;
            crate::tests_route::glm_placements(c);
        });
        let sid = bound(&r.core);
        let t0 = Instant::now();
        crate::tests_route::turn(&r.core, Some(&sid), SAID, None).await;
        took.push(t0.elapsed());
    }
    assert!(
        took.iter().all(|t| *t < Duration::from_millis(1_000)),
        "the turn did not wait for the batch: {took:?}"
    );
    assert!(
        took[1] < took[0] + Duration::from_millis(500),
        "onto.v1 added no wait: {took:?}"
    );
}

/// The naming call: Jev says `new` and no mined name fits, so one cheap
/// call names it, once a session, kind and day: a second such message the
/// same day makes none, and the session's next onto.v1 offers the names.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_naming_call_is_made_once_a_session_kind_and_day() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "none", 0.9);
    let named = Scripted::tools(
        "",
        &[(
            "tu_1",
            "name_category",
            json!({"names": ["Ledger move", "Lantern ledger", "Ledger migration"],
                   "description": "Moving the ledger onto Lantern."}),
        )],
    );
    let r = rig_with(jev, vec![named.clone(), named], |_| {});
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    for _ in 0..100 {
        if r.namer.requests().len() == 1 && !kinds(&r.core.store, "onto.named").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(r.namer.requests().len(), 1, "one naming call");
    let row = &kinds(&r.core.store, "onto.named")[0].data;
    assert_eq!(
        row["names"],
        json!(["Ledger move", "Lantern ledger", "Ledger migration"])
    );
    assert!(row["cost_usd"].as_f64().is_some());
    // Nothing was minted: no name fit.
    assert!(cards(&r.core).is_empty());
    // The same day, the same kind: no second call; the names are offered.
    said(&r, &sid, SAID).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(r.namer.requests().len(), 1, "once a day");
    let batch = r
        .jev
        .seen()
        .into_iter()
        .map(|s| s.body)
        .rfind(|b| b["questions"].to_string().contains("onto.v1/topic_name"))
        .unwrap();
    assert!(batch["state"].to_string().contains("Ledger move"));
}

/// A naming call cut at its cap (theseus-opypg) says so in its row, turns
/// thinking off, and does not spend that day's naming slot: the session's
/// next such message names it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_naming_call_cut_at_its_cap_keeps_the_days_slot() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "none", 0.9);
    let cut = Scripted::Blocks {
        blocks: vec![
            json!({"type": "tool_use", "id": "tu_1", "name": "name_category", "input": {}}),
        ],
        stop_reason: "max_tokens".into(),
    };
    let named = Scripted::tools(
        "",
        &[(
            "tu_2",
            "name_category",
            json!({"names": ["Ledger move"], "description": "Moving the ledger."}),
        )],
    );
    let r = rig_with(jev, vec![cut, named], |_| {});
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    for _ in 0..100 {
        if !kinds(&r.core.store, "onto.named").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let row = &kinds(&r.core.store, "onto.named")[0].data;
    let why = row["failed"].as_str().unwrap();
    assert!(why.contains("max_tokens") && why.contains("300"), "{why}");
    let sent = r.namer.requests();
    let want = r.core.runner.catalog.thinking_off(&sent[0].model);
    assert!(
        want.is_some(),
        "{}: a model that turns thinking off",
        sent[0].model
    );
    assert_eq!(sent[0].thinking, want);
    // The slot is not spent: the same day's next message is named.
    said(&r, &sid, SAID).await;
    for _ in 0..100 {
        if r.namer.requests().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        r.namer.requests().len(),
        2,
        "a cut call does not use the day's slot"
    );
}

/// `ontology.kind.add` adds an interpreted kind Jev may assign, after the
/// last one; it is not added twice, and `retire` takes away only a kind
/// nothing holds and that is not seeded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_operator_adds_and_retires_a_kind() {
    use theseus_protocol::{OntologyKindAddParams, OntologyKindRetireParams};
    let r = rig(FakeJev::start().unwrap());
    let add = |name: &str| OntologyKindAddParams {
        name: name.into(),
        description: "A dataset the work uses".into(),
        mined_from: vec!["paths".into()],
        ..Default::default()
    };
    let k = r
        .core
        .ontology_kind_add(&add("dataset"), "the CLI")
        .unwrap()
        .kind;
    assert_eq!(k.assigned_by, ["operator", "jev"]);
    assert_eq!(k.precedence, 110, "ten after the last seeded kind");
    let again = r
        .core
        .ontology_kind_add(&add("dataset"), "the CLI")
        .unwrap_err();
    assert!(format!("{again:#}").contains("already"), "{again:#}");
    let retire = |name: &str| OntologyKindRetireParams {
        name: name.into(),
        ..Default::default()
    };
    let seeded = r
        .core
        .ontology_kind_retire(&retire("topic"), "the CLI")
        .unwrap_err();
    assert!(format!("{seeded:#}").contains("seeded"), "{seeded:#}");
    operator_writes(
        &r.core,
        vec![Record::Category(Category::new(
            id("dataset:tides"),
            "tides",
            "ada",
        ))],
    );
    assert!(r
        .core
        .ontology_kind_retire(&retire("dataset"), "the CLI")
        .is_err());
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(
        o.kind("dataset").unwrap().retired_ms.is_none(),
        "still held"
    );
    operator_writes(
        &r.core,
        vec![Record::Category(
            Category::new(id("dataset:tides"), "tides", "ada").retired(2_000),
        )],
    );
    assert!(
        r.core
            .ontology_kind_retire(&retire("dataset"), "the CLI")
            .unwrap()
            .retired
    );
    assert_eq!(
        kinds(&r.core.store, "ontology.kind").len(),
        2,
        "its add and its retire"
    );
}

/// `judge.latency` reads the batch's requests by day from the judgments' rows:
/// one request, carrying onto.v1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn judge_latency_counts_each_batch_request_once() {
    let r = rig(FakeJev::start().unwrap());
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    let l = r.core.judge_latency(Default::default()).unwrap();
    assert_eq!(l.point, "inbound");
    let [d] = l.days.as_slice() else {
        panic!("{l:?}")
    };
    assert_eq!((d.requests, d.with_onto), (1, 1), "{d:?}");
}

/// A project the owner made, as the task graph holds it.
fn project_named(core: &Core, id: &str, title: &str) {
    use theseus_protocol::tasks::{TaskKind, TaskRecord, TaskState};
    let t = TaskRecord {
        id: id.into(),
        title: title.into(),
        objective: "Move the ledger to the new store".into(),
        kind: TaskKind::Project,
        state: TaskState::Accepted,
        version: 1,
        owner: "agent".into(),
        ..TaskRecord::default()
    };
    core.store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
}

fn project_of(core: &Core, sid: &str) -> Option<String> {
    core.store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .project
}

/// KEY (theseus-jrf4c): `project = c1` at the act band attaches the session to
/// the project, whose slice reaches its next compile; the footer says so, the
/// `onto.filed` row records it, and no category is minted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_project_answer_at_the_act_band_attaches_an_unattached_session() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("project", "c1", 0.9);
    let r = rig(jev);
    project_named(&r.core, "tsk_ledger", "Lantern Ledger migration");
    let sid = bound(&r.core);
    assert_eq!(project_of(&r.core, &sid), None);
    said(&r, &sid, SAID).await;
    assert_eq!(project_of(&r.core, &sid).as_deref(), Some("tsk_ledger"));
    assert_eq!(
        reply(&r.core)["result"]["filed"],
        "filed: project › Lantern Ledger migration"
    );
    let filed = kinds(&r.core.store, "onto.filed");
    assert_eq!(filed[0].data["steps"][0]["outcome"], "attached");
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(
        o.member_list(&sid, "project").is_none(),
        "no list is written"
    );
    assert!(cards(&r.core).is_empty(), "a plain attach sends no card");
}

/// A project the session already has is never moved by Jev, and the confirm
/// band attaches nothing: it asks, with the command that does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jev_never_moves_a_project_and_the_confirm_band_only_asks() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("project", "c1", 0.7);
    let r = rig(jev);
    project_named(&r.core, "tsk_ledger", "Lantern Ledger migration");
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    assert_eq!(project_of(&r.core, &sid), None, "nothing is attached");
    let cards = cards(&r.core);
    assert_eq!(cards.len(), 1);
    let text = cards[0]["text"].as_str().unwrap();
    assert!(text.contains("Lantern Ledger migration"), "{text}");
    assert!(
        text.contains(&format!("theseus project attach {sid} tsk_ledger")),
        "{text}"
    );
    // The owner attaches another; a later act-band answer leaves it alone.
    project_named(&r.core, "tsk_other", "Harbour survey");
    r.core
        .store
        .update_session(&sid, |s| {
            s.project = Some("tsk_other".into());
            Ok(vec![])
        })
        .unwrap();
    crate::judge::onto::stage::attach(&r.core, &sid, "tsk_ledger");
    assert_eq!(project_of(&r.core, &sid).as_deref(), Some("tsk_other"));
}

/// KEY (theseus-jrf4c): recall's category term reads what two sessions share:
/// a project, a topic, a thread's weight; none in a shared place, none with
/// the term off, and the asking session's own notes are not lifted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_category_scene_weights_what_two_sessions_share() {
    use theseus_memory::{Candidate, Place};
    let r = rig(FakeJev::start().unwrap());
    let mk = |s: &str| {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        let _ = s;
        rec.session_id
    };
    let (here, topical, projected, stranger) =
        (mk("here"), mk("topical"), mk("projected"), mk("x"));
    operator_writes(
        &r.core,
        vec![
            Record::Category(Category::new(id("topic:web"), "web", "ada")),
            held(&here, &["topic:web"]),
            held(&topical, &["topic:web"]),
        ],
    );
    for s in [&here, &projected] {
        r.core
            .store
            .update_session(s, |rec| {
                rec.project = Some("tsk_ledger".into());
                Ok(vec![])
            })
            .unwrap();
    }
    let _ = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let cand = |node: &str, session: &str| Candidate {
        node_id: node.into(),
        chunk: 0,
        session_id: session.into(),
        position: 1,
        kind: "user_message".into(),
        origin: "operator".into(),
        external: false,
        text: "words".into(),
        fused: 0.05,
        index_rank: 1,
        place: Place::Private,
        time_ms: 0,
        entities: Vec::new(),
        matched: Vec::new(),
    };
    let cands = [
        cand("a", &topical),
        cand("b", &projected),
        cand("c", &stranger),
        cand("d", &here),
    ];
    let scene = r
        .core
        .runner
        .category_scene(Some(&here), true, None)
        .expect("a scene");
    let b = scene.boost(&cands).expect("something is shared");
    assert_eq!(b.gain, 0.25);
    assert_eq!(b.shared.get(&topical), Some(&0.6), "a topic");
    assert_eq!(b.shared.get(&projected), Some(&1.0), "a project");
    assert!(!b.shared.contains_key(&stranger), "nothing shared");
    assert!(
        !b.shared.contains_key(&here),
        "its own notes are not lifted"
    );
    // A shared place reads no interpretation.
    assert!(r
        .core
        .runner
        .category_scene(Some(&here), false, None)
        .is_none());
}

/// With `recall_boost = 0` the scene is none, and the persona question's
/// state carries the session's interpreted categories by path, never a
/// person's, and nothing in a shared place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_term_is_off_at_zero_and_the_persona_state_names_the_categories() {
    use crate::places::PlaceClass;
    let r = rig_with(FakeJev::start().unwrap(), Vec::new(), |c| {
        c.ontology.recall_boost = 0.0;
    });
    let sid = bound(&r.core);
    operator_writes(
        &r.core,
        vec![
            Record::Category(Category::new(id("topic:web"), "web", "ada")),
            held(&sid, &["topic:web"]),
        ],
    );
    let _ = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(r
        .core
        .runner
        .category_scene(Some(&sid), true, None)
        .is_none());
    assert_eq!(
        r.core.runner.category_names(&sid, PlaceClass::Private),
        vec!["web".to_string()]
    );
    assert!(r
        .core
        .runner
        .category_names(&sid, PlaceClass::Shared)
        .is_empty());
}

/// A picture file as a message carries it.
fn picture_file(name: &str, bytes: &[u8]) -> theseus_protocol::Attachment {
    theseus_protocol::Attachment {
        name: name.into(),
        media_type: "image/png".into(),
        size: bytes.len() as u64,
        data: Some(crate::blobs::encode(bytes)),
        ..Default::default()
    }
}

/// A session bound to the place `target` (`dm:77`): not the owner's.
fn bound_to(core: &Core, place: &str) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(place, &rec.session_id).unwrap();
    rec.session_id
}

/// The `onto.v1` question ids a request carried.
fn onto_questions(s: &theseus_judge::fake::Seen) -> Vec<String> {
    s.body["questions"]
        .as_object()
        .map(|q| {
            q.keys()
                .filter_map(|k| k.strip_prefix("onto.v1/").map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The requests the fake saw that carried a picture.
fn with_assets(jev: &FakeJev) -> Vec<theseus_judge::fake::Seen> {
    jev.seen()
        .into_iter()
        .filter(|s| s.body.get("assets").is_some())
        .collect()
}

/// KEY: a message with a GPS-tagged PNG and a text file reaches Jev through
/// the whole core: the header comes, the picture arrives re-encoded from its
/// pixels (no GPS, no chunks of metadata anywhere in the request), and the
/// state names the picture, the text file's excerpt, and no skipped row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_gps_tagged_picture_reaches_jev_without_its_gps_and_with_the_header() {
    use theseus_judge::fake::{gps_png, GPS_NEEDLE};
    let raw = gps_png(600, 400);
    assert!(raw.windows(GPS_NEEDLE.len()).any(|w| w == GPS_NEEDLE));
    let jev = FakeJev::start().unwrap();
    let r = rig(jev);
    let sid = bound(&r.core);
    let notes = theseus_protocol::Attachment {
        name: "notes.txt".into(),
        media_type: "text/plain".into(),
        size: 3_000,
        text: Some("x".repeat(3_000)),
        ..Default::default()
    };
    crate::tests_route::turn_attached(
        &r.core,
        Some(&sid),
        "what is this?",
        None,
        vec![picture_file("shot.png", &raw), notes],
    )
    .await;
    let seen = with_assets(&r.jev);
    assert_eq!(seen.len(), 1, "one request carried the picture");
    assert!(seen[0].images_header, "the images header came");
    let assets = seen[0].body["assets"].as_object().unwrap();
    assert_eq!(assets.len(), 1);
    let sent = crate::blobs::decode(
        assets.values().next().unwrap()["data_base64"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    for needle in [GPS_NEEDLE, b"GPS", b"tEXt", b"eXIf"] {
        assert!(
            !sent.windows(needle.len()).any(|w| w == needle),
            "the picture left without {needle:?}"
        );
    }
    let state = seen[0].body["state"].to_string();
    assert!(
        state.contains("shot.png") || state.contains("images"),
        "{state}"
    );
    assert!(state.contains("notes.txt"), "the text file is in the state");
    assert!(!state.contains(&"x".repeat(2_001)), "its excerpt is capped");
    assert!(!state.contains("skipped_images\":["), "{state}");
}

/// KEY: a picture in a shared place is asked about for its own sake:
/// the request carries it, but no title, membership, candidate or name of the
/// owner's goes with it, and nothing is filed from the answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_places_picture_is_asked_without_candidates_and_files_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.95);
    let r = rig(jev);
    operator_writes(
        &r.core,
        vec![Record::Category(Category::new(
            id("topic:ledger"),
            "Ledger",
            "ada",
        ))],
    );
    let sid = bound_to(&r.core, "dm:77");
    let raw = theseus_judge::fake::gps_png(500, 500);
    crate::tests_route::turn_attached(
        &r.core,
        Some(&sid),
        "what is this?",
        None,
        vec![picture_file("shot.png", &raw)],
    )
    .await;
    let seen = with_assets(&r.jev);
    assert_eq!(seen.len(), 1, "the shared place's picture is asked about");
    let state = seen[0].body["state"].to_string();
    assert!(
        !state.contains("Ledger"),
        "no category of the owner's: {state}"
    );
    let asked = onto_questions(&seen[0]);
    assert!(asked.contains(&"image_subject".to_string()), "{asked:?}");
    assert!(
        !asked.contains(&"topic".to_string()),
        "no filing question: {asked:?}"
    );
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.member_list(&sid, "topic").is_none(), "nothing filed");
    assert!(cards(&r.core).is_empty(), "no card");
}

/// A shared place's text-only message asks `onto.v1` nothing, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_places_text_message_still_asks_onto_nothing() {
    let jev = FakeJev::start().unwrap();
    let r = rig(jev);
    let sid = bound_to(&r.core, "dm:77");
    crate::tests_route::turn(&r.core, Some(&sid), SAID, None).await;
    let asked_onto = r.jev.seen().iter().any(|s| !onto_questions(s).is_empty());
    assert!(!asked_onto, "no onto question left for a shared place");
}

/// The naming call asks its own target model's forcing: on a Sonnet profile
/// it is not forced (theseus-7q9wt).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_a_sonnet_profile_the_naming_call_is_not_forced() {
    let jev = FakeJev::start().unwrap();
    let named = Scripted::tools(
        "",
        &[(
            "tu_1",
            "name_category",
            json!({"names": ["Ledger move"], "description": "Moving the ledger."}),
        )],
    );
    let r = rig_with(jev, vec![named], |c| c.serve_glm_as("claude-sonnet-5-5"));
    let sid = bound(&r.core);
    r.core.onto_name(&sid, "topic", SAID).await.unwrap();
    let sent = r.namer.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].model, "claude-sonnet-5-5");
    crate::provider::one_tool::assert_unforced(&sent[0], crate::judge::onto::name::TOOL);
}

/// The naming call forces `name_category` where thinking is off, and asks
/// `auto` with a strict, closed tool where it cannot be (theseus-yym79).
#[test]
fn the_naming_request_forces_its_tool_only_where_thinking_is_off() {
    crate::provider::one_tool::assert_both_shapes(
        |how| crate::judge::onto::name::request("m", how, "topic", "said", &|t| t.to_string()),
        crate::judge::onto::name::TOOL,
    );
}
