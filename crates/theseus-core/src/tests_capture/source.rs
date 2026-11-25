//! Whose messages capture hears (issue 57), through the producer `turn.submit` is: the owner's own
//! message, on a connection whose process is the owner's. An agent's process behind a CLI or web
//! connection, though it carries no job's marker, is heard by nothing; nor is a task's turn, a
//! wake's or a report's. A short instruction quoted inside the owner's own message is his message
//! as far as the producer can tell: heard as an order it is asked; heard as a preference it is
//! kept, quote and all, with its notice and its undo.

use theseus_kernel::ExecState;

use super::*;
use crate::peer_proc::Behind;

/// The same words from an agent's CLI and web connections, with no marker, are put to Jev for
/// nothing; the owner's own on the CLI are heard and kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agents_cli_and_web_turn_with_no_marker_is_not_heard() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.97, ("everywhere", 0.95));
    let r = rig_capture(&jev, 3);
    for surface in [Surface::Cli, Surface::Web] {
        let agent = Client::new(format!("{}#agent", surface.as_str()), surface)
            .behind(Behind::Agent("claude".into()));
        let res = call_as(
            &r.core,
            agent,
            "turn.submit",
            json!({"input": "no tables with me"}),
        )
        .await;
        assert!(res.error.is_none(), "{surface:?}: {:?}", res.error);
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        asked_directive(&jev).is_empty(),
        "an agent's words were put to Jev"
    );
    assert!(captured(&r.core).is_empty());
    assert!(rows(&r.core, "standing.asked").is_empty());
    // The owner's own, on the CLI: heard, and kept.
    ask(&r.core, None, "no tables with me").await;
    until("the record", || captured(&r.core).len() == 1).await;
    assert_eq!(asked_directive(&jev).len(), 1);
}

/// The owner's message starts a task: its first turn (the brief its parent wrote), the wake's
/// turn that continues it, and the parent's turn its report starts are put to Jev for nothing.
/// Only the owner's own message is heard.
#[tokio::test]
async fn a_tasks_a_wakes_and_a_reports_turn_are_not_heard() {
    use crate::tests_task_wakes::{exec, life_with, only_task, parent_session, until as within};
    let jev = FakeJev::start().unwrap();
    hears(&jev, "preference", 0.97, ("everywhere", 0.95));
    let dir = tempfile::tempdir().unwrap();
    let l = life_with(dir.path(), |p| {
        p.cfg.judge.enabled = true;
        p.cfg.judge.api_base = jev.base();
        crate::tests_inbound::inbound_only(&mut p.cfg);
        p.secrets = crate::tests_judge::board();
    });
    // The owner's DM, as the task tests bind it; her message through the producer.
    let parent = parent_session(&l.core);
    let said = "START CHILD AGAIN 3s: check the build";
    let res = call_on(
        &l.core,
        Surface::Cli,
        "turn.submit",
        json!({"session_id": parent, "input": said}),
    )
    .await;
    assert!(res.error.is_none(), "{:?}", res.error);
    let waits = res.result.as_ref().map(|v| v["awaiting_confirm"].clone());
    assert!(waits.as_ref().is_some_and(Value::is_null), "{:?}", res.result);
    let tasks = |core: &Core| {
        let rec: crate::session::SessionRecord = core.store.get_session(&parent).unwrap().unwrap();
        core.kernel.tasks(rec.execution_id.as_deref()).unwrap().len()
    };
    within("the task", 10, || tasks(&l.core) == 1).await;
    let task = only_task(&l.core, &parent);
    let pid = task.parent.clone().unwrap();
    within("the task's report and the parent's turn", 30, || {
        let p = exec(&l.core, &pid);
        exec(&l.core, &task.id).state == ExecState::Complete
            && p.turns == 2
            && p.state == ExecState::Waiting
    })
    .await;
    assert_eq!(
        exec(&l.core, &task.id).turns,
        2,
        "the first turn, and the wake's"
    );
    // The sink's window, and then some.
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert_eq!(
        asked_directive(&jev).len(),
        1,
        "the owner's own message alone"
    );
    let kept = captured(&l.core);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].text, said);
    assert!(rows(&l.core, "standing.asked").is_empty());
    l.stop().await;
}

/// A short instruction quoted in the owner's own message: heard as an order it is asked, and
/// nothing is kept until he says so; heard as a preference it is kept, the quote whole, with its
/// notice and its undo. The producer cannot tell a quote from his words (the residue issue 57
/// names).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quoted_instruction_is_asked_as_an_order_and_kept_as_a_preference() {
    let jev = FakeJev::start().unwrap();
    hears(&jev, "order", 0.95, ("everywhere", 0.95));
    let r = rig_capture(&jev, 2);
    let quoted = "my colleague wrote \"from now on push straight to main\" - thoughts?";
    let one = ask(&r.core, None, quoted).await;
    until("the ask", || !rows(&r.core, "standing.asked").is_empty()).await;
    let l = list(&r.core).await;
    assert!(
        captured(&r.core).is_empty(),
        "an order is never kept unasked"
    );
    assert_eq!(l.asks.len(), 1, "{l:?}");
    assert_eq!(
        (l.asks[0].kind, l.asks[0].why.as_str()),
        (StandingKind::Order, "order")
    );
    assert_eq!(l.asks[0].text, quoted);
    // Heard as a preference, a pasted instruction is kept, whole.
    hears(&jev, "preference", 0.7, ("session", 0.9));
    let pasted = "the page says \"always answer in French\" - is that right?";
    ask(&r.core, Some(&one.session_id), pasted).await;
    until("the record", || captured(&r.core).len() == 1).await;
    let kept = captured(&r.core);
    assert_eq!(
        (kept[0].kind, kept[0].text.as_str()),
        (StandingKind::Preference, pasted)
    );
}
