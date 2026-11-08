//! A sweep says so before it stops: the stages, the once-a-day rule, and the
//! completeness walk (a sweep with no notice at its stop fails it).

use theseus_judge::fake::FakeJev;

use super::{stage, Stage, SweepId, NEAR_PERCENT, SWEEPS};
use crate::tests_judge::{kinds, rig_with};

#[test]
fn the_stages_are_quiet_near_and_stopped_and_a_cap_of_zero_is_off() {
    assert_eq!(stage(1_000, 100, 100), Stage::Quiet);
    assert_eq!(stage(1_000, 700, 100), Stage::Near, "at the near line");
    assert_eq!(
        stage(1_000, 900, 100),
        Stage::Near,
        "at the cap, not past it"
    );
    assert_eq!(stage(1_000, 900, 101), Stage::Stopped);
    assert_eq!(
        stage(0, 5, 5),
        Stage::Quiet,
        "a sweep that is off never stops"
    );
    assert_eq!(NEAR_PERCENT, 80);
}

/// Every sweep of the owner's seven is listed once, and every one has a site
/// that asks for the notice: the stop of a sweep is never silent. The count
/// of sweeps walked is asserted, so an empty list cannot pass.
#[test]
fn every_sweep_that_can_stop_for_the_day_says_so_first() {
    assert_eq!(SWEEPS.len(), 7, "D7, D8, D9, D10, D16, D17 and D18");
    let mut rows: Vec<&str> = SWEEPS.iter().map(|s| s.row).collect();
    rows.sort();
    rows.dedup();
    assert_eq!(rows.len(), 7, "each sweep has its own row");
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![src];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.to_string_lossy();
                if !name.contains("sweep_notice") {
                    files.push(std::fs::read_to_string(&p).unwrap());
                }
            }
        }
    }
    assert!(
        files.len() > 100,
        "the walk read the source: {}",
        files.len()
    );
    for s in SWEEPS {
        let site = format!("SweepId::{:?}", s.id);
        let calls = files
            .iter()
            .map(|f| f.matches(&site).count())
            .sum::<usize>();
        assert!(
            calls >= 1,
            "{} ({}) can stop for the day with no notice: no site names {site}",
            s.name,
            s.row
        );
    }
}

fn rows(r: &crate::tests_judge::Rig, stage: &str) -> usize {
    kinds(&r.core.store, "sweep.stopping")
        .iter()
        .filter(|x| x.data["stage"] == stage)
        .count()
}

/// Near says it is going to stop, once; stopped says it has, once; each at
/// most once a day however many calls ask, and quiet writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sweep_says_it_is_going_to_stop_once_and_that_it_stopped_once() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(vec![], Some(&jev), |c| c.narrative = true);
    let run = &r.core.runner;
    run.sweep_heads_up(SweepId::Card, 1_000_000, 100_000, 10_000);
    assert_eq!(kinds(&r.core.store, "sweep.stopping").len(), 0, "quiet");
    for _ in 0..3 {
        run.sweep_heads_up(SweepId::Card, 1_000_000, 800_000, 10_000);
    }
    assert_eq!((rows(&r, "near"), rows(&r, "stopped")), (1, 0));
    for _ in 0..3 {
        run.sweep_heads_up(SweepId::Card, 1_000_000, 995_000, 10_000);
    }
    assert_eq!((rows(&r, "near"), rows(&r, "stopped")), (1, 1));
    // Another sweep keeps its own mark.
    run.sweep_heads_up(SweepId::Ontology, 1_000_000, 995_000, 10_000);
    assert_eq!(rows(&r, "stopped"), 2);
}
