//! A task's arrangement in `theseus tasks` (M5 27, theseus-vug.2): its mark
//! on the task's line, and a line for each piece under it; and a check's
//! basis under it (M5 28a, theseus-vug.3).

use theseus_protocol::{TaskArrangement, TaskInfo};

/// The mark on a task's line: `📎 3 pieces`, and the fidelity check's ack.
pub fn clip(a: &TaskArrangement) -> String {
    let n = a.pieces.len();
    let mut s = format!(" · 📎 {n} piece{}", if n == 1 { "" } else { "s" });
    if a.fidelity_ack {
        s.push_str(" (fidelity acknowledged)");
    }
    s
}

/// One line per piece, indented under its task: its index and role, the
/// node it quotes, who wrote it and when (on this machine's clock), and its
/// first line.
pub fn task_pieces(t: &TaskInfo) -> Vec<String> {
    let Some(a) = &t.arrangement else {
        return vec![];
    };
    a.pieces
        .iter()
        .map(|p| {
            let who = match &p.author {
                Some(a) => format!("{} ({a})", p.origin),
                None => p.origin.clone(),
            };
            let mut marks = String::new();
            if p.trusted {
                marks.push_str(", trusted");
            }
            if let Some(b) = p.superseded_by {
                marks.push_str(&format!(", superseded by {b}"));
            }
            format!(
                "  📎 {} {}{marks}\t{}\t{who}, {}\t{}",
                p.index,
                p.role,
                p.node_id,
                super::time::fmt_hm(p.at_ms),
                p.first_line
            )
        })
        .collect()
}

/// A check's basis, under its task: its line (`🔍 check of task a1b2c3 ·
/// independent (excluded ses_…, model)`), then each flagged span.
pub fn task_check(t: &TaskInfo) -> Vec<String> {
    let Some(c) = &t.check else {
        return vec![];
    };
    let mut out = vec![format!("  {}", c.line())];
    for o in &c.overlaps {
        out.push(format!(
            "  🔍 overlap: {} shares {} words with {}\t\"{}\"",
            o.source, o.words, o.node_id, o.span
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::ArrangementPiece;

    #[test]
    fn a_checks_basis_is_shown_under_it() {
        let mut t = TaskInfo::default();
        assert!(task_check(&t).is_empty());
        t.check = Some(theseus_protocol::TaskCheck {
            checked_short: "a1b2c3".into(),
            excluded_sessions: vec!["ses_000000a1b2c3".into()],
            model: "glm-4.6".into(),
            overlaps: vec![theseus_protocol::CheckOverlap {
                source: "brief".into(),
                node_id: "trs_1".into(),
                words: 12,
                span: "the north pier".into(),
            }],
            ..Default::default()
        });
        assert_eq!(
            task_check(&t),
            [
                "  🔍 check of task a1b2c3 · independent (excluded ses_…a1b2c3, glm-4.6) · overlap: 1 span",
                "  🔍 overlap: brief shares 12 words with trs_1\t\"the north pier\""
            ]
        );
    }

    #[test]
    fn a_tasks_pieces_are_listed_under_it() {
        let mut t = TaskInfo {
            short: "a1b2c3".into(),
            ..Default::default()
        };
        assert!(task_pieces(&t).is_empty());
        let piece = |index, role: &str| ArrangementPiece {
            index,
            role: role.into(),
            node_id: format!("msg_{index}"),
            session_id: "ses_lighthouse".into(),
            origin: "operator".into(),
            author: Some("cli".into()),
            at_ms: 1_790_000_000_000,
            first_line: "Paint the lamp room.".into(),
            trusted: index == 1,
            superseded_by: (index == 0).then_some(1),
        };
        let a = TaskArrangement {
            node_id: "arr_1".into(),
            pieces: vec![piece(0, "objective"), piece(1, "design")],
            fidelity_ack: true,
        };
        assert_eq!(clip(&a), " · 📎 2 pieces (fidelity acknowledged)");
        t.arrangement = Some(a);
        assert_eq!(
            task_pieces(&t),
            [
                "  📎 0 objective, superseded by 1\tmsg_0\toperator (cli), 07:13\tPaint the lamp room.",
                "  📎 1 design, trusted\tmsg_1\toperator (cli), 07:13\tPaint the lamp room."
            ]
        );
    }
}
