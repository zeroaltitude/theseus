//! The undo of a prompt file's change (issue 23): `config.undo` on a change
//! that was a prompt file puts back the text the file had before it, whole and
//! exactly, by the same card as the change. If the
//! file changed again since, the card says so, and shows those later edits as
//! the lines this removes.

use serde_json::Value;

use super::history::FileEntry;
use super::put::{put_file, Ask};
use super::{read_text, resolve};
use crate::config_change::history::Entry;
use crate::config_change::undo::{Since, Undoes};
use crate::config_copy::sha256;
use crate::rpc::Core;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// Plan and put the undo of change `e`, a prompt file's.
pub fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    core: &Core,
    correlation_id: &str,
    e: &Entry,
    f: &FileEntry,
    reason: &str,
) -> Result<(String, Value), String> {
    let n = e.number;
    let named = resolve(core, &f.path).map_err(|why| {
        format!("the file of change #{n} cannot be changed now, so it cannot be undone: {why}")
    })?;
    let before = f.before.as_deref().ok_or_else(|| {
        format!(
            "the text from before change #{n} is no longer kept (only the last {} changes' are), \
             so it cannot be undone",
            crate::config_change::write::KEEP
        )
    })?;
    let now = read_text(&named.path).map_err(|why| format!("{}: {why}", f.path))?;
    if now == before {
        return Err(format!(
            "there is nothing to undo: the file already says what it said before change #{n}"
        ));
    }
    let changed_since = sha256(&now) != f.after_sha;
    let when = theseus_protocol::human_time::ago(theseus_protocol::now_unix_ms(), e.at_ms);
    let mut preface =
        format!("This puts back the file's text as it was before change #{n} ({when}), whole.\n");
    if changed_since {
        preface.push_str(
            "The file has changed again since that change, so this overwrites those later edits \
             (they are the lines removed below).\n",
        );
    }
    let undoes = Undoes {
        number: n,
        question: e.question.clone(),
        at_ms: e.at_ms,
        place: e.place.clone(),
        since: match changed_since {
            true => vec![Since {
                path: vec![f.path.clone()],
                made: None,
                now: None,
                back: None,
            }],
            false => Vec::new(),
        },
        already: Vec::new(),
    };
    let ask = Ask {
        named: &named,
        before: &now,
        after: before,
        reason,
        undoes: Some((&undoes, preface)),
    };
    put_file(rt, tc, core, correlation_id, ask)
}
