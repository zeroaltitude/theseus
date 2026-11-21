//! A fold node's block (design §5.2: its title, the person's words in it, the account, the handle) and a
//! band's closing count (§5.3: every band ends with what was left out, and a handle that opens it).

use crate::recall::stamp::{self, ZoneAt};
use crate::view::budget::Residue;
use crate::view::fold::FoldNode;
use crate::view::render::MarkedBlock;
use crate::view::select::Band;
use crate::view::{Anchor, Handle, Label, PointOfView, Pos, SourceRange, Space};

use super::labels;

/// A fold node's block. `words` are the texts of [`FoldNode::his_words`], in order, as stored (a source that
/// cannot be read is `None`, and says so). The person's words are shown word for word and never wrapped; the
/// account is Theseus's, and is wrapped as outside text when the node was made from any.
///
/// ```text
/// earlier here: 20 exchanges, Mon 2026-11-16 09:02 MST to Wed 2026-11-18 17:40 MST
/// the person's words, word for word:
/// - fix the flaky test
/// - no, 41
/// (and 3 more of their lines, counted)
/// what Theseus did: …
/// (open fold.…)
/// ```
pub fn fold_block(
    node: &FoldNode,
    words: &[Option<String>],
    pov: &PointOfView,
    zone: ZoneAt<'_>,
) -> MarkedBlock {
    let here = matches!(&pov.anchor, Anchor::Place(p) if *p == node.place);
    let label = match (node.marks.outside, here) {
        (true, _) => Label::OutsideText,
        (false, true) => Label::Here,
        (false, false) => Label::Testimony,
    };
    let account = match node.marks.outside {
        true => labels::wrap_outside(Some("tool results in these exchanges"), &node.account),
        false => node.account.clone(),
    };
    let mut lines = vec!["the person's words, word for word:".to_string()];
    lines.extend(words.iter().zip(&node.his_words).map(|(w, id)| match w {
        Some(w) => format!("- {}", w.replace('\n', "\n  ")),
        None => format!("- (its source, {id}, cannot be read)"),
    }));
    if node.his_counted > 0 {
        lines.push(format!(
            "(and {} more of their lines, counted)",
            crate::narrative::thousands(u64::from(node.his_counted))
        ));
    }
    lines.push(format!("what Theseus did: {account}"));
    lines.push(format!("(open {})", node.handle.0));
    MarkedBlock {
        label,
        heading: title(node, here, zone),
        text: lines.join("\n"),
        // The node itself, keyed in its space by its first exchange.
        sources: vec![SourceRange {
            space: Space::Folds {
                place: node.place.clone(),
                level: node.level,
            },
            first: node.first.clone(),
            last: node.first.clone(),
        }],
        handle: Some(node.handle.clone()),
    }
}

/// `earlier here: 20 exchanges, <first> to <last>`; above the first level `, level 2`; from another place
/// `earlier in <place>`.
fn title(node: &FoldNode, here: bool, zone: ZoneAt<'_>) -> String {
    let at = |p: &Pos| stamp::said_in(p.at_ms, zone);
    let whence = match here {
        true => "earlier here".to_string(),
        false => format!("earlier {}", labels::surface(&node.place)),
    };
    let level = match node.level {
        0 | 1 => String::new(),
        l => format!(", level {l}"),
    };
    format!(
        "{whence}{level}: {}, {} to {}",
        crate::narrative::count(u64::from(node.exchanges), "exchange", "exchanges"),
        at(&node.first),
        at(&node.last)
    )
}

/// A band's closing count of the person's lines and exchanges left out: `[left out: 12 of the person's lines
/// and 30 exchanges; open <handle>]`. `None` when nothing was.
pub fn left_out(lines: u32, exchanges: u32, handle: &Handle) -> Option<String> {
    let lines = (lines > 0).then(|| {
        format!(
            "{} of the person's lines",
            crate::narrative::thousands(u64::from(lines))
        )
    });
    let exchanges = (exchanges > 0)
        .then(|| crate::narrative::count(u64::from(exchanges), "exchange", "exchanges"));
    let what = match (lines, exchanges) {
        (None, None) => return None,
        (Some(l), Some(e)) => format!("{l} and {e}"),
        (Some(one), None) | (None, Some(one)) => one,
    };
    Some(format!("[left out: {what}; open {}]", handle.0))
}

/// The count that ends a band whose pieces the budget cut: `[3 more of the owner's words up to 7 days, about
/// 1,200 tokens, left out; open <handle>]`. `None` when nothing was.
pub fn residue_line(r: &Residue, handle: &Handle) -> Option<String> {
    (r.pieces > 0).then(|| {
        format!(
            "[{} more of {}, about {} tokens, left out; open {}]",
            crate::narrative::thousands(u64::from(r.pieces)),
            band(r.band),
            crate::narrative::thousands(r.tokens),
            handle.0
        )
    })
}

/// A band in plain words.
pub fn band(b: Band) -> &'static str {
    match b {
        Band::Frame => "the frame",
        Band::Now => "the clock and the message",
        Band::Floor => "the newest exchanges here",
        Band::WindowWords => "this conversation's words",
        Band::ElsewhereWords => "the owner's words elsewhere in the last two days",
        Band::Waiting => "what is waiting",
        Band::Past7 => "the owner's words up to 7 days",
        Band::WhoAndWhere => "who and where",
        Band::Ladder => "this conversation's earlier part",
        Band::WindowTools => "this conversation's tool results",
        Band::Reached => "the exchanges this message reached",
        Band::TaskCard => "the task's card",
        Band::Named => "the things this message named",
        Band::InPlay => "what is in play",
        Band::ElsewhereReplies => "Theseus's replies elsewhere",
        Band::Past30 => "the owner's words from 7 to 30 days",
        Band::PastOlder => "the owner's words older than 30 days",
        Band::Recall => "what search recalled",
    }
}
