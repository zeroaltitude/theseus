//! `theseus dismiss ID` and `theseus retry ID [--note WORDS]` (theseus-7zgsm):
//! a failed task stays flagged until you dismiss it, and a retry opens a new,
//! linked task from the same brief. Each is one call (`execution.dismiss`,
//! `task.retry`) the daemon answers in one frame. ID is the whole id of the
//! task or its execution, or at least the last four characters of either
//! (`resolve.rs`'s rule). Both are the operator's: refused inside a Theseus
//! job (`client::OPERATORS`).

use anyhow::{bail, Result};
use theseus_client::Conn;
use theseus_protocol::{
    method, ExecutionDismissParams, ExecutionDismissResult, ExecutionInfo, TaskRetryParams,
    TaskRetryResult,
};

use crate::cmd::output;
use theseus_client::render::short_id;
use theseus_client::resolve;

/// `theseus retry`'s arguments.
#[derive(clap::Args, Debug)]
pub struct RetryArgs {
    /// The failed task: its id, its execution's id, or at least the last four characters of
    /// either, as `theseus tasks` shows them.
    #[arg(value_name = "ID")]
    pub id: String,
    /// Why, or what is different: the new task reads it after the brief, and the ledger keeps it.
    #[arg(long, value_name = "WORDS")]
    pub note: Option<String>,
}

/// The failure `given` names among `execs`: failed, blocked or out of
/// budget. `task_only` for a retry, which opens a task from a task.
pub fn target<'a>(
    given: &str,
    execs: &'a [ExecutionInfo],
    task_only: bool,
) -> Result<&'a ExecutionInfo> {
    let found = resolve::pick(
        given,
        "task",
        execs,
        |e| vec![e.session_id.as_str(), e.execution_id.as_str()],
        |e| e.session_id.as_str(),
    )?;
    let e = found[0];
    if !matches!(e.state.as_str(), "failed" | "blocked" | "budget_exhausted") {
        bail!(
            "{} is {}, not a failure: only a failed, blocked or budget-exhausted one is {}",
            short_id(&e.execution_id),
            e.state,
            if task_only { "retried" } else { "dismissed" }
        );
    }
    if task_only && e.kind != "task" {
        bail!(
            "{} is a conversation, not a task: dismiss its failure, or send it another message",
            short_id(&e.execution_id)
        );
    }
    Ok(e)
}

/// `dismiss` and `retry`: one arm of `main`'s `run`, which is at clippy's line
/// limit.
pub async fn run(conn: &mut Conn, json: bool, cmd: crate::Cmd) -> Result<()> {
    match cmd {
        crate::Cmd::Dismiss { id } => dismiss(conn, json, id).await,
        crate::Cmd::Retry(a) => retry(conn, json, a).await,
        _ => unreachable!("main sends only dismiss and retry"),
    }
}

/// `theseus dismiss ID`.
pub async fn dismiss(conn: &mut Conn, json: bool, id: String) -> Result<()> {
    resolve::long_enough(&id, "task")?;
    let execs = resolve::executions(conn).await?;
    let execution_id = target(&id, &execs, false)?.execution_id.clone();
    let v = conn
        .request(
            method::EXECUTION_DISMISS,
            ExecutionDismissParams {
                execution_id,
                discord: None,
            },
        )
        .await?;
    output(json, v, |r: ExecutionDismissResult| {
        println!(
            "{} dismissed: it stops needing you, and its record stays",
            short_id(&r.execution_id)
        );
        Ok(())
    })
}

/// `theseus retry ID [--note WORDS]`.
pub async fn retry(conn: &mut Conn, json: bool, a: RetryArgs) -> Result<()> {
    resolve::long_enough(&a.id, "task")?;
    let execs = resolve::executions(conn).await?;
    let execution_id = target(&a.id, &execs, true)?.execution_id.clone();
    let v = conn
        .request(
            method::TASK_RETRY,
            TaskRetryParams {
                execution_id,
                note: a.note,
                discord: None,
            },
        )
        .await?;
    output(json, v, |r: TaskRetryResult| {
        println!(
            "{} retried as {} with ${:.2} (the failed one had ${:.2}); {} is dismissed",
            short_id(&r.retry_of),
            short_id(&r.task_id),
            r.limit_usd,
            r.failed_limit_usd,
            short_id(&r.retry_of)
        );
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info(session: &str, kind: &str, state: &str) -> ExecutionInfo {
        serde_json::from_value(json!({
            "execution_id": session.replace("ses_", "exe_"), "session_id": session,
            "kind": kind, "state": state, "turns": 1, "interrupted": 0, "outstanding": 0,
            "queued_results": 0,
            "budget": {"limit_usd": 5.0, "spent_usd": 1.0, "reserved_usd": 0.0,
                       "available_usd": 4.0, "held_unknown_usd": 0.0, "pinned": true},
            "created_at_ms": 1, "updated_at_ms": 2
        }))
        .unwrap()
    }

    /// A short end names the failed task, by its session's end or its
    /// execution's; a running task, or a conversation asked to retry, is
    /// refused with words.
    #[test]
    fn an_end_names_a_failure_and_nothing_else_is_taken() {
        let execs = vec![
            info("ses_000000tide1", "task", "failed"),
            info("ses_000000tide2", "task", "running"),
            info("ses_000000quay3", "conversation", "failed"),
        ];
        assert_eq!(
            target("tide1", &execs, true).unwrap().session_id,
            "ses_000000tide1"
        );
        assert_eq!(
            target("exe_000000tide1", &execs, false).unwrap().session_id,
            "ses_000000tide1"
        );
        let e = target("tide2", &execs, true).unwrap_err().to_string();
        assert!(e.contains("is running, not a failure"), "{e}");
        let e = target("quay3", &execs, true).unwrap_err().to_string();
        assert!(e.contains("is a conversation, not a task"), "{e}");
        assert!(target("quay3", &execs, false).is_ok());
        let e = target("tid", &execs, false).unwrap_err().to_string();
        assert!(e.contains("too short"), "{e}");
    }
}
