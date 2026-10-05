//! `theseus history`'s own lines: each node with its short id, which
//! `theseus reach` takes (theseus-glyw), and where the page it printed ends,
//! with the command for the next one either way (theseus-xo0m, theseus-kym3).

use theseus_protocol::{short_id as short, NodeInfo, SessionHistoryResult};

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

/// `line` with `id` after its mark: `[06:26:41.234Z] `, or the indent and
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
            with_id("[06:26:41.234Z] operator: hi", "msg·f1a2b3"),
            "[06:26:41.234Z] msg·f1a2b3 operator: hi"
        );
        assert_eq!(
            with_id("      ← fs.list ok · 9 B: a.md", "res·a1b2c3"),
            "      ← res·a1b2c3 fs.list ok · 9 B: a.md"
        );
        assert_eq!(with_id("odd", "x"), "odd x");
    }
}
