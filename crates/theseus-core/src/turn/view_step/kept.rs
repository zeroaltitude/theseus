//! What a replay needs of a view turn's chain, kept as it begins (`compiler::view::kept`), and the gate's
//! word on what a view laid, asked again now: before a call of the turn (`view_revoked`) and before a replay
//! of it sends anything (`own_bench::rebuild`).

use super::super::compile_step::Keeping;
use super::{Compiled, Turn, TurnRunner};
use crate::compiler::view::kept::{kept, Kept};
use crate::compiler::view::Frozen;
use crate::store::Store;
use theseus_memory::gate::Seal;

/// The view record of `compiled`, a new compilation the turn persists: what its view showed, the room the
/// turn made, and what a replay needs. Every compilation an answer can name keeps its own (a routed turn's
/// second compile, a new chain within the turn), so a replay finds the one its answer names; routing's
/// first compile keeps one only when the call uses it (`route_step::keep_first`), so a discarded probe
/// leaves none.
pub(in crate::turn) fn record_kept(t: &mut Turn<'_>, compiled: &Compiled) {
    let (turn, store) = (t.tc.turn_id, t.tc.store);
    let Some(f) = (t.view.as_ref()).filter(|f| f.turn_id == turn && !f.declined) else {
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

/// Whether the gate of now, for a reader in `asker`, holds anything a view laid: a session sealed or not
/// proven clear; an exchange that is gone or holds a message moved into a thread not proven clear; a node
/// its recall laid (`recalled`) that is gone or erased (`Body::Erased`), or sealed by its session or by
/// where it was moved.
pub(crate) fn held_now(
    runner: &TurnRunner,
    asker: &str,
    (sessions, exchanges, recalled): (&[String], &[String], &[String]),
) -> bool {
    let sealed = |s: &str| crate::view_gate::seal_of(runner, Some(asker), s) != Seal::Clear;
    if sessions.iter().any(|s| sealed(s)) {
        return true;
    }
    let moved = crate::view_gate::Moved::read(runner, Some(asker));
    let store = &runner.store;
    let gone_or_moved = (exchanges.iter()).any(|id| match crate::exchange::exchange(store, id) {
        // Only a readable exchange whose nodes are all clear proves admission: a missing one is held.
        Ok(Some(e)) => moved.holds(&e),
        Ok(None) | Err(_) => true,
    });
    gone_or_moved || recall_held(runner, asker, recalled)
}

/// Whether a node of `recalled` is gone, or held from a reader in `asker` (`view_gate::node_seal`).
fn recall_held(runner: &TurnRunner, asker: &str, recalled: &[String]) -> bool {
    if recalled.is_empty() {
        return false;
    }
    let Ok(board) = runner.threads.all(&runner.store) else {
        return true;
    };
    recalled.iter().any(|id| match runner.store.get_node(id) {
        // Erased since (an import's erasure, or another path): its words must not be sent again.
        Ok(Some((_, n))) if matches!(n.body, crate::node::Body::Erased { .. }) => true,
        Ok(Some((_, n))) => {
            let seal =
                crate::view_gate::node_seal(&runner.store, &board, Some(asker), &n.session_id, id);
            seal != Seal::Clear
        }
        Ok(None) | Err(_) => true,
    })
}
