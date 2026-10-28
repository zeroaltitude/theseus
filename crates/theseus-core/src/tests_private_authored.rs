//! What another session wrote for a session to see is left out of every
//! request to a model of restricted origin (theseus-lt4xh, theseus-9ymz0's
//! review B1): a standing record kept from its words (`capture:<node>`), and
//! any milestone reminder. Requests to a model of US origin are unchanged. The
//! rig is `tests_dm_rig`'s, with a judge that finds nothing. (These were
//! written for a Google-private session's words; a label belongs to the session
//! that raised it, so it is the other session's words that are held, as
//! theseus-gyjyo.5 leaves it.)

use std::sync::Arc;

use serde_json::json;
use theseus_kernel::{NewWake, Purpose, WakeRun};
use theseus_protocol::standing::{StandingAddParams, StandingKind, StandingScope};

use crate::provider::FakeProvider;
use crate::tests_dm_rig::{
    blocked, cn_session, dm, jev, judging, record, rig_with, say, text, OPEN_A, OPEN_B,
};
use crate::tests_route::{call, mode, Rig};
use crate::Core;

/// What the private session's kept order says, and the owner's own order.
const HERON: &str = "Always cc the Heronsgate survey lead";
const SHORT: &str = "Answer in short lines";

fn rig() -> (
    Rig,
    Arc<FakeProvider>,
    tempfile::TempDir,
    theseus_judge::fake::FakeJev,
) {
    rig_mode("chat")
}

/// `rig`, Jev judging every message of `mode` (`trivial`: a detour).
fn rig_mode(
    mode_name: &str,
) -> (
    Rig,
    Arc<FakeProvider>,
    tempfile::TempDir,
    theseus_judge::fake::FakeJev,
) {
    let jev = jev(&[OPEN_A, OPEN_B]);
    mode(
        &jev,
        mode_name,
        if mode_name == "chat" { 0.2 } else { 0.95 },
    );
    let (r, zai, dir) = rig_with(&jev, None, |_| {});
    // A judge that finds nothing, so a session may use `dsflash` at all.
    judging(&r);
    (r, zai, dir, jev)
}

/// A session P (with a node) and the two orders: `std_a`
/// kept from P's node, scoped as given, and `std_b` typed on the CLI.
/// Returns the ids of `std_a` and `std_b`.
async fn orders(r: &Rig, scope: StandingScope) -> (String, String) {
    let p = dm(&r.core);
    say(&r.core, &p, "hello", Some("default")).await;
    let node = r.core.store.transcript(&p).unwrap()[0].1.id.clone();
    let add = |text: &str, source: Option<String>, scope| {
        r.core
            .standing_add(
                &StandingAddParams {
                    kind: StandingKind::Order,
                    text: text.into(),
                    scope,
                    source,
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap()
            .record
            .id
    };
    let a = add(HERON, Some(format!("capture:{node}")), scope);
    let b = add(SHORT, None, StandingScope::Everywhere);
    (a, b)
}

fn manifest_standing(core: &Core, sid: &str) -> Vec<(String, bool)> {
    let c = record(core, sid).compilation_id.unwrap();
    core.store
        .get_compilation(&c)
        .unwrap()
        .unwrap()
        .manifest
        .standing
        .into_iter()
        .map(|u| (u.id, u.shown))
        .collect()
}

/// The key test. P's kept order never reaches dsflash, the owner's own order
/// does; the manifest lists the held record unshown; the turn answers with no
/// refusal; the same session on Claude carries it; S is never marked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_sessions_kept_order_stays_out_of_a_restricted_request() {
    let (r, zai, _dir, _jev) = rig();
    let (a, b) = orders(&r, StandingScope::Everywhere).await;
    let s = cn_session(&r).await;
    let res = say(&r.core, &s, OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(&zai.requests()[0]);
    assert!(body.contains(SHORT), "{body}");
    assert!(!body.contains("Heronsgate"), "{body}");
    let listed = manifest_standing(&r.core, &s);
    assert!(listed.contains(&(a.clone(), false)), "{listed:?}");
    assert!(listed.contains(&(b, true)), "{listed:?}");
    assert_eq!(blocked(&r.core), 0);

    call(&r.core, "profile.use", json!({"name": "default"})).await;
    let before = r.claude.requests().len();
    say(&r.core, &s, OPEN_B, None).await;
    let sent = r.claude.requests();
    assert!(sent.len() > before);
    assert!(text(sent.last().unwrap()).contains("Heronsgate"));
}

/// As the key test, with the record scoped to the turn's surface: the
/// "Standing here:" tail leaves it out and `missing_here` does not refuse.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_surface_record_stays_out_of_the_tail() {
    let (r, zai, _dir, _jev) = rig();
    let (_a, _b) = orders(&r, StandingScope::Surface("terminal".into())).await;
    let s = cn_session(&r).await;
    let res = say_on_terminal(&r, &s, OPEN_A).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(&zai.requests()[0]);
    assert!(!body.contains("Heronsgate"), "{body}");
    assert!(body.contains(SHORT), "{body}");
    assert_eq!(blocked(&r.core), 0);

    call(&r.core, "profile.use", json!({"name": "default"})).await;
    let before = r.claude.requests().len();
    say_on_terminal(&r, &s, OPEN_B).await;
    let sent = r.claude.requests();
    assert!(sent.len() > before);
    assert!(text(sent.last().unwrap()).contains("Heronsgate"));
}

/// A turn whose input came through the terminal surface (`Surface`'s records
/// hold only for a turn that names one).
async fn say_on_terminal(r: &Rig, sid: &str, input: &str) -> theseus_protocol::TurnSubmitResult {
    let rec = record(&r.core, sid);
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = crate::bus::EventSink::new(r.core.bus.clone(), sid, None);
    r.core
        .runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: Some("terminal"),
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{}", crate::tests_dm_rig::OWNER),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

/// A detour (a plain DM session on Claude whose trivial message goes to
/// dsflash) holds back the same record, and its check passes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_to_a_restricted_model_holds_back_the_same_records() {
    let (r, zai, _dir, _jev) = rig_mode("trivial");
    let _ = orders(&r, StandingScope::Everywhere).await;
    let s = dm(&r.core);
    let res = say(&r.core, &s, OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(&zai.requests()[0]);
    assert!(body.contains(SHORT), "{body}");
    assert!(!body.contains("Heronsgate"), "{body}");
    assert_eq!(blocked(&r.core), 0);
}

/// Two pending wakes: the session's own and a milestone's due reminder. A
/// restricted request shows the first and counts one; Claude's shows both.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_milestone_reminder_stays_out_of_a_restricted_request() {
    let (r, zai, _dir, _jev) = rig();
    let s = cn_session(&r).await;
    say(&r.core, &s, OPEN_A, None).await;
    let exec = record(&r.core, &s).execution_id.unwrap();
    let far = r.core.kernel.now_ms() + 30 * 86_400_000;
    let wake = |id: &str, note: &str, purpose| {
        r.core
            .kernel
            .create_wake(
                &exec,
                Some(id.into()),
                NewWake {
                    due_at_ms: far,
                    note: note.into(),
                    target: None,
                    repeat: None,
                    purpose,
                    run: if purpose == Purpose::Remind {
                        WakeRun::Notice
                    } else {
                        WakeRun::Turn
                    },
                },
                "the CLI",
            )
            .unwrap();
    };
    wake("wak_own", "check the Widgetco build", Purpose::Check);
    wake(
        "wak_mile",
        "⚑ due today: Harbour survey, milestone 1: Survey the moorings",
        Purpose::Remind,
    );
    let res = say(&r.core, &s, OPEN_B, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(zai.requests().last().unwrap());
    assert!(body.contains("check the Widgetco build"), "{body}");
    assert!(!body.contains("⚑"), "{body}");
    assert!(
        body.contains("pending wakes in this conversation, 1:"),
        "{body}"
    );

    call(&r.core, "profile.use", json!({"name": "default"})).await;
    let before = r.claude.requests().len();
    say(&r.core, &s, OPEN_A, None).await;
    let sent = r.claude.requests();
    assert!(sent.len() > before);
    let body = text(sent.last().unwrap());
    assert!(
        body.contains("check the Widgetco build") && body.contains("⚑"),
        "{body}"
    );
}

/// A guard of what the open view used to do: the task view and the project
/// slice carry titles any session wrote, and neither reaches dsflash. Deleted
/// with the open view and restored (theseus-9ymz0, the review's B1): under
/// content flags a label belongs to the session that raised it, so a request
/// to a restricted model leaves out what another session wrote instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_task_view_and_slice_never_reach_a_restricted_request() {
    let (r, zai, _dir, _jev) = rig();
    let s = cn_session(&r).await;
    let p = r
        .core
        .project_create(
            &serde_json::from_value(json!({
                "title": "Heronsgate tide tables",
                "milestones": [{"title": "Heronsgate tide tables"}],
                "review": "none",
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap();
    r.core
        .project_attach(
            &theseus_protocol::projects::ProjectAttachParams {
                session: s.clone(),
                project: Some(p.project.id),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    // A plain task in S's scope, titled by another session.
    let task = crate::task_graph::NewTask {
        id: "tsk_heronsgate".into(),
        title: "Heronsgate tide tables",
        objective: String::new(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: theseus_protocol::tasks::TaskOrigin {
            session: s.clone(),
            principal: "agent".into(),
            by_model: true,
            ..Default::default()
        },
        state: theseus_protocol::tasks::TaskState::Accepted,
    }
    .build(r.core.kernel.now_ms());
    r.core
        .store
        .append(&[crate::task_graph::record(&task).unwrap()])
        .unwrap();
    let res = say(&r.core, &s, OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(&zai.requests()[0]);
    assert!(!body.contains(crate::task_graph::view::HEAD), "{body}");
    assert!(!body.contains(crate::project::slice::HEAD), "{body}");
    assert!(!body.contains("Heronsgate"), "{body}");

    // The same session on Claude carries both: nothing is lost for a model
    // that may see them.
    call(&r.core, "profile.use", json!({"name": "default"})).await;
    let before = r.claude.requests().len();
    say(&r.core, &s, OPEN_B, None).await;
    let sent = r.claude.requests();
    assert!(sent.len() > before);
    let body = text(sent.last().unwrap());
    assert!(body.contains(crate::task_graph::view::HEAD), "{body}");
    assert!(body.contains(crate::project::slice::HEAD), "{body}");
}

/// A record kept from another session's message is held from a restricted
/// request, for it is that session's content (theseus-9ymz0, the review's B1):
/// the owner's own order reaches dsflash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_record_kept_from_another_sessions_message_is_held_and_the_owners_own_is_not() {
    let (r, zai, _dir, _jev) = rig();
    let (_a, _b) = orders(&r, StandingScope::Everywhere).await;
    let s = cn_session(&r).await;
    let res = say(&r.core, &s, OPEN_A, None).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);
    let body = text(&zai.requests()[0]);
    assert!(body.contains(SHORT) && !body.contains(HERON), "{body}");
}
