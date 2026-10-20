//! An answer from an agent's process is labelled as the agent's, and what
//! spends money or changes AWS is the owner's own (theseus-5rvss). Answers go
//! through real protocol connections (`rpc_as`); a connection says who is
//! behind it as its listener would (`Client::behind`, a test's word for what
//! `peer_proc` finds in `/proc`). A child of `tests_m3` for its rig.

use super::*;
use crate::approval::Surface::Cli;
use crate::peer_proc::Behind;
use theseus_protocol::{method, DigestKind};

const WHY: &str = "this spends money or changes AWS, so only the owner answers it";

/// A connection from a process with no terminal, or under `claude`.
fn agent(name: &str) -> crate::approval::Client {
    surface("sock#9", Cli).behind(Behind::Agent(name.into()))
}

/// A connection from the owner's own terminal.
fn owner() -> crate::approval::Client {
    surface("sock#1", Cli).behind(Behind::Owner)
}

async fn confirm(
    r: &Rig,
    who: crate::approval::Client,
    question: &str,
    approve: bool,
) -> Result<Value, theseus_protocol::RpcError> {
    rpc_as(
        &r.core,
        who,
        method::ACTION_CONFIRM,
        json!({"correlation_id": question, "approve": approve}),
    )
    .await
}

/// A question the gate would ask, planted: its action waits for a confirm in
/// a conversation of its own. Answering it binds the confirm; nothing runs.
fn plant(core: &Core, tool: &str) -> String {
    use theseus_kernel::{Authority, Proposal, RetryClass, TurnEnd, Wake};
    let k = &core.kernel;
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    let operator = Authority {
        principal: crate::turn::OPERATOR.into(),
        ..Default::default()
    };
    let e = k
        .open_execution(
            &rec.session_id,
            SessionKind::Conversation,
            operator,
            None,
            None,
        )
        .unwrap();
    rec.execution_id = Some(e.id.clone());
    core.store.put_session(&rec.session_id, &rec).unwrap();
    k.wake_input(&e.id).unwrap();
    let guard = k.admit(&e.id).unwrap();
    let p = Proposal {
        tool: tool.into(),
        args: json!({"stack": "example-harbour"}),
        resource: None,
        policy_context: Value::Null,
    };
    let a = k
        .plan_confirm(&guard, &p, RetryClass::NonRepeatable, Some(600_000))
        .unwrap();
    let wake = Wake::Confirm {
        confirm_id: a.correlation_id.clone(),
    };
    k.end_turn(guard, TurnEnd::Wait { wake }).unwrap();
    a.correlation_id
}

fn still_waiting(r: &Rig, question: &str) {
    let a = r.core.kernel.action(question).unwrap().unwrap();
    assert!(a.awaits_confirm(), "{question} is answered: {a:?}");
}

/// The key test. An `aws.stack.apply` question answered `approve` from a
/// process with no terminal is refused with words, ledgered as
/// `approval.refused` naming the agent, and stays open; the owner's own
/// terminal answers it.
#[tokio::test]
async fn an_agents_approve_of_a_stack_apply_is_refused_and_the_owners_binds() {
    let r = rig(vec![]);
    let q = plant(&r.core, "aws.stack.apply");

    let e = confirm(&r, agent(crate::peer_proc::NO_TERMINAL), &q, true)
        .await
        .expect_err("an agent does not answer a stack apply");
    assert!(e.message.contains(WHY), "{}", e.message);
    assert!(e.message.contains("press it in the DM"), "{}", e.message);
    let refused = ledgered(&r, "approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["by"], "agent:no terminal");
    assert_eq!(refused[0]["via"], "cli");
    assert!(refused[0]["why"].as_str().unwrap().contains(WHY));
    assert!(ledgered(&r, "action.confirm_answered").is_empty());
    still_waiting(&r, &q);

    // A decline is refused the same way: the owner's call, either answer.
    confirm(&r, agent("claude"), &q, false)
        .await
        .expect_err("an agent does not decline it either");
    assert_eq!(ledgered(&r, "approval.refused").len(), 2);
    still_waiting(&r, &q);

    confirm(&r, owner(), &q, true).await.expect("the owner's");
    let answered = ledgered(&r, "action.confirm_answered");
    assert_eq!(answered.len(), 1, "{answered:?}");
    assert_eq!(answered[0]["by"], "the CLI");
    let a = r.core.kernel.action(&q).unwrap().unwrap();
    assert!(a.confirm.is_some(), "the owner's yes bound the confirm");
}

/// Every AWS tool that changes AWS or costs, and none that only reads for
/// free, is the owner's alone; so is the budget question.
#[test]
fn money_and_aws_are_the_owners_alone_and_free_reads_are_not() {
    use crate::approval::owners_alone;
    for tool in [
        "aws.stack.plan",
        "aws.stack.apply",
        "aws.stack.status",
        "aws.stack.delete",
        "aws.call",
        "aws.s3.put",
        "aws.hands.run",
        "aws.cost",
        "aws.inventory",
        "aws.logs.query",
        theseus_kernel::BUDGET_TOOL,
    ] {
        assert!(owners_alone(tool), "{tool}");
    }
    for tool in [
        "aws.describe",
        "aws.whoami",
        "aws.s3.list",
        "aws.s3.get",
        "aws.logs.tail",
        "aws.trail",
        "fs.write",
        "proc.run",
    ] {
        assert!(!owners_alone(tool), "{tool}");
    }
}

/// A budget question and the spend reset it answers follow the same rule.
#[tokio::test]
async fn an_agent_cannot_reset_a_spend_limit_and_the_owner_can() {
    use theseus_kernel::ExecState;
    let r = over_budget_rig(vec![Scripted::text("Done.")]);
    let res = turn(&r.core, None, "diff these").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    let q = res.awaiting_confirm.clone().expect("the budget question");
    let exec = res.execution_id.clone().unwrap();

    for approve in [true, false] {
        let e = confirm(&r, agent(crate::peer_proc::NO_TERMINAL), &q, approve)
            .await
            .expect_err("an agent answers no budget question");
        assert!(e.message.contains(WHY), "{}", e.message);
    }
    assert_eq!(ledgered(&r, "approval.refused").len(), 2);
    assert!(ledgered(&r, "budget.reset").is_empty());
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!((e.state, e.budget.resets), (ExecState::Waiting, 0));

    confirm(&r, owner(), &q, true).await.expect("the owner's");
    let e = r.core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.budget.resets, 1);
    assert_eq!(ledgered(&r, "budget.reset")[0]["by"], "the CLI");
}

/// An ordinary approval is an agent's to answer, and it runs; the row says
/// who, by the agent's name.
#[tokio::test]
async fn an_agent_may_answer_an_ordinary_approval_and_it_is_labelled_as_its_own() {
    let r = rig(write_script());
    let res = turn(&r.core, None, "write out.txt").await;
    let q = res.awaiting_confirm.clone().expect("the write waits");
    confirm(&r, agent("claude"), &q, true).await.unwrap();
    let cont = r
        .core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cont.output, "Written.");
    assert!(r.root.join("out.txt").exists(), "the approved write ran");
    let rows = ledgered(&r, "action.confirm_answered");
    assert_eq!(rows[0]["by"], "agent:claude", "{rows:?}");
    assert!(ledgered(&r, "approval.refused").is_empty());
}

/// The digest says who answered, from the notice ring and from the ledger
/// alike: "you answered" for the owner's own (and for a row that names no
/// agent), "answered by an agent (<name>)" for an agent's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_digest_names_an_agent_that_answered_and_says_you_for_the_owner() {
    use crate::push::WorkScope;
    use std::collections::HashSet;
    for (who, words, by) in [
        (
            agent(crate::peer_proc::NO_TERMINAL),
            "✓ answered by an agent (no terminal): fs.write",
            Some("no terminal"),
        ),
        (
            agent("claude"),
            "✓ answered by an agent (claude): fs.write",
            Some("claude"),
        ),
        (owner(), "✓ you answered: fs.write", None),
    ] {
        let r = rig(write_script());
        // The board is fed once something watches it, as `work.watch` does.
        r.core.push.ensure(&r.core).await.unwrap();
        let since = r.core.push.work_snapshot(1, &WorkScope::default()).0;
        let res = turn(&r.core, None, "write out.txt").await;
        let q = res.awaiting_confirm.clone().expect("the write waits");
        confirm(&r, who, &q, true).await.unwrap();
        // The answer's frame reaches the board and the ledger.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        for from_ledger in [false, true] {
            let digest = r
                .core
                .digest_blocking(
                    since,
                    None,
                    &WorkScope::default(),
                    &HashSet::new(),
                    from_ledger,
                )
                .unwrap();
            let lines: Vec<_> = digest
                .lines
                .iter()
                .filter(|l| l.kind == DigestKind::Answered)
                .collect();
            assert_eq!(lines.len(), 1, "{:?}", digest.lines);
            assert!(
                lines[0].words.starts_with(words),
                "from the {}: {}",
                if from_ledger { "ledger" } else { "ring" },
                lines[0].words
            );
            assert_eq!(lines[0].by.as_deref(), by);
        }
    }
}

/// `confirm.list` tells an agent's connection it may not answer a spend
/// limit's question, so no surface draws a button whose answer is refused;
/// the owner's connection may.
#[tokio::test]
async fn confirm_list_says_an_agent_may_not_answer_a_money_question() {
    let r = over_budget_rig(vec![]);
    let res = turn(&r.core, None, "diff these").await;
    assert_eq!(res.stop_reason, "budget", "{res:?}");
    for (who, may) in [
        (agent(crate::peer_proc::NO_TERMINAL), false),
        (owner(), true),
    ] {
        let list = rpc_as(&r.core, who, method::CONFIRM_LIST, json!({}))
            .await
            .unwrap();
        let asked = list["confirms"].as_array().unwrap();
        assert_eq!(asked.len(), 1, "{list}");
        assert_eq!(asked[0]["may_answer"], json!(may), "{list}");
    }
}
