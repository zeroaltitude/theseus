//! The reading sheet (`exchange.gists`, issue 44, step 4; design §10): a
//! sample of `did` lines and of first-level fold nodes, each beside its
//! writer's material, so the owner can say whether each is true. The
//! material is rebuilt now, at the writer's cuts and with the values the
//! writer substitutes substituted (`writer::prepared`), from what the store
//! holds now: no copy of what a writer was given is kept. An item whose
//! material may differ from what its writer was given (an older writer,
//! sources no longer in the store) says so in its notes. A line is shown as
//! the view shows it ([`did::shown_line`]), and a node as the view's fold
//! block. The draw is by a seed, so the same seed over the same records draws
//! the same items. It only reads: nothing is stored and no model is called.

use std::collections::{HashMap, HashSet};

use theseus_protocol::exchange::{ExchangeGistsParams, ExchangeGistsResult, GistLine, GistNode};
use theseus_store::{kinds, Store as _};

use super::did::{self, DidLine, Said, DID_PREFIX};
use super::fold::{self, FoldNode, FOLD_PREFIX};
use super::writer::{prepared, WRITER_VERSION};
use crate::scrub::Scrubber;
use crate::store::Store;

/// The most lines, and nodes, one sheet holds.
pub const MOST_LINES: u32 = 500;
pub const MOST_NODES: u32 = 100;

/// Why `p` asks for more than one sheet holds; none when it does not.
pub fn too_many(p: &ExchangeGistsParams) -> Option<String> {
    (p.lines > MOST_LINES || p.nodes > MOST_NODES).then(|| {
        format!(
            "a reading sheet holds at most {MOST_LINES} summaries and {MOST_NODES} first-level fold \
             nodes; {} and {} were asked",
            p.lines, p.nodes
        )
    })
}

/// The sheet for `p`: its seed, or one chosen now and returned. `scrubber`
/// is the summaries' writer's.
pub fn sheet(
    store: &Store,
    scrubber: &Scrubber,
    p: &ExchangeGistsParams,
) -> anyhow::Result<ExchangeGistsResult> {
    if let Some(why) = too_many(p) {
        anyhow::bail!(why);
    }
    let seed = p
        .seed
        .unwrap_or_else(|| theseus_protocol::now_unix_ms() % 1_000_000);
    let place = p.place.as_deref();
    let mut lines: Vec<DidLine> = Vec::new();
    for r in store.inner().latest_with_prefix(kinds::META, DID_PREFIX)? {
        let d: DidLine = r.decode()?;
        if place.is_none_or(|p| d.marks.place.as_deref() == Some(p)) {
            lines.push(d);
        }
    }
    // The prefix is not the boundary: a node is of the place, and of the
    // first level, when it says so itself.
    let prefix = match place {
        Some(p) => format!("{FOLD_PREFIX}{p}{}1{}", super::SEP, super::SEP),
        None => FOLD_PREFIX.to_string(),
    };
    let mut nodes: Vec<FoldNode> = Vec::new();
    for r in store.inner().latest_with_prefix(kinds::META, &prefix)? {
        let n: FoldNode = r.decode()?;
        if n.level == 1 && place.is_none_or(|p| n.place == p) {
            nodes.push(n);
        }
    }
    let (lines_total, nodes_total) = (lines.len() as u64, nodes.len() as u64);
    let mut seen = Outside::new();
    let mut lines = draw(lines, p.lines as usize, seed)
        .iter()
        .map(|d| line(store, scrubber, &mut seen, d))
        .collect::<anyhow::Result<Vec<_>>>()?;
    lines.sort_by(|a, b| (a.at_ms, &a.turn_id).cmp(&(b.at_ms, &b.turn_id)));
    // A second stream of the seed, so a change in how many lines are asked
    // never changes which nodes are drawn.
    let mut nodes = draw(nodes, p.nodes as usize, seed ^ NODES)
        .iter()
        .map(|n| node(store, scrubber, &mut seen, n))
        .collect::<anyhow::Result<Vec<_>>>()?;
    nodes.sort_by(|a, b| (a.first_ms, &a.id).cmp(&(b.first_ms, &b.id)));
    Ok(ExchangeGistsResult {
        place: p.place.clone(),
        seed,
        lines_total,
        nodes_total,
        lines,
        nodes,
    })
}

const NODES: u64 = 0x6e6f_6465_7321;

/// `k` of `all` (in the store's key order), drawn by `seed`: a partial
/// Fisher-Yates shuffle on splitmix64, written here so a seed draws the
/// same items on every build.
fn draw<T>(mut all: Vec<T>, k: usize, seed: u64) -> Vec<T> {
    let mut state = seed;
    let k = k.min(all.len());
    for i in 0..k {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let j = i + (z % (all.len() - i) as u64) as usize;
        all.swap(i, j);
    }
    all.truncate(k);
    all
}

/// An older writer's item: its material is today's, which it may not have been given.
fn older(writer: u32) -> String {
    format!(
        "Written by an older writer (version {writer}; today's is {WRITER_VERSION}): the material \
         below is rebuilt today and may not match what that writer was given. Skip it, or judge it \
         knowing that; do not count it as an ordinary verdict."
    )
}

/// What of an exchange's words is no longer in the store.
fn lost(r: &did::Read) -> (bool, bool) {
    (
        r.ex.ask.is_some() && r.ask.is_none(),
        r.ex.reply.is_some() && r.reply.is_none(),
    )
}

/// Each session a sheet has read, and its turns that read outside text: a
/// session is scanned once a sheet, not once an item.
type Outside = HashMap<String, HashSet<String>>;

/// [`did::read`], `turn`'s session scanned once a sheet.
fn read(store: &Store, seen: &mut Outside, turn: &str) -> anyhow::Result<Option<did::Read>> {
    did::read_with(store, turn, |ex| {
        if !seen.contains_key(&ex.session_id) {
            let turns = store
                .session_nodes(&ex.session_id)?
                .into_iter()
                .filter(|(_, n)| did::outside(n))
                .filter_map(|(_, n)| n.turn_id)
                .collect();
            seen.insert(ex.session_id.clone(), turns);
        }
        Ok(seen[&ex.session_id].contains(turn))
    })
}

/// A line beside its writer's material for its exchange.
fn line(
    store: &Store,
    scrubber: &Scrubber,
    seen: &mut Outside,
    d: &DidLine,
) -> anyhow::Result<GistLine> {
    let r = read(store, seen, &d.turn_id)?;
    let mut notes = Vec::new();
    if d.writer < WRITER_VERSION {
        notes.push(older(d.writer));
    }
    match r.as_ref().map(lost) {
        None => notes.push(
            "Its exchange is no longer in the store: its writer's material cannot be shown.".into(),
        ),
        Some((ask, reply)) => {
            if ask {
                notes.push(
                    "Its message is no longer in the store: the material below lacks it.".into(),
                );
            }
            if reply {
                notes.push(
                    "Its final reply is no longer in the store: the material below lacks it."
                        .into(),
                );
            }
        }
    }
    Ok(GistLine {
        turn_id: d.turn_id.clone(),
        place: d.marks.place.clone().unwrap_or_default(),
        at_ms: r.as_ref().map_or(d.at_ms, |r| r.ex.start_ms),
        model: d.model.clone(),
        writer: d.writer,
        read: r.as_ref().map(|r| prepared(scrubber, &did::material(r))),
        line: did::shown_line(store, d),
        notes,
    })
}

/// A node beside its writer's material: each exchange still readable,
/// with the `did` line it had when the node was written (a line written
/// after it was not read), and the node as the view's fold block shows it.
fn node(
    store: &Store,
    scrubber: &Scrubber,
    seen: &mut Outside,
    n: &FoldNode,
) -> anyhow::Result<GistNode> {
    let mut reads = Vec::new();
    for t in &n.exchanges {
        let Some(r) = read(store, seen, t)? else {
            continue;
        };
        let said = match did::said(store, t)? {
            Said::Line(d) if d.at_ms > n.at_ms => Said::NotSummarised { handle: t.clone() },
            said => said,
        };
        reads.push((r, said));
    }
    let mut notes = Vec::new();
    if n.writer < WRITER_VERSION {
        notes.push(older(n.writer));
    }
    let gone = n.exchanges.len() - reads.len();
    if gone > 0 {
        notes.push(format!(
            "{gone} of its {} exchanges {} no longer in the store: the material below lacks \
             them.",
            n.exchanges.len(),
            if gone == 1 { "is" } else { "are" }
        ));
    }
    let partial = reads
        .iter()
        .filter(|(r, _)| lost(r) != (false, false))
        .count();
    if partial > 0 {
        notes.push(format!(
            "{partial} of the exchanges shown {} lost its message or reply: the material below \
             lacks those words.",
            if partial == 1 { "has" } else { "have" }
        ));
    }
    let renderer = crate::view::wire_text::TextRenderer {
        store,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
    };
    let pov = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place(n.place.clone()),
        person: Some(crate::exchange::OWNER.into()),
        present: crate::view::Present::Private,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let shown = renderer
        .ladder(&pov, &[super::view::shown(n)])?
        .into_iter()
        .map(|b| format!("{}\n{}", b.heading, b.text))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(GistNode {
        id: n.id.clone(),
        place: n.place.clone(),
        first_ms: n.first_ms,
        last_ms: n.last_ms,
        exchanges: n.exchanges.len() as u32,
        readable: reads.len() as u32,
        model: n.model.clone(),
        writer: n.writer,
        read: prepared(scrubber, &fold::material(&reads)),
        shown,
        notes,
    })
}
