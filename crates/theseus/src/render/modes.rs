//! A conversation's mode and plan, as a terminal says them (theseus-lyxy): the
//! `session.mode` notification's lines in `watch` and `ask`, from the words
//! `theseus_protocol::modes` holds once.

use theseus_protocol::modes::{checklist, plan_line, SessionModeChanged};

use super::{Line, Tag};

/// What a change says: the mode, the plan's count, and the checklist.
pub fn lines(m: &SessionModeChanged) -> Vec<String> {
    let plan = plan_line(&m.steps).map_or(String::new(), |p| format!(" · {p}"));
    let mut out = vec![format!(
        "  ⇄ mode: {} · {}{plan} ({})",
        m.mode.status(),
        m.mode.effect(),
        m.by
    )];
    out.extend(
        checklist(&m.steps)
            .into_iter()
            .map(|l| format!("      {l}")),
    );
    out
}

/// `lines` as the event's `Line`s: the first one a warning, the checklist dim.
pub fn event_lines(m: &SessionModeChanged) -> Vec<Line> {
    lines(m)
        .into_iter()
        .enumerate()
        .map(|(i, l)| Line::new(if i == 0 { Tag::Warn } else { Tag::Dim }, l))
        .collect()
}
