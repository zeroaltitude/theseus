//! `work.digest` through whole cores (theseus-qkoi3): the scenario of the work
//! board's tests, told from the notice ring and from the ledger, for one
//! window; a window past the ring's start; one task's own timeline; and what a
//! connection that reads no private text may be told.

use std::collections::BTreeMap;
use std::collections::HashSet;

use theseus_protocol::{DigestKind, WorkDigestParams, WorkDigestResult};

use super::*;
use crate::approval::Surface;
use crate::tests_tasks::{parent_session, until};

/// What a digest tells, as (kind, the work's id) with how many each: the two
/// sources agree on this, not on the second a row was written.
fn tally(r: &WorkDigestResult) -> BTreeMap<(DigestKind, String), u32> {
    let mut out = BTreeMap::new();
    for l in &r.lines {
        *out.entry((l.kind, l.work.id.clone())).or_default() += l.count;
    }
    out
}

fn of_kind(r: &WorkDigestResult, kind: DigestKind) -> Vec<String> {
    r.lines
        .iter()
        .filter(|l| l.kind == kind)
        .map(|l| l.work.title.clone())
        .collect()
}

/// The whole scenario, then the digest from the position before it began.
async fn told() -> (Scene, u64) {
    let mut s = Scene::new().await;
    let (since, _, _) = {
        let (p, v, t) = s.r.core.push.work_snapshot(200, &WorkScope::default());
        (p, v, t)
    };
    the_tree_is_built(&mut s).await;
    let q = a_cancel_a_job_and_a_question(&mut s).await;
    the_answer(&mut s, &q).await;
    a_wake_and_the_steps_close(&mut s).await;
    // The last frames reach the board and the ledger.
    tokio::time::sleep(Duration::from_millis(100)).await;
    (s, since)
}

fn all() -> (WorkScope, HashSet<String>) {
    (WorkScope::default(), HashSet::new())
}

/// The ring and the ledger tell one window alike: the same things happened,
/// each once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_ring_and_the_ledger_agree_on_the_same_window() {
    let (s, since) = told().await;
    let (scope, named) = all();
    let ring =
        s.r.core
            .digest_blocking(since, None, &scope, &named, false)
            .unwrap();
    let ledger =
        s.r.core
            .digest_blocking(since, None, &scope, &named, true)
            .unwrap();
    assert!(!ring.from_ledger, "the ring reaches back to the start");
    assert!(ledger.from_ledger);
    assert_eq!((ring.from, ring.to), (ledger.from, ledger.to.max(ring.to)));
    // The scenario: A and C report, B is cancelled, the question is
    // answered, the wake fires (the job that ended well is no news).
    for r in [&ring, &ledger] {
        let reported: u32 = r
            .lines
            .iter()
            .filter(|l| l.kind == DigestKind::Reported)
            .map(|l| l.count)
            .sum();
        assert_eq!(reported, 2, "{:?}", r.lines);
        let titles = of_kind(r, DigestKind::Reported);
        assert!(titles.iter().any(|t| t.contains("CHILD-A")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("CHILD-C")), "{titles:?}");
        assert!(
            of_kind(r, DigestKind::Cancelled)
                .iter()
                .any(|t| t.contains("CHILD-B")),
            "{:?}",
            r.lines
        );
        assert_eq!(of_kind(r, DigestKind::Answered).len(), 1, "{:?}", r.lines);
        assert_eq!(of_kind(r, DigestKind::Woke).len(), 1, "{:?}", r.lines);
    }
    // And they tell the same things, each once: kind, work and count.
    assert_eq!(tally(&ring), tally(&ledger));
    // The board's counts and its waiting questions are the board's, either way.
    assert_eq!(ring.now, ledger.now);
    s.drain.abort();
}

/// A wake is told once however a client's position falls and whichever source
/// answers: at every position of the scenario the ring and the ledger tell the
/// same wakes. The ring's old notice for a wake came a frame before its
/// `wake.fired` row, so a client that left between the two was told it again
/// from the ledger after a restart (theseus-qkoi3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wake_is_told_the_same_from_every_position_by_both_sources() {
    let (s, since) = told().await;
    let (scope, named) = all();
    let end = s.r.core.push.work_snapshot(1, &scope).0;
    let wakes = |r: &WorkDigestResult| -> u32 {
        r.lines
            .iter()
            .filter(|l| matches!(l.kind, DigestKind::Woke | DigestKind::Reminder))
            .map(|l| l.count)
            .sum()
    };
    let mut told_somewhere = 0;
    for at in since..=end {
        let ring =
            s.r.core
                .digest_blocking(at, None, &scope, &named, false)
                .unwrap();
        let ledger =
            s.r.core
                .digest_blocking(at, None, &scope, &named, true)
                .unwrap();
        assert!(!ring.from_ledger);
        assert_eq!(wakes(&ring), wakes(&ledger), "from position {at}");
        told_somewhere += wakes(&ring);
    }
    assert!(told_somewhere > 0, "the wake is told from the start");
    s.drain.abort();
}

/// A window that starts before the ring does is read from the ledger, and says
/// so; one inside it is not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_past_the_rings_start_reads_the_ledger_and_says_so() {
    use crate::tests_tasks::{answers_a_call, first_user, last_user, rig, turn, ASK};
    let r = rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("Counted.");
            }
            if answers_a_call(req) || !last_user(req).contains("background task") {
                return Scripted::text("Noted.");
            }
            start_one("CHILD: count to three")
        },
        |_| {},
    );
    r.model.gate.add_permits(100);
    let parent = parent_session(&r.core);
    let ended = |n: usize| {
        let c = r.core.clone();
        move || {
            let done = c.kernel.tasks(None).unwrap();
            done.len() == n && done.iter().all(|e| e.state.is_terminal())
        }
    };
    // A task ends before the board exists: the ring cannot know it.
    turn(&r.core, &parent, ASK).await;
    until("the first task ends", ended(1)).await;
    r.core.push.ensure(&r.core).await.unwrap();
    let (since, _, _) = r.core.push.work_snapshot(200, &WorkScope::default());
    // (Another conversation: a task's arrangement quotes its parent's words
    // once.)
    let other = parent_session(&r.core);
    turn(&r.core, &other, ASK).await;
    until("the second task ends", ended(2)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let read = |p: WorkDigestParams| {
        let core = r.core.clone();
        async move { core.work_digest_for(p, Surface::Cli).await.unwrap() }
    };
    let reported = |d: &WorkDigestResult| -> u32 {
        d.lines
            .iter()
            .filter(|l| l.kind == DigestKind::Reported)
            .map(|l| l.count)
            .sum()
    };
    let inside = read(WorkDigestParams {
        since: Some(since),
        ..Default::default()
    })
    .await;
    assert!(!inside.from_ledger, "{inside:?}");
    assert_eq!(
        reported(&inside),
        1,
        "the second task only: {:?}",
        inside.lines
    );
    let before = read(WorkDigestParams {
        since: Some(0),
        ..Default::default()
    })
    .await;
    assert!(before.from_ledger, "the ring began after position 0");
    assert_eq!(reported(&before), 2, "both: {:?}", before.lines);
    // A time reads the ledger's index for its position.
    let ms = read(WorkDigestParams {
        since_ms: Some(0),
        ..Default::default()
    })
    .await;
    assert_eq!(reported(&ms), 2, "{:?}", ms.lines);
    assert_eq!((inside.to, before.to), (ms.to, ms.to));
}

/// `roots` names a task and gets that task's own timeline.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn roots_scope_the_digest_to_one_tasks_own_timeline() {
    let (s, since) = told().await;
    let mut s = s;
    let held = s.w.held();
    let a = held
        .values()
        .find(|v| v.title.contains("CHILD-A"))
        .expect("task A is on the board");
    let session = a.session_id.clone().expect("a task has a session");
    for since in [since, 0] {
        let one =
            s.r.core
                .work_digest_for(
                    WorkDigestParams {
                        since: Some(since),
                        roots: Some(vec![session.clone()]),
                        ..Default::default()
                    },
                    Surface::Cli,
                )
                .await
                .unwrap();
        assert!(!one.lines.is_empty(), "A has a line (since {since})");
        assert!(
            one.lines.iter().all(|l| l.work.id == a.id),
            "only A's: {:?}",
            one.lines
        );
    }
    s.drain.abort();
}

/// A connection that reads no private text names its conversations, is told of
/// those of a shared place alone, and reads their words scrubbed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shared_place_sees_only_its_roots_scrubbed() {
    use crate::places::BoundPlace;
    use crate::tests_tasks::{answers_a_call, first_user, last_user, rig, turn, ASK};
    let token = ["ghp_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8"].concat();
    let t2 = token.clone();
    let r = rig(
        move |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("Counted.");
            }
            if answers_a_call(req) || !last_user(req).contains("background task") {
                return Scripted::text("Noted.");
            }
            start_one(&format!("CHILD: count to three with {t2}"))
        },
        |_| {},
    );
    r.model.gate.add_permits(100);
    r.core.bind_places(vec![BoundPlace {
        target: "discord:channel:777".into(),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    let shared = crate::tests_places::session(&r.core, Some("channel:777"));
    let private = parent_session(&r.core);
    turn(&r.core, &shared, ASK).await;
    turn(&r.core, &private, ASK).await;
    let both = vec![shared.clone(), private.clone()];
    until("both tasks end", || {
        let tasks = r.core.kernel.tasks(None).unwrap();
        tasks.len() == 2 && tasks.iter().all(|e| e.state.is_terminal())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The owner's surface sees both, and the words as typed.
    let whole = r
        .core
        .work_digest_for(
            WorkDigestParams {
                since: Some(0),
                ..Default::default()
            },
            Surface::Cli,
        )
        .await
        .unwrap();
    assert_eq!(of_kind(&whole, DigestKind::Reported).len(), 2, "{whole:?}");
    // The binding must name conversations.
    let refused = r
        .core
        .work_digest_for(WorkDigestParams::default(), Surface::Unnamed)
        .await;
    assert!(
        refused.is_err(),
        "a surface that reads no private text names its roots"
    );
    // Naming both, it is told of the shared one's task alone, scrubbed.
    for since in [0, whole.from] {
        let told = r
            .core
            .work_digest_for(
                WorkDigestParams {
                    since: Some(since),
                    roots: Some(both.clone()),
                    ..Default::default()
                },
                Surface::Unnamed,
            )
            .await
            .unwrap();
        assert_eq!(told.lines.len(), 1, "one place's task: {:?}", told.lines);
        let words = serde_json::to_string(&told).unwrap();
        assert!(!words.contains(&token), "the scrubber ran: {words}");
        assert!(
            told.questions.iter().all(|q| !q.may_answer),
            "it may answer nothing"
        );
    }
    // Naming only a private conversation, a surface that reads no private
    // text is cut to nothing.
    told_nothing_naming_a_private_one(&r.core, &private, whole.from).await;
    // The in-process Discord binding is told of its private DM too, and of
    // nothing it did not name.
    let dm = r
        .core
        .work_digest_for(
            WorkDigestParams {
                since: Some(0),
                roots: Some(vec![private.clone()]),
                ..Default::default()
            },
            Surface::Discord,
        )
        .await
        .unwrap();
    assert_eq!(dm.lines.len(), 1);
}

/// What a surface that reads no private text may name is empty when it names
/// only a private conversation, so the place rule (`WorkScope::sees`) is what
/// cuts it to nothing, from either position.
async fn told_nothing_naming_a_private_one(core: &Arc<Core>, private: &str, from: u64) {
    for since in [0, from] {
        let none = core
            .work_digest_for(
                WorkDigestParams {
                    since: Some(since),
                    roots: Some(vec![private.to_string()]),
                    ..Default::default()
                },
                Surface::Unnamed,
            )
            .await
            .unwrap();
        assert!(none.lines.is_empty(), "{:?}", none.lines);
    }
}
