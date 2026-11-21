//! `view.show` (issue 44; `crate::view::record`): what a turn was shown and why, from its view records. A
//! read, from the terminal or the web UI alone, as the other ledgers of a turn are. Ids, kinds, forms,
//! tokens, reasons and counts; a piece's text only when asked for and only for the owner's own shell: the
//! CLI on this machine, with no Theseus job's marker and no agent's process behind it.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::view::{ViewShowParams, ViewShowResult};
use theseus_protocol::{error_code, method};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::approval::Surface;
use crate::view::record::show::{PieceText, Reader};
use crate::view::record::{self, StoreText};

/// The methods `rpc_views` answers.
pub(super) const OWN: &[&str] = &[method::VIEW_SHOW];

/// What a reader is told when no text was asked for.
const NO_TEXT: &str =
    "a piece's text is shown only when asked for (`--text`), at the owner's own terminal";

/// Why `--text` is refused on `conn`, or none for the owner's own shell.
fn text_refused(conn: &Conn<'_>, from_job: Option<&str>) -> Option<String> {
    let who = conn.answerer(None, None);
    refused_for(who.surface, from_job.is_some(), who.agent.as_deref())
}

/// [`text_refused`], on the facts: the CLI with no job's marker and no agent's process behind it.
fn refused_for(surface: Surface, job: bool, agent: Option<&str>) -> Option<String> {
    match (surface, job, agent) {
        (Surface::Cli, false, None) => None,
        (Surface::Cli, true, _) => Some("this shell is a Theseus job's".into()),
        (Surface::Cli, false, Some(agent)) => {
            Some(format!("it came from an agent's process ({agent})"))
        }
        (other, _, _) => Some(format!("it came through {}", other.name())),
    }
}

impl Core {
    /// `turn.inspect` and `view.show`: what one turn carried, and what it was shown.
    pub(super) async fn rpc_turn_reads(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::TURN_INSPECT => self.rpc_inspect(params, conn).await,
            _ => self.rpc_views(params, conn).await,
        }
    }

    async fn rpc_views(
        self: Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if !conn.surface.reads_private() {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                "what a turn was shown is read from your own terminal or the web UI",
            ));
        }
        let p: ViewShowParams = parse(params)?;
        if p.text {
            if let Some(why) = text_refused(&conn, p.from_job.as_deref()) {
                return Err(RpcFailure::new(
                    error_code::REFUSED,
                    format!("{why}, and a piece's text is shown only at the owner's own terminal: run it there without `--text` for ids and counts"),
                ));
            }
        }
        let core = self.clone();
        let r = super::import::blocking(move || core.view_show(&p)).await?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// A turn's views, for the owner: every piece, its text when `p.text` (checked by the caller).
    fn view_show(&self, p: &ViewShowParams) -> anyhow::Result<ViewShowResult> {
        let views = record::of_turn(&self.store, &p.turn_id)?;
        let text = StoreText(&self.store);
        let shown: Option<&dyn PieceText> = p.text.then_some(&text);
        let reader = Reader {
            place: None,
            text: shown,
        };
        Ok(ViewShowResult {
            turn_id: p.turn_id.clone(),
            views: views.iter().map(|v| reader.show(v)).collect(),
            withheld: (!p.text).then(|| NO_TEXT.into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_for_the_owners_own_shell_alone() {
        assert_eq!(refused_for(Surface::Cli, false, None), None);
        for (surface, job, agent) in [
            (Surface::Cli, true, None),
            (Surface::Cli, false, Some("claude")),
            (Surface::Web, false, None),
            (Surface::Discord, false, None),
            (Surface::Mcp, false, None),
            (Surface::Unnamed, false, None),
        ] {
            assert!(
                refused_for(surface, job, agent).is_some(),
                "{surface:?} {job} {agent:?}"
            );
        }
    }
}
