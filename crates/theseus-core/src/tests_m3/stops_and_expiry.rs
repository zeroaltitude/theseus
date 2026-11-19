//! A `/stop` that lands before an input's turn is admitted (theseus-hmwv), and
//! a question nobody answers noticed, not cancelled, at the time its card gives
//! (theseus-830, theseus-v2dnc).
//! A child of `tests_m3` for its rig and helpers, split out under bench2's file
//! ceiling.

use super::question_waits::{bind_dm, waiting};
use super::*;

/// theseus-hmwv: a `/stop` that lands after an input arrived and before its
/// turn is admitted stops that turn as it starts: no model call, stop reason
/// `stopped`, and an `execution.stopped` row for its turn. The stop's answer
/// and its row say a turn was running, as the turn waiting for admission is
/// the one it reaches. The session's next input runs as usual. Here the input's turn waits for admission
/// behind another session's turn, whose job's disk check is held (the
/// ceiling is one); in the daemon the window is a start's wait for the
/// secrets, or admission under load. Before, the stop found the execution
/// idle and marked nothing, and the turn ran as if no stop had come.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_before_an_inputs_turn_is_admitted_stops_that_turn() {
    let (held, mut there, go) = Held::new();
    let r = rig_parts(
        vec![
            Scripted::text("Hello."),
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["touch", "a-ran"]}))],
            ),
            Scripted::text("A is done."),
            Scripted::text("After the stop."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.server.disk_floor_mb = 1024;
            cfg.kernel.admission_ceiling = 1;
        },
        |_| {},
    );
    let b = turn(&r.core, None, "hello").await;
    let exec_b = b.execution_id.clone().unwrap();
    r.core.tools.disk.set_probe(Arc::new(HeldDiskCheck {
        held,
        once: std::sync::Once::new(),
    }));
    // Session A's turn holds the one admission, its job held at its start.
    let core = r.core.clone();
    let a = tokio::spawn(async move { turn(&core, None, "touch a file").await });
    there.recv().await.unwrap();
    // B's input arrives, and its turn waits for admission.
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&b.session_id)
        .unwrap()
        .unwrap();
    let (live, _) = r.core.live_profile();
    let req = TurnRequest {
        prompt: None,
        surface: None,
        sink: EventSink::new(r.core.bus.clone(), &rec.session_id, None),
        session: rec,
        input: Some("do the thing".into()),
        target: r
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap(),
        author: "test".into(),
        recompile: None,
        attachments: vec![],
        arrived: Some(std::time::Instant::now()),
        reply_to: None,
        answer_at: None,
    };
    let core = r.core.clone();
    let b2 = tokio::spawn(async move { core.runner.run(req).await.unwrap() });
    let calls = || r.fake.requests.lock().unwrap().len();
    let before = calls();
    // B's turn is in the admission's queue before the stop lands: the stop
    // reaches it, though it holds nothing yet, so nothing is marked.
    let t0 = std::time::Instant::now();
    while !r.core.runner.admit_waiting.reaches(
        &b.session_id,
        std::time::Instant::now() + Duration::from_secs(3600),
    ) {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "B's turn never waited"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let stop = r.core.stop_execution(&exec_b, "test").await.unwrap();
    assert!(stop.stopped && stop.turn_running, "{stop:?}");
    go.send(()).unwrap();
    let a = tokio::time::timeout(Duration::from_secs(20), a)
        .await
        .expect("A's turn ended")
        .unwrap();
    assert_eq!(a.output, "A is done.");
    let b2 = tokio::time::timeout(Duration::from_secs(20), b2)
        .await
        .expect("B's turn ended")
        .unwrap();
    assert_eq!(b2.stop_reason, "stopped", "{b2:?}");
    assert_eq!(
        calls(),
        before + 1,
        "only A's last call was made after the stop"
    );
    let rows = ledgered(&r, "execution.stopped");
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        rows[0]["turn_running"], true,
        "the stop's own row: {rows:?}"
    );
    assert_eq!(rows[1]["turn_running"], true);
    // The next input runs.
    let b3 = turn(&r.core, Some(&b.session_id), "and now?").await;
    assert_eq!(b3.output, "After the stop.");
    assert_ne!(b3.stop_reason, "stopped");
}

/// A session with an execution of its own, as one that has had a turn, and
/// the request of an input for it.
fn input_for_a_session_with_an_execution(
    r: &Rig,
) -> (SessionRecord, theseus_kernel::Execution, TurnRequest) {
    use theseus_kernel::Authority;
    // A session with an execution of its own, as one that has had a turn.
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    let exec = r
        .core
        .kernel
        .open_execution(
            &rec.session_id,
            SessionKind::Conversation,
            Authority {
                principal: crate::turn::OPERATOR.into(),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    rec.execution_id = Some(exec.id.clone());
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = r.core.live_profile();
    let req = TurnRequest {
        prompt: None,
        surface: None,
        sink: EventSink::new(r.core.bus.clone(), &rec.session_id, None),
        session: rec.clone(),
        input: Some("do the thing".into()),
        target: r
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap(),
        author: "test".into(),
        recompile: None,
        attachments: vec![],
        arrived: Some(std::time::Instant::now()),
        reply_to: None,
        answer_at: None,
    };
    (rec, exec, req)
}

/// A `/stop` that lands while an input's turn still waits for its secrets,
/// before it has reached `admit()`, reaches that turn as well: the answer and
/// the kernel's row say a turn was running (the turn stops itself on the
/// stop's notice when it is admitted, and plans nothing). The turn is known
/// from the start of its run, not from its admission.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_while_an_inputs_turn_waits_for_its_secrets_counts_that_turn() {
    use crate::secrets::{Secret, SecretBoard};
    let key = "anthropic_api_key".to_string();
    let board = SecretBoard::new([key.clone()], std::time::Instant::now());
    let held_board = board.clone();
    let r = rig_parts(
        vec![Scripted::text("never sent")],
        |_| {},
        move |p| p.secrets = board,
    );
    let (rec, exec, req) = input_for_a_session_with_an_execution(&r);
    let core = r.core.clone();
    let turn = tokio::spawn(async move { core.runner.run(req).await.unwrap() });
    let t0 = std::time::Instant::now();
    while !r.core.runner.admit_waiting.reaches(
        &rec.session_id,
        std::time::Instant::now() + Duration::from_secs(3600),
    ) {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the turn never began"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !r.core.kernel.holds_turn(&exec.id),
        "the turn waits for its secrets, holding nothing"
    );
    let stop = r.core.stop_execution(&exec.id, "test").await.unwrap();
    assert!(stop.stopped && stop.turn_running, "{stop:?}");
    let rows = ledgered(&r, "execution.stopped");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["turn_running"], true, "the row: {rows:?}");
    held_board.publish(
        std::collections::BTreeMap::from([(key, Ok(Secret::new("k-test-0123456789".into())))]),
        "test",
    );
    let res = tokio::time::timeout(Duration::from_secs(20), turn)
        .await
        .expect("the turn ended")
        .unwrap();
    assert_eq!(res.stop_reason, "stopped", "{res:?}");
    assert_eq!(r.fake.requests.lock().unwrap().len(), 0, "no model call");
}

/// A turn that a stop counted (it waited for its secrets) and that then ends
/// for a reason of its own, before it is admitted, was not ended by the stop.
/// The stop's answer and the kernel's row had gone, and said a turn was
/// reached; the ledger and the session then say what in fact ended it. The
/// window is held open by order, with no clock: the turn is polled once, so it
/// is listed and waiting for its secret, the stop lands, and only then does
/// the secret fail.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_counted_turn_that_fails_before_admission_says_what_ended_it() {
    use crate::secrets::SecretBoard;
    use futures_util::FutureExt;
    let key = "anthropic_api_key".to_string();
    let board = SecretBoard::new([key.clone()], std::time::Instant::now());
    let secrets = board.clone();
    let r = rig_parts(
        vec![Scripted::text("never sent")],
        |_| {},
        move |p| p.secrets = board,
    );
    let (rec, exec, req) = input_for_a_session_with_an_execution(&r);
    let mut turn = r.core.runner.run(req);
    assert!(
        turn.as_mut().now_or_never().is_none(),
        "it waits for its secret"
    );
    // The stop counts the turn: it waits listed, holding nothing.
    let stop = r.core.stop_execution(&exec.id, "test").await.unwrap();
    assert!(stop.stopped && stop.turn_running, "{stop:?}");
    assert!(
        ledgered(&r, "execution.stop_not_taken").is_empty(),
        "not yet"
    );
    // Its secret is refused: the turn ends for its own reason.
    secrets.withdraw(&key, "the vault refused it");
    let failed = turn.await.expect_err("a refused secret fails the turn");
    let rows = ledgered(&r, "execution.stop_not_taken");
    assert_eq!(rows.len(), 1, "{rows:?}: {failed:#}");
    assert_eq!(rows[0]["by"], "test");
    assert_eq!(rows[0]["execution_id"], json!(exec.id));
    assert!(
        rows[0]["why"].as_str().unwrap().contains("secret_failed"),
        "{rows:?}"
    );
    let lines = narrated(&r, &rec.session_id);
    assert!(
        said(&lines, "session", "ended before it was, for its own reason"),
        "{}",
        dump(&lines)
    );
}

/// A turn no stop counted that fails before admission says nothing of a stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_no_stop_counted_that_fails_before_admission_says_nothing_of_one() {
    use crate::secrets::SecretBoard;
    use futures_util::FutureExt;
    let key = "anthropic_api_key".to_string();
    let board = SecretBoard::new([key.clone()], std::time::Instant::now());
    let secrets = board.clone();
    let r = rig_parts(
        vec![Scripted::text("never sent")],
        |_| {},
        move |p| p.secrets = board,
    );
    let (_rec, _exec, req) = input_for_a_session_with_an_execution(&r);
    let mut turn = r.core.runner.run(req);
    assert!(
        turn.as_mut().now_or_never().is_none(),
        "it waits for its secret"
    );
    secrets.withdraw(&key, "the vault refused it");
    turn.await.expect_err("a refused secret fails the turn");
    assert!(ledgered(&r, "execution.stop_not_taken").is_empty());
}

/// theseus-830, changed by theseus-v2dnc: a call's question nobody answers
/// is not cancelled when its card's time comes (its plan's time and
/// `confirm_ttl_secs`). It stays pending in `confirm.list`, its call
/// unanswered and its execution parked, and one quiet notice names it and its
/// wait; the owner can still approve it. Before, it was declined by `expiry`
/// and the model read that it was not run.
#[tokio::test]
async fn a_question_nobody_answers_expires_at_the_time_its_card_gives() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "fs_write",
                json!({"path": "note.txt", "content": "hi"}),
            )],
        ),
        Scripted::text("Written after all."),
    ]);
    bind_dm(&r);
    let res = turn(&r.core, None, "write a note").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let listed = r.core.confirm_list().unwrap();
    assert_eq!(listed.len(), 1);
    // The list says no deadline for a held question (theseus-lrc3m); the
    // time its card gives, and its first notice, is its plan's plus the TTL.
    assert_eq!(
        listed[0].expires_at_ms, 0,
        "a held question lists no expiry"
    );
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    let due = listed[0].requested_at_ms + ttl;
    assert_eq!(due, a.planned_at_ms + ttl, "the time the card gives");
    assert_eq!(r.core.expire_questions(due - 1), 0, "not a moment before");
    assert!(waiting(&r).is_empty(), "no notice before it");
    assert_eq!(r.core.confirm_list().unwrap().len(), 1);
    assert_eq!(r.core.expire_questions(due), 0, "held, not expired");
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert!(a.awaits_confirm(), "{a:?}");
    assert_eq!(a.resolution, None);
    assert_eq!(r.core.confirm_list().unwrap().len(), 1);
    assert_eq!(ledgered(&r, "action.expired").len(), 0);
    assert!(ledgered(&r, "action.declined").is_empty());
    let told = waiting(&r);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0]["correlation_id"], corr.as_str());
    // A second pass in the same life tells nothing more.
    assert_eq!(r.core.expire_questions(due + 60_000), 0);
    assert_eq!(waiting(&r).len(), 1);
    assert!(results(&r.core, &res.session_id).is_empty());
    // Approved late, it runs, and the model goes on.
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .expect("the answer woke its execution");
    assert_eq!(cont.output, "Written after all.");
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs.len(), 1, "{rs:?}");
    assert_ne!(rs[0].0, ResultStatus::Declined, "{rs:?}");
    assert!(r.root.join("note.txt").exists());
}

/// theseus-830, end to end, changed by theseus-v2dnc: the driver tells of a
/// question within a tick of its time and leaves it pending, its call
/// unanswered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_driver_expires_a_question_within_a_tick_of_its_time() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "fs_write",
                    json!({"path": "note.txt", "content": "hi"}),
                )],
            ),
            Scripted::text("Nobody answered."),
        ],
        |cfg| cfg.kernel.confirm_ttl_secs = 1,
    );
    bind_dm(&r);
    let res = turn(&r.core, None, "write a note").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let driver = tokio::spawn(crate::harness::drive(r.core.clone()));
    let t0 = std::time::Instant::now();
    while waiting(&r).is_empty() {
        assert!(t0.elapsed() < Duration::from_secs(10), "it never noticed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let told = waiting(&r);
    assert!(
        told[0]["text"].as_str().unwrap().contains("1 minute"),
        "{told:?}"
    );
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert!(a.awaits_confirm(), "still pending: {a:?}");
    assert!(results(&r.core, &res.session_id).is_empty());
    assert_eq!(ledgered(&r, "action.expired").len(), 0);
    r.core.shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(5), driver).await;
}
