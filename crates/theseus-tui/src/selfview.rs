//! The self view (theseus-pw1q.19): what Theseus changed about itself, in a
//! pane over the whole screen. Top to bottom: the switch's state in the CLI's
//! words (`render::self_state_line`), the self budget's day and its newest
//! branch against their lines (once the daemon sends them), and the last
//! twenty `self.log` rows, newest first, one line each.
//!
//! It reads `self.log` when it opens and again when something it already
//! hears says self work may have moved: an execution that changed state, a
//! task, a question's answer. One read at a time, a second wanted while one is
//! in flight is made when it lands, so a burst is two reads and nothing is
//! polled on a timer. Rows with no execution beside them (a night's exam
//! pass) show at the next move or when the pane is opened again.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{json, Value};
use theseus_client::render::{self, Tag};
use theseus_protocol::rsi::{SelfLogResult, SelfLogRow, SelfMode};
use theseus_protocol::{method, Event};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Call, Effect, Mode, Purpose, WIDE};
use crate::ui::wrap;

/// How many rows the pane reads, and shows on a wide screen.
pub const ROWS: usize = 20;

/// How many rows a narrow screen keeps.
pub const FOLDED_ROWS: usize = 5;

/// What the pane holds between reads.
#[derive(Debug, Default)]
pub struct SelfView {
    /// The last `self.log` answer.
    pub log: Option<SelfLogResult>,
    /// Why the last read failed (an older daemon has no `self.log`).
    pub failed: Option<String>,
    /// The row under the cursor.
    pub picked: usize,
    /// The picked row, whole.
    pub whole: bool,
    /// A read is in flight.
    reading: bool,
    /// Something moved while it was.
    stale: bool,
}

impl SelfView {
    /// Whether a read should be sent now. One at a time: a second want is
    /// kept for when the first lands.
    fn want(&mut self) -> bool {
        if self.reading {
            self.stale = true;
            return false;
        }
        self.reading = true;
        true
    }

    /// The connection closed: a read in flight will not be answered.
    pub fn dropped(&mut self) {
        self.reading = false;
        self.stale = false;
    }

    /// The rows the pane shows at this width.
    fn shown(&self, wide: bool) -> &[SelfLogRow] {
        let rows = self.log.as_ref().map_or(&[][..], |l| &l.rows[..]);
        &rows[..rows.len().min(if wide { ROWS } else { FOLDED_ROWS })]
    }
}

/// Whether an event says self work may have moved.
pub fn moves(e: &Event) -> bool {
    match e {
        Event::ExecutionChanged(v) => v.previous.as_deref() != Some(v.state.as_str()),
        Event::TaskChanged(_) | Event::ConfirmResolved(_) | Event::ConfirmRequested(_) => true,
        _ => false,
    }
}

fn read() -> Effect {
    Effect::Call(Call {
        method: method::SELF_LOG,
        params: json!({ "limit": ROWS }),
        purpose: Purpose::SelfLog,
    })
}

impl App {
    /// `l`: open the pane and read the log.
    pub(crate) fn open_self(&mut self) -> Vec<Effect> {
        self.mode = Mode::SelfView;
        self.selfview.picked = 0;
        self.selfview.whole = false;
        self.selfview.failed = None;
        self.self_read()
    }

    /// Read the log, unless a read is in flight.
    pub(crate) fn self_read(&mut self) -> Vec<Effect> {
        match self.selfview.want() {
            true => vec![read()],
            false => Vec::new(),
        }
    }

    /// Something that may have moved arrived: read again while the pane is
    /// open.
    pub(crate) fn self_moved(&mut self, e: &Event) -> Vec<Effect> {
        match self.mode == Mode::SelfView && moves(e) {
            true => self.self_read(),
            false => Vec::new(),
        }
    }

    /// `self.log`'s answer.
    pub(crate) fn self_answered(
        &mut self,
        result: Result<Value, theseus_protocol::RpcError>,
    ) -> Vec<Effect> {
        let v = &mut self.selfview;
        v.reading = false;
        match result.map(serde_json::from_value::<SelfLogResult>) {
            Ok(Ok(log)) => {
                v.failed = None;
                v.picked = v.picked.min(log.rows.len().saturating_sub(1));
                v.log = Some(log);
            }
            Ok(Err(e)) => v.failed = Some(format!("the self log did not decode: {e}")),
            Err(e) => v.failed = Some(format!("self.log: {}", e.message)),
        }
        match std::mem::take(&mut v.stale) {
            true => self.self_read(),
            false => Vec::new(),
        }
    }

    /// A key in the pane: up and down pick, enter shows the row whole, esc
    /// (or `l`) closes.
    pub(crate) fn selfview_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        let wide = self.width >= WIDE;
        let last = self.selfview.shown(wide).len().saturating_sub(1);
        let v = &mut self.selfview;
        match k.code {
            KeyCode::Char('q') => return vec![Effect::Quit],
            KeyCode::Esc | KeyCode::Char('l') if v.whole => v.whole = false,
            KeyCode::Esc | KeyCode::Char('l') => self.mode = Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') => v.picked = (v.picked + 1).min(last),
            KeyCode::Up | KeyCode::Char('k') => v.picked = v.picked.saturating_sub(1),
            KeyCode::Enter => v.whole = !v.whole && !v.shown(wide).is_empty(),
            _ => {}
        }
        Vec::new()
    }

    /// The pane's lines for a screen `width` wide, each with its tag.
    pub fn self_lines(&self, width: u16) -> Vec<(Tag, String)> {
        let wide = width >= WIDE;
        let w = width.saturating_sub(2) as usize;
        let v = &self.selfview;
        let mut out = vec![(Tag::Plain, " Self-improvement".to_string())];
        if let Some(why) = &v.failed {
            out.push((Tag::Bad, format!(" {why}")));
        }
        let Some(log) = &v.log else {
            if v.failed.is_none() {
                out.push((Tag::Dim, " reading the self log…".into()));
            }
            return out;
        };
        if log.state.mode == SelfMode::Off {
            out.push((
                Tag::Warn,
                " Self-improvement is off. Nothing has run.".into(),
            ));
        }
        for row in wrap(&render::self_state_line(&log.state), w) {
            out.push((Tag::Plain, format!(" {row}")));
        }
        if wide {
            if let Some(b) = &log.budget {
                // The day, then the newest branch: the rest is `theseus self log`.
                for l in render::budget_lines(b).into_iter().take(2) {
                    out.push((Tag::Dim, fit(&format!(" {l}"), w + 1)));
                }
            }
        }
        out.push((Tag::Plain, String::new()));
        if log.building {
            out.push((
                Tag::Dim,
                " the ledger's index is still being built after a start".into(),
            ));
        }
        let rows = v.shown(wide);
        if rows.is_empty() && !log.building {
            out.push((
                Tag::Dim,
                " Theseus has changed nothing about itself in this window.".into(),
            ));
        }
        if v.whole {
            if let Some(r) = rows.get(v.picked) {
                out.extend(whole(r, w));
            }
            return out;
        }
        for (i, r) in rows.iter().enumerate() {
            let mark = if i == v.picked { '▸' } else { ' ' };
            out.push((
                if i == v.picked {
                    Tag::Reply
                } else {
                    Tag::Plain
                },
                fit(&format!("{mark}{}", row_line(r)), w + 1),
            ));
        }
        if (!wide && log.rows.len() > FOLDED_ROWS) || (wide && log.more) {
            out.push((Tag::Dim, " older: theseus self log --since 30d".into()));
        }
        out
    }

    /// The pane's footer keys.
    pub fn self_keys(&self) -> &'static str {
        match self.selfview.whole {
            true => "[esc] back  [q] quit ",
            false => "[↑↓] pick  [enter] whole row  [esc] close ",
        }
    }
}

/// A row in a line: its time, kind, what, cost and undo.
pub fn row_line(r: &SelfLogRow) -> String {
    let mut l = format!(
        "{} {:<20} {}",
        render::fmt_date(r.at_unix_ms).get(5..).unwrap_or_default(),
        r.kind,
        r.what
    );
    let cost = cost_of(&r.numbers);
    if cost > 0.0 {
        l.push_str(&format!(" · {}", usd(cost)));
    }
    if let Some(u) = &r.undo {
        l.push_str(&format!(" · undo: {u}"));
    }
    l
}

/// A row, whole: every field it carries.
fn whole(r: &SelfLogRow, w: usize) -> Vec<(Tag, String)> {
    let mut out = vec![(
        Tag::Plain,
        format!(" {} {}", render::fmt_date(r.at_unix_ms), r.kind),
    )];
    let mut field = |name: &str, text: &str| {
        for (i, l) in wrap(text, w.saturating_sub(10)).into_iter().enumerate() {
            let lead = if i == 0 {
                format!(" {name:<9}")
            } else {
                " ".repeat(10)
            };
            out.push((Tag::Plain, format!("{lead}{l}")));
        }
    };
    field("what", &r.what);
    if let Some(why) = &r.why {
        field("why", why);
    }
    if !r.numbers.is_null() {
        field("numbers", &r.numbers.to_string());
    }
    if let Some(u) = &r.undo {
        field("undo", u);
    }
    if let Some(s) = &r.session_id {
        field("session", s);
    }
    field("position", &r.position.to_string());
    out
}

/// What a row cost, from its numbers: a cost, a writer's cost, or a spend
/// (the digest's rule).
fn cost_of(numbers: &Value) -> f64 {
    ["cost_usd", "writer_usd", "spent_usd"]
        .iter()
        .filter_map(|k| numbers[*k].as_f64())
        .fold(0.0, |a, c| a + c)
}

fn usd(v: f64) -> String {
    match v < 0.01 {
        true => format!("${v:.4}"),
        false => format!("${v:.2}"),
    }
}

/// `text` cut to `width` columns, `…` at the cut.
fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw + 1 > width {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out
}
