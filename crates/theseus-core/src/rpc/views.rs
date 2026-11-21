//! `view.show` (issue 44; `crate::view::record`): what a turn was shown and why, from its view records. The
//! owner's inspection, whole or not at all: it answers the owner in person alone, by the rule the other
//! owner-only methods use (`places::owner_in_person_of`): the CLI or the web UI with no agent's process
//! behind the connection (a Theseus job's is one), and no job's marker. Anyone else is refused in words, the
//! record and its text alike. Every piece's source is checked again today; a sealed, seal-pending or
//! unreadable one is not listed, and inspecting lifts no seal.

use std::sync::Arc;

use serde_json::Value;
use theseus_memory::gate::{PointOfView, Present};
use theseus_memory::recall::Place;
use theseus_protocol::view::{ViewCount, ViewShowParams, ViewShowResult, ViewTally};
use theseus_protocol::{error_code, method};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::view::record::show::{name, PieceText, Reader};
use crate::view::record::source::GateSource;
use crate::view::record::{self, Count, StoreText, Tally, ViewRecord};
use crate::view::Anchor;

/// The methods `rpc_views` answers.
pub(super) const OWN: &[&str] = &[method::VIEW_SHOW];

/// What a reader is told when no text was asked for.
const NO_TEXT: &str = "a piece's text is shown only when asked for (`--text`)";

/// Why `view.show` is refused on `conn`, or none for the owner in person.
fn refused(core: &Core, conn: &Conn<'_>, from_job: Option<&str>) -> Option<String> {
    if !conn.surface.reads_private() {
        return Some(format!(
            "it came through {}, and what a turn was shown is the owner's to inspect, from his own \
             terminal or the web UI",
            conn.surface.name()
        ));
    }
    if from_job.is_some() {
        return Some(
            "this shell is a Theseus job's, and what a turn was shown is the owner's alone to inspect"
                .into(),
        );
    }
    let who = conn.answerer(None, None);
    crate::places::owner_in_person_of(
        &who,
        &core.runner.place_rule,
        &core.cfg,
        "what a turn was shown is the owner's alone to inspect",
        "it is inspected",
    )
    .err()
}

/// A stored tally, by name.
fn tally_shown(t: &Tally) -> ViewTally {
    fn by<K: serde::Serialize>(
        m: &std::collections::BTreeMap<K, Count>,
    ) -> std::collections::BTreeMap<String, ViewCount> {
        m.iter()
            .map(|(k, c)| {
                let c = ViewCount {
                    pieces: c.pieces,
                    tokens: c.tokens,
                };
                (name(k), c)
            })
            .collect()
    }
    ViewTally {
        compilation: t.compilation.clone(),
        present: name(&t.present),
        slots: by(&t.slots),
        reasons: by(&t.reasons),
        dropped: t
            .dropped
            .iter()
            .map(|(k, c)| {
                let c = ViewCount {
                    pieces: c.pieces,
                    tokens: c.tokens,
                };
                (k.clone(), c)
            })
            .collect(),
        dropped_reasons: by(&t.dropped_reasons),
        tokens: t.tokens,
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
        let p: ViewShowParams = parse(params)?;
        if let Some(why) = refused(&self, &conn, p.from_job.as_deref()) {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!("not shown: {why}"),
            ));
        }
        let core = self.clone();
        let r = super::import::blocking(move || core.view_show(&p)).await?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// A turn's views, for the owner in person (checked by the caller).
    fn view_show(&self, p: &ViewShowParams) -> anyhow::Result<ViewShowResult> {
        let views = record::of_turn(&self.store, &p.turn_id)?;
        let text = StoreText(&self.store);
        let shown: Option<&dyn PieceText> = p.text.then_some(&text);
        let tallies = match p.tally {
            true => views
                .iter()
                .filter_map(|v| record::tally(&self.store, &v.compilation).ok().flatten())
                .map(|t| tally_shown(&t))
                .collect(),
            false => Vec::new(),
        };
        Ok(ViewShowResult {
            turn_id: p.turn_id.clone(),
            views: views.iter().map(|v| self.owners(v, shown)).collect(),
            tallies,
            withheld: (!p.text).then(|| NO_TEXT.into()),
        })
    }

    /// One view as the owner inspects it: the owner alone, at the view's own anchor, with no asker, so a
    /// sealed or seal-pending source stays out.
    fn owners(
        &self,
        v: &ViewRecord,
        text: Option<&dyn PieceText>,
    ) -> theseus_protocol::view::ViewShown {
        let anchor = match &v.pov.anchor {
            Anchor::Place(p) => p.clone(),
            Anchor::Task(t) => format!("task:{t}"),
        };
        let source = GateSource {
            runner: &self.runner,
            pov: PointOfView {
                anchor,
                place: Place::Private,
                present: Present::OwnerAlone,
            },
            asker: None,
        };
        Reader {
            source: &source,
            text,
            restricted: false,
        }
        .show(v)
    }
}
