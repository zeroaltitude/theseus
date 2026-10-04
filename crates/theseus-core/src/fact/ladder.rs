//! The ladder's facts (M5 26a; design §2.7, §2.13): a pack version's mode
//! changing (`pack.mode`), and an event its rollback rules count
//! (`pack.event`). Each row is scoped per pack id by its writer
//! (`judge::ladder`), so a first read is a few rows: the modes in
//! `pack:<id>`, a day's events in `pack.event:<id>:<day>`. A rollback is a
//! notice on the owner's surfaces as the judge's other notices are: its
//! sentence in the narrative's session part.

use serde_json::Value;
use theseus_judge::learn::CanaryEvent;
use theseus_protocol::packs::PackModeRow;
use theseus_protocol::{LedgerKind, NarrativePart};

use super::{Fact, Say};

/// A pack version's mode moved, or a card for one was declined (a row that
/// changes nothing, `declined`).
pub struct PackModeSet<'a> {
    pub row: &'a PackModeRow,
}

/// A row's data: the row without where it landed (the record says that).
pub fn data(row: &PackModeRow) -> Value {
    let mut v = serde_json::to_value(row).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("position");
        o.remove("at_unix_ms");
    }
    v
}

/// "until 00:00" for a brake that lapses at a local midnight.
pub fn until_words(until_ms: Option<u64>) -> String {
    until_ms.map_or_else(String::new, |ms| {
        let l = crate::wake::local(ms);
        format!(" until {:02}:{:02}", l.hour, l.minute)
    })
}

/// How a mode reads in a sentence: `canary 0.2`, `rolled back`.
pub fn mode_words(mode: &str, share: Option<f64>) -> String {
    match (mode, share) {
        ("canary", Some(s)) => format!("canary {s:?}"),
        ("rolled_back", _) => "rolled back".into(),
        (m, _) => m.into(),
    }
}

impl Fact for PackModeSet<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PackMode);

    fn row(&self) -> Value {
        data(self.row)
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.row;
        let to = mode_words(&r.mode, r.share);
        let line = if r.declined {
            format!(
                "{}'s promotion to {to} was not approved ({}); it stays {}.",
                r.pack,
                r.why,
                mode_words(&r.from, None)
            )
        } else if r.mode == "rolled_back" {
            format!(
                "{} rolled back{} ({}): {}. It records in shadow and acts on nothing.",
                r.pack,
                until_words(r.until_ms),
                r.rule.as_deref().unwrap_or(r.who.as_str()),
                r.words.as_deref().unwrap_or(r.why.as_str())
            )
        } else {
            let forced = match (&r.forced, &r.numbers) {
                (true, Some(n)) => format!(", forced short of the bar: {n}"),
                (true, None) => ", forced".into(),
                _ => String::new(),
            };
            format!(
                "{} is {to}, was {} ({}: {}{forced}).",
                r.pack,
                mode_words(&r.from, None),
                r.who,
                r.why
            )
        };
        say.line(NarrativePart::Session, line);
    }
}

/// An event a pack's rollback rules count, kept so a restart reads the
/// day's count back.
pub struct PackEventLanded<'a> {
    pub pack: &'a str,
    pub event: &'a CanaryEvent,
}

impl Fact for PackEventLanded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PackEvent);

    fn row(&self) -> Value {
        serde_json::json!({"pack": self.pack, "event": self.event})
    }
}
