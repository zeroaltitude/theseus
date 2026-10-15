//! Rewind (theseus-23rk, R20), the client's half: put the files an agent's
//! edits changed, and the conversation, back to before one of your messages.
//! It is `files.restore` (the files), then `session.fork` at the node before
//! the message (the conversation), called in that order. The CLI's `theseus
//! rewind` and any other surface that does the two together use it; the
//! terminal chat sends the same requests itself, as effects.

use anyhow::{bail, Result};
use theseus_protocol::{
    method, FilesChangesParams, FilesChangesResult, FilesRestoreParams, FilesRestoreResult,
    NodeReachParams, NodeReachResult, RewindPoint, SessionForkParams, SessionForkResult,
};

use crate::Conn;

/// What a rewind takes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The files and the conversation.
    Both,
    /// A fork at the node before the message; the files stay as they are.
    ConversationOnly,
    /// The files; the conversation stays where it is.
    FilesOnly,
}

/// What a rewind did.
#[derive(Debug, Clone)]
pub struct Done {
    /// The message it went back to.
    pub point: RewindPoint,
    pub restored: Option<FilesRestoreResult>,
    pub forked: Option<SessionForkResult>,
}

/// The session that holds `node`, and the node whole: its reach at
/// generation 0 names it, so this is a lookup, not a walk.
pub async fn locate(conn: &mut Conn, node: &str) -> Result<(String, String)> {
    let v = conn
        .request(
            method::NODE_REACH,
            NodeReachParams {
                node_id: node.to_string(),
                max_generations: Some(0),
            },
        )
        .await?;
    let r = serde_json::from_value::<NodeReachResult>(v)?;
    Ok((r.session_id, r.node_id))
}

/// The point for one message of a session, with the files changed since it.
pub async fn point(conn: &mut Conn, session: &str, node: &str) -> Result<RewindPoint> {
    let p = FilesChangesParams {
        session_id: session.to_string(),
        node: Some(node.to_string()),
    };
    let r: FilesChangesResult =
        serde_json::from_value(conn.request(method::FILES_CHANGES, p).await?)?;
    match r.points.into_iter().next() {
        Some(p) => Ok(p),
        None => bail!("{node} is not a message of yours in {session}"),
    }
}

/// Go back to before the message `node` of `session` (both given whole).
pub async fn run(conn: &mut Conn, session: &str, node: &str, mode: Mode) -> Result<Done> {
    let point = point(conn, session, node).await?;
    // Settle the conversation's half first: a first message has no node
    // before it to fork at, and nothing should have been restored then.
    if mode != Mode::FilesOnly && point.fork_at.is_none() {
        bail!(
            "{node} is the first message of the conversation: there is nothing before it to go back \
             to. Rewind the files alone (--files-only), or start a new conversation"
        );
    }
    let mut done = Done {
        point: point.clone(),
        restored: None,
        forked: None,
    };
    if mode != Mode::ConversationOnly {
        let p = FilesRestoreParams {
            session_id: session.to_string(),
            since_node: point.node_id.clone(),
            dry_run: false,
        };
        done.restored = Some(serde_json::from_value(
            conn.request(method::FILES_RESTORE, p).await?,
        )?);
    }
    if mode != Mode::FilesOnly {
        let p = SessionForkParams {
            session: session.to_string(),
            at: point.fork_at.clone().unwrap_or_default(),
            ..Default::default()
        };
        done.forked = Some(serde_json::from_value(
            conn.request(method::SESSION_FORK, p).await?,
        )?);
    }
    Ok(done)
}
