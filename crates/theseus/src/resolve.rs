//! One id resolver for the commands that name a session, an execution or a
//! question (theseus-0n1v): `confirm`, `history`, `watch`, `stop`, `cancel`,
//! `wait` and `explain`. An id is named whole, or by a unique end of it of at
//! least four characters, as `theseus tasks` and the terminal UI show them
//! (`…a1b2c3`). A task's ids share their tail (`act_X` opens `exe_X` in
//! `ses_X`, recorded as `tsk_X`), so an end may match a session's id and its
//! execution's at once: that is one thing, not two. An end that matches two
//! things is refused, naming them; one under four characters is refused; an
//! unknown one is refused. Each resolution makes at most one read, and none
//! for an id named whole where the daemon refuses an unknown one itself. (A
//! name that needs the executions reads them a page at a time.)

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use theseus_protocol::{
    method, ConfirmListResult, ExecutionInfo, ExecutionListParams, ExecutionListResult,
    SessionListResult,
};

use crate::client::Conn;

/// The fewest characters of an id that name it.
pub const SHORTEST: usize = 4;

/// What a person typed, without the `…` the lists print before an id's end.
pub fn trimmed(given: &str) -> &str {
    given.trim().trim_start_matches('…')
}

/// An id named whole, as the daemon writes them (`ses_…`, `act_…`): ends
/// that people type carry no `_`.
pub fn whole(given: &str) -> bool {
    trimmed(given).contains('_')
}

/// A session's id named whole (`ses_…`): it needs no read to name the
/// session. Any other whole id (an execution's) is found among the
/// executions.
pub fn whole_session(given: &str) -> Option<&str> {
    let s = trimmed(given);
    s.starts_with("ses_").then_some(s)
}

/// The things `given` names among `items`, each answering to its `ids`, and
/// grouped by `key` (a session's executions are one session): the one
/// group whose ids hold `given` whole, else the one with an id that ends
/// with it. `noun` names what is looked for, in the refusals.
pub fn pick<'a, T>(
    given: &str,
    noun: &str,
    items: &'a [T],
    ids: impl Fn(&'a T) -> Vec<&'a str>,
    key: impl Fn(&'a T) -> &'a str,
) -> Result<Vec<&'a T>> {
    let s = trimmed(given);
    let exact: Vec<&T> = items.iter().filter(|t| ids(t).contains(&s)).collect();
    let found = if exact.is_empty() {
        if s.chars().count() < SHORTEST {
            bail!(
                "`{given}` is too short to name a {noun}: give at least four characters of its id"
            );
        }
        items
            .iter()
            .filter(|t| ids(t).iter().any(|id| id.ends_with(s)))
            .collect()
    } else {
        exact
    };
    let mut keys: Vec<&str> = Vec::new();
    for t in &found {
        if !keys.contains(&key(t)) {
            keys.push(key(t));
        }
    }
    match keys.as_slice() {
        [] => bail!("no {noun} is named `{given}`"),
        [one] => Ok(found.into_iter().filter(|t| key(t) == *one).collect()),
        many => {
            let mut named: Vec<&str> = many.iter().take(5).copied().collect();
            if many.len() > 5 {
                named.push("…");
            }
            bail!(
                "`{given}` names {} {noun}s ({}): give more of its id",
                many.len(),
                named.join(", ")
            )
        }
    }
}

/// The session `given` names among the open executions: its own, a task's
/// and an execution's ids all name it.
pub fn session_of<'a>(given: &str, execs: &'a [ExecutionInfo]) -> Result<Vec<&'a ExecutionInfo>> {
    pick(
        given,
        "session",
        execs,
        |e| vec![e.session_id.as_str(), e.execution_id.as_str()],
        |e| e.session_id.as_str(),
    )
}

/// The executions a name is looked for among, newest first: every one, read a
/// page at a time (theseus-7bee), so no one read is the size of the store. A
/// name that is a unique end must be unique among all of them.
pub async fn executions(conn: &mut Conn) -> Result<Vec<ExecutionInfo>> {
    let mut all = Vec::new();
    let mut before = None;
    loop {
        let params = ExecutionListParams {
            n: Some(PAGE),
            before,
            ..Default::default()
        };
        let l: ExecutionListResult =
            serde_json::from_value(conn.request(method::EXECUTION_LIST, params).await?)?;
        all.extend(l.executions);
        // A daemon that answers every execution at once gives no cursor.
        match l.older {
            Some(o) if before != Some(o) => before = Some(o),
            _ => return Ok(all),
        }
    }
}

/// An execution's page when a name is looked for.
const PAGE: usize = 1000;

/// The session id `given` names, for a command whose daemon refuses an
/// unknown id itself (`history`): a whole session id as it is, with no read;
/// an execution's id, or an end of either, from the executions.
pub async fn session(conn: &mut Conn, given: &str) -> Result<String> {
    if let Some(s) = whole_session(given) {
        return Ok(s.to_string());
    }
    long_enough(given, "session")?;
    let execs = executions(conn).await?;
    Ok(session_of(given, &execs)?[0].session_id.clone())
}

/// The session id `given` names, known to exist (`watch`, whose daemon would
/// watch an unknown id and wait forever): a whole session id read alone, any
/// other name from the executions.
pub async fn existing_session(conn: &mut Conn, given: &str) -> Result<String> {
    if let Some(s) = whole_session(given) {
        let l: SessionListResult = serde_json::from_value(
            conn.request(method::SESSION_LIST, json!({ "ids": [s] }))
                .await?,
        )?;
        return l
            .sessions
            .into_iter()
            .find(|i| i.session_id == s)
            .map(|i| i.session_id)
            .ok_or_else(|| anyhow!("no session is named `{given}`"));
    }
    long_enough(given, "session")?;
    let execs = executions(conn).await?;
    Ok(session_of(given, &execs)?[0].session_id.clone())
}

/// The question `given` names: its correlation id whole, with no read (the
/// daemon refuses an unknown one); else, from what waits, the one question
/// whose correlation id, session or execution `given` names.
pub async fn question(conn: &mut Conn, given: &str) -> Result<String> {
    let s = trimmed(given);
    if whole(given) && !["ses_", "exe_"].iter().any(|p| s.starts_with(p)) {
        return Ok(s.to_string());
    }
    long_enough(given, "question")?;
    let waiting = serde_json::from_value::<ConfirmListResult>(
        conn.request(method::CONFIRM_LIST, Value::Null).await?,
    )?
    .confirms;
    Ok(pick(
        given,
        "question",
        &waiting,
        |c| {
            vec![
                c.correlation_id.as_str(),
                c.session_id.as_str(),
                c.execution_id.as_str(),
            ]
        },
        |c| c.correlation_id.as_str(),
    )?[0]
        .correlation_id
        .clone())
}

/// Refuse a name too short to be one, before anything is read or sent.
pub fn long_enough(given: &str, noun: &str) -> Result<()> {
    if trimmed(given).chars().count() < SHORTEST {
        bail!("`{given}` is too short to name a {noun}: give at least four characters of its id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(exe: &str, ses: &str, kind: &str) -> ExecutionInfo {
        serde_json::from_value(json!({
            "execution_id": exe, "session_id": ses, "kind": kind, "state": "waiting",
            "turns": 1, "interrupted": 0, "outstanding": 0, "queued_results": 0,
            "budget": {"limit_usd": 1.0, "spent_usd": 0.0, "reserved_usd": 0.0,
                       "held_unknown_usd": 0.0, "available_usd": 1.0}, "wake": null, "created_at_ms": 0, "updated_at_ms": 0
        }))
        .unwrap()
    }

    fn names(given: &str, execs: &[ExecutionInfo]) -> Result<String> {
        session_of(given, execs).map(|v| v[0].session_id.clone())
    }

    #[test]
    fn one_rule_unique_ambiguous_short_and_unknown() {
        let execs = [
            // A task: its execution and session share their tail.
            exec("exe_0c7e527d2a", "ses_0c7e527d2a", "task"),
            exec("exe_11aa99e5f6", "ses_22bb99e5f6", "conversation"),
            exec("exe_33cc4d4e5f6", "ses_44dd4d4e5f6", "conversation"),
        ];
        // An end shared by a task's session and execution names one session.
        assert_eq!(names("e527d2a", &execs).unwrap(), "ses_0c7e527d2a");
        assert_eq!(names("…527d2a", &execs).unwrap(), "ses_0c7e527d2a");
        // Whole ids, a session's and an execution's.
        assert_eq!(names("ses_22bb99e5f6", &execs).unwrap(), "ses_22bb99e5f6");
        assert_eq!(names("exe_11aa99e5f6", &execs).unwrap(), "ses_22bb99e5f6");
        // Ambiguous: refused, naming both.
        let many = names("e5f6", &execs).unwrap_err().to_string();
        assert!(many.contains("names 2 sessions"), "{many}");
        assert!(
            many.contains("ses_22bb99e5f6") && many.contains("ses_44dd4d4e5f6"),
            "{many}"
        );
        // Too short, and unknown.
        let short = names("5f6", &execs).unwrap_err().to_string();
        assert!(short.contains("too short"), "{short}");
        let none = names("zzzz99", &execs).unwrap_err().to_string();
        assert_eq!(none, "no session is named `zzzz99`");
        assert!(long_enough("abc", "task").is_err());
        assert!(long_enough("…abcd", "task").is_ok());
        assert!(whole("ses_x") && !whole("e527d2"));
        assert_eq!(whole_session("…ses_x"), Some("ses_x"));
        assert_eq!(whole_session("exe_x"), None);
    }

    /// The review's cases against the daemon's own rules (`task::resolve`,
    /// `wake::resolve`): an exact id wins over an end of another; a 4-character
    /// end that names a question's session and another's correlation id is two
    /// questions; a 4-character end names, a 3-character one is refused.
    #[test]
    fn exact_before_ends_and_a_question_by_its_session_or_its_id() {
        let execs = [
            exec("exe_q7f3k2", "ses_q7f3k2", "task"),
            exec("exe_zz", "ses_xses_q7f3k2", "conversation"),
        ];
        assert_eq!(names("ses_q7f3k2", &execs).unwrap(), "ses_q7f3k2");
        assert!(names("q7f3k2", &execs)
            .unwrap_err()
            .to_string()
            .contains("names 2"));
        let asks = [
            ("act_aa11b0c3", "ses_0d9e7f", "exe_0d9e7f"),
            ("act_77b0c3", "ses_7e7e9f", "exe_7e7e9f"),
        ];
        let q = |given: &str| {
            pick(given, "question", &asks, |c| vec![c.0, c.1, c.2], |c| c.0).map(|v| v[0].0)
        };
        // `b0c3` ends both correlation ids; `9e7f` one question's session.
        let two = q("b0c3").unwrap_err().to_string();
        assert!(two.contains("names 2 questions"), "{two}");
        assert_eq!(q("9e7f").unwrap(), "act_aa11b0c3");
        assert_eq!(q("exe_7e7e9f").unwrap(), "act_77b0c3");
        assert!(q("e9f").unwrap_err().to_string().contains("too short"));
    }
}
