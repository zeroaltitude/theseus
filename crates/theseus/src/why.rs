//! `theseus why [TURN]` (theseus-ighf): what a turn's model was shown, and
//! why each piece was there. Pure reading: `context.explain` for the turn
//! (its parts, its recalls with their reasons, what the compile left out),
//! `policy.explain` for the gate's layers, `node.list` for the tools the
//! turn called, and one ledger read for its `route.decided` row. The words
//! are `theseus_protocol::why`'s, which Discord's `/why` uses too: this
//! module only asks and prints. No recompile, no write, no frame.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use theseus_client::{resolve, Conn};
use theseus_protocol::context::{ContextExplainParams, ContextExplainResult};
use theseus_protocol::why::{tools_called, turn_rows, why_lines, WhyInput};
use theseus_protocol::{
    method, LedgerTailParams, LedgerTailResult, NodeListParams, NodeListResult,
    PolicyExplainParams, PolicyExplainResult, SessionListResult,
};

/// The most nodes and ledger rows read for a turn's tool calls and route.
const ROWS: usize = 1000;

/// What the daemon answered, as it came.
pub struct Asked {
    pub context: Value,
    pub policy: Value,
    pub route: Value,
    pub nodes: Value,
}

pub async fn why(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    turn: Option<String>,
) -> Result<()> {
    let session_id = match session {
        Some(s) => resolve::session(conn, &s).await?,
        None => latest_session(conn).await?,
    };
    let a = ask(conn, &session_id, turn.as_deref()).await?;
    if json {
        let v =
            json!({"context": a.context, "policy": a.policy, "route": a.route, "nodes": a.nodes});
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    let x: ContextExplainResult = serde_json::from_value(a.context)?;
    let p: PolicyExplainResult = serde_json::from_value(a.policy)?;
    let (route, nodes): (LedgerTailResult, NodeListResult) = (
        serde_json::from_value(a.route)?,
        serde_json::from_value(a.nodes)?,
    );
    let turn_id = x.turn_id.clone().unwrap_or_default();
    let route = turn_rows(&route, &turn_id).pop();
    let tools = tools_called(&nodes, &turn_id);
    let lines = why_lines(&WhyInput {
        explain: &x,
        policy: Some(&p),
        route: route.as_ref(),
        tools: &tools,
        hide_sources: false,
    });
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

/// `theseus context [SESSION]`: the short form (theseus-wy6v.2), one
/// `context.explain` for the session's last turn, in the words of
/// `theseus_protocol::fill`.
pub async fn context(conn: &mut Conn, json: bool, session: Option<String>) -> Result<()> {
    let session_id = match session {
        Some(s) => resolve::session(conn, &s).await?,
        None => latest_session(conn).await?,
    };
    let v = conn
        .request(
            method::CONTEXT_EXPLAIN,
            ContextExplainParams {
                session_id,
                turn_id: None,
            },
        )
        .await?;
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    let x: ContextExplainResult = serde_json::from_value(v)?;
    for l in theseus_protocol::fill::context_short(&x) {
        println!("{l}");
    }
    Ok(())
}

/// The reads: the turn named (a whole id, or a unique start or end of one of
/// at least four characters), else the session's last.
pub async fn ask(conn: &mut Conn, session_id: &str, turn: Option<&str>) -> Result<Asked> {
    let turn_id = match turn {
        None => None,
        Some(t) => Some(find_turn(conn, session_id, t).await?),
    };
    let context = conn
        .request(
            method::CONTEXT_EXPLAIN,
            ContextExplainParams {
                session_id: session_id.to_string(),
                turn_id,
            },
        )
        .await?;
    let policy = conn
        .request(
            method::POLICY_EXPLAIN,
            PolicyExplainParams {
                session_id: Some(session_id.to_string()),
                tool: None,
            },
        )
        .await?;
    let route = conn
        .request(
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(ROWS),
                kind: Some("route.decided".into()),
                session_id: Some(session_id.to_string()),
                ..LedgerTailParams::default()
            },
        )
        .await?;
    let nodes = conn
        .request(
            method::NODE_LIST,
            NodeListParams {
                session_id: Some(session_id.to_string()),
                kind: Some("tool_call".into()),
                n: Some(ROWS),
            },
        )
        .await?;
    Ok(Asked {
        context,
        policy,
        route,
        nodes,
    })
}

/// A turn of the session by a whole id, or by a unique start or end of one,
/// among the turns that compiled a context.
pub(crate) async fn find_turn(conn: &mut Conn, session_id: &str, given: &str) -> Result<String> {
    let s = resolve::trimmed(given);
    let list = conn
        .request(
            method::CONTEXT_EXPLAIN,
            ContextExplainParams {
                session_id: session_id.to_string(),
                turn_id: None,
            },
        )
        .await?;
    let x: ContextExplainResult = serde_json::from_value(list)?;
    let ids: Vec<&str> = x.turns.iter().map(|t| t.turn_id.as_str()).collect();
    pick_turn(
        &ids,
        s,
        "`theseus why` reads the newest {n} turns that compiled a context",
    )
}

/// Every turn the session has, newest first: its `turn.started` rows, which a
/// turn that compiled no context (a detour's) writes too (theseus-v8h48).
pub(crate) async fn started_turns(conn: &mut Conn, session_id: &str) -> Result<Vec<String>> {
    let v = conn
        .request(
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(ROWS),
                kind: Some("turn.started".into()),
                session_id: Some(session_id.to_string()),
                ..LedgerTailParams::default()
            },
        )
        .await?;
    let mut rows = serde_json::from_value::<LedgerTailResult>(v)?.rows;
    rows.sort_by_key(|r| std::cmp::Reverse(r.position));
    Ok(rows.into_iter().filter_map(|r| r.turn_id).collect())
}

/// The same pick among every turn the session has, not only those that
/// compiled a context: what `theseus open turn` names.
pub(crate) async fn find_started_turn(
    conn: &mut Conn,
    session_id: &str,
    given: &str,
) -> Result<String> {
    let ids = started_turns(conn, session_id).await?;
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    pick_turn(
        &ids,
        resolve::trimmed(given),
        "the newest {n} turns of the session",
    )
}

/// `ids` hold `s` whole, else exactly one of them starts or ends with it;
/// `listed` says, with `{n}` for the count, which turns were looked in.
fn pick_turn(ids: &[&str], s: &str, listed: &str) -> Result<String> {
    if let Some(t) = ids.iter().find(|t| **t == s) {
        return Ok((*t).to_string());
    }
    if s.chars().count() < resolve::SHORTEST {
        bail!("`{s}` is too short to name a turn: give at least four characters of its id");
    }
    let found: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| id.starts_with(s) || id.ends_with(s))
        .collect();
    match found.as_slice() {
        [] => Err(anyhow!(
            "no turn of this session is named `{s}` ({})",
            listed.replace("{n}", &ids.len().to_string())
        )),
        [one] => Ok((*one).to_string()),
        many => bail!(
            "`{s}` names {} turns: {}",
            many.len(),
            many.iter().take(5).copied().collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The session a command means when it names none: the most recently active
/// (as `theseus history` picks it).
pub(crate) async fn latest_session(conn: &mut Conn) -> Result<String> {
    let l: SessionListResult =
        serde_json::from_value(conn.request(method::SESSION_LIST, Value::Null).await?)?;
    l.sessions
        .first()
        .map(|s| s.session_id.clone())
        .ok_or_else(|| anyhow!("there are no sessions yet"))
}
