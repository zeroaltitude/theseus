//! The sweep asks Jev about its own categories (theseus-nzzz4), through a
//! whole core and the fake Jev: two topics that look alike merge when Jev
//! calls them one thing, a large topic splits in two named children, each
//! write is one undo, and the day's cap holds.

use std::sync::Arc;

use theseus_judge::fake::{FakeJev, Scripted};
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Ontology, Origin, Record};
use theseus_protocol::{now_unix_ms, SessionKind};

use crate::judge::onto::judged::{Mark, MARK};
use crate::provider::FakeProvider;
use crate::rpc::Parts;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_judge::{board, judge_config, texts, warm};
use crate::{Config, Core};

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) jev: FakeJev,
    _dir: tempfile::TempDir,
}

pub(crate) fn rig(jev: FakeJev, more: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = judge_config(dir.path(), Some(&jev));
    // A pin: an unset naming key asks Jev for its model too, which these tests
    // count the asks of.
    cfg.ontology.name_profile = Some("haiku".into());
    more(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(2))), store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    warm(&core);
    Rig {
        core,
        jev,
        _dir: dir,
    }
}

pub(crate) fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

pub(crate) fn session(core: &Core, sid: &str, title: &str, active: u64) {
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.session_id = sid.into();
    rec.title = Some(title.into());
    rec.last_active_ms = active;
    core.store.put_session(sid, &rec).unwrap();
}

pub(crate) fn held(sid: &str, category: &str, at: u64) -> Record {
    Record::Members(MemberList {
        session: sid.into(),
        kind: "topic".into(),
        members: vec![Membership {
            category: id(category),
            origin: Origin::Jev,
            confidence: Some(0.9),
            as_of_ms: at,
        }],
    })
}

pub(crate) fn machine(core: &Core, write: &str, records: Vec<Record>) {
    core.ontology_machine_write(Origin::Jev, write, records)
        .unwrap();
}

pub(crate) fn topic(i: &str, name: &str) -> Record {
    Record::Category(Category::new(id(i), name, "jev:j-0001"))
}

pub(crate) fn snapshot(core: &Core) -> Arc<Ontology> {
    core.runner.ontology.snapshot(&core.store).unwrap()
}

pub(crate) fn fresh() -> u64 {
    now_unix_ms()
}

/// Two machine topics that look alike: `release process` (one session) and
/// `release checklist` (two).
fn plant_pair(core: &Core) {
    let at = fresh();
    session(core, "s1", "How we release the ledger", at);
    session(core, "s2", "Release checklist for the ledger", at);
    session(core, "s3", "Tag and publish 0.5", at);
    machine(
        core,
        "j-0001",
        vec![
            topic("topic:release-process", "release process"),
            topic("topic:release-checklist", "release checklist"),
            held("s1", "topic:release-process", at),
            held("s2", "topic:release-checklist", at),
            held("s3", "topic:release-checklist", at),
        ],
    );
}

/// KEY: Jev calls the pair one thing at 0.9: one merge write, the survivor
/// the better held, the memberships moved, and `ontology_undo` restores the
/// ontology equal to before. At 0.7 nothing is written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_merges_a_pair_jev_calls_the_same_thing() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_thing", Scripted::Noul(0.9));
    let r = rig(jev, |_| {});
    plant_pair(&r.core);
    let before = snapshot(&r.core);
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    assert_eq!(swept.merged, 0, "no two names fold to one");
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((swept.judged_merges, swept.failed), (1, 0));
    assert_eq!(swept.writes.len(), 1);
    assert!(swept.writes[0].ends_with("-merge"), "{:?}", swept.writes);
    let o = snapshot(&r.core);
    assert!(o.category(&id("topic:release-process")).is_none());
    let moved = o.member_list("s1", "topic").unwrap();
    assert_eq!(moved.members[0].category, id("topic:release-checklist"));
    assert_eq!(moved.members[0].origin, Origin::Sweep);
    assert_eq!(r.jev.seen().len(), 1, "one pair, one call");

    r.core
        .ontology_undo(&swept.writes[0], "test", "test")
        .unwrap();
    assert_eq!(*snapshot(&r.core), *before);

    // Asked once: the next run, a month's worth of nights, asks nothing.
    let mut again = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut again).await.unwrap();
    assert_eq!((again.judged_merges, r.jev.seen().len()), (0, 1));

    // A pair answered at 0.7, the confirm band, writes nothing.
    let jev = FakeJev::start().unwrap();
    jev.script("same_thing", Scripted::Noul(0.7));
    let r = rig(jev, |_| {});
    plant_pair(&r.core);
    let before = snapshot(&r.core);
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((swept.judged_merges, swept.writes.len()), (0, 0));
    assert_eq!(*snapshot(&r.core), *before);
    assert_eq!(r.jev.seen().len(), 1, "it was asked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pair_answered_below_the_act_band_writes_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_thing", Scripted::Noul(0.7));
    let r = rig(jev, |_| {});
    plant_pair(&r.core);
    let before = snapshot(&r.core);
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((swept.judged_merges, swept.writes.len()), (0, 0));
    assert_eq!(*snapshot(&r.core), *before);
    assert_eq!(r.jev.seen().len(), 1, "it was asked");
}

/// A pair the operator's category is in is never asked, and a pair whose
/// names are not alike is not either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_machine_pairs_that_look_alike_are_asked() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_thing", Scripted::Noul(0.9));
    let r = rig(jev, |_| {});
    let at = fresh();
    machine(
        &r.core,
        "j-0001",
        vec![
            topic("topic:release-process", "release process"),
            topic("topic:garden-notes", "garden notes"),
            held("s1", "topic:release-process", at),
        ],
    );
    r.core
        .runner
        .ontology
        .write(
            &r.core.store,
            vec![Record::Category(Category::new(
                id("topic:release-checklist"),
                "release checklist",
                "operator",
            ))],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    let mut swept = r.core.onto_sweep(at).unwrap();
    r.core.onto_sweep_judged(at, &mut swept).await.unwrap();
    assert_eq!(r.jev.seen().len(), 0, "{:?}", swept);
}

/// The cap: with what the day's mark leaves under a call's worst case, the
/// call is not begun; with the day's cap whole, it is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_judged_sweep_stops_at_its_day_cap() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_thing", Scripted::Noul(0.9));
    let r = rig(jev, |c| c.ontology.sweep_usd_per_day = 1.0);
    plant_pair(&r.core);
    let today = crate::judge::spend::local_day(fresh());
    // The day has spent all but a millionth of a dollar.
    let mark = Mark {
        day: today,
        spent_usd: 1.0 - 1e-9,
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &mark).unwrap();
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!(r.jev.seen().len(), 0, "the call was not begun");
    assert_eq!(swept.judged_merges, 0);

    // The next day, the cap is whole.
    let tomorrow = fresh() + 36 * 3_600_000;
    let mut next = r.core.onto_sweep(tomorrow).unwrap();
    r.core.onto_sweep_judged(tomorrow, &mut next).await.unwrap();
    assert_eq!(r.jev.seen().len(), 1);
    assert_eq!(next.judged_merges, 1);
    // And what it spent is kept where a restart reads it back, on its day.
    let kept: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert_eq!(kept.day, crate::judge::spend::local_day(tomorrow));
    assert!(kept.spent_usd > 0.0, "{kept:?}");
}

/// `sweep_usd_per_day = 0` leaves the deterministic sweep as it was: the same
/// writes, the same ontology, and no question asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sweep_usd_per_day_zero_leaves_the_deterministic_sweep() {
    let run = |cap: f64| async move {
        let jev = FakeJev::start().unwrap();
        jev.script("same_thing", Scripted::Noul(0.9));
        let r = rig(jev, move |c| c.ontology.sweep_usd_per_day = cap);
        plant_pair(&r.core);
        // A deterministic twin too: two spellings of one name.
        let at = fresh();
        machine(
            &r.core,
            "j-0002",
            vec![
                topic("topic:rust-harness", "Rust Harness"),
                topic("topic:rust-harness-2", "rust-harness"),
                held("s4", "topic:rust-harness-2", at),
            ],
        );
        let mut swept = r.core.onto_sweep(at).unwrap();
        r.core.onto_sweep_judged(at, &mut swept).await.unwrap();
        (swept, (*snapshot(&r.core)).clone(), r.jev.seen().len())
    };
    let (zero, zero_o, asked) = run(0.0).await;
    assert_eq!(asked, 0, "the model's part is off");
    assert_eq!((zero.merged, zero.judged_merges), (1, 0));
    // The old run: the deterministic sweep alone, no judge at all.
    let alone = {
        let jev = FakeJev::start().unwrap();
        let r = rig(jev, |_| {});
        plant_pair(&r.core);
        machine(
            &r.core,
            "j-0002",
            vec![
                topic("topic:rust-harness", "Rust Harness"),
                topic("topic:rust-harness-2", "rust-harness"),
                held("s4", "topic:rust-harness-2", fresh()),
            ],
        );
        let swept = r.core.onto_sweep(fresh()).unwrap();
        (swept, (*snapshot(&r.core)).clone())
    };
    assert_eq!(zero.merged, alone.0.merged);
    assert_eq!(zero_o.records().len(), alone.1.records().len());
    assert!(zero.writes.iter().all(|w| w.ends_with("-merge")));
}

const SIX_AND_SIX: [(&str, &str); 12] = [
    ("a1", "Plan the Ledger migration"),
    ("a2", "Fix the Ledger import"),
    ("a3", "Ledger schema review"),
    ("a4", "Ledger backfill dry run"),
    ("a5", "Reconcile the Ledger totals"),
    ("a6", "Ledger audit trail"),
    ("b1", "Design the Beacon dashboard"),
    ("b2", "Beacon alert thresholds"),
    ("b3", "Dashboard colors for Beacon"),
    ("b4", "Beacon on-call page"),
    ("b5", "Beacon export to CSV"),
    ("b6", "Beacon mobile layout"),
];

/// A topic of twelve sessions, six about one thing and six about another.
fn plant_large(core: &Core, by: &str) {
    let at = fresh();
    let mut records = vec![Record::Category(Category::new(
        id("topic:lantern"),
        "lantern",
        by,
    ))];
    for (n, (sid, title)) in SIX_AND_SIX.iter().enumerate() {
        // The first listed is the newest, so session 1 is about the Ledger.
        session(core, sid, title, at - n as u64);
        records.push(held(sid, "topic:lantern", at));
    }
    if by.starts_with("jev") {
        machine(core, "j-0003", records);
    } else {
        core.runner
            .ontology
            .write(&core.store, records, Origin::Operator, |_| Ok(vec![]))
            .unwrap();
    }
}

/// The names `first_name` and `second_name` choose among, as the state
/// offers them.
fn offered() -> Vec<String> {
    let input = theseus_judge::builders::TwoThingsInput {
        kind: "topic".into(),
        category: theseus_judge::builders::SweepCategory {
            name: "lantern".into(),
            path: "lantern".into(),
            description: String::new(),
            titles: vec![],
        },
        sessions: SIX_AND_SIX
            .iter()
            .map(|(i, t)| (i.to_string(), t.to_string()))
            .collect(),
    };
    input.names()
}

fn script_split(jev: &FakeJev, first: &str, second: &str) {
    jev.script("two_things", Scripted::Noul(0.9));
    jev.script(
        "first_name",
        Scripted::Choice {
            option: first.into(),
            confidence: 0.9,
        },
    );
    jev.script(
        "second_name",
        Scripted::Choice {
            option: second.into(),
            confidence: 0.9,
        },
    );
    // The first six sampled sessions are about the first thing, the rest not.
    jev.script("in_first", Scripted::Noul(0.95));
    for n in 7..=10 {
        jev.script(&format!("in_first.{n}"), Scripted::Noul(0.05));
    }
    for n in 1..=2 {
        jev.script(&format!("in_first_more.{n}"), Scripted::Noul(0.05));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_topic_splits_into_two_named_children() {
    let names = offered();
    let first = names.iter().position(|n| n == "Ledger").expect("Ledger") + 1;
    let second = names.iter().position(|n| n == "Beacon").expect("Beacon") + 1;
    let jev = FakeJev::start().unwrap();
    script_split(&jev, &format!("n{first}"), &format!("n{second}"));
    let r = rig(jev, |_| {});
    plant_large(&r.core, "jev:j-0003");
    let before = snapshot(&r.core);
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((swept.splits, swept.failed), (1, 0));
    let write = swept.writes.last().unwrap().clone();
    assert!(write.ends_with("-split"), "{write}");
    let o = snapshot(&r.core);
    let kids: Vec<_> = o.children(Some(&id("topic:lantern"))).into_iter().collect();
    let mut names: Vec<&str> = kids.iter().map(|c| c.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Beacon", "Ledger"]);
    let child = |name: &str| kids.iter().find(|c| c.name == name).unwrap().id.clone();
    for (sid, _) in &SIX_AND_SIX[..6] {
        let l = o.member_list(sid, "topic").unwrap();
        assert_eq!(l.members[0].category, child("Ledger"), "{sid}");
    }
    for (sid, _) in &SIX_AND_SIX[6..] {
        let l = o.member_list(sid, "topic").unwrap();
        assert_eq!(l.members[0].category, child("Beacon"), "{sid}");
    }
    // One write, one undo.
    r.core.ontology_undo(&write, "test", "test").unwrap();
    assert_eq!(*snapshot(&r.core), *before);
}

/// Two names that are one name, or one that is `none`, write nothing; and a
/// topic the operator wrote is never asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_split_is_refused_when_the_names_are_equal_or_one_is_none() {
    let names = offered();
    let first = names.iter().position(|n| n == "Ledger").unwrap() + 1;
    for (a, b) in [
        (format!("n{first}"), format!("n{first}")),
        (format!("n{first}"), "none".to_string()),
    ] {
        let jev = FakeJev::start().unwrap();
        script_split(&jev, &a, &b);
        let r = rig(jev, |_| {});
        plant_large(&r.core, "jev:j-0003");
        let before = snapshot(&r.core);
        let mut swept = r.core.onto_sweep(fresh()).unwrap();
        r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
        assert_eq!((swept.splits, swept.writes.len()), (0, 0), "{a} {b}");
        assert_eq!(*snapshot(&r.core), *before);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_operators_large_topic_is_never_split() {
    let jev = FakeJev::start().unwrap();
    script_split(&jev, "n1", "n2");
    let r = rig(jev, |_| {});
    plant_large(&r.core, "operator");
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((r.jev.seen().len(), swept.splits), (0, 0));
}

/// A session answered below the act band stays in the parent, and a child
/// Jev put no session in is not made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_answered_below_the_act_band_stays_in_the_parent() {
    let names = offered();
    let first = names.iter().position(|n| n == "Ledger").unwrap() + 1;
    let second = names.iter().position(|n| n == "Beacon").unwrap() + 1;
    let jev = FakeJev::start().unwrap();
    script_split(&jev, &format!("n{first}"), &format!("n{second}"));
    // Session 3 is a maybe.
    jev.script("in_first.3", Scripted::Noul(0.7));
    let r = rig(jev, |_| {});
    plant_large(&r.core, "jev:j-0003");
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!(swept.splits, 1);
    let o = snapshot(&r.core);
    let l = o.member_list("a3", "topic").unwrap();
    assert_eq!(l.members[0].category, id("topic:lantern"));
}

#[test]
fn the_packs_names_are_mined_from_the_titles() {
    let names = offered();
    assert!(names.contains(&"Ledger".to_string()), "{names:?}");
    assert!(names.contains(&"Beacon".to_string()), "{names:?}");
}

/// The sweep's mark is written after each paid question (theseus-23wd8): a
/// run stopped while its second question waits (the daemon's stop drops the
/// run's future) keeps the first question's spend and its asked key, so the
/// next run neither spends it again nor asks it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_mark_survives_a_stop() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_thing", Scripted::Noul(0.7));
    let r = rig(jev, |_| {});
    plant_pair(&r.core);
    let at = fresh();
    session(&r.core, "g1", "Bed rotation plan", at);
    machine(
        &r.core,
        "j-0002",
        vec![
            topic("topic:garden-notes", "garden notes"),
            topic("topic:garden-notebook", "garden notebook"),
            held("g1", "topic:garden-notes", at),
        ],
    );
    let o = snapshot(&r.core);
    let pairs = o.merge_pairs();
    assert_eq!(pairs.len(), 2, "{pairs:?}");
    // The second question waits until the test lets it go.
    r.jev.hold_when(&o.category(&pairs[1].0).unwrap().name);
    let core = r.core.clone();
    let run = tokio::spawn(async move {
        let mut swept = core.onto_sweep(fresh()).unwrap();
        core.onto_sweep_judged(fresh(), &mut swept).await
    });
    let waited = std::time::Instant::now();
    while r.jev.seen().len() < 2 {
        assert!(
            waited.elapsed().as_secs() < 30,
            "the second question never came"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    run.abort();
    assert!(run.await.unwrap_err().is_cancelled());
    r.jev.release();
    let kept: Mark = r
        .core
        .store
        .get_meta(MARK)
        .unwrap()
        .expect("the mark was written before the stop");
    let first = format!("pair {} {}", pairs[0].0, pairs[0].1);
    assert_eq!(kept.asked.keys().collect::<Vec<_>>(), [&first]);
    assert!(kept.spent_usd > 0.0, "{kept:?}");
}
