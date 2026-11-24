//! `view.show` (issue 44; `crate::view::record`): what a turn was shown and why, from its view records. A
//! read: since 2026-11-22 (anything goes) every reader is answered, the record and its text alike. Every piece's source is checked again today; a sealed, seal-pending or
//! unreadable one is not listed, and inspecting lifts no seal.

use std::sync::Arc;

use serde_json::Value;
use theseus_memory::gate::{PointOfView, Present};
use theseus_memory::recall::Place;
use theseus_protocol::method;
use theseus_protocol::view::{
    ViewCount, ViewPreviewParams, ViewPreviewResult, ViewShowParams, ViewShowResult, ViewTally,
};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::view::record::show::{name, PieceText, Reader};
use crate::view::record::source::GateSource;
use crate::view::record::{self, Count, StoreText, Tally, ViewRecord};
use crate::view::Anchor;

/// The methods `rpc_views` answers.
pub(super) const OWN: &[&str] = &[method::VIEW_SHOW, method::VIEW_PREVIEW];

/// What a reader is told when no text was asked for.
const NO_TEXT: &str = "a piece's text is shown only when asked for (`--text`)";

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
            method::VIEW_PREVIEW => self.rpc_preview(params).await,
            _ => self.rpc_views(params).await,
        }
    }

    async fn rpc_views(self: Arc<Self>, params: Value) -> Result<Value, RpcFailure> {
        // A read (anything goes, 2026-11-22): every reader is answered, the text included.
        let p: ViewShowParams = parse(params)?;
        let core = self.clone();
        let r = super::import::blocking(move || core.view_show(&p)).await?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// `view.preview`: any place's next turn, for any reader.
    async fn rpc_preview(self: Arc<Self>, params: Value) -> Result<Value, RpcFailure> {
        // A read (anything goes, 2026-11-22): every reader is answered, the text included.
        let p: ViewPreviewParams = parse(params)?;
        let core = self.clone();
        let r = super::import::blocking(move || core.view_preview(&p)).await?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// The preview of `p.place`'s next turn: its newest session there.
    fn view_preview(&self, p: &ViewPreviewParams) -> anyhow::Result<ViewPreviewResult> {
        let Some(rec) = self.runner.place_sessions(&p.place)?.into_iter().next() else {
            anyhow::bail!(
                "no session's turns are at {}: there is nothing to preview",
                p.place
            );
        };
        let (live, _) = self.live_profile();
        let spec = self.runner.preview_spec(&rec, &live)?;
        let text = StoreText(&self.store);
        let preview = crate::turn::Preview {
            place: &p.place,
            session: &rec.session_id,
            message: p.message.as_deref(),
            text: p.text.then_some(&text as &dyn PieceText),
        };
        self.runner.view_preview(&preview, &spec)
    }

    /// A turn's views.
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
            views: self.owners_all(&views, shown).0,
            tallies,
            withheld: (!p.text).then(|| NO_TEXT.into()),
        })
    }

    /// Every view of a call as the owner inspects it, and the units spent proving its folds and task cards:
    /// one source for each anchor, made here and dropped when this returns, so what several views hold is
    /// proven once a call and nothing is kept for the next.
    pub(crate) fn owners_all(
        &self,
        views: &[ViewRecord],
        text: Option<&dyn PieceText>,
    ) -> (Vec<theseus_protocol::view::ViewShown>, u64) {
        let mut sources: std::collections::BTreeMap<String, GateSource<'_>> = Default::default();
        for v in views {
            (sources.entry(anchor_of(v))).or_insert_with_key(|anchor| self.owners_source(anchor));
        }
        let shown = (views.iter())
            .map(|v| self.owners(&sources[&anchor_of(v)], v, text))
            .collect();
        (shown, sources.values().map(GateSource::reads).sum())
    }

    /// The owner alone, at a view's own anchor, with no asker, so a sealed or seal-pending source stays out.
    fn owners_source(&self, anchor: &str) -> GateSource<'_> {
        let pov = PointOfView {
            anchor: anchor.into(),
            place: Place::Private,
            present: Present::OwnerAlone,
        };
        GateSource::new(&self.runner, pov, None)
    }

    /// One view as the owner inspects it, through `source`.
    fn owners(
        &self,
        source: &GateSource<'_>,
        v: &ViewRecord,
        text: Option<&dyn PieceText>,
    ) -> theseus_protocol::view::ViewShown {
        let mut shown = Reader {
            source,
            text,
            restricted: false,
        }
        .show(v);
        // What Jev says about each piece's items (issue 44): data for a person, nothing withheld on it.
        let o = self.runner.ontology.snapshot(&self.store).ok();
        for p in shown.slots.iter_mut().flat_map(|s| s.pieces.iter_mut()) {
            let at = (p.kind.as_str(), p.first.as_str(), p.last.as_str());
            p.labels = crate::item_labels::piece_lines(&self.store, o.as_deref(), at);
        }
        shown
    }
}

/// A view's anchor, as the gate names it.
fn anchor_of(v: &ViewRecord) -> String {
    match &v.pov.anchor {
        Anchor::Place(p) => p.clone(),
        Anchor::Task(t) => format!("task:{t}"),
    }
}
