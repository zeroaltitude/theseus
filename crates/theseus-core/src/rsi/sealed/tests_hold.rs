//! The sealed hold's tests (theseus-pw1q.21.7): the rule on made-up scores,
//! the behaviour list against the tree, main's cache, the weekly drift and
//! its one cause, and that no row holds an item's text. The join's own
//! phase is `join/tests_sealed.rs`; both score through a stand-in
//! [`Scorer`], since a live run needs L1 in a non-root daemon.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfBacklogParams, SelfMode};

use super::hold::{judge, touches, wildcard, Fut, Scorer, Whose, BEHAVIOUR};
use super::tests_relay::{rig, Answers, Rig};
use super::{FamilyScore, Scores};
use crate::rpc::Core;

/// A set's scores: each family `(name, items, passed)`; a failure id for each
/// item that failed, `f_<family>_<n>`.
pub(crate) fn scores(set: &str, families: &[(&str, u64, u64)]) -> Scores {
    let mut s = Scores {
        set: set.into(),
        ..Scores::default()
    };
    for (name, items, passed) in families {
        s.families.insert(
            (*name).into(),
            FamilyScore {
                items: *items,
                passed: *passed,
                failed: items - passed,
                ..FamilyScore::default()
            },
        );
        s.items += items;
        s.passed += passed;
        s.failed += items - passed;
        s.failures
            .extend((*passed..*items).map(|n| format!("f_{name}_{n}")));
    }
    s.failures.sort();
    s
}

/// A scorer that gives one answer for the build whose `head` is main's and
/// another for any other, and notes each head it was asked.
pub(crate) struct Canned {
    pub main_head: Mutex<String>,
    pub main: Mutex<Vec<Scores>>,
    pub other: Mutex<Vec<Scores>>,
    pub asked: Mutex<Vec<String>>,
}

impl Canned {
    pub(crate) fn new(main_head: &str, main: Vec<Scores>, other: Vec<Scores>) -> Arc<Self> {
        Arc::new(Self {
            main_head: Mutex::new(main_head.into()),
            main: Mutex::new(main),
            other: Mutex::new(other),
            asked: Mutex::default(),
        })
    }

    pub(crate) fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Scorer for Canned {
    fn score<'a>(&'a self, _core: &'a Arc<Core>, whose: &'a Whose) -> Fut<'a> {
        self.asked.lock().unwrap().push(whose.head.clone());
        let mine = if whose.head == *self.main_head.lock().unwrap() {
            &self.main
        } else {
            &self.other
        };
        let out = mine.lock().unwrap().clone();
        Box::pin(async move { Ok(out) })
    }
}

fn whose(head: &str) -> Whose {
    Whose {
        branch: "self/tide".into(),
        head: head.into(),
        bins: PathBuf::from("/nowhere"),
    }
}

#[test]
fn a_family_drop_holds_the_join() {
    // A six-item family loses two; the other gains two, so the total is level
    // and only the family's rule can hold it.
    let main = [scores("tides", &[("fact", 6, 6), ("route", 6, 3)])];
    let cand = [scores("tides", &[("fact", 6, 4), ("route", 6, 5)])];
    let v = judge(&main, &cand);
    assert_eq!((v.before, v.after, v.total), (9, 9, 12));
    assert!(v.holds(), "{v:?}");
    assert_eq!(v.holds.len(), 1);
    assert_eq!(v.holds[0].family.as_deref(), Some("fact"));
    assert_eq!((v.holds[0].before, v.holds[0].after), (6, 4));
    assert_eq!(v.broken.len(), 2, "the ids of the two items that fail now");
    assert!(v.why().contains("family fact of set tides: 6 → 4 of 6"));
}

#[test]
fn equal_scores_pass() {
    let main = [scores("tides", &[("fact", 6, 5), ("route", 3, 3)])];
    let v = judge(&main, &main);
    assert!(
        !v.holds() && v.reported.is_empty() && v.broken.is_empty(),
        "{v:?}"
    );
    assert_eq!(v.line(), "sealed 8/9 → 8/9");
    // Better is a pass too, and nothing is reported.
    let better = [scores("tides", &[("fact", 6, 6), ("route", 3, 3)])];
    let v = judge(&main, &better);
    assert!(!v.holds() && v.reported.is_empty(), "{v:?}");
    assert_eq!(v.line(), "sealed 8/9 → 9/9");
}

#[test]
fn the_total_falling_holds() {
    // Each family loses one item, which its rule allows; the total falls by
    // two, which nothing allows.
    let main = [scores("tides", &[("fact", 6, 6), ("route", 6, 6)])];
    let cand = [scores("tides", &[("fact", 6, 5), ("route", 6, 5)])];
    let v = judge(&main, &cand);
    assert!(v.holds(), "{v:?}");
    assert_eq!(v.holds.len(), 1);
    assert_eq!(v.holds[0].family, None, "the total, not a family");
    assert_eq!(v.reported.len(), 2, "the two families are reported");
    assert!(v.why().contains("set tides total: 12 → 10 of 12"));
}

#[test]
fn a_family_under_five_items_is_reported_never_held() {
    // A four-item family loses three; the other gains three, so the total
    // holds level: n is noise below five.
    let main = [scores("tides", &[("rare", 4, 4), ("route", 6, 3)])];
    let cand = [scores("tides", &[("rare", 4, 1), ("route", 6, 6)])];
    let v = judge(&main, &cand);
    assert!(!v.holds(), "{v:?}");
    assert_eq!(v.reported.len(), 1);
    assert_eq!(v.reported[0].family.as_deref(), Some("rare"));
    assert_eq!((v.reported[0].before, v.reported[0].after), (4, 1));
}

#[test]
fn sets_are_judged_apart_and_a_set_main_lacks_is_not_judged() {
    let main = [scores("tides", &[("fact", 6, 6)])];
    let cand = [
        scores("tides", &[("fact", 6, 6)]),
        scores("winds", &[("fact", 6, 0)]),
    ];
    let v = judge(&main, &cand);
    assert!(!v.holds() && v.total == 6, "{v:?}");
}

#[test]
fn wildcards_match_a_run_and_a_behaviour_path_is_not_a_test() {
    assert!(wildcard("a/*", "a/b/c.rs"));
    assert!(wildcard("a/*.rs", "a/b/c.rs"));
    assert!(wildcard("a/c.rs", "a/c.rs"));
    assert!(!wildcard("a/c.rs", "a/c.rs.bak"));
    assert!(!wildcard("a/*.rs", "a/b/c.toml"));
    assert!(wildcard("*", ""));
    let paths = ["README.md", "crates/theseus-core/src/compiler/ring.rs"];
    assert_eq!(
        touches(&paths).as_deref(),
        Some("crates/theseus-core/src/compiler/ring.rs")
    );
    // A test of a behaviour path measures it, so changing one is no change
    // of behaviour; nor is a path nothing lists.
    let tests = [
        "crates/theseus-core/src/compiler/tests_ring.rs",
        "crates/theseus-core/src/compiler/tests.rs",
        "crates/theseus-core/src/rsi/sealed/hold.rs",
        "docs/spec/01-front.md",
    ];
    assert_eq!(touches(&tests), None);
}

fn files_under(dir: &std::path::Path, out: &mut Vec<String>, root: &std::path::Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "target" || name == "node_modules" || name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            files_under(&p, out, root);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}

/// The list cannot rot silently: each pattern matches a file of the tree.
#[test]
fn every_behaviour_glob_matches_a_file() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut files = Vec::new();
    files_under(&root.join("crates"), &mut files, &root);
    assert!(files.len() > 100, "the walk found the tree");
    for pattern in BEHAVIOUR {
        let hit = files.iter().find(|f| wildcard(pattern, f));
        assert!(hit.is_some(), "{pattern} matches no file of the tree");
    }
}

/// A rig with two items in a set `tides`, and a stand-in scorer.
async fn rigged(main: Vec<Scores>, other: Vec<Scores>) -> (Rig, Arc<Canned>) {
    let r = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |cfg| cfg.self_improve.backlog_starts = Some(0),
    )
    .await;
    for (i, fam) in ["fact", "route"].into_iter().enumerate() {
        let item = json!({"v": 1, "task": format!("Question {i}?"), "check": "reply has \"x\""});
        r.core
            .sealed_put("tides", fam, item.to_string().as_bytes(), "reviewer-a")
            .unwrap();
    }
    let canned = Canned::new("mainsha", main, other);
    (r, canned)
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn mains_score_is_cached_per_commit_and_set() {
    let main = vec![scores("tides", &[("fact", 1, 1), ("route", 1, 1)])];
    let (r, canned) = rigged(main.clone(), vec![]).await;
    let ask = |head: &str| {
        let (core, canned, w) = (r.core.clone(), canned.clone(), whose(head));
        async move { core.sealed_main(&*canned, &w).await.unwrap() }
    };
    let (got, cached) = ask("mainsha").await;
    assert_eq!((got, cached), (main.clone(), false));
    // The same commit and sets: a hit, and nothing scored again.
    let (got, cached) = ask("mainsha").await;
    assert_eq!((got, cached), (main.clone(), true));
    assert_eq!(canned.asked().len(), 1);
    // Another branch on the same main reuses it; another main does not.
    let (_, cached) = ask("mainsha").await;
    assert!(cached);
    *canned.main_head.lock().unwrap() = "newsha".into();
    let (_, cached) = ask("newsha").await;
    assert!(!cached);
    assert_eq!(canned.asked().len(), 2);
    // A changed set is a new key: the same commit is scored again.
    let item = json!({"v": 1, "task": "A third question?", "check": "reply has \"x\""});
    r.core
        .sealed_put("tides", "fact", item.to_string().as_bytes(), "reviewer-a")
        .unwrap();
    let (_, cached) = ask("newsha").await;
    assert!(!cached, "the set changed under the same commit");
    assert_eq!(canned.asked().len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn off_or_halted_scores_nothing_and_spends_nothing() {
    let main = vec![scores("tides", &[("fact", 1, 1)])];
    let (r, canned) = rigged(main, vec![]).await;
    r.core
        .self_halt(
            &theseus_protocol::rsi::SelfHaltParams::default(),
            &crate::approval::Answerer::from("the CLI"),
        )
        .unwrap();
    let n = rows(&r.core, "self.sealed.scored").len();
    assert!(r
        .core
        .sealed_main(&*canned, &whose("mainsha"))
        .await
        .is_err());
    let bins = PathBuf::from("/nowhere");
    assert!(r
        .core
        .sealed_drift(&*canned, bins.clone(), 1, true)
        .await
        .is_err());
    assert!(canned.asked().is_empty(), "nothing was scored");
    assert!(r
        .core
        .store
        .get_meta::<Value>("self.sealed.baseline")
        .unwrap()
        .is_none());
    assert_eq!(rows(&r.core, "self.sealed.scored").len(), n);
    // The real scorer is gated too: no run, no row, no relay spend.
    assert!(r.core.sealed_score(&whose("mainsha")).await.is_err());
    // Mode off is the same.
    let off = rig(
        Answers::Whole {
            input: 1,
            output: 1,
        },
        |c| {
            c.self_improve.mode = SelfMode::Off;
        },
    )
    .await;
    assert!(off.core.sealed_main(&*canned, &whose("x")).await.is_err());
    assert!(canned.asked().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn drift_files_one_cause() {
    let was = vec![scores("tides", &[("fact", 6, 6), ("route", 6, 6)])];
    let now = vec![scores("tides", &[("fact", 6, 4), ("route", 6, 6)])];
    let (r, canned) = rigged(was, now.clone()).await;
    let bins = PathBuf::from("/nowhere");
    // The scorer answers `main` for "installed" too: first the baseline.
    *canned.main_head.lock().unwrap() = "installed".into();
    let first = r
        .core
        .sealed_drift(&*canned, bins.clone(), 1_000, true)
        .await
        .unwrap();
    assert!(first.is_empty(), "the first score is the baseline");
    assert!(rows(&r.core, "self.sealed.drift").is_empty());
    // Then the installed main is worse in one family, twice in a row.
    *canned.main.lock().unwrap() = now;
    for at in [2_000, 3_000] {
        let lost = r
            .core
            .sealed_drift(&*canned, bins.clone(), at, true)
            .await
            .unwrap();
        // The family, and the set's total that fell with it.
        assert_eq!(lost.len(), 2);
        assert_eq!(lost[0].family.as_deref(), Some("fact"));
        assert_eq!(lost[1].family, None);
    }
    let drift = rows(&r.core, "self.sealed.drift");
    assert_eq!(drift.len(), 4);
    assert_eq!(drift[0]["family"], "fact");
    assert_eq!(
        (drift[0]["before"].clone(), drift[0]["after"].clone()),
        (json!(6), json!(4))
    );
    // The backlog files one cause for both, on two nights.
    for _ in 0..2 {
        r.core
            .backlog_night(theseus_protocol::now_unix_ms())
            .unwrap();
        r.core
            .sealed_drift(&*canned, bins.clone(), 4_000, true)
            .await
            .unwrap();
    }
    let items = r
        .core
        .self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items;
    let ours: Vec<_> = items
        .iter()
        .filter(|i| i.cause == "sealed.drift:fact")
        .collect();
    assert_eq!(ours.len(), 1, "one open cause: {ours:?}");
    assert!(ours[0].count >= 2, "the evidence of every row: {ours:?}");
    let total = items.iter().filter(|i| i.cause == "sealed.drift:_total");
    assert_eq!(total.count(), 1, "the total's cause is one too");
}

#[tokio::test(flavor = "current_thread")]
async fn drift_waits_for_its_week_and_a_changed_set_is_a_new_baseline() {
    let was = vec![scores("tides", &[("fact", 6, 6)])];
    let (r, canned) = rigged(was, vec![]).await;
    *canned.main_head.lock().unwrap() = "installed".into();
    let bins = PathBuf::from("/nowhere");
    let day = 86_400_000;
    r.core
        .sealed_drift(&*canned, bins.clone(), day, false)
        .await
        .unwrap();
    // Not due again for drift_every_days (7).
    r.core
        .sealed_drift(&*canned, bins.clone(), 3 * day, false)
        .await
        .unwrap();
    assert_eq!(canned.asked().len(), 1);
    r.core
        .sealed_drift(&*canned, bins.clone(), 9 * day, false)
        .await
        .unwrap();
    assert_eq!(canned.asked().len(), 2);
    // A changed set is no drift: it is scored and becomes the baseline.
    let item = json!({"v": 1, "task": "A new question?", "check": "reply has \"x\""});
    r.core
        .sealed_put("tides", "fact", item.to_string().as_bytes(), "reviewer-a")
        .unwrap();
    *canned.main.lock().unwrap() = vec![scores("tides", &[("fact", 7, 0)])];
    let lost = r
        .core
        .sealed_drift(&*canned, bins, 20 * day, false)
        .await
        .unwrap();
    assert!(lost.is_empty() && rows(&r.core, "self.sealed.drift").is_empty());
}

/// Nothing a row says, or META keeps, is an item's text: a canary in a
/// sealed item's task and in its expected answer is found nowhere in the
/// ledger or in the log the CLI prints, and a row that would carry one is
/// refused by the filter.
#[tokio::test(flavor = "current_thread")]
async fn no_item_text_in_any_row() {
    const TASK: &str = "Where does the saffron meridian canary-9f3a end?";
    const ANSWER: &str = "ultramarine-harbour-4c1e";
    let main = vec![scores("tides", &[("fact", 6, 6), ("route", 6, 3)])];
    let cand = vec![scores("tides", &[("fact", 6, 4), ("route", 6, 5)])];
    let (r, canned) = rigged(main.clone(), cand).await;
    let item = json!({"v": 1, "task": TASK, "check": format!("reply has \"{ANSWER}\""),
                      "canary": "saffron-meridian-canary"});
    r.core
        .sealed_put("tides", "fact", item.to_string().as_bytes(), "reviewer-a")
        .unwrap();
    let (got, _) = r
        .core
        .sealed_main(&*canned, &whose("mainsha"))
        .await
        .unwrap();
    assert_eq!(got, main);
    // The sealed filter refuses a row's text that holds an item's string.
    let sealed = r.core.sealed().unwrap();
    assert!(!sealed.allows(&format!("held: {ANSWER}")).unwrap());
    assert!(!sealed.allows("saffron-meridian-canary").unwrap());
    assert!(sealed.allows("sealed 9/12 → 9/12").unwrap());
    // Everything written: the ledger's rows, META's keys, and the log.
    let ledger: String = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .iter()
        .map(|(_, row)| serde_json::to_string(row).unwrap())
        .collect();
    let kept = serde_json::to_string(
        &r.core
            .store
            .get_meta::<Value>(&format!(
                "self.sealed.main.mainsha.{}",
                r.core.sealed_digest().await.unwrap()
            ))
            .unwrap(),
    )
    .unwrap();
    let log = serde_json::to_string(
        &r.core
            .self_log(&theseus_protocol::rsi::SelfLogParams::default())
            .unwrap(),
    )
    .unwrap();
    for (what, text) in [("ledger", &ledger), ("META", &kept), ("log", &log)] {
        for needle in ["saffron", "canary-9f3a", ANSWER, "Where does"] {
            assert!(!text.contains(needle), "{what} holds {needle}");
        }
    }
    let _: BTreeMap<String, u64> = r.core.sealed().unwrap().counts();
}
