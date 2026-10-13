//! The nightly sync's people step (theseus-pjmj7), with a fake model and a
//! fake Jev: the extraction reads only the messages written after the
//! session's mark and moves the mark (the first pass reads all of a session,
//! an append's pass only the appended ones, a night with nothing new calls
//! nothing); a session not in the sync's list is never read; and the step
//! holds the sweep's daily cap and shares its day with it. Every name is
//! invented.

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};

use super::sync::{SyncDay, DAY};
use super::tests::{answer, billed, people_on, script};
use crate::import::session_id_of;
use crate::import::tests_append::{sync, TAG};
use crate::judge::categorize::Mark;
use crate::tests_categorize::{rig, Rig};

fn import(r: &Rig, episodes: &[serde_json::Value]) {
    let lines: Vec<ImportLine> = episodes
        .iter()
        .enumerate()
        .map(|(i, e)| ImportLine {
            line: i as u64 + 1,
            text: serde_json::to_string(e).unwrap(),
        })
        .collect();
    let out = crate::import::write::import_batch(
        &r.core.store,
        &ImportEpisodesParams {
            file: "sync.jsonl".into(),
            lines,
        },
        "test",
    )
    .unwrap();
    assert!(out.rejected.is_empty(), "{:?}", out.rejected);
}

fn mark(r: &Rig, sid: &str) -> Option<Mark> {
    r.core
        .store
        .get_meta(&format!("{}{sid}", super::live::MARK_PREFIX))
        .unwrap()
}

fn asked(r: &Rig) -> Vec<String> {
    r.fake.requests().iter().map(|q| format!("{q:?}")).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_people_step_reads_only_new_messages_and_moves_each_sessions_mark() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    let (a, b) = (sync(10, TAG, &[0, 2, 4]), sync(11, TAG, &[0, 2]));
    import(&r, &[a.clone(), b.clone()]);
    let (sa, sb) = (
        session_id_of(a["episode_id"].as_str().unwrap()),
        session_id_of(b["episode_id"].as_str().unwrap()),
    );
    r.fake.script.lock().unwrap().push_back(answer(json!([])));
    let first = r.core.people_sync(vec![sa.clone()]).await.unwrap();
    assert_eq!((first.due, first.swept), (1, 1), "{first:?}");
    assert_eq!(first.done, vec![sa.clone()]);
    let reqs = asked(&r);
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].contains("operator: Message 0") && reqs[0].contains("operator: Message 4"),
        "{}",
        reqs[0]
    );
    let m1 = mark(&r, &sa).expect("the session's mark");
    assert!(m1.through > 0);
    assert!(
        mark(&r, &sb).is_none(),
        "a session not in the list is never read"
    );

    // The session grows by two messages: only they are read.
    import(&r, &[sync(10, "openclaw-sync-2026-11", &[0, 2, 4, 6, 8])]);
    r.fake.script.lock().unwrap().push_back(answer(json!([])));
    let again = r.core.people_sync(vec![sa.clone()]).await.unwrap();
    assert_eq!((again.due, again.swept), (1, 1), "{again:?}");
    let reqs = asked(&r);
    assert_eq!(reqs.len(), 2);
    assert!(
        reqs[1].contains("Message 6") && reqs[1].contains("Message 8"),
        "{}",
        reqs[1]
    );
    // The session's title is its first message's words: a line is `L<n> <author>: <text>`.
    assert!(
        !reqs[1].contains("operator: Message 0")
            && !reqs[1].contains("operator: Message 2")
            && !reqs[1].contains("operator: Message 4"),
        "an old message was read again: {}",
        reqs[1]
    );
    let m2 = mark(&r, &sa).unwrap();
    assert!(m2.through > m1.through, "{m1:?} {m2:?}");

    // A night with nothing new: nothing read, no call.
    let none = r.core.people_sync(vec![sa.clone()]).await.unwrap();
    assert_eq!((none.due, none.swept), (0, 0), "{none:?}");
    assert_eq!(none.done, vec![sa.clone()]);
    assert_eq!(asked(&r).len(), 2);
    assert_eq!(mark(&r, &sa).unwrap(), m2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_people_step_holds_the_sweeps_cap_and_shares_its_day() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    // A cap too small for one session's worst case: none is begun, and it
    // stays in the list.
    let tiny = rig(Some(&jev), 0, |c| {
        people_on(c);
        c.people.sweep_usd_per_day = 0.000_1;
    });
    let e = sync(12, TAG, &[0, 2]);
    import(&tiny, std::slice::from_ref(&e));
    let sid = session_id_of(e["episode_id"].as_str().unwrap());
    let s = tiny.core.people_sync(vec![sid.clone()]).await.unwrap();
    assert_eq!((s.due, s.swept), (1, 0), "{s:?}");
    assert!(
        s.stopped
            .as_deref()
            .unwrap()
            .starts_with("at the day's cap"),
        "{s:?}"
    );
    assert!(s.done.is_empty(), "left for the next run");
    assert!(tiny.fake.requests().is_empty(), "no call past the cap");
    assert!(mark(&tiny, &sid).is_none());

    // Room for one: its spend is the day's, and the sweep sees it.
    let r = rig(Some(&jev), 0, |c| {
        people_on(c);
        c.people.sweep_usd_per_day = 3.0;
    });
    import(&r, std::slice::from_ref(&e));
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(billed(answer(json!([]))));
    let s = r.core.people_sync(vec![sid.clone()]).await.unwrap();
    assert_eq!(s.swept, 1, "{s:?}");
    let today = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let day: SyncDay = r.core.store.get_meta(DAY).unwrap().unwrap();
    assert_eq!(day.day, today);
    assert!(day.spent_usd > 0.0);
    assert_eq!(r.core.people_sync_spent(&today).unwrap(), day.spent_usd);
    let sw = r.core.people_sweep("nightly").await.unwrap();
    assert!(sw.stopped.is_none(), "{sw:?}");
    let row = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(50)
        .unwrap()
        .into_iter()
        .map(|(_, x)| x)
        .find(|x| x.kind == "people.swept")
        .expect("the sweep's row");
    assert_eq!(row.data["spent_before_usd"].as_f64(), Some(day.spent_usd));
}

/// The step is off with the sweep: a cap of 0 reads nothing and keeps nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_people_step_is_off_when_the_sweep_is() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, |c| {
        people_on(c);
        c.people.sweep_usd_per_day = 0.0;
    });
    let e = sync(13, TAG, &[0, 2]);
    import(&r, std::slice::from_ref(&e));
    let sid = session_id_of(e["episode_id"].as_str().unwrap());
    let s = r.core.people_sync(vec![sid.clone()]).await.unwrap();
    assert_eq!((s.swept, s.done.clone()), (0, vec![sid]));
    assert!(s.stopped.as_deref().unwrap().contains("is off"), "{s:?}");
    assert!(r.fake.requests().is_empty());
}
