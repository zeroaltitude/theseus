//! The owner's corrections of routing (theseus-q31l): "that should have been
//! on fable", a reaction on a reply, or a press of a route footer's control.
//!
//! - **The owner's alone.** Words count only in a private place (the place
//!   rule), from the owner's own message: a turn of a task, a message a job
//!   sent (`opened_from`), or one through the MCP server is a message, never
//!   a correction. A reaction, a press and the CLI go through `route.correct`,
//!   judged as `judge.label` is (`judge_act`: the owner, from a private
//!   place). No tool writes one: the model can neither label nor correct.
//! - **A label.** A correction of a turn whose route pack judged it writes
//!   the owner's label on that judgment (`not:<mode>` on the `mode` question,
//!   when the place the owner names is not one the mode leads to), with the
//!   correction named in its note, and a `route.corrected` row (scoped
//!   [`SCOPE`]) that keeps its provenance: the owner's message, reaction or
//!   press, by id. Both in one frame. The nightly loop reads the label as it
//!   reads any operator label.
//! - **At once.** The session runs where the owner said from the next turn
//!   (words: from the correcting message's own turn, so "rerun that on opus"
//!   answers there), through routing's switch of the session's `routed`
//!   profile, recorded on `route.decided` as `source: correction`.
//! - **The layer** ([`layer`]): a later message close to a corrected one runs
//!   where the owner said, ahead of the verdict, until its pack version no
//!   longer acts. In memory, bounded, rebuilt after serving from
//!   [`SCOPE`]'s rows.

pub mod layer;
pub mod words;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde_json::{json, Value};

pub use layer::{Entry, Layer, Steer};
pub use words::{Said, To};

use crate::config::routing::CorrectionsConfig;
use crate::config::RoutingConfig;
use crate::routing::{pick, Picked, Profiles};

/// The `route.corrected` rows' scope: the layer's truth.
pub const SCOPE: &str = "route.corrections";
/// The routed turns kept for a correction, a reaction or a press to find.
pub const RECENT: usize = 256;
/// The question every route pack version asks of the mode.
pub const QUESTION: &str = "mode";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A turn routing decided, as a correction finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedTurn {
    pub session: String,
    pub turn: String,
    /// The route judgment the turn read, when it read one.
    pub judgment: Option<String>,
    /// The route pack version asked for it.
    pub pack: String,
    /// The mode the verdict answered.
    pub mode: Option<String>,
    /// The session's profile before routing.
    pub from: String,
    /// Where it ran.
    pub profile: String,
    /// The message's content words ([`layer::words`]).
    pub words: Vec<String>,
}

/// Who steers a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// The owner's correction itself: a switch at once.
    Owner,
    /// A layer's entry, close to the message: the cache's rule holds.
    Layer,
}

/// Where a turn runs ahead of the verdict, and what it follows.
#[derive(Debug, Clone, PartialEq)]
pub struct Steering {
    pub steer: Steer,
    /// The owner's label it follows, else the correction.
    pub follows: String,
    pub by: By,
}

/// A correction resolved against the turn it corrects.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The profile the session moves to, when there is one to move to.
    pub profile: Option<String>,
    /// The mode the owner named.
    pub mode: Option<String>,
    pub steer: Option<Steer>,
    /// The label's words for `judge.label`'s check (`not:<mode>`).
    pub label: Option<String>,
    /// What the owner said, as a line names it.
    pub to: String,
}

/// What a correction may name in words: every configured profile with its
/// model, and the acting route pack's modes.
pub fn names(cfg: &crate::Config, pack: Option<&theseus_judge::Pack>) -> words::Names {
    let modes = pack
        .and_then(|p| p.question(QUESTION))
        .map(|q| q.options.iter().map(|o| o.id.clone()).collect())
        .unwrap_or_default();
    words::Names {
        profiles: cfg
            .all_profiles()
            .iter()
            .map(|(n, p)| (n.clone(), p.model.clone()))
            .collect(),
        modes,
    }
}

/// Read a correction's target from a protocol word: a profile, a mode,
/// `stronger` (`up`), or `cheaper` (`down`).
pub fn to_of(word: &str, names: &words::Names) -> Option<To> {
    let w = word.trim().to_lowercase();
    match w.as_str() {
        "stronger" | "up" | "more" => return Some(To::Stronger),
        "cheaper" | "down" | "less" => return Some(To::Cheaper),
        _ => {}
    }
    if let Some((p, _)) = names
        .profiles
        .iter()
        .find(|(p, m)| p.to_lowercase() == w || m.to_lowercase() == w)
    {
        return Some(To::Profile(p.clone()));
    }
    names
        .modes
        .iter()
        .find(|m| m.to_lowercase() == w)
        .map(|m| To::Mode(m.clone()))
}

/// The correction, against the turn it corrects: where the session moves,
/// and the label, which says the verdict's mode was wrong only when the
/// place the owner names is not one that mode leads to.
pub fn resolve(to: &To, last: &RoutedTurn, cfg: &RoutingConfig, profiles: &Profiles) -> Resolved {
    let not = |leads: bool| {
        last.mode
            .as_ref()
            .filter(|_| !leads)
            .map(|m| format!("not:{m}"))
    };
    match to {
        To::Profile(p) => {
            let leads = last
                .mode
                .as_ref()
                .is_some_and(|m| cfg.modes.of(m).contains(p));
            Resolved {
                profile: Some(p.clone()),
                mode: None,
                steer: Some(Steer::Profile(p.clone())),
                label: not(leads),
                to: p.clone(),
            }
        }
        To::Mode(m) => {
            let profile = match pick(cfg.modes.of(m), profiles, false, None) {
                Picked::Profile(p) | Picked::Capped(Some(p)) => p,
                _ => last.from.clone(),
            };
            Resolved {
                profile: Some(profile),
                mode: Some(m.clone()),
                steer: Some(Steer::Mode(m.clone())),
                label: not(last.mode.as_deref() == Some(m.as_str())),
                to: format!("mode {m}"),
            }
        }
        To::Stronger | To::Cheaper => {
            let up = *to == To::Stronger;
            let profile = next(profiles, &last.profile, up);
            Resolved {
                steer: profile.clone().map(Steer::Profile),
                profile,
                mode: None,
                label: not(false),
                to: if up { "stronger" } else { "cheaper" }.into(),
            }
        }
    }
}

/// The usable profile next dearer (`up`) or next cheaper than `from` for a
/// short turn at catalog prices; none past either end.
pub fn next(profiles: &Profiles, from: &str, up: bool) -> Option<String> {
    let cost = profiles.get(from).and_then(|p| p.short_cost)?;
    let usable = profiles
        .iter()
        .filter(|(n, p)| p.unusable.is_none() && n.as_str() != from)
        .filter_map(|(n, p)| p.short_cost.map(|c| (n, c)));
    let pick = |(n, c): (&String, f64)| (c, n.clone());
    let mut found: Vec<(f64, String)> = match up {
        true => usable
            .filter(|(_, c)| *c > cost + 1e-12)
            .map(pick)
            .collect(),
        false => usable
            .filter(|(_, c)| *c < cost - 1e-12)
            .map(pick)
            .collect(),
    };
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    match up {
        true => found.into_iter().next(),
        false => found.into_iter().next_back(),
    }
    .map(|(_, n)| n)
}

/// The corrections the daemon holds: the turns routing decided lately, the
/// words a `turn.submit` read, the steers waiting for a session's next turn,
/// and the layer.
#[derive(Default)]
pub struct Corrections {
    recent: Mutex<VecDeque<RoutedTurn>>,
    said: Mutex<HashMap<String, (Said, String)>>,
    pending: Mutex<HashMap<String, Steering>>,
    layer: Mutex<Layer>,
    warmed: AtomicBool,
}

impl Corrections {
    /// A turn routing decided.
    pub fn noted(&self, t: RoutedTurn) {
        let mut r = lock(&self.recent);
        r.push_back(t);
        while r.len() > RECENT {
            r.pop_front();
        }
    }

    /// The session's last turn routing decided.
    pub fn last_of(&self, session: &str) -> Option<RoutedTurn> {
        lock(&self.recent)
            .iter()
            .rev()
            .find(|t| t.session == session)
            .cloned()
    }

    /// A turn routing decided, by id.
    pub fn turn(&self, turn: &str) -> Option<RoutedTurn> {
        lock(&self.recent)
            .iter()
            .rev()
            .find(|t| t.turn == turn)
            .cloned()
    }

    /// Words `turn.submit` read as a correction, for the turn of `session`
    /// whose input is `text`: its inbound point takes them, and acts on them
    /// only in a private place.
    pub fn expect(&self, session: &str, said: Said, text: &str) {
        lock(&self.said).insert(session.to_string(), (said, text.to_string()));
    }

    /// The words read for this input, taken.
    pub fn take_said(&self, session: &str, text: &str) -> Option<Said> {
        let mut m = lock(&self.said);
        match m.get(session) {
            Some((_, t)) if t == text => m.remove(session).map(|(s, _)| s),
            _ => None,
        }
    }

    /// A correction for the session's next turn (a reaction, a press, the CLI).
    pub fn set_pending(&self, session: &str, s: Steering) {
        lock(&self.pending).insert(session.to_string(), s);
    }

    pub fn take_pending(&self, session: &str) -> Option<Steering> {
        lock(&self.pending).remove(session)
    }

    /// An entry, into the layer.
    pub fn add(&self, e: Entry, cfg: &CorrectionsConfig) {
        lock(&self.layer).add(e, cfg.max_entries as usize);
    }

    /// The entry closest to a message's `words`, under `acting`.
    pub fn nearest(
        &self,
        words: &[String],
        acting: &str,
        cfg: &CorrectionsConfig,
    ) -> Option<(Entry, f64)> {
        if !cfg.enabled || words.is_empty() {
            return None;
        }
        lock(&self.layer).nearest(words, acting, cfg.similarity)
    }

    /// The layer's entries under `acting`, newest first, and how many retired.
    pub fn list(&self, acting: &str) -> (Vec<Entry>, u64) {
        let mut l = lock(&self.layer);
        l.retire(acting);
        let mut out: Vec<Entry> = l.entries().cloned().collect();
        out.reverse();
        (out, l.retired)
    }

    /// The layer rebuilt from [`SCOPE`]'s rows, once (after serving, never
    /// on the start path): the newest `max_entries` of the acting pack
    /// version, oldest first. Later ones were added as they were written.
    pub fn warm(&self, store: &crate::store::Store, acting: &str, cfg: &CorrectionsConfig) {
        if self.warmed.swap(true, Ordering::SeqCst) {
            return;
        }
        let rows = match store.scope_after(SCOPE, 0) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "corrections: the layer was not read");
                return;
            }
        };
        let read: Vec<Entry> = rows
            .iter()
            .filter_map(|r| r.decode::<crate::ledger::LedgerRow>().ok())
            .filter(|r| r.kind == theseus_protocol::LedgerKind::RouteCorrected.as_str())
            .filter_map(|r| entry_of(&r.data))
            .filter(|e| e.pack == acting)
            .collect();
        let mut l = lock(&self.layer);
        let have: Vec<Entry> = l.entries().cloned().collect();
        let mut fresh = Layer::default();
        for e in read.into_iter().chain(have) {
            fresh.add(e, cfg.max_entries as usize);
        }
        fresh.retired = l.retired;
        *l = fresh;
    }
}

/// A `route.corrected` row's entry, when it steers.
pub fn entry_of(d: &Value) -> Option<Entry> {
    let s = |k: &str| d[k].as_str().map(str::to_string);
    let steer: Steer = serde_json::from_value(d["steer"].clone()).ok()?;
    Some(Entry {
        id: s("id")?,
        label: s("label"),
        pack: s("pack")?,
        session: s("session").unwrap_or_default(),
        turn: s("turn")?,
        words: serde_json::from_value(d["words"].clone()).unwrap_or_default(),
        steer,
        at_ms: d["at_ms"].as_u64().unwrap_or(0),
    })
}

/// A steer's words, as a line or a listing says it.
pub fn steer_line(s: &Steer) -> String {
    match s {
        Steer::Mode(m) => format!("mode {m}"),
        Steer::Profile(p) => p.clone(),
    }
}

/// A row's `steer`, as stored.
pub fn steer_json(s: Option<&Steer>) -> Value {
    s.map_or(Value::Null, |s| json!(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Profile;

    fn profiles() -> Profiles {
        let mut out = Profiles::new();
        for (n, cost) in [
            ("haiku", 0.001),
            ("sonnet", 0.01),
            ("opus", 0.05),
            ("fable", 0.08),
        ] {
            out.insert(
                n.into(),
                Profile {
                    provider: "anthropic".into(),
                    model: format!("claude-{n}"),
                    unusable: None,
                    short_cost: Some(cost),
                    vision: true,
                },
            );
        }
        out
    }

    fn last(mode: Option<&str>, profile: &str) -> RoutedTurn {
        RoutedTurn {
            session: "ses_a".into(),
            turn: "turn_1".into(),
            judgment: Some("jdg_1".into()),
            pack: "route.v2".into(),
            mode: mode.map(str::to_string),
            from: "sonnet".into(),
            profile: profile.into(),
            words: vec![],
        }
    }

    /// The label says the mode was wrong only when the place the owner
    /// names is not one the mode leads to.
    #[test]
    fn a_correction_labels_the_mode_only_when_the_mode_was_wrong() {
        let cfg = RoutingConfig::default();
        let ps = profiles();
        let r = resolve(
            &To::Profile("fable".into()),
            &last(Some("chat"), "sonnet"),
            &cfg,
            &ps,
        );
        assert_eq!(r.label.as_deref(), Some("not:chat"));
        assert_eq!(r.steer, Some(Steer::Profile("fable".into())));
        // sophisticated leads to opus, then fable: the mode was right.
        let r = resolve(
            &To::Profile("fable".into()),
            &last(Some("sophisticated"), "opus"),
            &cfg,
            &ps,
        );
        assert_eq!(r.label, None);
        let r = resolve(
            &To::Mode("deep_coding".into()),
            &last(Some("quick"), "haiku"),
            &cfg,
            &ps,
        );
        assert_eq!(
            (r.label.as_deref(), r.profile.as_deref()),
            (Some("not:quick"), Some("opus"))
        );
        // No verdict, no label.
        let r = resolve(&To::Stronger, &last(None, "sonnet"), &cfg, &ps);
        assert_eq!((r.label, r.profile.as_deref()), (None, Some("opus")));
    }

    #[test]
    fn stronger_and_cheaper_are_the_next_usable_profile_each_way() {
        let mut ps = profiles();
        assert_eq!(next(&ps, "sonnet", true).as_deref(), Some("opus"));
        assert_eq!(next(&ps, "sonnet", false).as_deref(), Some("haiku"));
        assert_eq!(next(&ps, "fable", true), None);
        assert_eq!(next(&ps, "haiku", false), None);
        ps.get_mut("opus").unwrap().unusable = Some("key");
        assert_eq!(next(&ps, "sonnet", true).as_deref(), Some("fable"));
    }

    #[test]
    fn a_protocol_word_names_a_profile_a_mode_or_a_direction() {
        let n = words::Names {
            profiles: vec![("fable".into(), "claude-fable-5-1".into())],
            modes: vec!["deep_coding".into()],
        };
        assert_eq!(to_of("Fable", &n), Some(To::Profile("fable".into())));
        assert_eq!(
            to_of("claude-fable-5-1", &n),
            Some(To::Profile("fable".into()))
        );
        assert_eq!(
            to_of("deep_coding", &n),
            Some(To::Mode("deep_coding".into()))
        );
        assert_eq!(to_of("up", &n), Some(To::Stronger));
        assert_eq!(to_of("cheaper", &n), Some(To::Cheaper));
        assert_eq!(to_of("mars", &n), None);
    }

    /// The words a submit read reach only the turn of that input.
    #[test]
    fn the_words_read_reach_only_their_own_input() {
        let c = Corrections::default();
        let said = Said {
            to: To::Stronger,
            rerun: false,
        };
        c.expect("ses_a", said.clone(), "that needed more effort");
        assert_eq!(c.take_said("ses_a", "something else"), None);
        assert_eq!(c.take_said("ses_a", "that needed more effort"), Some(said));
        assert_eq!(c.take_said("ses_a", "that needed more effort"), None);
    }

    #[test]
    fn the_recent_turns_are_bounded_and_found_by_session_and_id() {
        let c = Corrections::default();
        for i in 0..(RECENT + 3) {
            let mut t = last(Some("chat"), "sonnet");
            t.turn = format!("turn_{i}");
            t.session = format!("ses_{}", i % 2);
            c.noted(t);
        }
        assert_eq!(lock(&c.recent).len(), RECENT);
        assert_eq!(
            c.last_of("ses_0").unwrap().turn,
            format!("turn_{}", RECENT + 2)
        );
        assert!(c.turn("turn_0").is_none(), "the oldest out");
        assert!(c.turn("turn_10").is_some());
    }
}
