//! A task the owner started with no model turn (theseus-7oq0w, `task.start`): its
//! `task.started_by_owner` row, written in the frame that opens the task, and
//! its sentence.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart};

use super::{Fact, Say};
use crate::narrative;

/// `task.start` opened a task.
pub struct TaskStartedByOwner<'a> {
    pub short: &'a str,
    pub title: &'a str,
    pub session_id: &'a str,
    /// The conversation it goes under, and carves from.
    pub home_session: &'a str,
    pub limit: theseus_kernel::Micros,
    pub available_before: theseus_kernel::Micros,
    /// How many quoted pieces it carries (each is outside text).
    pub quotes: usize,
    /// Its report goes to the board alone.
    pub board: bool,
    pub starts_at_ms: Option<u64>,
    /// Who asked, and through what.
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for TaskStartedByOwner<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskStartedByOwner);

    fn row(&self) -> Value {
        json!({"task": self.session_id, "short": self.short, "title": self.title,
               "home": self.home_session,
               "limit_usd": theseus_kernel::micros_to_usd(self.limit),
               "available_before_usd": theseus_kernel::micros_to_usd(self.available_before),
               "quotes": self.quotes, "report_to": if self.board { "board" } else { "home" },
               "starts_at_ms": self.starts_at_ms, "by": self.by, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let when = match self.starts_at_ms {
            Some(at) => format!("to start {}", crate::task_start::day_hm(at)),
            None => "to run now".to_string(),
        };
        say.line(
            NarrativePart::Session,
            format!(
                "The owner started task {} (\"{}\") {when}, with no model turn, from {} through \
                 {}: its session {} carves {} from the {} that {} had left, and its report goes \
                 {}{}.",
                self.short,
                self.title,
                self.by,
                self.via,
                narrative::short(self.session_id),
                narrative::dollars(self.limit),
                narrative::dollars(self.available_before),
                narrative::short(self.home_session),
                if self.board {
                    "to the board"
                } else {
                    "to its parent"
                },
                match self.quotes {
                    0 => String::new(),
                    n => format!(
                        "; it holds {} as outside text",
                        narrative::count(n as u64, "quoted piece", "quoted pieces")
                    ),
                }
            ),
        );
    }
}
