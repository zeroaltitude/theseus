//! `theseus fork <node>` (theseus-2qzx, R19): a new conversation from any
//! moment of another, by reference, and the lines that show a fork: its
//! result, the inherited prefix in `theseus history`, and the forks in
//! `theseus reach`.

use anyhow::Result;
use theseus_client::render::fork::forked_lines;
use theseus_client::Conn;
use theseus_protocol::{
    method, NodeReachParams, NodeReachResult, SessionForkParams, SessionForkResult,
};

use crate::cmd::output;

/// `theseus fork`, as `run` in main.rs hands it on whole (one line there:
/// that match is at clippy's length limit).
pub(crate) async fn cmd(conn: &mut Conn, json: bool, cmd: crate::Cmd) -> Result<()> {
    let crate::Cmd::Fork {
        node,
        title,
        profile,
        budget,
        session,
    } = cmd
    else {
        unreachable!("main.rs hands fork::cmd only `theseus fork`")
    };
    run(conn, json, node, session, (title, profile, budget)).await
}

/// `theseus fork NODE`: the session that holds NODE (or `--session`), forked
/// there.
pub async fn run(
    conn: &mut Conn,
    json: bool,
    node: String,
    session: Option<String>,
    (title, profile, budget_usd): (Option<String>, Option<String>, Option<f64>),
) -> Result<()> {
    let (session, node) = match session {
        Some(s) => (theseus_client::resolve::session(conn, &s).await?, node),
        // The node's own session: its reach at generation 0, which names
        // the node whole, so the fork is a lookup, not a walk of the keys.
        None => {
            let v = conn
                .request(
                    method::NODE_REACH,
                    NodeReachParams {
                        node_id: node,
                        max_generations: Some(0),
                    },
                )
                .await?;
            let r = serde_json::from_value::<NodeReachResult>(v)?;
            (r.session_id, r.node_id)
        }
    };
    let p = SessionForkParams {
        session,
        at: node,
        title,
        profile,
        budget_usd,
    };
    let v = conn.request(method::SESSION_FORK, &p).await?;
    output(json, v, |r: SessionForkResult| {
        crate::print::lines(&mut std::io::stdout().lock(), &forked_lines(&r))?;
        Ok(())
    })
}
