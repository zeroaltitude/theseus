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
