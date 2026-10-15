//! Capture (theseus-slvl.5; the personality north star, theseus-slvl): the
//! owner's words become standing records without a command.
//!
//! `directive.v1` rides the inbound request (`judge::inbound`, batched with
//! `classify.v1` and `role.v1`: no call of its own) for the owner's own
//! messages in a place only the owner writes in. Its verdict is acted on here,
//! off the turn's path, by bands (the owner's, 2026-10-13: killer OD5):
//!
//! - **0.80 and up**: kept as a standing record (source `capture:<node>`),
//!   with a notice and an undo. Its scope is the one Jev judged when it is
//!   sure of it, else where the words were said (a Discord place, else the
//!   session).
//! - **0.50 to 0.80**: asked (keep, keep everywhere, or not this).
//! - **below 0.50**: nothing, except a correction said twice in a week
//!   (their words overlap), which is asked even though each was below the band.
//! - **an order is always asked**, whatever its confidence: it changes what
//!   Theseus may do, so no confidence of Jev's keeps one alone.
//! - **said again**: a message that is the same kind as a live record and
//!   shares its words writes `standing.repeated` and no record; the week's
//!   count is on `standing.list`. A non-zero week is a finding.
//!
//! The words kept are the owner's own, as one line (`words`): Jev answers
//! choices, not text. Nothing here widens a record: an undo (`standing.revoke`)
//! ends it and labels the judgment `wrong capture`, which rolls the pack back
//! when it happens three times in a day. `[standing] capture = false`, or the
//! ladder's rollback to shadow, stops all of it; the hand-made records stand.
//!
//! Layout: `words` (the line and the overlap), `state` (what is kept between
//! messages: one META record), this file (the pure decision), `act` (the
//! writes), `expect` (which messages are the owner's own); the owner's
//! answers and the lists are `rpc/capture.rs`.

use theseus_judge::band::Band;
use theseus_judge::client::Answer;
use theseus_judge::{Judgment, Mode};
use theseus_protocol::{StandingKind, StandingRecord, StandingScope};

use crate::places::PlaceClass;

pub mod act;
pub mod expect;
pub mod state;
#[cfg(test)]
mod tests;
pub mod words;

use state::State;

/// The pack, by root name.
pub const PACK: &str = "directive.v1";

/// The key of a kept record's `standing.captured` row: its judgment's label
/// finds its pack there while the judge's sink has not written the judgment's
/// own row (its window is 2 s), as a notice's row serves for a noticed call.
pub fn captured_key(judgment: &str) -> String {
    format!("capture_{judgment}")
}

/// A correction below the asking band is noted from this share of
/// `correction` on, to wait for a second.
pub const CANDIDATE_FLOOR: f64 = 0.15;

/// The one rule for whose words these are: the CLI and the web UI (no place)
/// and a DM with an owner (a DM's class is private only for an owner's). A
/// guild channel bound private is not one: anyone it lets in writes there.
pub fn owner_alone(class: PlaceClass, place: Option<&str>) -> bool {
    class == PlaceClass::Private && place.is_none_or(|p| p.starts_with("discord:dm:"))
}

/// What the owner said, as `directive.v1`'s `kind` named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    Nothing,
    Order,
    Preference,
    Fact,
    Correction,
}

impl Heard {
    fn parse(choice: &str) -> Heard {
        match choice {
            "order" => Heard::Order,
            "preference" => Heard::Preference,
            "fact" => Heard::Fact,
            "correction" => Heard::Correction,
            _ => Heard::Nothing,
        }
    }

    /// The record it becomes: a correction is a preference the owner states
    /// by the way of a complaint.
    pub fn record_kind(self) -> Option<StandingKind> {
        match self {
            Heard::Nothing => None,
            Heard::Order => Some(StandingKind::Order),
            Heard::Preference | Heard::Correction => Some(StandingKind::Preference),
            Heard::Fact => Some(StandingKind::Fact),
        }
    }
}

/// Where Jev said it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    Session,
    Place,
    Everywhere,
    Unsure,
}

/// `directive.v1`'s judgment, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub heard: Heard,
    pub band: Band,
    pub confidence: f64,
    /// The probability Jev gave `correction`, whatever it chose.
    pub p_correction: f64,
    pub said: Said,
    /// The scope answer reached the act band.
    pub sure_of_scope: bool,
    pub judgment: String,
    /// The pack version that judged (`directive.v1`, or a learned one).
    pub pack: String,
}

impl Verdict {
    /// The verdict a judgment holds, when it acts: answered by the pinned
    /// model, live (or a canary's acting arm), and a `kind` answer.
    pub fn of(j: &Judgment) -> Option<Verdict> {
        if !j.actionable() || !matches!(j.mode, Mode::Live | Mode::Canary) {
            return None;
        }
        let kind = j.answer("kind")?;
        let Answer::Choice {
            choice,
            probabilities,
            confidence,
        } = &kind.answer
        else {
            return None;
        };
        let p_correction = probabilities
            .iter()
            .find(|(o, _)| o == "correction")
            .map_or(0.0, |(_, p)| *p);
        let (said, sure_of_scope) = match j.answer("scope") {
            Some(a) => match &a.answer {
                Answer::Choice { choice, .. } => (
                    match choice.as_str() {
                        "session" => Said::Session,
                        "place" => Said::Place,
                        "everywhere" => Said::Everywhere,
                        _ => Said::Unsure,
                    },
                    a.band.band == Band::Act,
                ),
                _ => (Said::Unsure, false),
            },
            None => (Said::Unsure, false),
        };
        Some(Verdict {
            heard: Heard::parse(choice),
            band: kind.band.band,
            confidence: *confidence,
            p_correction,
            said,
            sure_of_scope,
            judgment: j.id.clone(),
            pack: j.pack.clone(),
        })
    }
}

/// What the core does about a verdict.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Nothing,
    /// Keep it, with a notice and an undo.
    Keep(StandingKind),
    /// Ask the owner: `why` is `unsure`, `order` or `twice`.
    Ask {
        kind: StandingKind,
        why: &'static str,
    },
    /// A correction below the band, noted to wait for a second.
    Candidate,
    /// The owner said a live record again.
    Repeat(String),
}

/// The decision, pure: the verdict, the owner's line, the records live in the
/// turn's scope, and what capture holds. `node` is the message's own node,
/// which no candidate of its own can make twice.
pub fn decide(
    v: &Verdict,
    line: &str,
    node: &str,
    live: &[&StandingRecord],
    state: &State,
    now: u64,
) -> Plan {
    let kind = v.heard.record_kind();
    if let Some(k) = kind.filter(|_| v.band != Band::Escalate) {
        // Said again: the same kind of record as one the turn already holds.
        if let Some(r) = live
            .iter()
            .find(|r| r.kind == k && words::same(&r.text, line))
        {
            return Plan::Repeat(r.id.clone());
        }
        // Already waiting on the owner: ask once.
        if state
            .asks
            .iter()
            .any(|o| o.ask.kind == k && words::same(&o.ask.text, line))
        {
            return Plan::Nothing;
        }
        return match (k, v.band) {
            (StandingKind::Order, _) => Plan::Ask {
                kind: k,
                why: "order",
            },
            (_, Band::Act) => Plan::Keep(k),
            _ => Plan::Ask {
                kind: k,
                why: "unsure",
            },
        };
    }
    // Below the band, only a correction counts, and only if it is said twice.
    let strength = match v.heard {
        Heard::Correction => v.confidence,
        _ => v.p_correction,
    };
    if strength < CANDIDATE_FLOOR {
        return Plan::Nothing;
    }
    let twice = state.candidates.iter().any(|c| {
        c.node != node
            && now.saturating_sub(c.at_ms) <= state::WEEK_MS
            && words::same(&c.line, line)
    });
    match twice {
        true => Plan::Ask {
            kind: StandingKind::Preference,
            why: "twice",
        },
        false => Plan::Candidate,
    }
}

/// Where a kept or asked record holds: Jev's scope when it is sure of it,
/// else where the words were said (a Discord place, else the session).
pub fn scope_of(v: &Verdict, session: &str, place: Option<&str>) -> StandingScope {
    let here = match place {
        Some(p) if p.starts_with("discord:") => StandingScope::Place(p.to_string()),
        _ => StandingScope::Session(session.to_string()),
    };
    match (v.sure_of_scope, v.said) {
        (true, Said::Everywhere) => StandingScope::Everywhere,
        (true, Said::Session) => StandingScope::Session(session.to_string()),
        _ => here,
    }
}

/// The surface's name in the notice, from the inbound state's `place_kind`
/// (`cli (private)`).
pub fn surface_name(place_kind: &str) -> &'static str {
    match place_kind.split_whitespace().next() {
        Some("web") => "web",
        Some(s) if s.starts_with("discord") => "Discord",
        _ => "terminal",
    }
}

fn scope_words(scope: &StandingScope) -> &'static str {
    match scope {
        StandingScope::Everywhere => "everywhere",
        StandingScope::Place(_) => "in this place",
        StandingScope::Session(_) => "in this session",
        StandingScope::Surface(_) => "on this surface",
        StandingScope::Persona(_) => "for this persona",
    }
}

/// The notice for a kept record.
pub fn kept_line(kind: StandingKind, scope: &StandingScope, surface: &str, line: &str) -> String {
    format!(
        "📌 Kept as a standing {} {} ({surface}): {line}",
        kind.name(),
        scope_words(scope)
    )
}

/// The question for an ask.
pub fn ask_line(kind: StandingKind, scope: &StandingScope, why: &str, line: &str) -> String {
    let lead = match why {
        "order" => format!(
            "📌 Keep as a standing order {}? An order changes what I may do, so I always ask.",
            scope_words(scope)
        ),
        "twice" => format!(
            "📌 You have said this twice this week. Keep it as a standing {} {}?",
            kind.name(),
            scope_words(scope)
        ),
        _ => format!(
            "📌 Keep as a standing {} {}?",
            kind.name(),
            scope_words(scope)
        ),
    };
    format!("{lead} “{line}”")
}
