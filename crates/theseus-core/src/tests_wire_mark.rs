//! What the wire shows of a session's data (theseus-jid7t.2, theseus-gyjyo.5):
//! `SessionInfo.data` and `WorkView.data` read the session's labels
//! (`ContentFlags::mark`), and `ProfileInfo.origin` / `effort` say what a
//! profile is. The mark is `open` with no label, `private` for a label that
//! keeps a model of restricted origin off, `sensitive` for `personal`.

use serde_json::json;
use theseus_protocol::ops::DataMark;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::SessionListResult;

use crate::catalog::ModelOrigin;
use crate::content::Flag;
use crate::push::WorkScope;
use crate::test_support::label_session;
use crate::tests_route::{call, rig};

fn session(core: &crate::rpc::Core) -> String {
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn listed(r: &crate::tests_route::Rig, sid: &str) -> Option<DataMark> {
    let all: SessionListResult =
        serde_json::from_value(call(&r.core, "session.list", json!({})).await).unwrap();
    all.sessions
        .into_iter()
        .find(|s| s.session_id == sid)
        .expect("listed")
        .data
}

#[tokio::test]
async fn session_list_marks_a_session_by_its_labels() {
    let r = rig(None, 1, |_| {});
    let (plain, private, personal) = (session(&r.core), session(&r.core), session(&r.core));
    label_session(&r.core, &private, Flag::Confidential);
    label_session(&r.core, &personal, Flag::Personal);
    assert_eq!(listed(&r, &plain).await, Some(DataMark::Open));
    assert_eq!(listed(&r, &private).await, Some(DataMark::Private));
    assert_eq!(listed(&r, &personal).await, Some(DataMark::Sensitive));
}

/// A conversation's view and a task's, in a snapshot and in the frames sent as
/// they change, are stamped with their own labels' mark: a task started before
/// its parent took a label does not take it (a task copies its parent's labels
/// when it starts), and one with a label of its own shows it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn work_views_carry_the_mark_of_their_labels() {
    use crate::tests_tasks::{
        answers_a_call, arranged, exec, first_user, only_task, parent_session, rig as task_rig,
        turn, ASK,
    };
    let mut r = task_rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return crate::provider::Scripted::text("Counted.");
            }
            if answers_a_call(req) {
                return crate::provider::Scripted::text("Started it.");
            }
            crate::provider::Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                           "arrangement": arranged()}),
                )],
            )
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    let (mut w, _) = crate::tests_work::Watcher::start(&r.core, WorkScope::default()).await;
    turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    let task = only_task(&r.core, &parent);
    let by_session = |views: Vec<WorkView>, sid: &str| {
        views
            .into_iter()
            .find(|v| v.session_id.as_deref() == Some(sid))
            .unwrap_or_else(|| panic!("no view of {sid}"))
    };
    let all =
        |r: &crate::tests_tasks::Rig| r.core.work_snapshot_marked(200, &WorkScope::default()).1;
    // Open while nothing carries a label.
    let views = all(&r);
    assert_eq!(
        by_session(views.clone(), &parent).data,
        Some(DataMark::Open)
    );
    assert_eq!(
        by_session(views, &task.session_id).data,
        Some(DataMark::Open)
    );
    // The parent takes a label after the task started; the task has a label of its own.
    label_session(&r.core, &parent, Flag::Confidential);
    label_session(&r.core, &task.session_id, Flag::Personal);
    let views = all(&r);
    let (p, t) = (
        by_session(views.clone(), &parent),
        by_session(views, &task.session_id),
    );
    assert_eq!(
        (p.kind, p.data),
        (WorkKind::Conversation, Some(DataMark::Private))
    );
    assert_eq!(
        (t.kind, t.data),
        (WorkKind::Task, Some(DataMark::Sensitive))
    );
    // The frames the board sends say it too, and carry no mark for a job.
    r.model.gate.add_permits(1);
    crate::tests_tasks::until("the task completes", || {
        exec(&r.core, &task.id).state == theseus_kernel::ExecState::Complete
    })
    .await;
    let held = w.held();
    let heard = held
        .values()
        .find(|v| v.session_id.as_deref() == Some(&task.session_id));
    assert_eq!(heard.expect("heard").data, Some(DataMark::Sensitive));
    assert!(held
        .values()
        .filter(|v| !matches!(v.kind, WorkKind::Conversation | WorkKind::Task))
        .all(|v| v.data.is_none()));
}

#[tokio::test]
async fn profile_list_names_each_profiles_origin() {
    for (vouched, want) in [
        (None, "cn"),
        (Some(ModelOrigin::Unknown), "unknown"),
        (Some(ModelOrigin::Eu), "eu"),
    ] {
        let r = rig(None, 1, |c| {
            if let Some(o) = vouched {
                c.serve_glm_unrestricted();
                c.vouch_zai(o);
            }
        });
        let list = r.core.profile_list();
        let origin = |n: &str| {
            list.profiles
                .iter()
                .find(|p| p.name == n)
                .and_then(|p| p.origin.clone())
        };
        assert_eq!(origin("sonnet").as_deref(), Some("us"));
        assert_eq!(origin("glm").as_deref(), Some(want), "{vouched:?}");
    }
}

/// `ProfileInfo.effort` is the effort the profile is configured at, so the
/// cockpit can group `opus` and `opushi` as one model at two efforts before
/// any turn has said what it ran at (theseus-yxkww); a profile with none
/// configured says none.
#[tokio::test]
async fn profile_list_carries_each_profiles_configured_effort() {
    let r = rig(None, 1, |_| {});
    let list = r.core.profile_list();
    let effort = |n: &str| {
        list.profiles
            .iter()
            .find(|p| p.name == n)
            .unwrap_or_else(|| panic!("no profile {n}"))
            .effort
            .clone()
    };
    assert_eq!(effort("opushi").as_deref(), Some("high"));
    assert_eq!(effort("haiku").as_deref(), Some("low"));
    assert_eq!(effort("sonnet"), None);
}
