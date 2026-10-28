//! `work.answer` through whole cores (theseus-ds5os): one verb for every
//! question. Each kind and option routes to today's handler; an option the
//! question lacks is refused with the valid ones; the place rule judges it
//! exactly as it judges `action.confirm` (a shared place, the MCP surface);
//! `may_answer` says what a connection would be refused; a short end of an id
//! names one question or is refused; and an answer closes the question for
//! every surface at once, with the option on `confirm.resolved`. A child of
//! `tests_m3` for its rig and helpers.

use super::*;
use crate::approval::Surface::{Cli, Discord, Mcp};
use crate::rpc::answer::answer_tests::{pick, route};
use theseus_protocol::error_code;
use theseus_protocol::work::QuestionKind;

/// `work.answer` over a real connection, accepted as `client`.
async fn answer_with(
    r: &Rig,
    client: crate::approval::Client,
    id: &str,
    option: &str,
    extra: Value,
) -> Result<Value, theseus_protocol::RpcError> {
    let mut params = json!({"question_id": id, "option": option});
    for (k, v) in extra.as_object().into_iter().flatten() {
        params[k] = v.clone();
    }
    rpc_as(
        &r.core,
        client,
        theseus_protocol::method::WORK_ANSWER,
        params,
    )
    .await
}

async fn cli_answer(r: &Rig, id: &str, option: &str) -> Result<Value, theseus_protocol::RpcError> {
    answer_with(r, surface("sock#1", Cli), id, option, json!({})).await
}

/// A write that waits: its session, execution and question.
async fn waiting_write(r: &Rig) -> (String, String, String) {
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.clone().expect("the write waits");
    (res.session_id, res.execution_id.unwrap(), corr)
}

fn confirm_rows(r: &Rig) -> Vec<Value> {
    ledgered(r, "action.confirm_answered")
}

/// An approval's answers. `approve` runs the call; `decline` with a note does
/// not, and the row keeps the note and the option; `approve_trust` (the answer
/// that also trusted the session again, R6, theseus-gyjyo.8) is no answer at all.
#[tokio::test]
async fn an_approvals_options_run_it_or_decline_it() {
    let r = rig(write_script());
    let (_, exec, corr) = waiting_write(&r).await;
    let done = cli_answer(&r, &corr, "approve").await.unwrap();
    assert_eq!(done["question_id"], corr.as_str());
    assert_eq!(done["option"], "approve");
    assert_eq!(done["resumes"]["id"], exec.as_str(), "{done}");
    assert_eq!(done["resumes"]["kind"], "conversation");
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Written.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "approved\n"
    );
    assert_eq!(confirm_rows(&r)[0]["option"], "approve");

    let r = rig(write_script());
    let (_, _, corr) = waiting_write(&r).await;
    let done = answer_with(
        &r,
        surface("sock#1", Cli),
        &corr,
        "decline",
        json!({"note": "not now"}),
    )
    .await
    .unwrap();
    assert_eq!(done["option"], "decline");
    let rows = confirm_rows(&r);
    assert_eq!(
        (rows[0]["option"].as_str(), rows[0]["note"].as_str()),
        (Some("decline"), Some("not now"))
    );
    assert_eq!(rows[0]["approved"], json!(false));
    assert!(!r.root.join("out.txt").exists());

    let r = rig(write_script());
    let (_, _, corr) = waiting_write(&r).await;
    let e = cli_answer(&r, &corr, "approve_trust").await.unwrap_err();
    assert!(
        e.message.contains("is not an answer to this question"),
        "{e:?}"
    );
    assert!(confirm_rows(&r).is_empty(), "the call keeps waiting");
    assert!(!r.root.join("out.txt").exists());
}

/// A question names the turn whose call asks, so a chat that follows the
/// session shows only its own turns' (theseus-fkvb0): an approval's from its
/// call's node, a budget question's from the call it waits on, in
/// `confirm.list` and in `budget.list`.
#[tokio::test]
async fn confirm_list_carries_the_asking_turn() {
    let r = rig(write_script());
    let res = turn(&r.core, None, "write out.txt").await;
    let q = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(q.turn_id.as_deref(), Some(res.turn_id.as_str()));
    let r = over_limit_rig();
    let res = turn(&r.core, None, "hello").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    let q = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(q.turn_id.as_deref(), Some(res.turn_id.as_str()), "{q:?}");
    let listed = r.core.budget_list().unwrap();
    let asked = listed
        .executions
        .iter()
        .find_map(|b| b.question.as_ref())
        .unwrap();
    assert_eq!(asked.turn_id.as_deref(), Some(res.turn_id.as_str()));
}

/// A budget question's two answers: `reset` sends the spend back to $0 and
/// the call proceeds; `decline` leaves the session waiting. An approval's
/// option is not a budget's.
#[tokio::test]
async fn a_budget_questions_options_reset_it_or_decline_it() {
    let r = over_limit_rig();
    let res = turn(&r.core, None, "hello").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    let q = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(
        q.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["reset", "decline"]
    );
    let e = cli_answer(&r, &q.correlation_id, "approve")
        .await
        .expect_err("approve is not a budget's answer");
    assert_eq!(e.code, error_code::INVALID_PARAMS, "{e:?}");
    assert!(e.message.contains("reset, decline"), "{}", e.message);
    let done = cli_answer(&r, &q.correlation_id, "decline").await.unwrap();
    assert_eq!(done["option"], "decline");
    assert!(done.get("resumes").is_none(), "nothing resumes: {done}");
    assert!(
        done["detail"]
            .as_str()
            .is_some_and(|d| d.ends_with("keeps waiting on its budget; a new message asks again")),
        "{done}"
    );
    assert_eq!(confirm_rows(&r)[0]["option"], "decline");

    let r = over_limit_rig();
    let res = turn(&r.core, None, "hello").await;
    let exec = res.execution_id.unwrap();
    let q = r.core.confirm_list().unwrap().remove(0);
    let done = cli_answer(&r, &q.correlation_id, "reset").await.unwrap();
    assert_eq!(done["option"], "reset");
    assert_eq!(done["resumes"]["id"], exec.as_str());
    assert_eq!(confirm_rows(&r)[0]["option"], "reset");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.budget.resets, 1);
}

/// An option the question lacks is refused with the valid ones, and the
/// question keeps waiting.
#[tokio::test]
async fn an_unknown_option_is_refused_with_the_valid_ones() {
    let r = rig(write_script());
    let (_, _, corr) = waiting_write(&r).await;
    let e = cli_answer(&r, &corr, "maybe").await.expect_err("unknown");
    assert_eq!(e.code, error_code::INVALID_PARAMS, "{e:?}");
    assert!(
        e.message.contains("approve") && e.message.contains("decline"),
        "{}",
        e.message
    );
    let e = cli_answer(&r, &corr, "reset")
        .await
        .expect_err("a budget's");
    assert!(e.message.contains("is not an answer"), "{}", e.message);
    assert_eq!(r.core.confirm_list().unwrap().len(), 1, "it still waits");
}

/// The place rule judges `work.answer` as it judges `action.confirm`: the
/// owner from a shared channel is refused, ledgered, and the question keeps
/// waiting; the MCP surface is never an approval surface; the owner's DM
/// counts.
#[tokio::test]
async fn the_place_rule_refuses_a_shared_place_and_the_mcp_surface() {
    let r = owned_rig(write_script(), false);
    let (_, exec, corr) = waiting_write(&r).await;
    let discord = |guild: Option<&str>| json!({"discord": origin(Some((OWNER, guild))), "author": "discord:zeroaltitude"});
    let e = answer_with(
        &r,
        surface("discord", Discord),
        &corr,
        "approve",
        discord(Some(GUILD)),
    )
    .await
    .expect_err("a shared place");
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(e.message.contains("shared"), "{}", e.message);
    let e = answer_with(&r, surface("mcp", Mcp), &corr, "approve", json!({}))
        .await
        .expect_err("the MCP surface");
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(
        e.message.contains("never an approval surface"),
        "{}",
        e.message
    );
    let rows = ledgered(&r, "approval.refused");
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows.iter().all(|row| row["method"] == "action.confirm"
        || row["method"] == "work.answer"
        || row.get("method").is_none()));
    assert_eq!(
        r.core.kernel.action(&corr).unwrap().unwrap().state,
        theseus_kernel::ActionState::Planned,
        "a refusal answers nothing"
    );
    assert!(confirm_rows(&r).is_empty());
    // The owner's DM counts.
    answer_with(
        &r,
        surface("discord", Discord),
        &corr,
        "approve",
        discord(None),
    )
    .await
    .unwrap();
    r.core.continue_execution(&exec).await.unwrap();
    assert!(r.root.join("out.txt").exists());
}

/// `may_answer` is the connection's: the CLI may; a shared place's binding
/// and a connection no listener named may not; the owner's DM may.
#[tokio::test]
async fn may_answer_is_false_for_a_shared_bindings_connection() {
    let r = owned_rig(write_script(), false);
    let (_, _, corr) = waiting_write(&r).await;
    let listed = |client: crate::approval::Client, params: Value| {
        let core = r.core.clone();
        let corr = corr.clone();
        async move {
            let v = rpc_as(
                &core,
                client,
                theseus_protocol::method::CONFIRM_LIST,
                params,
            )
            .await
            .unwrap();
            let c = v["confirms"][0].clone();
            assert_eq!(c["correlation_id"], corr.as_str());
            c["may_answer"].clone()
        }
    };
    assert_eq!(listed(surface("sock#1", Cli), Value::Null).await, true);
    assert_eq!(
        listed(
            surface("discord", Discord),
            json!({"discord": origin(Some((OWNER, Some(GUILD))))})
        )
        .await,
        false
    );
    assert_eq!(
        listed(
            surface("discord", Discord),
            json!({"discord": origin(Some((OWNER, None)))})
        )
        .await,
        true
    );
    assert_eq!(listed(surface("sock#9", Discord), Value::Null).await, false);
    assert_eq!(
        listed(
            surface("test", crate::approval::Surface::Unnamed),
            Value::Null
        )
        .await,
        false
    );
    // The options ride the request itself, worded once: a posture's ask in
    // a private place is the plain yes and the no (it offered a "don't ask
    // again" between them until scoped grants went, theseus-gyjyo.10).
    let q = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(
        q.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["approve", "decline"]
    );
}

/// One answer closes the question everywhere: every connection that watches
/// (a session's, and the all-session push) hears one `confirm.resolved` with
/// the option, and the execution's change shows no question pending. An
/// `action.confirm` carries the option too.
#[tokio::test]
async fn an_answer_from_one_surface_clears_every_other() {
    let r = rig(write_script());
    let (tx_all, mut all) = tokio::sync::mpsc::unbounded_channel();
    // The push observes once someone watches (`executions.watch`).
    r.core.push.ensure(&r.core).await.unwrap();
    r.core.bus.watch_all("herdr", tx_all);
    let (_, exec, corr) = waiting_write(&r).await;
    let sid = r.core.kernel.execution(&exec).unwrap().unwrap().session_id;
    let (tx_one, mut one) = tokio::sync::mpsc::unbounded_channel();
    r.core.bus.watch(&sid, "tui", tx_one);
    // Drain what the ask itself sent.
    while all.try_recv().is_ok() {}
    cli_answer(&r, &corr, "approve").await.unwrap();
    for rx in [&mut one, &mut all] {
        let mut resolved = Vec::new();
        let mut changed = Vec::new();
        // The push's change follows the frame, off this thread, and the
        // ask's own change can still reach a watcher that joined after it:
        // read on until the settled one.
        let settled = |c: &Value| c["pending"].as_array().is_none_or(Vec::is_empty);
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        while resolved.is_empty() || !changed.last().is_some_and(settled) {
            let Ok(Some(m)) = tokio::time::timeout_at(until, rx.recv()).await else {
                break;
            };
            if let theseus_protocol::Message::Notification(n) = m {
                match n.method.as_str() {
                    "confirm.resolved" => resolved.push(n.params),
                    "execution.changed" => changed.push(n.params),
                    _ => {}
                }
            }
        }
        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0]["correlation_id"], corr.as_str());
        assert_eq!(resolved[0]["option"], "approve");
        assert_eq!(resolved[0]["approved"], json!(true));
        assert_eq!(resolved[0]["by"], "the CLI");
        let last = changed.last().expect("the push's change");
        assert!(
            last["pending"].as_array().is_none_or(Vec::is_empty),
            "{last}"
        );
    }
    // The older verb names the same option.
    let r = rig(write_script());
    let (sid, _, corr) = waiting_write(&r).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    r.core.bus.watch(&sid, "tui", tx);
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    let resolved = sent(&mut rx, "confirm.resolved");
    assert_eq!(resolved[0]["option"], "decline", "{resolved:?}");
}

/// A question with options is answered by choosing one (theseus-j7cje): a
/// `choice:` binds the confirm, and a yes or a decline is refused.
#[test]
fn a_question_with_options_is_answered_by_choosing_one() {
    let e = route(QuestionKind::Ask, "approve").expect_err("an ask has no plain yes");
    assert_eq!(e.code, error_code::REFUSED);
    assert!(
        e.message.contains("answer with its number"),
        "{}",
        e.message
    );
    assert!(route(QuestionKind::Ask, "choice:0").unwrap());
    assert!(route(QuestionKind::Approval, "approve").unwrap());
    assert!(!route(QuestionKind::Budget, "decline").unwrap());
    assert!(route(QuestionKind::Change, "accept").unwrap());
    assert!(route(QuestionKind::Extension, "ack").unwrap());
}

/// An id is named whole or by a unique end of four characters or more: an
/// end naming two questions is refused naming both, a shorter one is refused,
/// and an unknown one is not found.
#[test]
fn a_short_id_names_one_question_or_is_refused() {
    let names: Vec<String> = ["act_aa11b0c3", "act_77b0c3", "act_5e92d1f4"]
        .map(String::from)
        .to_vec();
    assert_eq!(pick("act_77b0c3", &names).unwrap(), "act_77b0c3");
    assert_eq!(pick("…d1f4", &names).unwrap(), "act_5e92d1f4");
    assert_eq!(pick(" d1f4 ", &names).unwrap(), "act_5e92d1f4");
    let two = pick("b0c3", &names).expect_err("two questions");
    assert_eq!(two.code, error_code::INVALID_PARAMS);
    assert!(
        two.message.contains("names 2 questions")
            && two.message.contains("act_aa11b0c3")
            && two.message.contains("act_77b0c3"),
        "{}",
        two.message
    );
    let short = pick("0c3", &names).expect_err("too short");
    assert!(short.message.contains("too short"), "{}", short.message);
    let none = pick("zzzz", &names).expect_err("unknown");
    assert_eq!(none.code, error_code::NOT_FOUND);
}

/// Through a daemon: a unique end of a real id answers it, and an end under
/// four characters is refused before anything is read.
#[tokio::test]
async fn a_real_questions_end_answers_it() {
    let r = rig(write_script());
    let (_, _, corr) = waiting_write(&r).await;
    let e = cli_answer(&r, &corr[corr.len() - 3..], "approve")
        .await
        .expect_err("three characters");
    assert!(e.message.contains("too short"), "{}", e.message);
    let done = cli_answer(&r, &corr[corr.len() - 6..], "approve")
        .await
        .unwrap();
    assert_eq!(done["question_id"], corr.as_str());
}

/// The session or the execution a question waits in names it too, whole or by
/// its end (`theseus confirm` took these).
#[tokio::test]
async fn a_questions_session_or_execution_answers_it() {
    let r = rig(write_script());
    let (session, _, corr) = waiting_write(&r).await;
    let done = cli_answer(&r, &session[session.len() - 6..], "decline")
        .await
        .unwrap();
    assert_eq!(done["question_id"], corr.as_str(), "{done}");
    let r = rig(write_script());
    let (_, exec, corr) = waiting_write(&r).await;
    let done = cli_answer(&r, &exec, "decline").await.unwrap();
    assert_eq!(done["question_id"], corr.as_str(), "{done}");
}
