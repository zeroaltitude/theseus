//! A session's directory at the protocol (theseus-aab7): `session.open`
//! and `turn.submit` take the client's, and a `turn.submit` that names a
//! session with another one moves it there. The tools' side is
//! `toolrun::session_dir`.

use theseus_protocol::error_code;

use super::server::RpcFailure;
use crate::session::SessionRecord;
use crate::toolrun::ToolRuntime;

/// The client's directory, checked: absolute, else invalid params.
pub(super) fn check(dir: Option<&str>) -> Result<Option<String>, RpcFailure> {
    crate::toolrun::session_dir::checked(dir)
        .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))
}

/// Put the turn's copy of `session` in `dir`, which its session write then
/// keeps (`SessionRecord::take_turns_fields`); none keeps its own. The
/// workspace roots, when this starts the session in a directory outside
/// them or moves it to one: the CLI says so once.
pub(super) fn moved(
    tools: &ToolRuntime,
    session: &mut SessionRecord,
    dir: Option<String>,
) -> Option<Vec<String>> {
    let dir = dir?;
    let starts = session.turns == 0 || session.dir.as_deref() != Some(dir.as_str());
    let outside = starts.then(|| tools.outside_roots(&dir)).flatten();
    session.dir = Some(dir);
    outside
}
