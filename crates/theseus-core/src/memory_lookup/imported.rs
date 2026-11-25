//! The history tool's search of imported conversations (`history_tool::imports`): this engine's search of
//! words, kept to imported hits, a page at a time.

use std::sync::Arc;

use super::{by_words, Args, Item, CANDIDATES_MAX};
use crate::recall::when::When;
use crate::Core;

/// One imported message a search found: its node, its session, its own time, its book and its words.
pub(crate) struct ImportHit {
    pub node_id: String,
    pub session_id: String,
    pub at_ms: u64,
    pub book: Option<String>,
    pub text: String,
}

/// The history tool's search of imported conversations (D6 step 6, round 2): this engine's [`by_words`], over
/// every session when unqualified (as the unrestricted search ranks), or narrowed to a `book`'s sessions or to
/// the episodes in `when` (round 3), and kept to imported hits; the hits from `offset` (clamped to the bound), at
/// most `limit`, whether more lie past them, and why the search may be incomplete (stale hits that filled what
/// was read, a hit that could not be read, a narrowing, or the bound of [`CANDIDATES_MAX`] filled).
pub(crate) async fn imported(
    core: &Arc<Core>,
    words: &str,
    when: Option<&When>,
    book: Option<&str>,
    (offset, limit): (u64, u64),
) -> Result<(Vec<ImportHit>, bool, Option<String>), String> {
    let a = Args {
        imports: true,
        book: book.map(str::to_string),
        ..Args::default()
    };
    // The most the index is asked, whatever the page (as the MCP search asks it): a total that moved with the
    // page would break a cursor. A forged offset past it is the bound, never an overflow.
    let bound = CANDIDATES_MAX as u64;
    let offset = offset.min(bound);
    let (_, items, _, incomplete) =
        by_words(core, None, words.to_string(), &a, when, bound).await?;
    let fetched = items.len();
    let items: Vec<Item> = (items.into_iter())
        .filter(|i| crate::import::is_imported(&i.session_id))
        .collect();
    let more = items.len() as u64 > offset.saturating_add(limit);
    // The bound filled (other conversations' hits among them): what lay past it was never ranked.
    let incomplete = incomplete.or_else(|| {
        (!more && fetched >= CANDIDATES_MAX)
            .then(|| format!("a search reads at most {CANDIDATES_MAX} hits"))
    });
    let hits = (items.into_iter().skip(offset as usize).take(limit as usize))
        .map(|i| ImportHit {
            node_id: i.node_id,
            session_id: i.session_id,
            at_ms: i.at_ms,
            book: i.book,
            text: i.text,
        })
        .collect();
    Ok((hits, more, incomplete))
}
