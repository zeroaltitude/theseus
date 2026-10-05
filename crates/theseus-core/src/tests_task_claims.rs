//! Claim leases (M7 step 39b, theseus-ext.14): two executions claim one task
//! and the second is blocked, with the holder and the time; a renewal keeps
//! the version, the holder's edits renew, a close ends the claim, and another
//! session's edit is not held back; a lapsed lease is freed by the due pass at
//! its `until`, not before, from the claims kept in memory, its record and
//! its `task.lease_expired` row in one frame.

use serde_json::json;

use crate::node::ResultStatus;
use crate::session::SessionRecord;
use crate::task_graph::{self as graph, TaskRecord};
use crate::tests_task_graph::{plan, results, rig, rows, session, turn, Rig};

fn execution_of(r: &Rig, sid: &str) -> String {
    let s: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    s.execution_id.expect("its execution")
}

/// A plan item, recorded from a fresh session.
async fn item(r: &Rig, title: &str) -> (String, TaskRecord) {
    let s = session(&r.core);
    r.calls(&[plan(title)]);
    turn(&r.core, &s, "plan it").await;
    (s, r.by_title(title))
}

async fn claim(r: &Rig, sid: &str, t: &TaskRecord, version: u64) -> (ResultStatus, String) {
    r.calls(&[("task_claim", json!({"id": t.id, "version": version}))]);
    turn(&r.core, sid, "claim it").await;
    results(&r.core, sid, "task.claim")
        .pop()
        .expect("its result")
}

/// The design's 39b test: two executions claim one task. The first holds it
/// (one `task.claimed`, the version moved by one); the second is refused
/// whatever version it names, `blocked: claimed by session … until HH:MM`,
/// and nothing of the record moves.
#[tokio::test]
async fn two_executions_claim_one_task_and_the_second_is_blocked() {
    let r = rig();
    let c = &r.core;
    c.warm_leases();
    let (a, t) = item(&r, "Chart the reef").await;
    let b = session(c);

    let (status, text) = claim(&r, &a, &t, 1).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let held = r.task(&t.id);
    assert_eq!(held.version, 2, "a claim moves the version");
    let cl = held.claim.clone().expect("claimed");
    assert_eq!(cl.by, execution_of(&r, &a));
    assert_eq!(cl.session, a);
    assert_eq!(c.tools.leases.held(), Some(1), "kept in memory");

    for version in [1, 2] {
        let (status, text) = claim(&r, &b, &t, version).await;
        assert_eq!(status, ResultStatus::Error, "{text}");
        let says = format!(
            "blocked: claimed by session {} until {}",
            cl.session_short(),
            crate::push::hm(cl.until_ms)
        );
        assert!(text.starts_with(&says), "{text}");
    }
    assert_eq!(r.task(&t.id), held, "the record as it was");
    assert_eq!(rows(c, "task.claimed").len(), 1);
    assert!(
        rows(c, "task.stale_refused").is_empty(),
        "a held task refuses before its version is compared"
    );
    // The view and the surfaces show it.
    assert!(
        graph::line(&held).contains(&format!("claimed by session {} until", cl.session_short())),
        "{}",
        graph::line(&held)
    );
}

/// The holder claiming again renews its lease and keeps the version; its
/// edit renews in the frame the edit writes; another session's edit is not
/// held back (compare-and-swap guards it) and leaves the claim; a close ends
/// it.
#[tokio::test]
async fn a_renewal_keeps_the_version_and_a_close_ends_the_claim() {
    let r = rig();
    let c = &r.core;
    c.warm_leases();
    let (a, t) = item(&r, "Chart the reef").await;
    let b = session(c);
    claim(&r, &a, &t, 1).await;
    let first = r.task(&t.id).claim.unwrap();

    let (status, text) = claim(&r, &a, &t, 2).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.starts_with("Renewed your claim"), "{text}");
    let renewed = r.task(&t.id);
    assert_eq!(renewed.version, 2, "a renewal alone never moves it");
    let second = renewed.claim.clone().unwrap();
    assert!(second.until_ms >= first.until_ms);
    assert_eq!(second.by, first.by);
    let claimed = rows(c, "task.claimed");
    assert_eq!(claimed.len(), 2);
    assert_eq!(claimed[1].data["renewed"], true);
    assert!(claimed[1].data.get("from").is_none(), "{:?}", claimed[1]);

    // The holder's edit: a version for the edit alone, and the lease renewed.
    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": 2, "patch": {"title": "Chart the reef's north edge"}}),
    )]);
    turn(c, &a, "rename it").await;
    let edited = r.task(&t.id);
    assert_eq!(edited.version, 3);
    assert!(edited.claim.as_ref().unwrap().until_ms >= second.until_ms);

    // Another session's edit applies, and the claim stays the holder's.
    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": 3, "patch": {"deps": []}}),
    )]);
    turn(c, &b, "touch it").await;
    let (status, text) = results(c, &b, "task.update").pop().unwrap();
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let touched = r.task(&t.id);
    assert_eq!(touched.version, 4);
    assert_eq!(touched.claim.as_ref().unwrap().by, first.by);

    // The holder closes it: the claim ends, and nothing is kept for it.
    r.calls(&[(
        "task_close",
        json!({"id": t.id, "version": 4, "outcome": "done", "evidence": [{"identity": "commit:0a1b2c3d"}]}),
    )]);
    turn(c, &a, "close it").await;
    let closed = r.task(&t.id);
    assert!(
        closed.state.is_closed() && closed.claim.is_none(),
        "{closed:?}"
    );
    assert_eq!(c.tools.leases.held(), Some(0));
}

/// The design's 39b test: a lease expires under the virtual clock. The due
/// pass, given the time, frees the task at its `until` and not a
/// millisecond before, from the claims built after serving (a build after
/// the claim reads it from its record): the record without its
/// claim and a `task.lease_expired` row in one frame, the version kept. A
/// second pass finds nothing.
#[tokio::test]
async fn a_lapsed_lease_is_freed_by_the_due_pass_at_its_until_not_before() {
    let r = rig();
    let c = &r.core;
    let (a, t) = item(&r, "Chart the reef").await;
    claim(&r, &a, &t, 1).await;
    let held = r.task(&t.id);
    let until = held.claim.as_ref().unwrap().until_ms;
    // The driver's first tick built the claims; a build after the claim
    // reads it from its record.
    let fresh = crate::task_graph::lease::Leases::new(60_000);
    assert_eq!(fresh.due(until), None, "nothing is kept before the build");
    assert!(fresh.build(&c.store).unwrap());
    assert_eq!(fresh.due(until - 1), Some(vec![]));
    assert_eq!(
        fresh.due(until),
        Some(vec![t.id.clone()]),
        "built from the record"
    );
    c.warm_leases();
    assert_eq!(c.tools.leases.held(), Some(1));

    assert_eq!(c.free_expired_leases(until - 1), 0);
    assert_eq!(r.task(&t.id), held, "held until its time");
    assert!(rows(c, "task.lease_expired").is_empty());

    let frames = theseus_store::frames_written_here();
    assert_eq!(c.free_expired_leases(until), 1);
    assert_eq!(
        theseus_store::frames_written_here(),
        frames + 1,
        "one frame"
    );
    let freed = r.task(&t.id);
    assert!(freed.claim.is_none(), "{freed:?}");
    assert_eq!(freed.version, held.version, "the lease's end is no edit");
    let lapsed = rows(c, "task.lease_expired");
    assert_eq!(lapsed.len(), 1);
    assert_eq!(lapsed[0].data["task"], t.id.as_str());
    assert_eq!(lapsed[0].data["session"], a.as_str());
    assert_eq!(c.tools.leases.held(), Some(0));
    assert_eq!(c.free_expired_leases(until + 60_000), 0);

    // Free, another session claims it.
    let b = session(c);
    let (status, text) = claim(&r, &b, &t, held.version).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
}

/// A claim past its `until` reads free before the pass clears it: the
/// record says so to every surface (`claim_at`), and a lease's minutes come
/// from `[kernel] task_lease_minutes`, at least one.
#[test]
fn a_claim_past_its_until_reads_free() {
    let mut t = TaskRecord {
        id: "tsk_0000reef".into(),
        ..TaskRecord::default()
    };
    t.claim = Some(graph::TaskClaim {
        by: "exe_0000reef".into(),
        session: "ses_0000reef".into(),
        until_ms: 1_000,
    });
    assert!(t.claim_at(999).is_some());
    assert!(t.claim_at(1_000).is_none());
    let mut k = crate::config::KernelSection::default();
    assert_eq!(k.task_lease_ms(), 30 * 60_000);
    k.task_lease_minutes = 0;
    assert_eq!(k.task_lease_ms(), 60_000);
}
