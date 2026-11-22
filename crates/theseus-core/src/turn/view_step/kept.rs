//! What a replay needs of a view turn's chain, kept as it begins (`compiler::view::kept`), and whether what
//! a view laid is gone now, asked before a call of the turn (`view_gone`) and before a replay of it sends
//! anything (`own_bench::rebuild`).

use super::super::compile_step::Keeping;
use super::{Compiled, Turn};
use crate::compiler::view::kept::{kept, Kept};
use crate::compiler::view::Frozen;
use crate::store::Store;

/// The view record of `compiled`, a new compilation the turn persists: what its view showed, the room the
/// turn made, and what a replay needs. Every compilation an answer can name keeps its own (a routed turn's
/// second compile, a new chain within the turn), so a replay finds the one its answer names; routing's
/// first compile keeps one only when the call uses it (`route_step::keep_first`), so a discarded probe
/// leaves none.
pub(in crate::turn) fn record_kept(t: &mut Turn<'_>, compiled: &Compiled) {
    let (turn, store) = (t.tc.turn_id, t.tc.store);
    let Some(f) = (t.view.as_ref()).filter(|f| f.turn_id == turn) else {
        return;
    };
    let Some(view) = f.record.as_ref() else {
        return;
    };
    let room = f.room.made(compiled.cleared.map_or(0, |p| p.results));
    let replay = chain_kept(store, &mut t.keeping, f);
    let id = &compiled.compilation.id;
    if let Err(e) = crate::view::record::keep_made(store, turn, id, view, (room, replay)) {
        tracing::warn!(error = %format!("{e:#}"), "the view's record was not kept");
    }
}

/// The chain `f` begins: its texts queued with the turn's system blocks (written on the blocking pool in
/// the frame-free batch `compile_step::keep_blocks` starts, never in a frame of their own), each once, and
/// the digests for its view record. None when a part does not serialize: the turn goes on, and a replay of
/// it is refused in words. None too, with nothing queued, when its recall laid anything of an imported
/// session ([`recalled_import`]): the replay cache keeps no copy of an import's words, which its erasure
/// must leave nowhere; that turn's replay is refused as one that kept nothing.
fn chain_kept(store: &Store, keeping: &mut Keeping, f: &Frozen) -> Option<Kept> {
    if recalled_import(store, f) {
        return None;
    }
    let (k, texts) = kept(f)?;
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    keeping.add(crate::compiler::cache::fresh(store.blobs(), &texts));
    Some(k)
}

/// Whether a node `f`'s recall laid is of an imported session: by its id, or by its session's record; a
/// session whose record cannot be read counts as one.
fn recalled_import(store: &Store, f: &Frozen) -> bool {
    (f.recalled.iter()).any(|r| {
        crate::import::is_imported(&r.session_id)
            || !matches!(
                store.get_session::<crate::session::SessionRecord>(&r.session_id),
                Ok(Some(rec)) if rec.imported.is_none()
            )
    })
}
