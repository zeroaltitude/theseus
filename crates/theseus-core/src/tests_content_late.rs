//! A background job's late result is judged like one read in the turn
//! (theseus-vahzo; theseus-9ymz0's `queue_content`). The job ends after its
//! turn, a continuation takes its result into the session
//! (`toolrun::late::absorb`), and the session's content flags must come to
//! hold it among the judged items: the real judge, against the fake Jev.
//! Nothing here names `queue_content`: the test reads the classification, so a
//! refactor that moves the call keeps it, and one that loses it fails it.
//! (A result nobody judged is a label never raised: the next restricted-origin
//! request is sent without it, far from its cause.)
//!
//! This pins the classification half of the row, and only that. The other half
//! of the issue's sentence -- that the late result is judged *before the next
//! request is routed* -- is an ordering, and is not pinned here: pinning it
//! needs the restricted-origin rig plus a late result through `turn.rs`'s
//! mid-turn site, where the stand-in judge lands verdicts whether or not
//! anything queued the item. Nor is it pinned that the other two `absorb`
//! callers still call it: this test rides `catch_up` alone, so dropping
//! `take_late` at the mid-turn or the finish site leaves it green. The
//! `queue_content` line itself is covered for all three, since all three
//! reach the same `absorb`.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::content::ContentFlags;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_judge::{rig_on, Rig};
use crate::tests_route::try_turn_attached;

/// Jev: nothing in anything is injection, a secret, confidential or personal.
fn clean(jev: &FakeJev) {
    for q in ["injection", "secret", "confidential", "personal"] {
        jev.script(
            q,
            Jev::Choice {
                option: "no".into(),
                confidence: 0.95,
            },
        );
    }
}

fn rig(jev: &FakeJev, script: Vec<Scripted>) -> Rig {
    rig_on(script, Some(jev), |c| {
        // content.v3 live (the judge rigs turn it off), the rest as they are.
        c.judge.packs.remove(crate::judge::content::PACK);
        c.judge.total_secs = 5;
        c.tools.proc_sync_secs = 1;
        c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
        c.model.retries.transient = 0;
        c.policy.tools.insert("proc.run".into(), Posture::Open);
    })
}

/// The ids of the session's transcript, in order, and the late results' among
/// them.
fn late_ids(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| matches!(n.body, Body::ToolResult { late: true, .. }))
        .map(|(_, n)| n.id.clone())
        .collect()
}

/// Whether the judge's mark has reached `node`: the mark's node is `node` or a
/// later one of the transcript.
fn judged(r: &Rig, sid: &str, node: &str) -> bool {
    let nodes = r.core.store.session_nodes(sid).unwrap();
    let at = |id: &str| nodes.iter().position(|(_, n)| n.id == id);
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    let flags: ContentFlags = rec.data.map(|b| *b).unwrap_or_default();
    match (flags.judged_through.as_deref().and_then(at), at(node)) {
        (Some(mark), Some(late)) => mark >= late,
        _ => false,
    }
}

/// The job ends after its turn, and the continuation that reads its result
/// leaves the result judged: in the session's flags, by the judge, not merely
/// written. Read at the turn's return, the mark is for the judge's own
/// asynchronous landing, so the wait is its own bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_result_is_judged_once_its_continuation_has_taken_it() {
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    let slow = Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            json!({"argv": ["bash", "-c", "sleep 1.5; echo slow done"]}),
        )],
    );
    let r = rig(
        &jev,
        vec![
            slow,
            Scripted::text("Started; I'll report back."),
            Scripted::text("The slow job finished."),
        ],
    );
    let first = try_turn_attached(&r.core, None, "run the slow build", None, vec![])
        .await
        .unwrap();
    let sid = first.session_id.clone();
    let exec = first.execution_id.clone().unwrap();
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        r.core.heartbeat("test");
        if r.core
            .kernel
            .execution(&exec)
            .unwrap()
            .unwrap()
            .state
            .as_str()
            == "queued"
        {
            break;
        }
    }
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        e.state.as_str(),
        "queued",
        "the job's end queued the execution"
    );
    assert!(
        late_ids(&r, &sid).is_empty(),
        "no late result is written yet"
    );

    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert!(cont.continuation);
    let late = late_ids(&r, &sid);
    assert_eq!(late.len(), 1, "the continuation wrote the late result");

    let t0 = Instant::now();
    while !judged(&r, &sid, &late[0]) {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the late result {} was never judged: the session's items stay unjudged",
            late[0]
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
