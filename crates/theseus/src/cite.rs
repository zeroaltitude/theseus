//! `theseus history <handle>` (theseus-ejdk): the source a reply's footnote
//! names. A handle (`m:a1b2c3`, as the reply wrote it between brackets) is
//! the end of a node's id: the daemon names the node (`node.reach`, as
//! `theseus reach` does), and its own page of the session shows it whole.

use std::io;

use anyhow::Result;
use theseus_client::render;
use theseus_client::Conn;
use theseus_protocol::cite::is_handle;
use theseus_protocol::{
    method, NodeReachParams, NodeReachResult, SessionHistoryParams, SessionHistoryResult,
};

use crate::cmd::output;
use crate::print;

/// `s` as a handle, whether the person typed it bare (`m:a1b2c3`) or as the
/// reply showed it (`[m:a1b2c3]`).
pub fn handle_of(s: &str) -> Option<&str> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    is_handle(s).then_some(s)
}

/// The cited message, whole, and where it sits in its session.
pub async fn open(conn: &mut Conn, json: bool, handle: &str) -> Result<()> {
    let tail = handle.trim_start_matches("m:");
    let r: NodeReachResult = serde_json::from_value(
        conn.request(
            method::NODE_REACH,
            NodeReachParams {
                node_id: tail.to_string(),
                max_generations: Some(0),
            },
        )
        .await?,
    )?;
    // The page after the position before it: the node itself.
    let v = conn
        .request(
            method::SESSION_HISTORY,
            SessionHistoryParams {
                session_id: r.session_id.clone(),
                n: Some(1),
                after: Some(r.position.saturating_sub(1)),
                before: None,
            },
        )
        .await?;
    output(json, v, |h: SessionHistoryResult| {
        let mut out = io::stdout().lock();
        for node in h.nodes.iter().filter(|n| n.node_id == r.node_id) {
            print::lines(&mut out, &render::history::node_lines(node, true))?;
        }
        print::lines(
            &mut out,
            &[render::Line::new(
                render::Tag::Dim,
                format!(
                    "── in {} at position {}: theseus history {} --before {}",
                    r.session_id,
                    r.position,
                    r.session_id,
                    r.position.saturating_add(1)
                ),
            )],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_is_read_bare_or_bracketed() {
        assert_eq!(handle_of("m:a1b2c3"), Some("m:a1b2c3"));
        assert_eq!(handle_of(" [m:a1b2c3] "), Some("m:a1b2c3"));
        assert_eq!(handle_of("ses_a1b2c3"), None);
        assert_eq!(handle_of("m:xyz"), None);
    }
}
