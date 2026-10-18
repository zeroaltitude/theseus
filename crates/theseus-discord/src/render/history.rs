//! The ended tasks `/tasks show:done` and `show:all` read from the store
//! (theseus-ypnxt.5), apart from the board's memory: the board keeps a done
//! task for a day, this keeps what the store knows. Pure: rows in, lines out.

use super::board_v2::ago;
use super::{clip, dollars};
use crate::runtime::emoji::state;
use theseus_protocol::work::WorkState;

/// How many rows `/tasks` shows; the rest are counted.
pub const SHOWN: usize = 10;

/// How a task ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    Done,
    /// With its reason, when the store has one.
    Failed(Option<String>),
    Stopped(Option<String>),
}

/// One ended task, as the store has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub short: String,
    pub title: String,
    pub ended: Ended,
    pub at_ms: u64,
    pub cost_usd: f64,
    /// The first line of its report, when its node is readable.
    pub said: Option<String>,
    /// The forum post or channel it worked in.
    pub link: Option<u64>,
}

/// One row as a line: mark, title, short id, how it ended and when, what it
/// cost, the report's first line, and the post or channel to tap.
pub fn line(r: &Row) -> String {
    let (mark, how) = match &r.ended {
        Ended::Done => (state(WorkState::Done), "done".to_string()),
        Ended::Failed(why) => (state(WorkState::Failed), with("failed", why)),
        Ended::Stopped(why) => (state(WorkState::Cancelled), with("stopped", why)),
    };
    let title = match r.title.trim() {
        "" => "(untitled task)".to_string(),
        t => clip(&t.replace('\n', " "), 60),
    };
    let mut s = format!(
        "{mark} **{title}** `…{}` · {how} {} · {}",
        r.short,
        ago(r.at_ms),
        dollars(r.cost_usd)
    );
    if let Some(said) = r.said.as_deref().filter(|x| !x.is_empty()) {
        s.push_str(&format!("\n-# {}", clip(said, 120)));
    }
    if let Some(id) = r.link {
        s.push_str(&format!(" · <#{id}>"));
    }
    s
}

fn with(word: &str, why: &Option<String>) -> String {
    match why.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        Some(w) => format!("{word} ({})", clip(&w.replace('\n', " "), 60)),
        None => word.to_string(),
    }
}

/// The section: its heading, the first [`SHOWN`] rows newest first, and how
/// many more there are.
pub fn lines(rows: &[Row], cap: usize) -> Vec<String> {
    let shown = rows.len().min(cap.min(SHOWN));
    let mut out = vec![format!("**{} Finished**", state(WorkState::Done))];
    out.extend(rows.iter().take(shown).map(line));
    match rows.len() - shown {
        0 if rows.is_empty() => out.push("-# No finished task here yet.".into()),
        0 => {}
        n => out.push(format!("-# +{n} more finished")),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: usize) -> Row {
        Row {
            short: format!("a1b2{n:02}"),
            title: format!("Chart the reef {n}"),
            ended: Ended::Done,
            at_ms: 1_790_000_000_000,
            cost_usd: 0.42,
            said: Some("The reef is charted.".into()),
            link: None,
        }
    }

    #[test]
    fn a_row_names_how_it_ended_when_what_it_cost_and_where_it_worked() {
        let mut r = row(1);
        r.link = Some(900_000_000_000_500_007);
        let l = line(&r);
        assert!(
            l.contains("**Chart the reef 1** `…a1b201` · done <t:1790000000:R> · $0.42"),
            "{l}"
        );
        assert!(l.contains("\n-# The reef is charted."), "{l}");
        assert!(l.ends_with("<#900000000000500007>"), "{l}");
        r.ended = Ended::Failed(Some("the probe broke".into()));
        r.said = None;
        assert!(
            line(&r).contains("failed (the probe broke)"),
            "{}",
            line(&r)
        );
        r.ended = Ended::Stopped(None);
        assert!(line(&r).contains(" stopped <t:"), "{}", line(&r));
    }

    #[test]
    fn ten_are_shown_and_the_rest_are_counted() {
        let rows: Vec<Row> = (0..13).map(row).collect();
        let l = lines(&rows, 12);
        assert_eq!(l.len(), 12, "a heading, ten rows, a count: {l:?}");
        assert_eq!(l.last().unwrap(), "-# +3 more finished");
        assert!(lines(&[], 12)[1].contains("No finished task"));
    }
}
