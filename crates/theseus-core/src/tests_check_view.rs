//! A check sees the checked task by title and state (theseus-w8ys), through
//! the whole core: a checked task that set deps and acceptance, with a task
//! under it, shows id, title, and state alone in its check's view, counted in
//! `context.compiled`'s `tasks.restricted`; the parent conversation's view is
//! unchanged. The pure rules are `task_graph::tests`.

use serde_json::json;

use crate::node::ResultStatus;
use crate::task_graph::{self as graph, view, TaskState};
use crate::tests_check::{
    check, finished, first_user, maker, parent_session, requests_with, rig, rows, the_check, turn,
    CHECK_BRIEF,
};

/// The view block of a request: the last block of its last message.
fn view_of(req: &crate::provider::ProviderRequest) -> Option<String> {
    let last = req.messages.last()?["content"].as_array()?.last()?.clone();
    view::is_view(&last).then(|| last["text"].as_str().unwrap_or_default().to_string())
}

#[tokio::test]
async fn a_checks_view_shows_the_checked_task_by_title_and_state() {
    let r = rig();
    let c = &r.core;
    let sid = parent_session(c);
    let m = maker(&r, &sid).await;
    let record = graph::of_session(&m.session_id);

    // The checked task waits on a plan item, keeps its acceptance, and has a
    // task under it.
    let now = theseus_protocol::now_unix_ms();
    let tide = graph::NewTask {
        id: "tsk_00000000000000000000000000tide".into(),
        title: "Read the tide table",
        objective: "read it".into(),
        acceptance: vec!["the low tide is known".into()],
        parent: None,
        deps: vec![],
        session: None,
        origin: graph::TaskOrigin {
            session: sid.clone(),
            principal: "operator".into(),
            by_model: true,
        },
        state: TaskState::Accepted,
    }
    .build(now);
    let mut sub = tide.clone();
    sub.id = "tsk_000000000000000000000000000sub".into();
    sub.title = "Count the south pier too".into();
    sub.parent = Some(record.clone());
    let mut checked = graph::get(&c.store, &record).unwrap().unwrap();
    checked.deps = vec![tide.id.clone()];
    checked.version += 1;
    assert!(!checked.acceptance.is_empty(), "{checked:?}");
    c.store
        .append(&[
            graph::record(&tide).unwrap(),
            graph::record(&sub).unwrap(),
            graph::record(&checked).unwrap(),
        ])
        .unwrap();

    // A check of it, under it.
    let (status, content, meta) = check(
        &r,
        &sid,
        json!({"brief": CHECK_BRIEF, "check_of": record, "parent": record}),
    )
    .await;
    assert_eq!(status, ResultStatus::Ok, "{content}");
    let e = the_check(&r, &meta);
    finished(c, &e).await;

    let checked = graph::shown(&c.kernel, graph::get(&c.store, &record).unwrap().unwrap());
    let bare = format!(
        "- {} \"{}\" [{}]",
        checked.id,
        checked.title,
        checked.state.as_str()
    );
    let seen: Vec<String> = requests_with(&r.model, "CHECKBRIEF")
        .iter()
        .filter_map(view_of)
        .collect();
    assert!(!seen.is_empty(), "the check's turns saw the graph");
    for v in &seen {
        let first = v.lines().nth(1).unwrap();
        assert_eq!(first, bare, "{v}");
        assert!(!v.contains("deps"), "no deps: {v}");
        assert!(!v.contains(&format!("{} \"", tide.id)), "{v}");
    }
    let compiled = rows(c, "context.compiled");
    let restricted: Vec<u64> = compiled
        .iter()
        .filter(|r| r.session_id.as_deref() == Some(e.session_id.as_str()))
        .filter_map(|r| r.data["tasks"]["restricted"].as_u64())
        .collect();
    assert!(
        !restricted.is_empty() && restricted.iter().all(|n| *n >= 1),
        "{restricted:?}"
    );

    // The parent's view is whole: the checked task's line as ever, and the
    // task under it.
    turn(c, &sid, "where are we").await;
    let parent = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|q| !first_user(q).starts_with("[Task "))
        .cloned()
        .unwrap();
    let v = view_of(&parent).expect("the parent's view");
    assert!(v.contains(&graph::line(&checked)), "{v}");
    assert!(v.contains(&graph::line(&sub)), "{v}");
    let last = rows(c, "context.compiled").pop().unwrap();
    assert_eq!(last.session_id.as_deref(), Some(sid.as_str()));
    assert!(last.data["tasks"].get("restricted").is_none(), "{last:?}");
}
