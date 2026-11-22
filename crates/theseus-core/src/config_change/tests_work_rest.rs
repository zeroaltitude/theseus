//! `look.work`'s second pass, played through the whole core (issue 28): the conversations that are running,
//! everything that waits on the owner (cards, the standing count), the limited read of
//! the task view, and the walls of a routine's run, the self loop and an MCP session.

use serde_json::json;
use theseus_kernel::Authority;
use theseus_protocol::card::{CardAsk, CardProposal, ProposalState};
use theseus_protocol::{SessionKind, StandingKind, StandingScope};

use super::tests::rig;
use super::tests_work_global::{conversation, latest, said, task};
use crate::node::ResultStatus;
use crate::session::SessionRecord;

/// A card proposal that waits (open, not expired), `posted` when the DM poster has sent it.
fn card(n: u64, posted: bool) -> CardProposal {
    let now = theseus_protocol::now_unix_ms();
    CardProposal {
        id: format!("cdp_{n:08}"),
        kind: StandingKind::Fact,
        scope: StandingScope::Everywhere,
        text: format!("Moors at the north jetty, case {n}"),
        question: format!("Keep: moors at the north jetty, case {n}?"),
        options: vec!["Keep".into(), "Not this".into()],
        sources: vec![],
        support: 2,
        created_ms: n,
        expires_ms: now + 86_400_000,
        state: ProposalState::Open,
        run: "crd_1".into(),
        answered_ms: None,
        answered_by: None,
        kept_text: None,
        standing_id: None,
        ask: CardAsk::Add,
        record: None,
        observations: vec![],
        posted_ms: posted.then_some(now),
    }
}

/// Everything that waits on the owner: a card's proposal beside the questions on calls, each with where
/// it waits; a standing record proposed is not a wait and is only counted, said so.
/// `scope: here` leaves out what is tied to no conversation.
#[tokio::test]
async fn needs_you_is_everything_that_waits_on_the_owner() {
    let r = rig();
    let here = conversation(&r).await;
    let recs = [card(1, true), card(2, false)]
        .iter()
        .map(|p| crate::card::state::record(p).unwrap())
        .collect::<Vec<_>>();
    r.core.store.append(&recs).unwrap();
    let text = said(&r, &here, json!({"what": ["needs_you"]})).await;
    assert!(text.contains("Waiting on the owner (2);"), "{text}");
    assert!(
        text.contains("card proposal · waits in the owner's DM"),
        "{text}"
    );
    assert!(
        text.contains("card proposal · waits in the card, not posted yet"),
        "{text}"
    );
    assert!(!text.contains("standing record"), "none proposed: {text}");
    let alone = said(&r, &here, json!({"what": ["needs_you"], "scope": "here"})).await;
    assert!(
        alone.contains("nothing waits here") && !alone.contains("card proposal"),
        "{alone}"
    );
    // The same view the command reads: a question past its end is not listed.
    let mut gone = card(3, true);
    gone.expires_ms = 1;
    r.core
        .store
        .append(&[crate::card::state::record(&gone).unwrap()])
        .unwrap();
    let again = said(&r, &here, json!({"what": ["needs_you"]})).await;
    assert!(again.contains("Waiting on the owner (2);"), "{again}");
}

/// A standing record proposed for review is counted and named as not asked of the owner.
#[tokio::test]
async fn a_proposed_standing_record_is_counted_and_not_called_a_wait() {
    let r = rig();
    let here = conversation(&r).await;
    let p = theseus_protocol::StandingProposal {
        id: "stp_000000000001".into(),
        persona: "harbour".into(),
        kind: StandingKind::Fact,
        text: "The moors are tarred in March".into(),
        from: "notes/moors.md".into(),
        proposed_ms: 1,
        state: "open".into(),
        record: None,
    };
    let rec = theseus_store::NewRecord::json(
        theseus_store::kinds::META,
        Some(&crate::standing_proposals::key(&p.id)),
        &p,
    )
    .unwrap();
    r.core.store.append(&[rec]).unwrap();
    let text = said(&r, &here, json!({"what": ["needs_you"]})).await;
    assert!(text.contains("nothing waits anywhere."), "{text}");
    assert!(
        text.contains("Not asked of the owner, open for review: 1 proposed standing record"),
        "{text}"
    );
    assert!(!text.contains("tarred"), "its words are not given: {text}");
}

/// The conversations that are working now, where each is and since when: the asking conversation is
/// working (this turn is under way) and is named as this one; `scope: here` is this one alone.
#[tokio::test]
async fn running_names_the_conversations_that_are_working_now() {
    let r = rig();
    let (here, other) = (conversation(&r).await, conversation(&r).await);
    let text = said(&r, &here, json!({"what": ["running"]})).await;
    assert!(
        text.starts_with("Running: conversations working now ("),
        "{text}"
    );
    let mine = text
        .lines()
        .find(|l| l.contains("· this conversation ·"))
        .unwrap();
    assert!(
        mine.starts_with(&format!("- {} ·", crate::task::short(&here))) && mine.contains(" turn"),
        "{mine}"
    );
    let alone = said(&r, &here, json!({"what": ["running"], "scope": "here"})).await;
    assert!(alone.contains("· this conversation ·"), "{alone}");
    assert!(!alone.contains(&crate::task::short(&other)), "{alone}");
}

/// The task view is read to a limit: the newest open ones are read whole and the older are counted, and
/// `Core::tasks` (the command's own read) is unchanged.
#[tokio::test]
async fn the_task_view_is_read_to_a_limit_and_the_command_reads_all_of_it() {
    let r = rig();
    let parent = conversation(&r).await;
    let all: Vec<_> = (0..5)
        .map(|i| task(&r, &parent, &format!("Chart reef number {i}"), None))
        .collect();
    let (read, older) = r.core.tasks_upto(None, None, 3).unwrap();
    assert_eq!((read.len(), older), (3, 2), "{read:?}");
    let newest: Vec<_> = all
        .iter()
        .rev()
        .take(3)
        .map(|e| e.session_id.clone())
        .collect();
    assert_eq!(
        read.iter().map(|t| t.task_id.clone()).collect::<Vec<_>>(),
        newest,
        "the newest first"
    );
    assert_eq!(r.core.tasks(None, None).unwrap().len(), 5);
    assert_eq!(r.core.tasks_upto(None, None, 5).unwrap().1, 0);
    // An ended task gives way to an open one in the limit.
    r.core
        .task_cancel_by(&all[4].session_id, "test")
        .await
        .unwrap();
    let (read, older) = r.core.tasks_upto(None, None, 4).unwrap();
    assert_eq!((read.len(), older), (4, 1), "{read:?}");
    assert!(
        !read.iter().any(|t| t.task_id == all[4].session_id),
        "the ended one is the one left unread: {read:?}"
    );
}

/// The tool's own answer says what it did not read: this conversation's tasks beyond its cap are counted.
#[tokio::test]
async fn the_tool_says_how_many_older_tasks_it_did_not_read() {
    let r = rig();
    let parent = conversation(&r).await;
    for i in 0..12 {
        task(&r, &parent, &format!("Chart reef number {i}"), None);
    }
    let text = said(&r, &parent, json!({"what": ["tasks"]})).await;
    assert!(text.contains("Tasks (12 in all):"), "{text}");
    assert!(
        text.contains("… and 2 more older tasks not listed here"),
        "{text}"
    );
}

/// A session opened with another principal than the owner's own turn is refused, in the look's words, and
/// reads nothing: a connected client (`mcp`) and the self-improvement loop (`self`).
#[tokio::test]
async fn an_mcp_session_and_the_self_loop_are_refused_and_read_nothing() {
    for principal in [crate::mcp_server::PRINCIPAL, crate::rsi::backlog::PRINCIPAL] {
        // The loop's turn is refused its budget while self-improvement is off: it is on, so that the
        // call is the look's and the refusal is the wall's.
        let r = super::tests::rig_with(|cfg, _| {
            cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
        });
        let other = conversation(&r).await;
        task(&r, &other, "Chart the outer reef", None);
        let mut rec = SessionRecord::new(SessionKind::Conversation, None);
        let e = r
            .core
            .kernel
            .open_execution(
                &rec.session_id,
                SessionKind::Conversation,
                Authority {
                    principal: principal.into(),
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap();
        rec.execution_id = Some(e.id.clone());
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        r.turn_with("look_work", &rec.session_id, json!({})).await;
        let (status, text) = latest(&r, &rec.session_id);
        assert_ne!(status, ResultStatus::Ok, "{principal}: {text}");
        assert!(
            text.contains("belongs to a job, the self-improvement loop or a connected client"),
            "{principal}: {text}"
        );
        assert!(
            !text.contains("Chart the outer reef"),
            "{principal}: {text}"
        );
    }
}

/// A routine's run is a task's session under the routine: refused as a task is.
#[tokio::test]
async fn a_routines_run_is_refused_and_reads_nothing() {
    let r = rig();
    let routine = r
        .core
        .routine_create(
            &serde_json::from_value(json!({
                "title": "Tide check", "schedule": "weekdays 08:00", "brief": "Read the tides."
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap()
        .routine;
    let run = task(&r, &routine.id, "Tide check run", None);
    r.turn_with("look_work", &run.session_id, json!({})).await;
    let (status, text) = latest(&r, &run.session_id);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("not for a task"), "{text}");
}
