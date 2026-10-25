//! Reports taken by name inside a turn (theseus-z9mze): the delivery shape the
//! in-turn wait builds on. A report taken with `Kernel::take_reports_of`
//! reaches the parent's session as a relayed harness node titled
//! `task:<short>`, which is no tool result, so the clearing pass (it stubs
//! `Kind::ToolResult` nodes alone) never stubs it; a report not named is
//! delivered by the next turn, and the taken one is not written twice.

use serde_json::json;
use theseus_kernel::{ExecState, TurnEnd, Wake};

use crate::node::{Body, Origin};
use crate::provider::Scripted;
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, last_user, parent_exec, parent_session, rig, turn,
    until, ASK,
};

#[tokio::test]
async fn a_report_taken_by_name_is_a_relayed_harness_node_and_is_written_once() {
    let r = rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD: one") {
                return Scripted::text("Report one.");
            }
            if first.contains("CHILD: two") {
                return Scripted::text("Report two.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started both.");
            }
            if last.starts_with("START") {
                let call =
                    |b: &str| json!({"brief": b, "budget_usd": 1.5, "arrangement": arranged()});
                return Scripted::tools(
                    "Starting.",
                    &[
                        ("t_a", "task_create", call("CHILD: one")),
                        ("t_b", "task_create", call("CHILD: two")),
                    ],
                );
            }
            Scripted::text("Both reports are in.")
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let rec: crate::session::SessionRecord = r.core.store.get_session(&parent).unwrap().unwrap();
    let tasks = r.core.kernel.tasks(rec.execution_id.as_deref()).unwrap();
    assert_eq!(tasks.len(), 2, "{tasks:?}");
    let pid = tasks[0].parent.clone().unwrap();
    until("both tasks complete", || {
        tasks
            .iter()
            .all(|t| exec(&r.core, &t.id).state == ExecState::Complete)
    })
    .await;
    assert_eq!(parent_exec(&r.core, &parent).reports.len(), 2);
    let (first, second) = (tasks[0].clone(), tasks[1].clone());
    let sid = parent.clone();

    // A turn of the parent takes the first task's report by name.
    let g = r.core.kernel.admit_input(&pid).unwrap();
    let taken = r
        .core
        .kernel
        .take_reports_of(&g, std::slice::from_ref(&first.id), |ids| {
            let mut records = Vec::new();
            for id in ids {
                let rep = crate::task::load_report(&r.core.store, &r.core.kernel, id)?
                    .expect("an ended task has a report");
                // As `read_reports` builds it (turn.rs is not reshaped here).
                let n = crate::node::Node::relayed(
                    &sid,
                    Some("trn_wait"),
                    Origin::Harness,
                    &format!("task:{}", rep.short),
                    &rep.node_text(),
                );
                records.push(n.record()?);
            }
            Ok(records)
        })
        .unwrap();
    assert_eq!(taken.ids, vec![first.id.clone()]);
    r.core
        .kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();

    let nodes = reports_in(&r.core, &parent);
    assert_eq!(nodes.len(), 1);
    let short = crate::task::short(&first.session_id);
    let n = &nodes[0];
    assert_eq!(n.author.as_deref(), Some(format!("task:{short}").as_str()));
    assert_eq!(n.origin, Origin::Harness);
    assert_ne!(
        n.kind_str(),
        "tool_result",
        "the clearing pass stubs only those"
    );
    assert!(text_of(n).contains("Report one."), "{}", text_of(n));
    assert_eq!(
        parent_exec(&r.core, &parent).reports,
        vec![second.id.clone()]
    );

    // The parent's next turn delivers the other report, and only it.
    turn(&r.core, &parent, "What came back?").await;
    let nodes = reports_in(&r.core, &parent);
    assert_eq!(nodes.len(), 2, "one each, none twice");
    let texts: Vec<String> = nodes.iter().map(text_of).collect();
    assert_eq!(
        texts.iter().filter(|t| t.contains("Report one.")).count(),
        1
    );
    assert_eq!(
        texts.iter().filter(|t| t.contains("Report two.")).count(),
        1
    );
    assert!(parent_exec(&r.core, &parent).reports.is_empty());
}

fn reports_in(core: &crate::Core, sid: &str) -> Vec<crate::node::Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .collect()
}

fn text_of(n: &crate::node::Node) -> String {
    match &n.body {
        Body::UserMessage { text, .. } => text.clone(),
        other => panic!("{other:?}"),
    }
}
