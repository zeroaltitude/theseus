//! The start's facts: what a start found that the last run left
//! (`theseusd`'s `after_serving`).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};
use crate::narrative;

/// The last run crashed (Review 2's consideration 1): its panic hook left a
/// crash file, which this start took into `crashes/`. The message stays in
/// the file: a panic's message can quote any text the daemon held.
pub struct CrashFound<'a> {
    pub crash: &'a crate::crash::Crash,
    /// Where the file went.
    pub file: &'a str,
}

impl Fact for CrashFound<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ServerCrashed);

    fn row(&self) -> Value {
        let c = self.crash;
        json!({"at_unix_ms": c.at_unix_ms, "pid": c.pid, "version": c.version, "mode": c.mode,
               "thread": c.thread, "location": c.location, "file": self.file})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let c = self.crash;
        say.line(
            Session,
            format!(
                "The last run crashed {} ago: a panic on thread {} at {}. Its crash file is {}.",
                narrative::duration(theseus_protocol::now_unix_ms().saturating_sub(c.at_unix_ms)),
                c.thread,
                c.location,
                self.file
            ),
        );
    }
}
