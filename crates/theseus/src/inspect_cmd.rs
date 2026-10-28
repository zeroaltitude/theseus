//! `theseus inspect [SESSION] [TURN]` (theseus-gyjyo R7b): one turn's requests
//! as the operator reads them: per loop, the model, the provider, the
//! compilation and the request's digest, and which vault secrets the request
//! carried, by name, and whether each was substituted before it left.
//!
//! That is the answer to "what key did we accidentally send?": a vault
//! secret, named, in a request, to a model. A key the vault never resolved is
//! not named here; it is in the record, found by reading it (`theseus
//! history`). The cockpit's inspector shows the same (`turn.inspect`).

use anyhow::Result;
use serde_json::{json, Value};
use theseus_client::{resolve, Conn};
use theseus_protocol::inspect::{TurnInspectParams, TurnInspectResult};
use theseus_protocol::method;

pub async fn inspect(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    turn: Option<String>,
) -> Result<()> {
    let session_id = match session {
        Some(s) => resolve::session(conn, &s).await?,
        None => crate::why::latest_session(conn).await?,
    };
    let turn_id = crate::open::turn_of(conn, &session_id, turn).await?;
    let v = conn
        .request(
            method::TURN_INSPECT,
            TurnInspectParams {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
            },
        )
        .await?;
    let r: TurnInspectResult = serde_json::from_value(v)?;
    if json {
        println!(
            "{}",
            json!({"session_id": r.session_id, "turn_id": r.turn_id,
            "loops": r.loops.iter().map(|l| loop_json(l, &r)).collect::<Vec<_>>()})
        );
        return Ok(());
    }
    for line in lines(&r) {
        println!("{line}");
    }
    Ok(())
}

/// Every `request.redacted` row of the turn that was a model call, as
/// (loop, row data), in the order they were written. A request whose call
/// failed has a row too, though no answer to carry it.
fn requests(r: &TurnInspectResult) -> Vec<(u32, &Value)> {
    r.rows
        .iter()
        .filter(|x| x.kind == "request.redacted" && x.data["op"] == "stream")
        .map(|x| (x.data["loop"].as_u64().unwrap_or(0) as u32, &x.data))
        .collect()
}

/// How one request's secrets read: `anthropic_key ×3, db_password ×1`.
fn secrets(d: &Value) -> String {
    let names: Vec<&str> = d["names"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    format!(
        "{} ({} met)",
        if names.is_empty() {
            "a vault value".into()
        } else {
            names.join(", ")
        },
        d["count"].as_u64().unwrap_or(0)
    )
}

fn loop_json(l: &theseus_protocol::inspect::InspectLoop, r: &TurnInspectResult) -> Value {
    let a = l.answer.as_ref();
    json!({
        "loop": l.loop_index,
        "provider": a.map(|a| a.provider.as_str()),
        "model": a.map(|a| a.model.as_str()),
        "compilation_id": a.and_then(|a| a.compilation_id.as_deref()),
        "request_digest": a.and_then(|a| a.request_digest.as_deref()),
        "redacted": requests(r).into_iter().filter(|(i, _)| *i == l.loop_index)
            .map(|(_, d)| json!({"names": d["names"], "count": d["count"],
                 "substituted": d["substituted"], "correlation_id": d["correlation_id"]}))
            .collect::<Vec<_>>(),
    })
}

/// The turn, per loop.
pub(crate) fn lines(r: &TurnInspectResult) -> Vec<String> {
    let mut out = vec![format!(
        "turn {} of {}: {}, {} loop{}",
        r.turn_id,
        r.session_id,
        if r.outcome.is_empty() {
            "running"
        } else {
            &r.outcome
        },
        r.loops.len(),
        if r.loops.len() == 1 { "" } else { "s" }
    )];
    let all = requests(r);
    let mut shown = 0;
    for l in &r.loops {
        let a = l.answer.as_ref();
        out.push(format!(
            "loop {}  {}/{}  compilation {}  digest {}",
            l.loop_index,
            a.map_or("?", |a| a.provider.as_str()),
            a.map_or("?", |a| a.model.as_str()),
            a.and_then(|a| a.compilation_id.as_deref()).unwrap_or("-"),
            a.and_then(|a| a.request_digest.as_deref()).unwrap_or("-"),
        ));
        for (_, d) in all.iter().filter(|(i, _)| *i == l.loop_index) {
            shown += 1;
            out.push(format!("  carried {}: {}", secrets(d), sent(d)));
        }
    }
    // A request whose loop wrote no answer (the call failed): still named.
    for (i, d) in &all {
        if !r.loops.iter().any(|l| l.loop_index == *i) {
            shown += 1;
            out.push(format!("loop {i}  carried {}: {}", secrets(d), sent(d)));
        }
    }
    if shown == 0 {
        out.push("no request carried a value the vault resolved.".into());
    }
    out
}

fn sent(d: &Value) -> &'static str {
    if d["substituted"].as_bool() == Some(true) {
        "substituted before it left"
    } else {
        "SENT AS WRITTEN (redact_outbound is off)"
    }
}
