//! The calls a cancel or a turn's end must answer, and what a restart between
//! a call's plan and its authorization leaves (theseus-0o8, theseus-ni5): the
//! transcript gets one answer for every call, never a question no card posted.
//! A child of `tests_m3` for its rig and helpers, split out under bench2's file
//! ceiling.

use super::*;

/// A cancel that finds a turn running writes no result into its transcript
/// (theseus-w98): that is the turn's own. The turn answers the calls it left
/// when it has ended (theseus-0o8), which is `answer_after_cancel`: here the
/// cancel lands with no result in its frame, as it does for a running turn,
/// and the turn's end finds a planned call that nothing answers. Answered
/// once, as the cancel's own sweep would have.
#[tokio::test]
async fn a_call_a_running_turn_never_answered_is_answered_at_its_end() {
    let r = rig_with(
        vec![Scripted::tools(
            "",
            &[("t1", "proc_run", json!({"argv": ["echo", "never run"]}))],
        )],
        |cfg| cfg.policy.enforcement = Posture::Approve,
    );
    let res = turn(&r.core, None, "echo something").await;
    let corr = res.awaiting_confirm.clone().expect("approve waits");
    let exec = res.execution_id.clone().unwrap();
    r.core
        .kernel
        .cancel_execution_with(&exec, "operator", |_| Ok(vec![]))
        .unwrap();
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    assert!(
        results(&r.core, &res.session_id).is_empty(),
        "the cancel wrote no result"
    );
    let sweep = || {
        r.core
            .tools
            .answer_after_cancel(&r.core.kernel, &r.core.store, &res.session_id, &exec)
            .unwrap()
    };
    assert_eq!(sweep().len(), 1);
    assert_eq!(
        results(&r.core, &res.session_id),
        vec![(
            ResultStatus::Cancelled,
            "Not run: the execution was cancelled by operator.".to_string()
        )]
    );
    assert!(sweep().is_empty(), "each call is answered once");
}

/// A cancel ends a background job, whose call was answered with a placeholder
/// ("still running"): the transcript gets its end, as a late result
/// (theseus-0o8). The test's launcher has no wrapper to signal, so the cancel
/// cannot stop the job, and the result says the job may have finished: its
/// outcome is unknown, never "not run". A job the cancel verified gone says
/// it was stopped. A second cancel writes nothing more.
#[tokio::test]
async fn a_cancel_ends_a_background_jobs_placeholder_in_the_transcript() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "echo started; while [ -e running.marker ]; do sleep 0.05; done"]}),
                )],
            ),
            Scripted::text("Started."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
    );
    std::fs::write(r.root.join("running.marker"), "").unwrap();
    let res = turn(&r.core, None, "start it").await;
    let exec = res.execution_id.clone().unwrap();
    let sid = res.session_id.clone();
    let (calls, got) = answers(&r.core, &sid);
    assert_eq!(calls, ["t1"]);
    assert_eq!(
        got.iter().map(|g| (g.1, g.3)).collect::<Vec<_>>(),
        [(ResultStatus::Background, false)],
        "answered with its placeholder"
    );

    let (_, killed) = r.core.cancel_execution(&exec, "operator").await.unwrap();
    assert_eq!(killed.len(), 1, "the job was told to stop");
    let (_, got) = answers(&r.core, &sid);
    assert_eq!(got.len(), 2, "the placeholder, then its end: {got:?}");
    let (id, status, text, late) = &got[1];
    assert_eq!(
        (id.as_str(), *status, *late),
        ("t1", ResultStatus::Unknown, true)
    );
    assert!(
        text.starts_with(
            "[The execution was cancelled by operator while this call was running; it cannot be \
             stopped once started, so it may have finished"
        ),
        "{text}"
    );
    r.core.cancel_execution(&exec, "operator").await.unwrap();
    assert_eq!(
        answers(&r.core, &sid).1.len(),
        2,
        "a second cancel writes none"
    );
    std::fs::remove_file(r.root.join("running.marker")).unwrap();
}

/// A job a cancel verified gone says it was stopped (theseus-0o8), with the
/// job's own result under it, as the late result of any job reads.
#[tokio::test]
async fn a_cancel_that_verified_a_job_gone_says_it_was_stopped() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "echo started; while [ -e running.marker ]; do sleep 0.05; done"]}),
                )],
            ),
            Scripted::text("Started."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 1;
        },
    );
    std::fs::write(r.root.join("running.marker"), "").unwrap();
    let res = turn(&r.core, None, "start it").await;
    let exec = res.execution_id.unwrap();
    let sid = res.session_id;
    // The cancel's frame, and the settle a job with a wrapper gets.
    let cancel = r
        .core
        .kernel
        .cancel_execution_with(&exec, "operator", |_| Ok(vec![]))
        .unwrap();
    assert_eq!(cancel.to_kill.len(), 1);
    let corr = &cancel.to_kill[0];
    r.core.kernel.cancel_acknowledged(corr).unwrap();
    let tree = theseus_kernel::Verdict::verified_as(theseus_kernel::VerifiedBy::Tree, Some(1));
    r.core.kernel.cancel_verified(corr, Some(&tree)).unwrap();
    let written = r
        .core
        .tools
        .answer_after_cancel(&r.core.kernel, &r.core.store, &sid, &exec)
        .unwrap();
    assert_eq!(written.len(), 1);
    let (_, got) = answers(&r.core, &sid);
    let (_, status, text, late) = &got[1];
    assert_eq!((*status, *late), (ResultStatus::Cancelled, true));
    assert!(
        text.starts_with(
            "[The execution was cancelled by operator while this call was running; it was stopped.]\n"
        ),
        "{text}"
    );
    std::fs::remove_file(r.root.join("running.marker")).unwrap();
}

/// What a restart between a call's plan and its authorization leaves
/// (theseus-ni5), built with the steps an older build wrote in separate
/// frames: a session whose last assistant message asks for a read, the call
/// planned with its node (its gate said `gate`) and no proposal on the action,
/// and the execution queued for the continuation a restart takes. Returns the
/// session, the execution, and the call's correlation id.
fn a_call_planned_and_never_authorized(r: &Rig, gate: &str) -> (String, String, String) {
    use crate::node::Node;
    use theseus_kernel::{Authority, Proposal, RetryClass, TurnEnd};
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    let sid = rec.session_id.clone();
    let k = &r.core.kernel;
    let exec = k
        .open_execution(
            &sid,
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
    k.wake(&exec.id, "input").unwrap();
    let guard = k.admit(&exec.id).unwrap();
    let turn = "trn_ni5";
    let user = Node::user(&sid, Some(turn), "test", "read a.txt");
    let asked = Node::assistant(
        &sid,
        turn,
        0,
        Body::AssistantMessage {
            blocks: vec![
                json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "a.txt"}}),
            ],
            model: "fake".into(),
            provider: "fake".into(),
            stop_reason: Some("tool_use".into()),
            usage: theseus_protocol::Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
        },
    );
    r.core
        .store
        .append(&[user.record().unwrap(), asked.record().unwrap()])
        .unwrap();
    let proposal = Proposal {
        tool: "fs.read".into(),
        args: json!({"path": "a.txt"}),
        resource: None,
        policy_context: Value::Null,
    };
    let a = k
        .plan_action_with(&guard, &proposal, RetryClass::SafeToRepeat, None, 0, |a| {
            let call = Node::tool_call(
                &sid,
                Some(turn),
                Some(0),
                Body::ToolCall {
                    tool_use_id: "t1".into(),
                    tool: "fs.read".into(),
                    wire_name: "fs_read".into(),
                    input: json!({"path": "a.txt"}),
                    assistant_node: asked.id.clone(),
                    correlation_id: Some(a.correlation_id.clone()),
                    gate: Some(Box::new(theseus_protocol::GateRecord {
                        result: theseus_protocol::GateResult {
                            gate: gate.into(),
                            ..Default::default()
                        },
                        validated: true,
                        proposal: proposal.clone(),
                        ..Default::default()
                    })),
                },
            );
            Ok(vec![call.record()?])
        })
        .unwrap();
    assert!(a.proposal.is_none() && a.awaits_confirm(), "{a:?}");
    // The restart: the turn is gone, and the execution waits to be resumed.
    k.end_turn(guard, TurnEnd::Requeue).unwrap();
    k.wake(&exec.id, "restart").unwrap();
    (sid, exec.id, a.correlation_id)
}

/// theseus-ni5: a call planned and never asked (the gate said `allow`, and a
/// restart came before its authorization) is not a question. A continuation
/// finds that its node's gate let it run, declines it as the harness's act,
/// and answers it not run, where it parked the turn on a question no card
/// ever posted.
#[tokio::test]
async fn a_planned_call_never_asked_is_answered_not_run_and_parks_nothing() {
    let r = rig_with(vec![Scripted::text("Understood.")], |cfg| {
        cfg.policy.enforcement = Posture::Approve
    });
    let (sid, exec, corr) = a_call_planned_and_never_authorized(&r, "allow");
    assert_eq!(
        r.core.kernel.pending_confirms().unwrap().len(),
        1,
        "the kernel cannot tell it from a question"
    );
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Understood.");
    assert!(cont.awaiting_confirm.is_none(), "{cont:?}");
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    assert_eq!(
        a.resolution.as_deref(),
        Some("declined by harness: planned before a restart and never asked")
    );
    assert!(r.core.kernel.pending_confirms().unwrap().is_empty());
    assert_eq!(
        results(&r.core, &sid),
        vec![(
            ResultStatus::Cancelled,
            "Not run: this call was planned before a restart and the operator was never asked \
             about it. Ask again if it is still wanted."
                .to_string()
        )]
    );
}

/// The other half of theseus-ni5's rule: a question stored before
/// theseus-0g4 has no proposal on its action either, and its node's gate said
/// `needs_confirm`. It was asked, and a continuation waits on it, as before.
#[tokio::test]
async fn a_question_stored_without_a_proposal_still_parks_the_continuation() {
    let r = rig_with(vec![Scripted::text("Understood.")], |cfg| {
        cfg.policy.enforcement = Posture::Approve
    });
    let (sid, exec, corr) = a_call_planned_and_never_authorized(&r, "needs_confirm");
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.awaiting_confirm.as_deref(), Some(corr.as_str()));
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Planned);
    assert_eq!(r.core.kernel.pending_confirms().unwrap().len(), 1);
    assert!(
        results(&r.core, &sid).is_empty(),
        "nothing answers a question"
    );
}
