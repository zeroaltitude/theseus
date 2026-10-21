//! `theseus judge replay --onto` (theseus-nngor), through a whole core and
//! the fake Jev: the sessions the import labelled are the cases (newest
//! first, nothing unlabelled), Jev answers them with the labels hidden, the
//! scores come out per kind, the rows are scoped `judge.replay:onto`, the
//! run needs the owner's consent and stays under `replay_limit_usd`, and the
//! undo rates count only Jev's writes the owner took back.

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted};
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::judge_runs::{JudgeReplayParams, JudgeReplayResult};

use super::onto_undo::undo_rates;
use crate::import::{episode, session_id_of, write};
use crate::ledger::LedgerRow;
use crate::tests_judge::{texts, Rig};
use crate::tests_replay::rig;
use crate::Core;

const TAG: &str = "tern-2026-03";

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn episode_id(n: usize) -> String {
    format!("ep_{:064x}", 0x5a11_0000 + n)
}

fn sid(n: usize) -> String {
    session_id_of(&episode_id(n))
}

/// An imported session whose one operator message carries a marker the
/// scripted Jev tells it by.
fn episode_line(n: usize) -> String {
    let mut v = json!({
        "format": 1, "import_tag": TAG, "episode_id": episode_id(n),
        "source": episode::SOURCES[0], "agent": null,
        "place": {"kind": "dm", "name": "wren"},
        "as_of": {"start": format!("2026-03-0{n}T09:00:00Z"), "end": format!("2026-03-0{n}T09:20:00Z")},
        "labels": {"sensitivity": "personal", "topic": []},
        "summary": null,
        "messages": [{"idx": 0, "time": format!("2026-03-0{n}T09:00:00Z"), "author": "wren",
            "integrity": "operator", "text": format!("replay marker zz{n}zz about the tern count"),
            "unit": format!("unit-{n}"), "sha256": "cd".repeat(32)}],
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

fn write_as(core: &Core, by: Origin, records: Vec<Record>) {
    core.runner
        .ontology
        .write(&core.store, records, by, |_| Ok(vec![]))
        .unwrap();
}

/// Four imported sessions; the first three the import labelled by topic and
/// resource (`alpha`/`repo-a`, `beta`/`repo-b`, `gamma`/`repo-c`), the
/// fourth by nothing.
fn seeded(core: &Core) {
    let lines: Vec<ImportLine> = (1..=4)
        .map(|n| ImportLine {
            line: n as u64,
            text: episode_line(n),
        })
        .collect();
    let r = write::import_batch(
        &core.store,
        &ImportEpisodesParams {
            file: "terns.jsonl".into(),
            lines,
            ..Default::default()
        },
        "test",
    )
    .unwrap();
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    // The import may label a resource too, once the kinds table says so.
    let mut resource = core
        .runner
        .ontology
        .snapshot(&core.store)
        .unwrap()
        .kind("resource")
        .unwrap()
        .clone();
    resource.version += 1;
    resource.assigned_by.push(Origin::Import);
    write_as(core, Origin::Operator, vec![Record::Kind(resource)]);
    let names = [
        ("topic", ["alpha", "beta", "gamma"]),
        ("resource", ["repo-a", "repo-b", "repo-c"]),
    ];
    for (kind, ns) in names {
        let cats = ns
            .iter()
            .map(|n| Record::Category(Category::new(id(&format!("{kind}:{n}")), *n, "ada")))
            .collect();
        write_as(core, Origin::Operator, cats);
        let lists = ns
            .iter()
            .enumerate()
            .map(|(i, n)| {
                Record::Members(MemberList {
                    session: sid(i + 1),
                    kind: kind.into(),
                    members: vec![Membership::import(id(&format!("{kind}:{n}")), 1_000)],
                })
            })
            .collect();
        write_as(core, Origin::Import, lists);
    }
}

fn consenting(c: &mut crate::Config) {
    c.judge.backfill_consent = true;
    c.judge.replay_limit_usd = 5.0;
}

fn onto_rig(jev: &FakeJev, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = rig(texts(0), jev, tweak);
    seeded(&r.core);
    r
}

fn params() -> JudgeReplayParams {
    JudgeReplayParams {
        onto: true,
        ..Default::default()
    }
}

/// Scripts the model to answer each case's kinds with the import's labels.
fn script(jev: &FakeJev, core: &Core) {
    for (n, (topic, resource)) in [("alpha", "repo-a"), ("beta", "repo-b"), ("gamma", "repo-c")]
        .into_iter()
        .enumerate()
    {
        let case = core
            .onto_cases(50)
            .unwrap()
            .cases
            .into_iter()
            .find(|c| c.session == sid(n + 1))
            .unwrap();
        for (kind, label) in [("topic", topic), ("resource", resource)] {
            // The options as Jev is shown them: the label is not the session's own.
            let mut o = case.input.onto.as_ref().unwrap().clone();
            for item in o.kinds.iter_mut().flat_map(|k| k.pool.iter_mut()) {
                item.session = false;
            }
            let k = o.kinds.iter().find(|k| k.kind == kind).unwrap();
            let at = k
                .pick(&case.input.message)
                .candidates
                .iter()
                .position(|i| i.id == format!("{kind}:{label}"))
                .unwrap();
            jev.script_when(
                &format!("zz{}zz", n + 1),
                None,
                &format!("onto.v1/{kind}"),
                Scripted::Choice {
                    option: format!("c{}", at + 1),
                    confidence: 0.9,
                },
            );
        }
    }
}

fn by_kind(rows: &[theseus_protocol::judge_runs::OntoKindScore], kind: &str) -> Vec<f64> {
    let k = rows.iter().find(|k| k.kind == kind).unwrap();
    vec![k.precision.unwrap(), k.recall.unwrap(), f64::from(k.hits)]
}

/// The key test: a store with three sessions the import labelled by topic
/// and resource and one it labelled by nothing replays through `onto.v1`
/// with Jev scripted to answer the labels. Precision and recall are 1.0 for
/// both kinds, the unlabelled session is no case, the state Jev was sent
/// never names a label as the session's membership, and the rows are scoped
/// `judge.replay:onto`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_import_labelled_store_replays_through_onto() {
    let jev = FakeJev::start().unwrap();
    let r = onto_rig(&jev, consenting);
    let c = &r.core;
    let cases = c.onto_cases(50).unwrap();
    let held: Vec<&str> = cases.cases.iter().map(|k| k.session.as_str()).collect();
    assert_eq!(held.len(), 3, "{held:?}");
    assert!(
        !held.contains(&sid(4).as_str()),
        "the unlabelled session is no case"
    );
    assert!(cases.left_out.is_empty());
    script(&jev, c);
    let out: JudgeReplayResult = c.judge_replay(params(), "cli").await.unwrap();
    let onto = out.onto.expect("an onto replay's numbers");
    assert_eq!((onto.sessions, onto.unanswered, onto.left_out), (3, 0, 0));
    for rows in [&onto.at_act, &onto.at_confirm] {
        for kind in ["topic", "resource"] {
            assert_eq!(by_kind(rows, kind), [1.0, 1.0, 3.0], "{kind}");
        }
    }
    assert!(onto
        .markdown
        .contains("| topic | 3 | 3 | 3 | 3 | 0 | 0 | 0 | 1.000 | 1.000 |"));
    // What Jev was sent: the marker's sessions, and no membership of theirs.
    let seen = jev.seen();
    assert_eq!(seen.len(), 3);
    for s in &seen {
        let state = s.body.to_string();
        assert!(state.contains("replay marker"), "{state}");
        assert!(
            !state.contains("memberships"),
            "a label reached Jev: {state}"
        );
    }
    // The rows: one judge.call each and the run, all scoped to onto.
    let rows: Vec<LedgerRow> = c
        .store
        .scope_after("judge.replay:onto", 0)
        .unwrap()
        .iter()
        .filter_map(|r| r.decode().ok())
        .collect();
    let calls: Vec<&LedgerRow> = rows.iter().filter(|r| r.kind == "judge.call").collect();
    assert_eq!(calls.len(), 3);
    for row in &calls {
        assert_eq!(row.data["context"]["purpose"], "replay");
        assert_eq!(row.data["context"]["run"], out.id);
    }
    assert_eq!(rows.iter().filter(|r| r.kind == "judge.replay").count(), 1);
}

/// A replay's score does not move when the labels are changed to Jev's:
/// the cases' labels are the import's memberships alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_labels_are_the_imports_memberships_alone() {
    let jev = FakeJev::start().unwrap();
    let r = onto_rig(&jev, consenting);
    let c = &r.core;
    // Jev's own membership on the first session is no label.
    write_as(
        c,
        Origin::Jev,
        vec![Record::Members(MemberList {
            session: sid(1),
            kind: "topic".into(),
            members: vec![
                Membership::import(id("topic:alpha"), 1_000),
                Membership {
                    category: id("topic:beta"),
                    origin: Origin::Jev,
                    confidence: Some(0.9),
                    as_of_ms: 2_000,
                },
            ],
        })],
    );
    let cases = c.onto_cases(50).unwrap().cases;
    let one = cases.iter().find(|k| k.session == sid(1)).unwrap();
    assert_eq!(
        one.labels["topic"].iter().collect::<Vec<_>>(),
        ["topic:alpha"],
        "only the import's membership is a label"
    );
}

/// `--limit` keeps the newest sessions.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_limit_keeps_the_newest_labelled_sessions() {
    let jev = FakeJev::start().unwrap();
    let r = onto_rig(&jev, consenting);
    let cases = r.core.onto_cases(2).unwrap().cases;
    assert_eq!(cases.len(), 2);
}

/// The replay sends the owner's history to Jev, so without
/// `backfill_consent` nothing is sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_onto_replay_is_refused_without_backfill_consent() {
    let jev = FakeJev::start().unwrap();
    let r = onto_rig(&jev, |c| c.judge.replay_limit_usd = 5.0);
    let err = r.core.judge_replay(params(), "cli").await.unwrap_err();
    assert!(format!("{err:#}").contains("backfill_consent"), "{err:#}");
    assert!(jev.seen().is_empty(), "a call was made");
}

/// An estimate over `replay_limit_usd` is refused with the numbers, before
/// any call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_onto_replay_is_refused_over_replay_limit_usd() {
    let jev = FakeJev::start().unwrap();
    let r = onto_rig(&jev, |c| {
        c.judge.backfill_consent = true;
        c.judge.replay_limit_usd = 0.000_001;
    });
    let err = r.core.judge_replay(params(), "cli").await.unwrap_err();
    let said = format!("{err:#}");
    assert!(
        said.contains("replay_limit_usd") && said.contains("would reserve"),
        "{said}"
    );
    assert!(jev.seen().is_empty(), "a call was made");
}

fn row(kind: &str, at: u64, data: Value) -> LedgerRow {
    LedgerRow {
        at_unix_ms: at,
        kind: kind.into(),
        session_id: None,
        turn_id: None,
        data,
    }
}

fn write_row(kind: &str, at: u64, of: &str, record_kind: &str) -> LedgerRow {
    row(
        kind,
        at,
        json!({"kind": record_kind, "undo": {"write": of, "key": "k", "wrote": null, "before": null}}),
    )
}

/// Jev's writes of two kinds, one undone; a sweep's write that was undone
/// (not Jev's); and a write before the window: only Jev's, in the window.
#[test]
fn undo_rates_count_the_owners_undos_per_kind() {
    let now = 10_000_000;
    let rows = vec![
        // jdg_a filed a topic (a category and a membership: one write).
        write_row("ontology.category", now, "jdg_a", "topic"),
        write_row("ontology.membership", now, "jdg_a", "topic"),
        write_row("ontology.membership", now, "jdg_b", "resource"),
        write_row("ontology.membership", now, "jdg_c", "topic"),
        // The sweep's decay, undone: not Jev's.
        write_row("ontology.membership", now, "sweep-1-0-decay", "topic"),
        // Before the window.
        write_row("ontology.membership", 10, "jdg_old", "topic"),
        row(
            "ontology.undo",
            now + 1,
            json!({"write": "jdg_c", "label": "wrong"}),
        ),
        row(
            "ontology.undo",
            now + 1,
            json!({"write": "sweep-1-0-decay", "label": "wrong"}),
        ),
        row(
            "ontology.undo",
            now + 1,
            json!({"write": "jdg_old", "label": "wrong"}),
        ),
    ];
    let rates = undo_rates(&rows, 1_000);
    let of = |k: &str| {
        let r = rates.iter().find(|r| r.kind == k).unwrap();
        (r.writes, r.undone, r.rate)
    };
    assert_eq!(of("topic"), (2, 1, Some(0.5)));
    assert_eq!(of("resource"), (1, 0, Some(0.0)));
    assert_eq!(rates.len(), 2);
}
