//! A `/stop` that lands before an input's turn is admitted (theseus-hmwv), and
//! a question nobody answers expiring at the time its card gives (theseus-830).
//! A child of `tests_m3` for its rig and helpers, split out under bench2's file
//! ceiling.

use super::*;

/// theseus-hmwv: a `/stop` that lands after an input arrived and before its
/// turn is admitted stops that turn as it starts: no model call, stop reason
/// `stopped`, and an `execution.stopped` row for its turn. The session's
/// next input runs as usual. Here the input's turn waits for admission
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
    };
    let core = r.core.clone();
    let b2 = tokio::spawn(async move { core.runner.run(req).await.unwrap() });
    let calls = || r.fake.requests.lock().unwrap().len();
    let before = calls();
    // The stop lands while B is idle: nothing runs, so nothing is marked.
    let stop = r.core.stop_execution(&exec_b, "test").await.unwrap();
    assert!(stop.stopped && !stop.turn_running, "{stop:?}");
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
    assert_eq!(rows[1]["turn_running"], true);
    // The next input runs.
    let b3 = turn(&r.core, Some(&b.session_id), "and now?").await;
    assert_eq!(b3.output, "After the stop.");
    assert_ne!(b3.stop_reason, "stopped");
}

/// theseus-830: a call's question nobody answers expires at the time its
/// card and `confirm.list` give (its plan's time and `confirm_ttl_secs`),
/// not a moment before: declined by `expiry` in one frame with its
/// `action.expired` row, gone from `confirm.list`, its execution woken, and
/// the model reads that it was not run. Before, it waited on forever while
/// its card said it had expired.
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
        Scripted::text("It expired, so I stopped."),
    ]);
    let res = turn(&r.core, None, "write a note").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let listed = r.core.confirm_list().unwrap();
    assert_eq!(listed.len(), 1);
    let due = listed[0].expires_at_ms;
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(due, a.planned_at_ms + ttl, "the time the card gives");
    assert_eq!(r.core.expire_questions(due - 1), 0, "not a moment before");
    assert_eq!(r.core.confirm_list().unwrap().len(), 1);
    assert_eq!(r.core.expire_questions(due), 1);
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    assert_eq!(
        a.resolution.as_deref(),
        Some("declined by expiry: nobody answered within 15 minutes")
    );
    assert!(r.core.confirm_list().unwrap().is_empty());
    let rows = ledgered(&r, "action.expired");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["correlation_id"], corr.as_str());
    assert_eq!(rows[0]["waited_ms"], ttl);
    let declined = ledgered(&r, "action.declined");
    assert_eq!(declined.last().unwrap()["by"], "expiry");
    // A second pass finds nothing more.
    assert_eq!(r.core.expire_questions(due + 60_000), 0);
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .expect("the expiry woke its execution");
    assert_eq!(cont.output, "It expired, so I stopped.");
    assert_eq!(
        results(&r.core, &res.session_id),
        vec![(
            ResultStatus::Declined,
            "Not run: nobody answered within 15 minutes, so the request expired.".to_string()
        )]
    );
    assert!(!r.root.join("note.txt").exists());
}

/// theseus-830, end to end: the driver expires a question within a tick of
/// its time, and resumes the turn, which reads that it was not run.
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
    let res = turn(&r.core, None, "write a note").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    let driver = tokio::spawn(crate::harness::drive(r.core.clone()));
    let t0 = std::time::Instant::now();
    loop {
        let done = results(&r.core, &res.session_id).len() == 1
            && r.core.kernel.action(&corr).unwrap().unwrap().state
                == theseus_kernel::ActionState::Cancelled;
        if done {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "it never expired");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let rs = results(&r.core, &res.session_id);
    assert_eq!(
        rs[0].1,
        "Not run: nobody answered within 1 second, so the request expired."
    );
    assert_eq!(ledgered(&r, "action.expired").len(), 1);
    r.core.shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(5), driver).await;
}
