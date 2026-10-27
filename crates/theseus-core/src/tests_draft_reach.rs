//! What a China-origin draft's view still bounds (theseus-9ymz0, narrowing the
//! open view into `draft_view.rs`): a draft is a model of restricted origin
//! reading a session, so it is offered no tool that starts or schedules work
//! outside it, reads a transcript's file under §3.9's hold, and may change only
//! its own task's record. Ported from the open view's tests of the same
//! behaviours, each against the draft's reach. Every name here is invented.

use serde_json::{json, Value};

use crate::provider::Scripted;
use crate::test_support::turn;
use crate::tests_tasks::{answers_a_call, parent_session, rig};

const BRIEF: &str = "Count the boats in the west quay";

fn record(core: &crate::Core, sid: &str) -> crate::session::SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

/// theseus-qul8y 1: a draft is offered no tool that starts or schedules work
/// outside it (`task.create`, `task.split`, `task.claim`, `wake.at`), and a
/// call of one is refused at the gate; it keeps the public tools that read, or
/// act on a task's own record. A shared place's offer is unchanged.
#[tokio::test]
async fn a_draft_is_not_offered_the_tools_that_start_work() {
    use crate::places::{offered, PlaceClass};
    let reach = crate::draft_view::DraftReach::of(vec![], std::path::Path::new("/tmp"));
    const DROPPED: [&str; 4] = ["task.create", "task.split", "task.claim", "wake.at"];
    for name in DROPPED {
        assert!(!reach.offers(name), "{name} is offered");
        assert!(offered(PlaceClass::Shared, name, &[]), "{name}: shared");
    }
    for name in [
        "web.search",
        "http.fetch",
        "tools.find",
        "task.update",
        "task.close",
    ] {
        assert!(reach.offers(name), "{name} is not offered");
    }
    assert!(
        !reach.offers("file.read"),
        "a transcript's file is the owner's"
    );
    // An allowlist: a public task tool added later is not offered to a draft
    // until it is judged there, though a shared place offers it.
    assert!(!reach.offers("task.schedule"), "an unjudged task tool");
    assert!(offered(PlaceClass::Shared, "task.schedule", &[]));

    // A call of each dropped tool from a draft's turn is refused.
    let calls: Vec<(String, &str, Value)> = [
        ("task_create", json!({"brief": BRIEF})),
        ("wake_at", json!({"after": "1h", "note": "check the quay"})),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (n, v))| (format!("t{i}"), n, v))
    .collect();
    let r = rig(
        move |req| match answers_a_call(req) {
            true => Scripted::text("Done."),
            false => {
                let calls: Vec<(&str, &str, Value)> = calls
                    .iter()
                    .map(|(i, n, v)| (i.as_str(), *n, v.clone()))
                    .collect();
                Scripted::tools("", &calls)
            }
        },
        |_| {},
    );
    let sid = parent_session(&r.core);
    crate::tests_dm_rig::mark_draft(&r.core, &sid);
    turn(&r.core, &sid, "START the count").await;
    let sent = r.model.requests.lock().unwrap().clone();
    let first = sent.first().expect("the turn's request");
    let names: Vec<&str> = first
        .tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for gone in ["task_create", "task_split", "task_claim", "wake_at"] {
        assert!(!names.contains(&gone), "{gone} offered: {names:?}");
    }
    let results = serde_json::to_string(&sent.last().unwrap().messages).unwrap();
    for name in ["task.create", "wake.at"] {
        assert!(
            results.contains(&format!("{name} is not offered in an open request")),
            "{name}: {results}"
        );
    }
    let rec = record(&r.core, &sid);
    assert!(
        r.core
            .kernel
            .tasks(rec.execution_id.as_deref())
            .unwrap()
            .is_empty(),
        "no task started"
    );
}

/// theseus-mhqlm F2: a transcript file a draft reads is outside text (§3.9's
/// hold), as a shared place's is. The draft's reach does not offer `file.read`
/// at all, so the call is refused and nothing is read; the hold is the second
/// guard if it ever is: whatever the gate says, the file's text never reaches a
/// request unless the session holds it.
#[tokio::test]
async fn a_drafts_transcript_read_takes_the_outside_text_hold() {
    const SAID: &str = "The widgetco quay ledger: 41 bollards, 3 cranes.";
    let r = rig(
        |req| match answers_a_call(req) {
            true => Scripted::text("Read it."),
            false => Scripted::tools("", &[("f1", "file_read", json!({"name": "ledger.txt"}))]),
        },
        |_| {},
    );
    let sid = parent_session(&r.core);
    crate::tests_dm_rig::mark_draft(&r.core, &sid);
    let attached = theseus_protocol::Attachment {
        name: "ledger.txt".into(),
        media_type: "text/plain".into(),
        size: SAID.len() as u64,
        text: None,
        data: Some(crate::blobs::encode(SAID.as_bytes())),
        not_read: None,
    };
    crate::tests_route::turn_attached(&r.core, Some(&sid), "Read the ledger", None, vec![attached])
        .await;
    let sent = r.model.requests.lock().unwrap().clone();
    assert!(
        sent.len() >= 2,
        "the call's result was sent: {}",
        sent.len()
    );
    let last = sent.last().unwrap().messages.last().cloned().unwrap();
    let result = &last["content"][0];
    assert_eq!(result["type"], "tool_result", "{last}");
    let read = result["is_error"] != json!(true);
    let refused = result
        .to_string()
        .contains("file.read is not offered in an open request");
    assert!(read || refused, "the call failed otherwise: {result}");
    let held = record(&r.core, &sid)
        .external
        .is_some_and(|h| h.url.starts_with("attachment:"));
    assert!(!read || held, "read without the hold: {result}");
}

/// theseus-ol8w0: a draft's `task.update` and `task.close` reach only its own
/// task's record; a call naming another is refused before any write. A call on
/// its own record passes the check, whole or by the end of its id; a shared
/// place's or a private session's pass whatever they name.
#[test]
fn a_drafts_own_record_passes_the_check() {
    use crate::toolrun::batch::own_record_only;
    let session = "ses_abc123def456";
    let own = crate::task_graph::of_session(session);
    for tool in ["task.update", "task.close"] {
        let by = |id: &str| own_record_only(true, Some(session), tool, &json!({"id": id}));
        assert_eq!(by(&own), None);
        assert_eq!(by(&own[own.len() - 8..]), None);
        assert!(by("tsk_other").is_some_and(|w| w.contains("only its own task")));
        assert!(own_record_only(true, None, tool, &json!({"id": own})).is_some());
        assert_eq!(
            own_record_only(false, Some(session), tool, &json!({"id": "tsk_other"})),
            None
        );
    }
    assert_eq!(
        own_record_only(
            true,
            Some(session),
            "tools.find",
            &json!({"id": "tsk_other"})
        ),
        None
    );
}
