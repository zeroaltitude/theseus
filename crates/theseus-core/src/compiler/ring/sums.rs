//! The ring's guess (theseus-0l3g): where its cut before a user message
//! falls, from the sums of its segments, so a long session's ring renders a
//! few candidates, not one per cut.
//!
//! Before it, the ring rendered every candidate from the earliest cut on,
//! each a whole request, until one fell under its target: a session of
//! 10,000 nodes rendered and decoded hundreds of candidates, the time
//! growing with the square of its length. A candidate's estimate is the
//! request's census at the catalog's figures, and a census is a sum over
//! the request's messages and blocks. So the census of the candidate at cut
//! `i` is, near enough, the last cut's (its system, tools, and the last
//! segment) plus that of each segment between: the nodes from one cut to
//! the next, rendered alone. The guess walks the segments from the last
//! back, rendering each once, until the sum passes the target; the ring
//! then renders the guess and its neighbour whole to settle it, so what it
//! picks is exactly the earliest cut whose own render is under the target,
//! as long as a cut further on never estimates more than one before it.
//!
//! The sums are near and not exact where a segment's render differs alone
//! from its render among the rest: a segment that ends in a user message
//! merges with the next one's (one message's framing fewer), a call answered
//! in a later segment, an answer a later one replaced, and the images and
//! PDFs budgeted in render order. Those move the guess by a cut or so, and
//! the settling renders find the cut the rule names.

use std::collections::HashSet;

use crate::node::Node;
use crate::provider::Census;
use crate::stub::Stub;

use super::super::{clearing, media_of, render_messages, CompileInput};
use super::Ring;

/// What the guess reads: the compile's input, the ring's, and the last
/// cut's census and image tokens.
pub(super) struct At<'a, 'b> {
    pub input: &'a CompileInput<'b>,
    pub r: &'a Ring<'b>,
    pub base: (Census, u64),
}

/// The earliest of `cuts` (indices into `seq`) whose sum is at most
/// `target`, walking back from the last cut, whose own render is `at.base`
/// and is under it. Renders only the segments it walks.
pub(super) fn guess(at: &At<'_, '_>, seq: &[&Stub], cuts: &[usize], target: u64) -> usize {
    let (input, r) = (at.input, at.r);
    let spec = r.recompiled;
    let media = media_of(spec, input.catalog, input.blobs, input.hidden);
    let retrying = input
        .overflowed
        .and_then(|o| o.retrying.as_deref())
        .or(spec.fallback.as_ref().map(|f| f.1.as_str()));
    let cleared: HashSet<&str> = clearing::drops(r.carried)
        .filter_map(|d| d.node_id.as_deref())
        .collect();
    let (mut census, mut images) = at.base;
    for i in (0..cuts.len().saturating_sub(1)).rev() {
        let nodes: Vec<&Node> = seq[cuts[i]..cuts[i + 1]].iter().map(|n| &***n).collect();
        let m = render_messages(
            &nodes,
            &[],
            true,
            &spec.provider,
            &media,
            retrying,
            (input.sources, &cleared),
        );
        add(&mut census, &Census::of_messages(&m.messages));
        images += m.image_tokens;
        let tokens = census.tokens(r.rates) + images;
        let tokens = input.overflowed.map_or(tokens, |o| o.scale(tokens));
        if tokens > target {
            return i + 1;
        }
    }
    0
}

/// The earliest cut whose own render's `tokens` is at most `target`, from
/// the guess: back from it while the cut before is under the target too,
/// else on from it to the first that is. None when no cut is.
pub(super) fn settle(
    last: usize,
    target: u64,
    guess: usize,
    tokens: &mut dyn FnMut(usize) -> u64,
) -> Option<usize> {
    let mut i = guess;
    if tokens(i) <= target {
        while i > 0 && tokens(i - 1) <= target {
            i -= 1;
        }
        Some(i)
    } else {
        (i + 1..=last).find(|&j| tokens(j) <= target)
    }
}

/// `into` plus `c`, class by class.
fn add(into: &mut Census, c: &Census) {
    into.json += c.json;
    into.text += c.text;
    into.opaque += c.opaque;
    into.messages += c.messages;
    into.blocks += c.blocks;
    into.ids += c.ids;
}
