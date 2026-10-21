//! One-turn conversations fold (theseus-ikw4p): top-level conversations that are idle, have one turn or none, no tasks and
//! no question, and were last active over an hour ago make one row at the foot of the sidebar, `▸ 20 one-turn
//! conversations`, which enter on the row opens (`▾`) and closes. Nothing that needs the owner folds.

use theseus_protocol::{Level, SessionKind};

use crate::app::App;
use crate::board::{Only, Row};

/// The fold row's session id: it names no session, so enter on it folds.
pub const FOLD_ID: &str = "fold:one-turn";
const AFTER_MS: u64 = 3_600_000;

impl App {
    /// The sidebar's rows with the one-turn conversations folded away, or shown above their fold row while it is open.
    /// Only the whole tree folds: a filter or a level key shows the sessions that match.
    pub(crate) fn fold_one_turn(&self, rows: Vec<Row>) -> Vec<Row> {
        if self.only != Only::All || !self.filter.trim().is_empty() {
            return rows;
        }
        let foldable: Vec<bool> = (0..rows.len())
            .map(|i| {
                let r = &rows[i];
                r.depth == 0
                    && r.folded == 0
                    && rows.get(i + 1).is_none_or(|n| n.depth == 0)
                    && matches!(r.level, Level::Ready | Level::Idle)
                    && !r.done
                    && !r.dismissed
                    && self.board.info(&r.session_id).is_some_and(|s| {
                        s.kind == SessionKind::Conversation
                            && s.turns <= 1
                            && s.pending_confirms == 0
                            && s.last_active_ms > 0
                            && self.now_ms.saturating_sub(s.last_active_ms) > AFTER_MS
                    })
            })
            .collect();
        let n = foldable.iter().filter(|f| **f).count();
        if n == 0 {
            return rows;
        }
        let spent_usd = rows
            .iter()
            .zip(&foldable)
            .filter(|(_, f)| **f)
            .map(|(r, _)| r.spent_usd)
            .sum();
        let mut out: Vec<Row> = match self.one_turn_open {
            true => rows,
            false => rows
                .into_iter()
                .zip(foldable)
                .filter(|(_, f)| !f)
                .map(|(r, _)| r)
                .collect(),
        };
        let mark = if self.one_turn_open { '▾' } else { '▸' };
        out.push(Row {
            session_id: FOLD_ID.to_string(),
            depth: 0,
            last: true,
            level: Level::Idle,
            done: false,
            dismissed: false,
            label: format!("{mark} {n} one-turn conversations"),
            name: String::new(),
            spent_usd,
            folded: 0,
            facts: None,
        });
        out
    }

    /// Enter on the fold row opens or closes it.
    pub(crate) fn toggle_one_turn(&mut self) {
        self.one_turn_open = !self.one_turn_open;
    }
}
