//! route.v10's three rollback rules, armed (theseus-ervz9, route.v10 row 5): they
//! are what make the flip reversible, so each is read from the pack's own file
//! and each fires on its own numbers. `adopt.rs` used to inject `PinsPerDay { 3 }`
//! for every route version, so v10's own 5 never armed: the file's rule wins, and
//! the adoption's stays the fallback for a pack that declares none (route.v5 to
//! v8, which name only two rules).

use std::path::Path;
use std::sync::Arc;

use theseus_judge::learn::{CanaryEvent, RollbackRule};

use crate::config::PackMode;
use crate::judge::ladder::{self, Rung};
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, judge_config, texts};

const V10: &str = "route.v10";

fn core_at(dir: &Path) -> Arc<Core> {
    let mut cfg = judge_config(dir, None);
    cfg.judge.enabled = true;
    cfg.judge.api_base = "http://127.0.0.1:9".into();
    cfg.validate().unwrap();
    let store = Store::open(&dir.join("store")).unwrap();
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(4))), store);
    p.secrets = board();
    Core::build(p).unwrap()
}

/// What the pack's file declares, and nothing the adoption table adds beside it.
#[test]
fn route_v10_is_armed_with_its_own_three_rules_and_no_others() {
    let rules = ladder::rules::rules_of(V10);
    assert_eq!(
        rules,
        vec![
            RollbackRule::LabelsPerDay {
                label: "wrong model".into(),
                count: 3
            },
            RollbackRule::OnPathP95 {
                max_ms: 250,
                min_samples: 20
            },
            RollbackRule::PinsPerDay { count: 5 },
        ],
        "the pack's pins_per_day is 5; adoption's constant 3 does not shadow it"
    );
}

/// A pack whose file declares no pins rule keeps the adoption's: route.v8 still
/// rolls back on 3 pins in a day.
#[test]
fn an_earlier_route_pack_keeps_the_adopted_three_pins() {
    for pack in ["route.v5", "route.v8"] {
        assert!(
            ladder::rules::rules_of(pack).contains(&RollbackRule::PinsPerDay { count: 3 }),
            "{pack}"
        );
        assert!(
            !ladder::rules::rules_of(pack).contains(&RollbackRule::PinsPerDay { count: 5 }),
            "{pack}"
        );
    }
}

/// The fifth pin rolls v10 back, the fourth does not (the third, where the
/// adoption's constant would have fired it, does not).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_fifth_pin_of_a_day_rolls_route_v10_back_and_the_third_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path());
    let j = &c.runner.judge;
    j.ladder().wire(V10, PackMode::Live);
    let day = j.today();
    for n in 1..=4 {
        j.land(V10, CanaryEvent::Pinned { day: day.clone() });
        assert_eq!(j.ladder().standing(V10).rung, Rung::Live, "pin {n}");
    }
    j.land(V10, CanaryEvent::Pinned { day });
    let s = j.ladder().standing(V10);
    assert_eq!(
        (s.rung, s.rule.as_deref()),
        (Rung::RolledBack, Some("pins_per_day"))
    );
    assert_eq!(j.mode_for(V10, "ses_a").mode, PackMode::Shadow);
}

/// Three `wrong model` labels in a day roll it back; two do not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_wrong_model_labels_roll_route_v10_back() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path());
    let j = &c.runner.judge;
    j.ladder().wire(V10, PackMode::Live);
    let label = |day: String| CanaryEvent::Label {
        day,
        label: "wrong model".into(),
    };
    let day = j.today();
    for _ in 0..2 {
        j.land(V10, label(day.clone()));
    }
    assert_eq!(j.ladder().standing(V10).rung, Rung::Live);
    j.land(V10, label(day));
    let s = j.ladder().standing(V10);
    assert_eq!(
        (s.rung, s.rule.as_deref()),
        (Rung::RolledBack, Some("labels_per_day"))
    );
}

/// An on-path p95 over 250 ms across 20 judgments rolls it back; 19 do not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_on_path_p95_rolls_route_v10_back() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path());
    let j = &c.runner.judge;
    j.ladder().wire(V10, PackMode::Live);
    for _ in 0..19 {
        j.land(V10, CanaryEvent::OnPath { ms: 400 });
    }
    assert_eq!(j.ladder().standing(V10).rung, Rung::Live);
    j.land(V10, CanaryEvent::OnPath { ms: 400 });
    let s = j.ladder().standing(V10);
    assert_eq!(
        (s.rung, s.rule.as_deref()),
        (Rung::RolledBack, Some("on_path_p95"))
    );
}

/// Visible: `pack.list` names the three rules of v10, and health says a rolled
/// back one by the rule that did it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pack_list_shows_the_three_rules_and_health_a_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path());
    let j = &c.runner.judge;
    j.ladder().wire(V10, PackMode::Live);
    let listed = c.pack_list();
    let info = listed.packs.iter().find(|p| p.pack == V10).expect("listed");
    assert_eq!(
        info.rules,
        ["labels_per_day", "on_path_p95", "pins_per_day"]
    );
    let day = j.today();
    for _ in 0..5 {
        j.land(V10, CanaryEvent::Pinned { day: day.clone() });
    }
    let line = j
        .health()
        .packs
        .into_iter()
        .find(|l| l.starts_with(V10))
        .expect("a health line");
    assert_eq!(line, "route.v10: rolled back (pins_per_day)");
}
