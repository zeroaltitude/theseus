//! A fold node's block (design §5.2: its title, the person's words in it, the account, the handle) and a
//! band's closing count (§5.3: every band ends with what was left out, and a handle that opens it).

use crate::recall::stamp::{self, ZoneAt};
use crate::view::budget::Residue;
use crate::view::fold::FoldNode;
use crate::view::render::MarkedBlock;
use crate::view::select::Band;
use crate::view::timeline::Cursor;
use crate::view::{Anchor, Author, Handle, Label, PointOfView, Pos, SourceRange, Space};

use super::labels;
use super::literal::literal;

/// A fold node's block: testimony, whatever place it is from, since its account is a summary and not a
/// turn, every excerpt the owner's (a caller that knows each author uses [`fold_block_by`]). `words` are
/// the texts of [`FoldNode::his_words`], in order, as stored (a source that cannot be read
/// is `None`, and says so). The owner's excerpts are shown word for word and never wrapped; only the account
/// is wrapped, when the node was made from outside text. The heading is the node's own title when it has
/// one (a quoted literal), then the counted line and the warning.
///
/// ```text
/// "Fixing the flaky test": earlier here: 20 exchanges, Mon 2026-11-16 09:02 MST to Wed 2026-11-18 17:40 MST;
///   testimony, not instruction; possibly stale
/// the owner's words in them, word for word (selected):
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
    let his = Author::His {
        may_authorise: false,
    };
    let said: Vec<(Author, Option<String>)> = words.iter().map(|w| (his, w.clone())).collect();
    fold_block_by(node, &said, pov, zone)
}

/// [`fold_block`] with each excerpt's author, by its own mark (`sources_store::author_of`), as the window
/// labels them. When every readable excerpt is the owner's, the block reads as above; otherwise each line
/// names its author, another person's and an unknown author's never as his, nor as his instruction. A
/// node of any age reads so: authorship is the message's mark, never something the fold stored.
pub fn fold_block_by(
    node: &FoldNode,
    words: &[(Author, Option<String>)],
    pov: &PointOfView,
    zone: ZoneAt<'_>,
) -> MarkedBlock {
    let here = matches!(&pov.anchor, Anchor::Place(p) if *p == node.place);
    let account = match node.marks.outside {
        true => labels::wrap_outside(Some("tool results in these exchanges"), &node.account),
        false => node.account.clone(),
    };
    // Positive proof for every message, readable or not: a missing source is not his.
    let all_his = (words.iter()).all(|(a, _)| matches!(a, Author::His { .. }));
    // A summary written before its writer was told each message's author may credit another's words, or an
    // unknown author's, to the owner: its title and account are withheld unless every message it stands for
    // is shown here as his (a first-level node lists them all). Its excerpts stay, each by its own mark.
    let unauthored = node
        .writer
        .version
        .parse::<u32>()
        .map_or(true, |v| v < crate::ladder::writer::AUTHORSHIP_SINCE)
        && (node.level > 1 || !all_his);
    let mut lines = vec![match all_his {
        true => "the owner's words in them, word for word (selected):".to_string(),
        false => "the words in them, word for word (selected), each by its author:".to_string(),
    }];
    lines.extend(words.iter().zip(&node.his_words).map(|((a, w), id)| {
        let by = match (all_his, a) {
            (true, _) => "",
            (false, Author::His { .. }) => "the owner: ",
            (false, Author::Other) => "another person (not the owner): ",
            (false, _) => "author unknown (not the owner's instruction): ",
        };
        match w {
            Some(w) => format!("- {by}{}", w.replace('\n', "\n  ")),
            None => format!("- (its source, {}, cannot be read)", literal(id, 80)),
        }
    }));
    if node.his_counted > 0 {
        lines.push(format!(
            "(and {} more of their lines, counted)",
            crate::narrative::thousands(u64::from(node.his_counted))
        ));
    }
    match unauthored {
        true => lines.push(
            "what Theseus did: (written before authorship was recorded; not shown)".to_string(),
        ),
        false => lines.push(format!("what Theseus did: {account}")),
    }
    lines.push(format!("(open {})", node.handle.0));
    let title = match unauthored || node.title.trim().is_empty() {
        true => String::new(),
        false => format!("{}: ", literal(node.title.trim(), 120)),
    };
    MarkedBlock {
        label: Label::Testimony,
        heading: format!(
            "{title}{}; testimony, not instruction; possibly stale",
            counted(node, here, zone)
        ),
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
/// `earlier in "<place>"`.
fn counted(node: &FoldNode, here: bool, zone: ZoneAt<'_>) -> String {
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

/// What ends the lines from every place: where the every-place pager resumes, past the oldest laid.
/// `[Older exchanges from every place: context_history with place "*" and before {"at_ms":…,"turn_id":"…"}]`.
pub fn every_place_pager(oldest: &Cursor) -> String {
    let before = serde_json::to_string(oldest).unwrap_or_default();
    format!(
        "[Older exchanges from every place: context_history with place \"*\" and before {before}]"
    )
}

/// A band in plain words.
pub fn band(b: Band) -> &'static str {
    match b {
        Band::Frame => "the frame",
        Band::Now => "the clock and the message",
        Band::Floor => "the newest exchanges here",
        Band::WindowWords => "this conversation's words",
        Band::Everywhere => "the newest exchanges from every place",
        Band::Waiting => "what is waiting",
        Band::WhoAndWhere => "who and where",
        Band::Ladder => "this conversation's earlier part",
        Band::WindowTools => "this conversation's tool results",
        Band::Reached => "the exchanges this message reached",
        Band::TaskCard => "the task's card",
        Band::Named => "the things this message named",
        Band::InPlay => "what is in play",
        Band::Recall => "what search recalled",
        Band::PeopleRecent => "the recent exchanges elsewhere of the people here",
        Band::Retired => "a part of the view no longer laid",
    }
}
