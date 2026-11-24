//! `look.work`'s conversations that are running now: what `theseus status` lists besides the tasks and
//! what waits. Read from the board the command reads (`Push::snapshot`: every execution that needs you
//! or works), no subscription made: where each runs, what it is doing, since when, and its turns and
//! spend. Names and counts; a conversation's title is not given. A conversation that needs the owner is
//! under `needs_you`, and a task is under `tasks`.

use std::fmt::Write;
use std::sync::Arc;

use theseus_protocol::{Level, SessionKind};

use super::reader::Reader;
use super::RUNNING_MAX;
use crate::Core;

/// The conversations that work now, newest change last.
pub(super) async fn running(core: &Arc<Core>, r: &Reader<'_>, here: bool) -> String {
    if core.push.ensure(core).await.is_err() {
        return "Running: it could not be read just now.\n".into();
    }
    let (_, views, _) = core.push.snapshot(0);
    let mut mine: Vec<_> = views
        .into_iter()
        .filter(|v| v.kind == SessionKind::Conversation && v.attention.level == Level::Working)
        .filter(|v| !here || v.session_id == r.here)
        .collect();
    let system = (!here).then(|| system_words(core)).flatten();
    let own = system
        .map(|w| format!("The daemon's own work: {w}\n"))
        .unwrap_or_default();
    if mine.is_empty() {
        let none = match here {
            true => "Running: this conversation is not working just now.\n",
            false => "Running: no conversation is working just now.\n",
        };
        return format!("{none}{own}");
    }
    mine.sort_by_key(|v| {
        (
            v.session_id != r.here,
            std::cmp::Reverse(v.attention.since_ms),
        )
    });
    let mut out = format!("Running: conversations working now ({}):\n", mine.len());
    for v in mine.iter().take(RUNNING_MAX) {
        let at = match v.session_id == r.here {
            true => "this conversation".to_string(),
            false => r.place(&v.session_id),
        };
        let _ = writeln!(
            out,
            "- {} · {at} · {} since {} · {} turn{} · ${:.2} of ${:.2}",
            crate::task::short(&v.session_id),
            super::clip(&v.attention.label, 80, r.cuts),
            crate::wake::local(v.attention.since_ms).full(),
            v.turns,
            if v.turns == 1 { "" } else { "s" },
            v.spent_usd,
            v.limit_usd
        );
    }
    if mine.len() > RUNNING_MAX {
        let _ = writeln!(
            out,
            "… and {} more not listed here",
            mine.len() - RUNNING_MAX
        );
    }
    out.push_str(&own);
    out
}

/// The daemon's own jobs running (`⚙ importing 41/120`), from the work board's `system` root: what the
/// first line of `theseus status` says of them. None when none runs.
fn system_words(core: &Core) -> Option<String> {
    let scope = crate::push::WorkScope {
        roots: Some(std::iter::once(theseus_protocol::work_system::ROOT.to_string()).collect()),
        guarded: false,
    };
    let (_, views, _) = core.work_snapshot_marked(0, &scope);
    theseus_protocol::work_system::status_words(&scope.cut(&views, &core.tools.scrubber))
}
