//! A task tree through the whole core (theseus-tecj5): ten levels of tasks
//! open and the eleventh is refused naming the depth; every descendant's brief
//! carries the root goal and the chain; a brief that repeats an ancestor's is
//! refused as a cycle.

use serde_json::json;
use theseus_kernel::Execution;

use crate::node::{Body, ResultStatus};
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, only_task, parent_session, rig, turn, until, ASK,
};

/// `task.create`, its arrangement quoting `quote`, a message of the calling
/// session (the conversation's ask, or a task's own brief).
fn start(brief: &str, quote: Option<&str>) -> Scripted {
    let arrangement = match quote {
        Some(q) => json!({"pieces": [{"quote": q, "role": "objective"}]}),
        None => arranged(),
    };
    let input = json!({ "brief": brief, "arrangement": arrangement });
    Scripted::tools("Starting it.", &[("t_task", "task_create", input)])
}

/// The brief a task was given: what its first request ends with.
fn level_of(req: &ProviderRequest) -> Option<u32> {
    let first = first_user(req);
    let at = first.rfind("LEVEL ")?;
    first[at + 6..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// A conversation that starts `LEVEL 1`, and tasks that each start the next
/// level, whatever it is.
fn climbing(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("done");
    }
    match level_of(req) {
        Some(k) => start(
            &format!("LEVEL {}: take the next step", k + 1),
            Some("take the next step"),
        ),
        None => start("LEVEL 1: take the next step", None),
    }
}

/// The chain below `parent` as the kernel holds it, outermost first.
fn chain(core: &crate::Core, parent: &str) -> Vec<Execution> {
    let mut out: Vec<Execution> = Vec::new();
    let mut at = parent_exec_id(core, parent);
    loop {
        let tasks = core.kernel.tasks(Some(&at)).unwrap();
        match tasks.into_iter().next() {
            Some(t) => {
                at = t.id.clone();
                out.push(t);
            }
            None => return out,
        }
    }
}

fn parent_exec_id(core: &crate::Core, session: &str) -> String {
    crate::tests_tasks::parent_exec(core, session).id
}

/// What a task's `task.create` call answered.
fn create_result(core: &crate::Core, task_session: &str) -> (ResultStatus, String) {
    core.store
        .session_nodes(task_session)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool,
                status,
                content,
                ..
            } if tool == "task.create" => Some((status, content)),
            _ => None,
        })
        .expect("the task made a task.create call")
}

/// The brief text a task session starts with.
fn brief_of(core: &crate::Core, task_session: &str) -> String {
    match core.store.first_node(task_session).unwrap().unwrap().1.body {
        Body::UserMessage { text, .. } => text,
        other => panic!("not a brief: {other:?}"),
    }
}

/// Ten levels open; the tenth task's own `task.create` is refused naming the
/// depth reached, and nothing is opened below it. Every brief below the first
/// carries the root goal and the chain, the tenth's included.
#[tokio::test]
async fn ten_levels_of_tasks_open_and_the_eleventh_is_refused_naming_the_depth() {
    // A quarter of what is left, ten times over, needs a limit this large for
    // the tenth task to afford one call: the carve shrinks by four at every level.
    let r = rig(climbing, |c| c.kernel.spend_limit_usd = 100_000_000.0);
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    until("ten levels exist", || chain(&r.core, &parent).len() == 10).await;
    let tasks = chain(&r.core, &parent);
    let tenth = tasks.last().unwrap();
    until("the tenth task ended", || {
        exec(&r.core, &tenth.id).state.is_terminal()
    })
    .await;
    assert_eq!(chain(&r.core, &parent).len(), 10, "no eleventh task");
    let (status, said) = create_result(&r.core, &tenth.session_id);
    assert_eq!(status, ResultStatus::Error);
    assert_eq!(said, crate::task_lineage::depth_refusal(10, 10));
    assert!(
        said.contains("task depth 10") && said.contains("10 deep"),
        "{said}"
    );
    for (i, t) in tasks.iter().enumerate() {
        assert_eq!(r.core.kernel.task_depth(&t.id).unwrap(), i + 1);
    }
    // The tenth task's brief: its place, the root goal (the first task's
    // brief), and the chain from the root with the short ids of the nine above.
    let brief = brief_of(&r.core, &tenth.session_id);
    assert!(brief.contains("depth 10 of 10"), "{brief}");
    assert!(
        brief.contains(
            "Root goal, the brief of this tree's first task: \"LEVEL 1: take the next step\""
        ),
        "{brief}"
    );
    for t in &tasks[..9] {
        assert!(
            brief.contains(&crate::task::short(&t.session_id)),
            "{brief}"
        );
    }
    assert!(brief.ends_with("LEVEL 10: take the next step"), "{brief}");
    assert!(
        brief.contains("You cannot start tasks of your own: this is the deepest level."),
        "{brief}"
    );
    // The first task is as it always was: no tree to speak of.
    let first = brief_of(&r.core, &tasks[0].session_id);
    assert!(first.contains("depth 1 of 10"), "{first}");
    assert!(!first.contains("Root goal"), "{first}");
    // What it costs: the place paragraph at depth ten.
    let header = brief.split_once("\n\n").unwrap().0;
    let note = &header[header.find("This task is at depth").unwrap()..];
    println!(
        "lineage note at depth 10: {} characters",
        note.chars().count()
    );
    assert!(note.chars().count() < 1_800, "{}", note.chars().count());
}

/// A brief equal to a grandparent's is refused as a cycle, and the refusal is
/// not the depth one.
#[tokio::test]
async fn a_brief_that_repeats_an_ancestors_is_refused_as_a_cycle() {
    let r = rig(
        |req| {
            if answers_a_call(req) {
                return Scripted::text("done");
            }
            let first = first_user(req);
            if first.ends_with("B: the other half") {
                return start("  A: the whole job \n", Some("the other half"));
            }
            if first.ends_with("A: the whole job") {
                return start("B: the other half", Some("the whole job"));
            }
            start("A: the whole job", None)
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    until("two levels exist", || chain(&r.core, &parent).len() == 2).await;
    let tasks = chain(&r.core, &parent);
    until("the second task ended", || {
        exec(&r.core, &tasks[1].id).state.is_terminal()
    })
    .await;
    assert_eq!(
        chain(&r.core, &parent).len(),
        2,
        "the repeat opened nothing"
    );
    let (status, said) = create_result(&r.core, &tasks[1].session_id);
    assert_eq!(status, ResultStatus::Error);
    assert!(said.starts_with("Refused as a cycle"), "{said}");
    assert!(
        said.contains(&crate::task::short(&tasks[0].session_id)),
        "{said}"
    );
    assert!(
        !said.contains("task depth"),
        "not the depth refusal: {said}"
    );
}

/// A task started from a conversation is as it was: one level, a brief that
/// says depth 1, and the owner's tool-free sessions untouched. The limit of one
/// still forbids a task's task (here through the kernel and the pre-check).
#[tokio::test]
async fn a_task_of_a_conversation_still_opens_and_the_limit_still_binds() {
    let r = rig(climbing, |c| c.kernel.task_depth = 1);
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    until("the task ended", || {
        exec(&r.core, &task.id).state.is_terminal()
    })
    .await;
    let (status, said) = create_result(&r.core, &task.session_id);
    assert_eq!(status, ResultStatus::Error);
    assert_eq!(said, crate::task_lineage::depth_refusal(1, 1));
    assert!(r.core.kernel.tasks(Some(&task.id)).unwrap().is_empty());
}
