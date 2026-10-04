//! A task's arrangement in `theseus tasks` (M5 27, theseus-vug.2): its mark
//! on the task's line, and a line for each piece under it.

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
/// node it quotes, who wrote it and when (UTC), and its first line.
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
                "  📎 {} {}{marks}\t{}\t{who}, {} UTC\t{}",
                p.index,
                p.role,
                p.node_id,
                theseus_protocol::utc_hm(p.at_ms),
                p.first_line
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::ArrangementPiece;

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
                "  📎 0 objective, superseded by 1\tmsg_0\toperator (cli), 14:13 UTC\tPaint the lamp room.",
                "  📎 1 design, trusted\tmsg_1\toperator (cli), 14:13 UTC\tPaint the lamp room."
            ]
        );
    }
}
