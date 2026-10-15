//! `theseus rewind [SID] --to NODE [--files-only | --conversation-only]`
//! (theseus-23rk, R20): the files an agent's edits changed, and the
//! conversation, put back to before one of your messages. For scripts and
//! the surfaces that have no picker; the chat's Esc Esc is the same thing.

use anyhow::Result;
use theseus_client::render::fork::forked_lines;
use theseus_client::render::rewind::{changes_words, restored_lines};
use theseus_client::rewind::{self, Mode};
use theseus_client::Conn;
use theseus_protocol::rewind::NOT_RESTORED;

use crate::cmd::output;

/// `theseus rewind`, as `run` in main.rs hands it on whole.
pub(crate) async fn cmd(conn: &mut Conn, json: bool, cmd: crate::Cmd) -> Result<()> {
    let crate::Cmd::Rewind {
        session,
        to,
        files_only,
        conversation_only,
    } = cmd
    else {
        unreachable!("main.rs hands rewind_cmd::cmd only `theseus rewind`")
    };
    let mode = match (files_only, conversation_only) {
        (true, _) => Mode::FilesOnly,
        (_, true) => Mode::ConversationOnly,
        _ => Mode::Both,
    };
    // `@<position>` names no message: it is a point in time, for undoing a
    // restore, so only the files can go back to it.
    if to.starts_with('@') {
        let session = match session {
            Some(s) => theseus_client::resolve::session(conn, &s).await?,
            None => anyhow::bail!("`--to @POSITION` needs the session: theseus rewind SID --to @POSITION --files-only"),
        };
        if mode != Mode::FilesOnly {
            anyhow::bail!("`--to @POSITION` is a point in time, not a message: add --files-only");
        }
        let p = theseus_protocol::FilesRestoreParams {
            session_id: session,
            since_node: to,
            dry_run: false,
        };
        let v = conn
            .request(theseus_protocol::method::FILES_RESTORE, &p)
            .await?;
        return output(json, v, |r: theseus_protocol::FilesRestoreResult| {
            crate::print::lines(&mut std::io::stdout().lock(), &restored_lines(&r))?;
            Ok(())
        });
    }
    let (session, node) = match session {
        Some(s) => (theseus_client::resolve::session(conn, &s).await?, to),
        None => rewind::locate(conn, &to).await?,
    };
    let done = rewind::run(conn, &session, &node, mode).await?;
    if json {
        let v = serde_json::json!({
            "node": done.point.node_id,
            "restored": done.restored,
            "forked": done.forked,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let mut out = std::io::stdout().lock();
    let text = &done.point.text;
    crate::print::lines(
        &mut out,
        &[theseus_client::render::Line::new(
            theseus_client::render::Tag::Plain,
            format!(
                "rewound to before \"{text}\" ({})",
                changes_words(&done.point)
            ),
        )],
    )?;
    if let Some(r) = &done.restored {
        crate::print::lines(&mut out, &restored_lines(r))?;
    }
    if let Some(f) = &done.forked {
        crate::print::lines(&mut out, &forked_lines(f))?;
        crate::print::lines(
            &mut out,
            &[theseus_client::render::Line::new(
                theseus_client::render::Tag::Plain,
                format!("go on in it: theseus chat {}", f.session_id),
            )],
        )?;
    }
    crate::print::lines(
        &mut out,
        &[theseus_client::render::Line::new(
            theseus_client::render::Tag::Dim,
            format!("not restored: {NOT_RESTORED}"),
        )],
    )?;
    Ok(())
}
