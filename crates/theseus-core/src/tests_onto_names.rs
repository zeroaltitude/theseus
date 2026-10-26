//! The sweep's questions about names (theseus-63tfk.1), through a whole core
//! and the fake Jev: a stale category is renamed by Jev's pick and undone,
//! `other` answers under two names that mean one kind cluster and mint, the
//! kind is named by Jev's choice or by the most shared name, and the day's
//! money stops all three.

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted};
use theseus_protocol::LedgerKind;

use crate::judge::onto::judged::{Mark, MARK};
use crate::judge::onto::renamed::stale_names;
use crate::ledger::LedgerRow;
use crate::tests_onto_judged::{fresh, held, id, machine, rig, session, snapshot, topic, Rig};

const TITLES: [&str; 6] = [
    "Plan the Ledger migration",
    "Fix the Ledger import",
    "Ledger schema review",
    "Ledger backfill dry run",
    "Reconcile the Ledger totals",
    "Ledger audit trail",
];

/// A machine topic `scratch notes` whose six sessions all talk about the
/// Ledger: its name is in none of their titles.
fn plant_stale(core: &crate::Core) {
    let at = fresh();
    let mut records = vec![topic("topic:scratch-notes", "scratch notes")];
    for (n, title) in TITLES.iter().enumerate() {
        let sid = format!("r{n}");
        session(core, &sid, title, at);
        records.push(held(&sid, "topic:scratch-notes", at));
    }
    machine(core, "j-0001", records);
}

fn pick(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "name",
        Scripted::Choice {
            option: option.into(),
            confidence,
        },
    );
}

/// The option that names `word` among what a stale `scratch notes` offers.
fn option_for(word: &str) -> String {
    let names = stale_names("scratch notes", &TITLES).expect("the name is stale");
    let at = names.iter().position(|n| n.contains(word)).unwrap();
    format!("n{}", at + 1)
}

/// KEY: Jev picks `Ledger` at 0.9: the category is renamed, its old name is
/// in its description, and the undo puts both back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_category_is_renamed_by_jevs_pick_and_undone() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, &option_for("Ledger"), 0.9);
    let r = rig(jev, |_| {});
    plant_stale(&r.core);
    let before = snapshot(&r.core);
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((swept.renamed, swept.failed), (1, 0), "{swept:?}");
    assert!(swept.writes[0].ends_with("-rename"), "{:?}", swept.writes);
    let o = snapshot(&r.core);
    let c = o.category(&id("topic:scratch-notes")).unwrap();
    assert_eq!(c.name, "Ledger");
    assert_eq!(c.description, "was: scratch notes");
    assert_eq!(r.jev.seen().len(), 1);

    r.core
        .ontology_undo(&swept.writes[0], "test", "test")
        .unwrap();
    assert_eq!(*snapshot(&r.core), *before);

    // Asked once: a later night asks nothing more about it.
    let mut again = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut again).await.unwrap();
    assert_eq!(r.jev.seen().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_below_the_act_band_writes_nothing() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, &option_for("Ledger"), 0.7);
    let r = rig(jev, |_| {});
    plant_stale(&r.core);
    let before = snapshot(&r.core);
    let mut swept = r.core.onto_sweep(fresh()).unwrap();
    r.core.onto_sweep_judged(fresh(), &mut swept).await.unwrap();
    assert_eq!((swept.renamed, swept.writes.len()), (0, 0));
    assert_eq!(r.jev.seen().len(), 1, "it was asked");
    assert_eq!(*snapshot(&r.core), *before);
}

#[test]
fn a_name_in_the_titles_or_too_few_sessions_is_not_stale() {
    assert!(
        stale_names("Ledger", &TITLES).is_none(),
        "the name is in the titles"
    );
    assert!(
        stale_names("scratch notes", &TITLES[..4]).is_none(),
        "too few"
    );
    let mixed = [
        "Plan the Ledger migration",
        "Bake bread",
        "Beacon alerts",
        "Garden beds",
        "Tax forms",
        "Voice echo",
    ];
    assert!(
        stale_names("scratch notes", &mixed).is_none(),
        "no name is shared by half"
    );
}

/// `other` under `names`, said by `session`.
fn said_other(r: &Rig, session: &str, names: &[&str]) {
    let data = json!({"judgment": format!("jdg_{session}"), "steps": [
        {"kind": "kind_of_thing", "outcome": "other", "names": names, "confidence": 0.9}]});
    let row = LedgerRow::new(LedgerKind::OntoFiled, Some(session), None, data);
    r.core.store.append_ledger(&row).unwrap();
}

/// Two sessions say `Lantern Ledger`, one says `Ledger App`: no name has
/// three sessions.
fn plant_other(r: &Rig) {
    said_other(r, "o1", &["Lantern Ledger"]);
    said_other(r, "o2", &["Lantern Ledger"]);
    said_other(r, "o3", &["Ledger App"]);
}

fn kind_jev(same: f64) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script("same_kind", Scripted::Noul(same));
    jev
}

/// KEY: Jev calls the two names one kind: three sessions cluster and one kind
/// is minted, with both names among its examples. Without the yes, none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_other_names_jev_calls_one_kind_cluster_and_mint() {
    let jev = kind_jev(0.9);
    pick(&jev, "none", 0.9);
    let r = rig(jev, |_| {});
    plant_other(&r);
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.failed), (1, 0), "{swept:?}");
    let o = snapshot(&r.core);
    let kind = o
        .kind("lantern-ledger")
        .expect("named by the most shared name");
    assert!(
        kind.examples.contains(&"Ledger App".to_string()),
        "{:?}",
        kind.examples
    );
    assert_eq!(o.lists().filter(|l| l.kind == "lantern-ledger").count(), 3);

    let no = rig(kind_jev(0.7), |_| {});
    plant_other(&no);
    let swept = no.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!(swept.kinds, 0, "{swept:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_minted_kind_is_named_by_jevs_choice() {
    let jev = kind_jev(0.9);
    // The names are offered most shared first: `Ledger App` is the second.
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    plant_other(&r);
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.named_kinds), (1, 1), "{swept:?}");
    let o = snapshot(&r.core);
    assert!(o.kind("ledger-app").is_some(), "Jev's pick names the kind");
    assert!(o.kind("lantern-ledger").is_none());
    let kind = o.kind("ledger-app").unwrap();
    assert_eq!(kind.examples[0], "Ledger App");
    assert!(kind.examples.contains(&"Lantern Ledger".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_pick_the_most_shared_name_names_it() {
    for (option, confidence) in [("none", 0.9), ("n2", 0.7)] {
        let jev = kind_jev(0.9);
        pick(&jev, option, confidence);
        let r = rig(jev, |_| {});
        plant_other(&r);
        let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
        assert_eq!(
            (swept.kinds, swept.named_kinds),
            (1, 0),
            "{option} {swept:?}"
        );
        let o = snapshot(&r.core);
        assert!(
            o.kind("lantern-ledger").is_some(),
            "{option}: the most shared name"
        );
    }
}

/// With the day's money spent to a millionth, none of the three asks is
/// begun; the next day all three are.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweeps_new_asks_stop_at_the_days_money() {
    let jev = kind_jev(0.9);
    pick(&jev, "n1", 0.9);
    let r = rig(jev, |c| c.ontology.sweep_usd_per_day = 1.0);
    plant_stale(&r.core);
    plant_other(&r);
    let today = crate::judge::spend::local_day(fresh());
    let spent = Mark {
        day: today,
        spent_usd: 1.0 - 1e-9,
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &spent).unwrap();
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!(r.jev.seen().len(), 0, "no call was begun: {swept:?}");
    assert_eq!((swept.kinds, swept.renamed), (0, 0));

    let tomorrow = fresh() + 36 * 3_600_000;
    let next = r.core.onto_sweep_night(tomorrow).await.unwrap();
    // The pair, the kind's name and the rename.
    assert_eq!(r.jev.seen().len(), 3, "{next:?}");
    assert_eq!((next.kinds, next.renamed), (1, 1));
}

/// A name Jev picked for a cluster is kept under its key when its kind does
/// not mint that night (`2026-…` is no kind's name), as long as the ask is
/// not asked again; a later night's cluster uses the kept pick, and asks no
/// kind name (only its pair; theseus-e3pxa).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kind_name_answer_waits_for_its_mint() {
    let jev = kind_jev(0.9);
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    said_other(&r, "o1", &["2026 Plan"]);
    said_other(&r, "o2", &["2026 Plan"]);
    said_other(&r, "o3", &["2026 Roadmap"]);
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.refused), (0, 1), "{swept:?}");
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert!(mark.asked.contains_key("kind-name 2026plan"), "{mark:?}");
    assert_eq!(mark.named["2026plan"], "2026 Roadmap");

    // The cluster mints later, under a name the kind can take.
    let r = rig(FakeJev::start().unwrap(), |_| {});
    plant_other(&r);
    let kept = Mark {
        day: crate::judge::spend::local_day(fresh()),
        asked: [("kind-name lanternledger".to_string(), fresh())].into(),
        named: [("lanternledger".to_string(), "Ledger App".to_string())].into(),
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &kept).unwrap();
    r.jev.script("same_kind", Scripted::Noul(0.9));
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.named_kinds), (1, 1), "{swept:?}");
    assert!(snapshot(&r.core).kind("ledger-app").is_some());
    assert_eq!(r.jev.seen().len(), 1, "only the pair was asked");
}

/// The state of the fake's request for the pair `Echo Ledger`, `Foxtrot Ledger`.
const PAIR_EF: &str = r#"["Echo Ledger"],"sessions":[]},"second":{"names":["Foxtrot Ledger"]"#;

/// Seven names one session each make 21 pairs and a run asks 20: the pair
/// `Foxtrot Ledger ~ Golf Ledger` waits for the next night. Night 1 says yes
/// to `Echo ~ Foxtrot` only and night 2 to `Foxtrot ~ Golf`: the kept yes
/// joins them, and the three sessions mint one kind (theseus-3327o).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chain_asked_over_two_nights_clusters() {
    let jev = kind_jev(0.1);
    pick(&jev, "none", 0.9);
    jev.script_when(PAIR_EF, None, "same_kind", Scripted::Noul(0.9));
    let r = rig(jev, |_| {});
    for (n, name) in [
        "Alpha", "Beta", "Charlie", "Delta", "Echo", "Foxtrot", "Golf",
    ]
    .iter()
    .enumerate()
    {
        said_other(&r, &format!("o{n}"), &[&format!("{name} Ledger")]);
    }
    let one = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((one.kinds, r.jev.seen().len()), (0, 20), "{one:?}");
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    let pair = ("echoledger".to_string(), "foxtrotledger".to_string());
    assert_eq!(mark.same, [pair].into(), "{mark:?}");

    r.jev.script("same_kind", Scripted::Noul(0.9));
    let two = r
        .core
        .onto_sweep_night(fresh() + 36 * 3_600_000)
        .await
        .unwrap();
    assert_eq!(two.kinds, 1, "{two:?}");
    let o = snapshot(&r.core);
    let made = o
        .kinds()
        .into_iter()
        .filter(|k| k.name.ends_with("-ledger"));
    assert_eq!(made.count(), 1);
    assert_eq!(r.jev.seen().len(), 22, "the pair, and the kind's name");
}

/// The chain test on a machine that says it is busy (theseus-75t4q): the sweep
/// waits out the host's CPU or IO pressure between two questions
/// (`judged::pace`), up to 10 s each, and the test asks 20 pairs on its first
/// night: 190 s on a loaded host, past nextest's 120 s kill, 1 run in 4 on the
/// owner's machine. The test runs again in namespaces with fake pressure files
/// saying IO is busy (as `tests_activation_pace` does), so the host's own
/// load is no input; where namespaces can't be made it says so and passes.
#[test]
fn a_chain_clusters_on_a_busy_machine() {
    let name = "tests_onto_names::a_chain_asked_over_two_nights_clusters";
    let Some(text) = crate::tests_activation_pace::namespaced(name, "test tests_onto_names") else {
        return;
    };
    assert!(text.contains(&format!("test {name} ... ok")), "{text}");
}

/// A kept yes goes with its `kind <a> <b>` ask at 30 days, and not before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_yes_is_pruned_with_its_ask() {
    let r = rig(FakeJev::start().unwrap(), |_| {});
    let old = fresh() - 31 * 24 * 3_600_000;
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    let kept = Mark {
        day: crate::judge::spend::local_day(fresh()),
        asked: [
            ("kind aaa bbb".to_string(), old),
            ("kind ccc ddd".to_string(), fresh()),
        ]
        .into(),
        same: [pair("aaa", "bbb"), pair("ccc", "ddd"), pair("eee", "fff")].into(),
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &kept).unwrap();
    r.core.onto_sweep_night(fresh()).await.unwrap();
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert_eq!(mark.same, [pair("ccc", "ddd")].into(), "{mark:?}");
}

/// A pick kept under `lanternledger` goes with the cluster when a name that
/// sorts first joins it: the key changes, the kind-name ask is not paid again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_pick_follows_its_cluster_to_a_new_key() {
    let r = rig(kind_jev(0.9), |_| {});
    plant_other(&r);
    said_other(&r, "o4", &["Ant Ledger"]);
    let kept = Mark {
        day: crate::judge::spend::local_day(fresh()),
        asked: [("kind-name lanternledger".to_string(), fresh())].into(),
        named: [("lanternledger".to_string(), "Ledger App".to_string())].into(),
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &kept).unwrap();
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.named_kinds), (1, 1), "{swept:?}");
    assert!(snapshot(&r.core).kind("ledger-app").is_some());
    let questions = r.jev.seen().len();
    assert_eq!(questions, 3, "three pairs, no kind name");
}

/// A pick kept under `lanternledger` was made among `Lantern Ledger` and
/// `Ledger App`. The owner then lists `Ledger App` and a kept yes joins
/// `Lantern Maps` to the key: the cluster no longer holds the pick, so the
/// pick and its ask are dropped and the new cluster is asked its name
/// (theseus-7e7hc).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_kept_pick_is_dropped_and_its_cluster_asked_again() {
    use theseus_protocol::OntologyKindAddParams;
    let jev = kind_jev(0.1);
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    plant_other(&r);
    said_other(&r, "o4", &["Lantern Maps"]);
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    let kept = Mark {
        day: crate::judge::spend::local_day(fresh()),
        asked: [
            ("kind lanternledger ledgerapp".to_string(), fresh()),
            ("kind lanternledger lanternmaps".to_string(), fresh()),
            ("kind-name lanternledger".to_string(), fresh()),
        ]
        .into(),
        named: [("lanternledger".to_string(), "Ledger App".to_string())].into(),
        same: [
            pair("lanternledger", "ledgerapp"),
            pair("lanternledger", "lanternmaps"),
        ]
        .into(),
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &kept).unwrap();
    r.core
        .ontology_kind_add(
            &OntologyKindAddParams {
                name: "ledger-apps".into(),
                description: "The apps that keep a ledger".into(),
                examples: vec!["Ledger App".into()],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.named_kinds), (1, 1), "{swept:?}");
    let o = snapshot(&r.core);
    assert!(o.kind("lantern-maps").is_some(), "Jev's pick names it");
    assert!(o.kind("ledger-app").is_none(), "the stale pick does not");
    assert_eq!(r.jev.seen().len(), 1, "only the kind's name was asked");
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert_ne!(
        mark.named.get("lanternledger").map(String::as_str),
        Some("Ledger App"),
        "{mark:?}"
    );
}

/// The sessions and kept yeses of `a_stale_kept_pick_is_dropped_and_its_cluster_asked_again`:
/// the pairs `lanternledger ~ ledgerapp` and `lanternledger ~ lanternmaps` were
/// asked and said yes, and `kind-name lanternledger` was asked (the pick was
/// none, or below the band), offering `offered`; `None` for a mark that
/// recorded no offer. The day's mark is today's.
fn kept_none_ask(r: &Rig, offered: Option<&[&str]>) {
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    let kept = Mark {
        day: crate::judge::spend::local_day(fresh()),
        asked: [
            ("kind lanternledger ledgerapp".to_string(), fresh()),
            ("kind lanternledger lanternmaps".to_string(), fresh()),
            ("kind-name lanternledger".to_string(), fresh()),
        ]
        .into(),
        same: [
            pair("lanternledger", "ledgerapp"),
            pair("lanternledger", "lanternmaps"),
        ]
        .into(),
        offered: offered
            .map(|o| {
                let set = o.iter().map(|f| f.to_string()).collect();
                [("lanternledger".to_string(), set)].into()
            })
            .unwrap_or_default(),
        ..Mark::default()
    };
    r.core.store.put_meta(MARK, &kept).unwrap();
}

/// The owner lists `Ledger App` in a kind of their own.
fn list_ledger_app(r: &Rig) {
    use theseus_protocol::OntologyKindAddParams;
    r.core
        .ontology_kind_add(
            &OntologyKindAddParams {
                name: "ledger-apps".into(),
                description: "The apps that keep a ledger".into(),
                examples: vec!["Ledger App".into()],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
}

/// A kept ask that got no pick offered `Lantern Ledger` and `Ledger App`. The
/// owner then lists `Ledger App` and a kept yes joins `Lantern Maps`: the
/// cluster's names are not those the ask offered, so the question changed and
/// the cluster is asked its name again (theseus-3ys6k).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_none_ask_on_a_changed_cluster_is_asked_again() {
    let jev = kind_jev(0.1);
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    plant_other(&r);
    said_other(&r, "o4", &["Lantern Maps"]);
    kept_none_ask(&r, Some(&["lanternledger", "ledgerapp"]));
    list_ledger_app(&r);
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.named_kinds), (1, 1), "{swept:?}");
    assert!(snapshot(&r.core).kind("lantern-maps").is_some());
    assert_eq!(r.jev.seen().len(), 1, "only the kind's name was asked");
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    let now: std::collections::BTreeSet<String> =
        ["lanternledger", "lanternmaps"].map(String::from).into();
    assert_eq!(mark.offered["lanternledger"], now, "{mark:?}");
}

/// The same cluster as the ask offered is not asked again: no question every night.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_none_ask_on_the_same_cluster_is_not_asked_again() {
    let jev = kind_jev(0.1);
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    plant_other(&r);
    said_other(&r, "o4", &["Lantern Maps"]);
    kept_none_ask(&r, Some(&["lanternledger", "ledgerapp", "lanternmaps"]));
    r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!(r.jev.seen().len(), 0, "no kind name was asked");
}

/// A mark written before `offered` recorded none: its ask is kept as it was
/// and ages out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_old_mark_with_no_offered_keeps_its_none_ask() {
    let jev = kind_jev(0.1);
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    plant_other(&r);
    said_other(&r, "o4", &["Lantern Maps"]);
    kept_none_ask(&r, None);
    list_ledger_app(&r);
    r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!(r.jev.seen().len(), 0, "no kind name was asked");
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert!(
        mark.asked.contains_key("kind-name lanternledger"),
        "{mark:?}"
    );
}

/// A paid kind-name ask records the names it offered, and the record goes with
/// its ask at 30 days.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kind_name_ask_records_its_offer_and_loses_it_with_the_ask() {
    let jev = kind_jev(0.9);
    pick(&jev, "n2", 0.9);
    let r = rig(jev, |_| {});
    said_other(&r, "o1", &["2026 Plan"]);
    said_other(&r, "o2", &["2026 Plan"]);
    said_other(&r, "o3", &["2026 Roadmap"]);
    r.core.onto_sweep_night(fresh()).await.unwrap();
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    let offer: std::collections::BTreeSet<String> =
        ["2026plan", "2026roadmap"].map(String::from).into();
    assert_eq!(mark.offered["2026plan"], offer, "{mark:?}");

    let old = fresh() - 31 * 24 * 3_600_000;
    let kept = Mark {
        day: crate::judge::spend::local_day(fresh()),
        asked: [
            ("kind-name aaa".to_string(), old),
            ("kind-name ccc".to_string(), fresh()),
        ]
        .into(),
        offered: [
            ("aaa".to_string(), offer.clone()),
            ("ccc".to_string(), offer),
            ("eee".to_string(), Default::default()),
        ]
        .into(),
        ..Mark::default()
    };
    let r = rig(FakeJev::start().unwrap(), |_| {});
    r.core.store.put_meta(MARK, &kept).unwrap();
    r.core.onto_sweep_night(fresh()).await.unwrap();
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert_eq!(mark.offered.keys().collect::<Vec<_>>(), ["ccc"], "{mark:?}");
}
