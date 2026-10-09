//! Terminals' facts (theseus-n88g.4): a terminal opened, and one closed,
//! by `term.close` or by its session's end, a cancel, a stop, or the
//! daemon's stop; and what a close left running, or a cancel or a stop
//! ended of it (theseus-ggqf). Each call of a `term.*` tool has its own
//! tool rows too.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Tool;

use super::{Fact, Say};
use crate::narrative;
use crate::term::pty::Kept;
use crate::term::{left_json, program, Closed};

/// A terminal opened (`term.opened`): its program, where, and its size.
pub struct TermOpened<'a> {
    pub id: &'a str,
    /// The open's `meta.opened`: argv, cwd, pid, rows, cols.
    pub opened: &'a Value,
}

impl TermOpened<'_> {
    fn argv(&self) -> Vec<String> {
        self.opened
            .get("argv")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Fact for TermOpened<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TermOpened);

    fn row(&self) -> Value {
        let mut row = json!({"terminal": self.id});
        if let (Some(r), Some(o)) = (row.as_object_mut(), self.opened.as_object()) {
            r.extend(o.clone());
        }
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let o = self.opened;
        say.line(
            Tool,
            format!(
                "Opened terminal {} on {}, {}x{}, pid {}.",
                self.id,
                program(&self.argv()),
                o["rows"],
                o["cols"],
                o["pid"]
            ),
        );
    }
}

/// A terminal closed (`term.closed`): by whom or what, how its program
/// ended, and what it moved.
pub struct TermClosed<'a> {
    pub closed: &'a Closed,
}

impl Fact for TermClosed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TermClosed);

    fn row(&self) -> Value {
        self.closed.meta()
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let c = self.closed;
        let killed = match c.killed {
            0 => String::new(),
            n => format!(
                ", {} killed after the grace",
                narrative::count(n as u64, "process", "processes")
            ),
        };
        say.line(
            Tool,
            format!(
                "Closed terminal {} ({}) because {}: {}{killed}, open {}.",
                c.id,
                program(&c.argv),
                match c.by.as_str() {
                    crate::term::BY_TOOL => "term.close asked",
                    by => by,
                },
                c.how(),
                narrative::duration(c.open_ms),
            ),
        );
    }
}

/// What a terminal's close left running (`term.left`), or, `ended`, what a
/// cancel or a `/stop` ended of it later: each process by its pid, its
/// program, and why it was left.
pub struct TermLeft<'a> {
    pub terminal: &'a str,
    pub program: &'a str,
    pub by: &'a str,
    pub procs: &'a [Kept],
    pub ended: bool,
}

impl Fact for TermLeft<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TermLeft);

    fn row(&self) -> Value {
        json!({"terminal": self.terminal, "program": self.program, "by": self.by,
            "ended": self.ended, "left": left_json(self.procs)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let each: Vec<String> = self
            .procs
            .iter()
            .map(|k| format!("pid {} ({}, {})", k.proc.pid, k.program, k.why))
            .collect();
        say.line(
            Tool,
            match self.ended {
                false => format!(
                    "Left running as terminal {} ({}) closed because {}: {}.",
                    self.terminal,
                    self.program,
                    self.by,
                    each.join(", ")
                ),
                true => format!(
                    "Ended what terminal {} ({}) left running, because {}: {}.",
                    self.terminal,
                    self.program,
                    self.by,
                    each.join(", ")
                ),
            },
        );
    }
}

/// The facts a `term.*` call's result names, recorded once it is written:
/// an open's, and a close's.
pub fn of_result(rec: &super::Rec<'_>, meta: &Value) {
    let Some(id) = meta.get("terminal").and_then(Value::as_str) else {
        return;
    };
    if let Some(opened) = meta.get("opened") {
        rec.record(&TermOpened { id, opened });
    }
    // A close's, and the ended terminal an open reclaimed (theseus-ggqf).
    for (id, c) in [
        meta.get("closed").map(|c| (id, c)),
        meta.get("reclaimed")
            .and_then(|c| Some((c.get("terminal")?.as_str()?, c))),
    ]
    .into_iter()
    .flatten()
    {
        let closed = Closed {
            id: id.into(),
            session: rec.session.unwrap_or_default().into(),
            argv: c["argv"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            by: c["by"].as_str().unwrap_or_default().into(),
            exit: c["exit"].as_i64().map(|v| v as i32),
            signal: c["signal"].as_i64().map(|v| v as i32),
            killed: c["killed"].as_u64().unwrap_or(0) as usize,
            left: Vec::new(),
            open_ms: c["open_ms"].as_u64().unwrap_or(0),
            bytes_out: c["bytes_out"].as_u64().unwrap_or(0),
            bytes_in: c["bytes_in"].as_u64().unwrap_or(0),
        };
        rec.record(&TermClosed { closed: &closed });
    }
}
