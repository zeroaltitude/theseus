//! The data mark on what the Operations page lists (theseus-jid7t.2):
//! `SessionInfo.data`, `WorkView.data` and `ProfileInfo.origin`.

use serde_json::json;
use theseus_protocol::ops::DataMark;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::SessionListResult;

use crate::catalog::ModelOrigin;
use crate::provider::Scripted;
use crate::push::WorkScope;
use crate::tests_route::{call, rig};
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, only_task, parent_session, rig as task_rig, turn,
    ASK,
};
use crate::Config;

fn google_on(c: &mut Config) {
    c.google.account = "owner@example.test".into();
    c.secrets.insert(
        "google_client".into(),
        "op://vault/google client/notesPlain".into(),
    );
    c.secrets.insert(
        "google_refresh_token".into(),
        "file:/tmp/unused-google-refresh-token".into(),
    );
}

fn session(core: &crate::rpc::Core) -> String {
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// Mark `sid` private, as a Google read does.
fn mark(core: &crate::rpc::Core, sid: &str) {
    core.store
        .put_meta(&crate::google::private::meta_key(sid), &"nd_test")
        .unwrap();
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
async fn session_list_marks_a_plain_session_open_and_a_marked_one_private() {
    let r = rig(None, 1, google_on);
    let (plain, private) = (session(&r.core), session(&r.core));
    mark(&r.core, &private);
    assert_eq!(listed(&r, &plain).await, Some(DataMark::Open));
    assert_eq!(listed(&r, &private).await, Some(DataMark::Private));
}

#[tokio::test]
async fn without_google_every_session_is_open() {
    let r = rig(None, 1, |_| {});
    let sid = session(&r.core);
    mark(&r.core, &sid); // a stale mark is not read while Google is off
    assert_eq!(listed(&r, &sid).await, Some(DataMark::Open));
}

/// A conversation's view and a task's, in a snapshot and in the frames sent
/// as they change: a task is private while its parent is, however late the
/// parent took the mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn work_views_carry_the_mark_and_a_task_follows_its_parent() {
    let mut r = task_rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("Counted.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                           "arrangement": arranged()}),
                )],
            )
        },
        google_on,
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
    // Open while nothing was read.
    let views = all(&r);
    assert_eq!(
        by_session(views.clone(), &parent).data,
        Some(DataMark::Open)
    );
    assert_eq!(
        by_session(views, &task.session_id).data,
        Some(DataMark::Open)
    );
    // The parent reads Google after the task started.
    mark(&r.core, &parent);
    let views = all(&r);
    let (p, t) = (
        by_session(views.clone(), &parent),
        by_session(views, &task.session_id),
    );
    assert_eq!(
        (p.kind, p.data),
        (WorkKind::Conversation, Some(DataMark::Private))
    );
    assert_eq!((t.kind, t.data), (WorkKind::Task, Some(DataMark::Private)));
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
    assert_eq!(heard.expect("heard").data, Some(DataMark::Private));
    assert!(held
        .values()
        .filter(|v| !matches!(v.kind, WorkKind::Conversation | WorkKind::Task))
        .all(|v| v.data.is_none()));
}

/// A sensitive read reaches the mark (the join fix J3 of theseus-jid7t.1 and
/// .2): the session lists `sensitive`, a task under it stamps `sensitive` on
/// its view, and its report is not moved into a home marked only private.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sensitive_read_marks_the_session_and_a_task_under_it_sensitive() {
    let mut r = task_rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("Counted.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                           "arrangement": arranged()}),
                )],
            )
        },
        google_on,
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    // The board holds views once something watches it.
    let (_w, _) = crate::tests_work::Watcher::start(&r.core, WorkScope::default()).await;
    turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    let task = only_task(&r.core, &parent);
    mark(&r.core, &parent);
    r.core
        .store
        .put_meta(&crate::google::private::sensitive_key(&parent), &"nd_test")
        .unwrap();
    let all: SessionListResult =
        serde_json::from_value(call(&r.core, "session.list", json!({})).await).unwrap();
    let listed = all
        .sessions
        .into_iter()
        .find(|s| s.session_id == parent)
        .expect("listed")
        .data;
    assert_eq!(listed, Some(DataMark::Sensitive));
    let views = r.core.work_snapshot_marked(200, &WorkScope::default()).1;
    let view = views
        .into_iter()
        .find(|v| v.session_id.as_deref() == Some(task.session_id.as_str()))
        .expect("the task's view");
    assert_eq!(view.data, Some(DataMark::Sensitive));
    let home = session(&r.core);
    mark(&r.core, &home);
    let rehome = theseus_protocol::ops::TaskRehomeParams {
        task: task.id.clone(),
        home: home.clone(),
        author: None,
        discord: None,
    };
    let said = format!(
        "{:#}",
        r.core
            .task_rehome(&rehome, &crate::test_support::owner_cli())
            .unwrap_err()
    );
    assert!(
        said.contains("a report never goes to a weaker home"),
        "{said}"
    );
    r.model.gate.add_permits(1);
    crate::tests_tasks::until("the task completes", || {
        exec(&r.core, &task.id).state == theseus_kernel::ExecState::Complete
    })
    .await;
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
