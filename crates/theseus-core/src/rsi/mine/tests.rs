//! The miner on a synthetic ledger (§2.3): the owner's example planted among
//! noise is found, with its arms, hand-back, inputs, cost and a skeleton spec;
//! support is counted per execution; the thresholds hold.

use super::fixture::{Land, Ledger, REPO};
use super::*;

fn found(l: &Ledger) -> Mined {
    mine(&l.runs, &l.vocab, &Params::default(), 10_000_000)
}

fn has_seed(m: &Motif, words: &[&str]) -> bool {
    m.seed.len() == words.len() && m.seed.iter().zip(words).all(|(s, w)| s.text.contains(w))
}

#[test]
fn the_owners_example_is_found_with_its_arms() {
    let mut l = Ledger::new();
    l.owners_example(REPO);
    let mined = found(&l);
    // The land stretch is one motif, not a motif for each of its seeds.
    let lands: Vec<_> = mined
        .motifs
        .iter()
        .filter(|m| m.steps.iter().any(|s| s.id == "merge"))
        .collect();
    assert_eq!(lands.len(), 1, "{}", lands.len());
    let land = lands[0];
    assert_eq!(
        land.support,
        Support {
            occurrences: 6,
            sessions: 6,
            days: 4,
            tasks: 0
        }
    );
    let ids: Vec<&str> = land
        .steps
        .iter()
        .filter(|s| s.core)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "fetch",
            "cargo_nextest_run",
            "merge",
            "cargo_nextest_run2",
            "push"
        ]
    );
    let commit = land
        .steps
        .iter()
        .find(|s| s.id == "commit")
        .expect("an optional step");
    assert!(!commit.core && commit.presence < 0.6);
    let b = land
        .branches
        .iter()
        .find(|b| b.at == "merge")
        .expect("a branch at merge");
    let arm = |on: &str| {
        b.arms
            .iter()
            .find(|a| a.on == on)
            .unwrap_or_else(|| panic!("{on}: {b:?}"))
    };
    let ok = arm("ok");
    assert_eq!((ok.count, &ok.exit), (3, &Exit::Ok));
    let resolved = arm("err:1+conflict+rerere-resolved");
    assert_eq!(resolved.count, 2);
    assert_eq!(resolved.then.first().map(String::as_str), Some("commit"));
    assert_eq!(resolved.then.last().map(String::as_str), Some("push"));
    assert_eq!(resolved.exit, Exit::Ok);
    let left = arm("err:1+conflict+rerere-left");
    assert_eq!(left.count, 1);
    assert!(left.then.is_empty());
    assert_eq!(left.exit, Exit::Handback("conflict-rerere-left".into()));
}

#[test]
fn the_sync_stretch_is_a_motif_of_its_own() {
    let mut l = Ledger::new();
    l.owners_example(REPO);
    let mined = found(&l);
    let sync = mined
        .motifs
        .iter()
        .find(|m| has_seed(m, &["git fetch", "git pull --ff-only", "cargo nextest run"]))
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                mined
                    .motifs
                    .iter()
                    .map(|m| m.seed.iter().map(|s| s.text.clone()).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(sync.support.occurrences, 6);
}

#[test]
fn a_motif_never_holds_the_models_authored_calls() {
    let mut l = Ledger::new();
    l.owners_example(REPO);
    for m in &found(&l).motifs {
        assert!(m.steps.iter().all(|s| !s.shape.authored), "{:?}", m.steps);
    }
}

#[test]
fn inputs_constants_and_the_skeleton_spec() {
    let mut l = Ledger::new();
    l.owners_example(REPO);
    let mined = found(&l);
    let land = mined
        .motifs
        .iter()
        .find(|m| m.steps.iter().any(|s| s.id == "merge"))
        .unwrap();
    // The branch varies: an input of type ref. The remote and the test's
    // package are constants.
    assert!(land
        .inputs
        .iter()
        .any(|i| i.step == "merge" && i.ty == SlotType::Reference));
    let merge = land.steps.iter().find(|s| s.id == "merge").unwrap();
    assert_eq!(merge.slots[0].kind, SlotKind::Input);
    let test = land
        .steps
        .iter()
        .find(|s| s.id == "cargo_nextest_run")
        .unwrap();
    assert_eq!(test.slots[0].kind, SlotKind::Constant);
    assert_eq!(test.slots[0].constant.as_deref(), Some("harbord"));
    let spec = land.spec();
    assert_eq!(spec["v"], 1);
    assert_eq!(spec["kind"], "flow");
    assert!(spec.get("name").is_none() && spec.get("description").is_none());
    assert_eq!(spec["scope"]["repos"][0], REPO);
    let step = spec["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "merge")
        .unwrap();
    let argv = &step["args"]["argv"];
    assert_eq!(argv[0], "git");
    assert_eq!(argv[1], "merge");
    assert_eq!(argv[2], "--no-ff");
    assert_eq!(argv[3], "$ref");
    assert_eq!(
        step["on"]["err:1+conflict+rerere-left"],
        "handback:conflict-rerere-left"
    );
    assert!(spec["exits"]["handback:conflict-rerere-left"].is_object());
    assert_eq!(spec["evidence"]["occurrences"], 6);
    assert!(spec["digest"].as_str().unwrap().starts_with("sha256:"));
    // The digest names the structure and is the same on a second run.
    assert_eq!(
        land.digest,
        found(&l)
            .motifs
            .iter()
            .find(|m| m.cause_key() == land.cause_key())
            .unwrap()
            .digest
    );
}

#[test]
fn evidence_is_positions_never_values() {
    let mut l = Ledger::new();
    l.owners_example(REPO);
    let mined = found(&l);
    let land = mined
        .motifs
        .iter()
        .find(|m| m.steps.iter().any(|s| s.id == "merge"))
        .unwrap();
    assert_eq!(land.occurrences.len(), 6);
    assert!(land
        .occurrences
        .iter()
        .all(|o| o.first > 0 && o.last >= o.first));
    let json = serde_json::to_string(&(land, land.spec())).unwrap();
    for v in ["feat/job-3", "src/a.rs", "origin/main"] {
        assert!(!json.contains(v), "{v} leaked");
    }
}

#[test]
fn a_loop_inside_one_execution_does_not_inflate_support() {
    let mut l = Ledger::new();
    // One turn lands ten times; two other sessions land once.
    let r = l.run("s-loop", "2026-10-10", REPO);
    for i in 0..10 {
        l.land(r, &format!("feat/x{i}"), Land::Clean);
    }
    for (s, d) in [("s-b", "2026-10-11"), ("s-c", "2026-10-12")] {
        let r = l.run(s, d, REPO);
        l.land(r, "feat/y", Land::Clean);
    }
    assert!(found(&l).motifs.is_empty(), "3 executions are not 5");
    // With five executions it is a motif, and its support is five, not fifteen.
    for (s, d) in [("s-d", "2026-10-13"), ("s-e", "2026-10-13")] {
        let r = l.run(s, d, REPO);
        l.land(r, "feat/z", Land::Clean);
    }
    let m = found(&l);
    assert_eq!(m.motifs.len(), 1, "{:?}", m.motifs.len());
    assert_eq!(m.motifs[0].support.occurrences, 5);
}

fn flow_runs(n: usize, sessions: usize, days: &[&str]) -> Ledger {
    let mut l = Ledger::new();
    for i in 0..n {
        let r = l.run(&format!("s{}", i % sessions), days[i % days.len()], REPO);
        l.land(r, "feat/q", Land::Clean);
    }
    l
}

#[test]
fn the_thresholds_are_five_occurrences_three_sessions_two_days() {
    assert!(
        found(&flow_runs(4, 4, &["d1", "d2"])).motifs.is_empty(),
        "4 occurrences"
    );
    assert!(found(&flow_runs(6, 6, &["d1"])).motifs.is_empty(), "1 day");
    assert!(
        found(&flow_runs(6, 2, &["d1", "d2"])).motifs.is_empty(),
        "2 sessions"
    );
    assert_eq!(
        found(&flow_runs(5, 3, &["d1", "d2"])).motifs.len(),
        1,
        "exactly the bar"
    );
    let p = Params {
        min_occurrences: 3,
        min_sessions: 3,
        min_days: 1,
        window_days: 14,
    };
    let l = flow_runs(4, 4, &["d1"]);
    assert_eq!(
        mine(&l.runs, &l.vocab, &p, 1).motifs.len(),
        1,
        "a lower bar finds it"
    );
}

#[test]
fn a_closed_pattern_drops_a_sub_pattern_with_equal_support() {
    // Every run has fetch, test, merge, push in a row: the 4-gram and its two
    // 3-grams have the same support, so only the 4-gram is a seed.
    let l = flow_runs(6, 6, &["d1", "d2"]);
    let m = found(&l);
    assert_eq!(m.motifs.len(), 1);
    assert!(
        m.motifs[0].seed.len() >= 4,
        "seed {}",
        m.motifs[0].seed.len()
    );
}

#[test]
fn each_scope_is_mined_by_itself() {
    let mut l = flow_runs(6, 6, &["d1", "d2"]);
    for r in &mut l.runs[..3] {
        r.scope = "r:0000000000feed02".into();
    }
    // Three and three: neither repository reaches five.
    assert!(found(&l).motifs.is_empty());
}

#[test]
fn a_single_shape_repeated_is_no_pattern() {
    let mut l = Ledger::new();
    for i in 0..6 {
        let r = l.run(&format!("s{i}"), if i % 2 == 0 { "d1" } else { "d2" }, REPO);
        l.model(r);
        for _ in 0..5 {
            l.argv(r, &["cargo", "check"], 0, "");
        }
    }
    assert!(found(&l).motifs.is_empty());
}

#[test]
fn a_call_that_is_never_mined_cuts_the_stretch() {
    let mut l = Ledger::new();
    for i in 0..6 {
        let r = l.run(&format!("s{i}"), if i % 2 == 0 { "d1" } else { "d2" }, REPO);
        l.model(r);
        l.argv(r, &["git", "fetch", "origin"], 0, "");
        l.argv(r, TEST_, 0, "ok");
        l.call(
            r,
            "channel.post",
            serde_json::json!({"text": "done"}),
            0,
            "sent",
        );
        l.argv(r, &["git", "push", "origin", "main"], 0, "");
        l.argv(r, &["git", "log", "--oneline"], 0, "");
    }
    // Neither side of the post reaches three calls, and nothing spans it.
    assert!(found(&l).motifs.is_empty());
}

const TEST_: &[&str] = &["cargo", "test"];
