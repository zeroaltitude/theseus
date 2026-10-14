//! `theseus history`'s own lines: each node with its short id, which
//! `theseus reach` takes (theseus-glyw), and where the page it printed ends,
//! with the command for the next one either way (theseus-xo0m, theseus-kym3).

use theseus_protocol::{short_id as short, NodeInfo, SessionHistoryResult};

use super::md::Markdown;
use super::style::{prose_width, Mode};
use super::{push, Line, Tag};

/// The page's cursors as the commands that read past them: older nodes
/// before this page, and more after it.
pub fn page_lines(h: &SessionHistoryResult) -> Vec<Line> {
    let mut out = Vec::new();
    let s = &h.session.session_id;
    if let Some(p) = h.older {
        push(
            &mut out,
            Tag::Dim,
            &format!("── older nodes before position {p}: theseus history {s} --before {p}"),
        );
    }
    if let Some(p) = h.next {
        push(
            &mut out,
            Tag::Dim,
            &format!("── more nodes after position {p}: theseus history {s} --after {p}"),
        );
    }
    out
}

/// A node's lines as `super::node_lines` writes them, its short id after its
/// first line's mark (the time, `⚙`, or `←`). The terminal UI's pane draws
/// `super::node_lines` without it.
pub fn node_lines(n: &NodeInfo, full: bool) -> Vec<Line> {
    let mut out = super::node_lines(n, full);
    if let Some(first) = out.first_mut() {
        first.text = with_id(&first.text, &short(&n.node_id));
    }
    out
}

/// A reply is drawn at most this many characters long unless `--full`.
const CLIP: usize = 300;

/// A node's lines for a terminal, each a finished string (theseus-rn7s): the
/// same words as `node_lines`, except that a reply's text is drawn as
/// markdown, indented as `--full` indents it. Short of `--full` a reply is
/// cut at `CLIP` characters first. `cols` is the terminal's width.
pub fn styled_node_lines(n: &NodeInfo, full: bool, cols: usize) -> Vec<String> {
    if n.kind != "assistant_message" || n.text.trim().is_empty() {
        return node_lines(n, full).into_iter().map(|l| l.text).collect();
    }
    let mut shown = n.clone();
    if !full && shown.text.chars().count() > CLIP {
        shown.text = format!("{}…", shown.text.chars().take(CLIP).collect::<String>());
    }
    let all = node_lines(&shown, true);
    let body = shown.text.lines().count().max(1);
    let mut out = vec![all[0].text.clone()];
    for l in Markdown::render(&shown.text, prose_width(cols).saturating_sub(4), Mode::Ansi) {
        out.push(format!("    {}", l.ansi()));
    }
    out.extend(
        all.into_iter()
            .skip(1 + body)
            .filter(|l| full || l.tag != Tag::Thinking)
            .map(|l| l.text),
    );
    out
}

/// `line` with `id` after its mark: `[23:26:41.234] `, or the indent and
/// symbol of a call or a result.
fn with_id(line: &str, id: &str) -> String {
    let at = if line.starts_with('[') {
        line.find("] ").map(|i| i + 2)
    } else {
        let body = line.trim_start();
        let lead = line.len() - body.len();
        body.find(' ').map(|i| lead + i + 1)
    };
    match at {
        Some(i) => format!("{}{id} {}", &line[..i], &line[i..]),
        None => format!("{line} {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_id_is_the_cockpits_and_sits_after_the_mark() {
        assert_eq!(short("msg_0198a0b1c2d3e4f5a6b7c8d9e0f1a2b3"), "msg·f1a2b3");
        assert_eq!(short("abcdefgh"), "cdefgh");
        assert_eq!(
            with_id("[23:26:41.234] operator: hi", "msg·f1a2b3"),
            "[23:26:41.234] msg·f1a2b3 operator: hi"
        );
        assert_eq!(
            with_id("      ← fs.list ok · 9 B: a.md", "res·a1b2c3"),
            "      ← res·a1b2c3 fs.list ok · 9 B: a.md"
        );
        assert_eq!(with_id("odd", "x"), "odd x");
    }
}
