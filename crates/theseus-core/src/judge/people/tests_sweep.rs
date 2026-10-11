//! The nightly sweep (theseus-j8qb), with a fake model and a fake Jev: a
//! private session whose gate shut is swept once (an extraction with the
//! purpose `sweep`, Jev's judgment, a proposal), its live mark moved; a
//! shared place's text and a session an extraction read are not; a second
//! run the same day finds nothing due and carries the day's spend, and so
//! does a run whose window is put back over the text it read; a cap
//! that cannot hold one session's worst case passes none and says so; and
//! the config's key has its default and refuses a negative. Every name here
//! is invented.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::OntologyProposalsParams;

use super::sweep::{SweepMark, LAST_RUN};
use super::tests::{answer, people_on, rows, script, until_rows};
use crate::judge::categorize::{Mark, EVERY};
use crate::tests_categorize::{moorings, rig, session, Rig};

/// The live point's mark of `sid`, once it is written.
async fn until_marked(r: &Rig, sid: &str) -> Mark {
    let t0 = Instant::now();
    loop {
        let m: Option<Mark> = r
            .core
            .store
            .get_meta(&format!("{}{sid}", super::live::MARK_PREFIX))
            .unwrap();
        if let Some(m) = m {
            return m;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no mark for {sid}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A rig whose gate shuts (Jev: no person not held is involved), with a
/// private session and a shared one that each had a due exchange, the
/// gate asked and the private mark moved with no extraction; the sweep's
/// cap `cap`.
pub(super) async fn shut_gate(cap: f64) -> (FakeJev, Rig, String, String) {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    jev.script("unlisted", Jev::Noul(0.1));
    let r = rig(Some(&jev), 2 * EVERY, |c| {
        people_on(c);
        c.people.sweep_usd_per_day = cap;
    });
    let private = session(&r.core, None);
    moorings(&r.core, &private, EVERY).await;
    let shared = session(&r.core, Some("channel:314159265358979323"));
    moorings(&r.core, &shared, EVERY).await;
    until_marked(&r, &private).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        rows(&r.core.store, "people.extracted").is_empty(),
        "the gate shut: no extraction"
    );
    (jev, r, private, shared)
}

fn swept_rows(r: &Rig) -> Vec<crate::ledger::LedgerRow> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(500)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.kind == "people.swept")
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_extracts_what_the_gate_let_pass_once_in_private_places() {
    let (_jev, r, private, shared) = shut_gate(2.0).await;
    let before = until_marked(&r, &private).await;
    r.fake.script.lock().unwrap().push_back(answer(json!([
        {"name": "Wren Halloway", "handles": [], "role_line": "Keeps the tide tables.", "evidence": ["L1"]}
    ])));
    let s = r.core.people_sweep("nightly").await.unwrap();
    assert_eq!((s.due, s.swept, s.judged), (1, 1, 1), "{s:?}");
    assert!(s.stopped.is_none(), "{s:?}");
    let x = rows(&r.core.store, "people.extracted");
    assert_eq!(x.len(), 1);
    assert_eq!(x[0].session_id.as_deref(), Some(private.as_str()));
    assert_eq!(x[0].data["purpose"], "sweep");
    assert!(
        x.iter()
            .all(|e| e.session_id.as_deref() != Some(shared.as_str())),
        "a shared place is never swept"
    );
    // The judgment is the sink's row, written as it lands.
    until_rows(&r.core.store, "judge.call", 1).await;
    // A proposal, never a membership.
    let listed = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].person.as_ref().unwrap().name, "Wren Halloway");
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o
        .memberships(&private)
        .iter()
        .all(|m| m.kind() != theseus_ontology::person::KIND));
    // The live mark is where the sweep read to: never back.
    let after = until_marked(&r, &private).await;
    assert!(after.through >= before.through, "{before:?} {after:?}");
    // Its row and its mark.
    let row = swept_rows(&r);
    assert_eq!(row.len(), 1);
    assert_eq!(
        (row[0].data["trigger"].as_str(), row[0].data["due"].as_u64()),
        (Some("nightly"), Some(1))
    );
    let m: SweepMark = r.core.store.get_meta(LAST_RUN).unwrap().unwrap();
    assert!(m.spent_usd > 0.0, "{m:?}");

    // Again the same day: nothing due (the extraction read it), the day's
    // spend carried.
    let again = r.core.people_sweep("nightly").await.unwrap();
    assert_eq!((again.due, again.swept), (0, 0), "{again:?}");
    let row = swept_rows(&r);
    assert_eq!(row.len(), 2);
    assert_eq!(row[1].data["spent_before_usd"].as_f64(), Some(m.spent_usd));
    assert_eq!(rows(&r.core.store, "people.extracted").len(), 1);

    // The window back over the same text: still nothing due, because an
    // extraction read it (the newest extraction's point, not the window,
    // decides).
    let back = SweepMark { at_ms: 0, ..m };
    r.core.store.put_meta(LAST_RUN, &back).unwrap();
    let wide = r.core.people_sweep("nightly").await.unwrap();
    assert_eq!((wide.due, wide.swept), (0, 0), "{wide:?}");
    assert_eq!(rows(&r.core.store, "people.extracted").len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cap_that_cannot_hold_a_session_passes_none() {
    let (_jev, r, _private, _shared) = shut_gate(0.000_1).await;
    let s = r.core.people_sweep("missed").await.unwrap();
    assert_eq!((s.due, s.swept), (1, 0), "{s:?}");
    assert!(
        s.stopped
            .as_deref()
            .unwrap()
            .starts_with("at the day's cap"),
        "{s:?}"
    );
    assert_eq!(r.fake.requests().len(), 2 * EVERY, "no extraction call");
    assert!(rows(&r.core.store, "people.extracted").is_empty());
    let row = swept_rows(&r);
    assert!(row[0].data["stopped"]
        .as_str()
        .unwrap()
        .contains("sessions left"));
}

#[test]
fn the_sweeps_cap_has_its_default_and_refuses_a_negative() {
    let c = crate::config::people::PeopleConfig::default();
    assert_eq!(c.sweep_usd_per_day, 2.0);
    c.validate().unwrap();
    let off = crate::config::people::PeopleConfig {
        sweep_usd_per_day: 0.0,
        ..c.clone()
    };
    off.validate().unwrap();
    for bad in [-1.0, f64::NAN] {
        let e = crate::config::people::PeopleConfig {
            sweep_usd_per_day: bad,
            ..c.clone()
        }
        .validate()
        .unwrap_err();
        assert!(e.to_string().contains("sweep_usd_per_day"), "{e}");
    }
}
