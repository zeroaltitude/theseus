//! The owner's corrections of routing (theseus-q31l): a reaction on a reply,
//! a press of a route footer's control, or `theseus judge correct`. All three
//! go through `route.correct`; nothing reads the owner's words for one (his
//! ruling of 2026-11-14: an act on what a person says is a tool or a slash
//! command, never a match of the message's words).
//!
//! - **The owner's alone.** `route.correct` is judged as `judge.label` is
//!   (`judge_act`: the owner, from a private place). No tool writes one: the
//!   model can neither label nor correct.
//! - **A label.** A correction of a turn whose route pack judged it writes
//!   the owner's label on that judgment (`not:<mode>` on the `mode` question,
//!   when the place the owner names is not one the mode leads to), with the
//!   correction named in its note, and a `route.corrected` row (scoped
//!   [`SCOPE`]) that keeps its provenance: the reaction or press, by id. Both
//!   in one frame. The nightly loop reads the label as it reads any operator
//!   label.
//! - **At the next turn.** The session runs where the owner said from its
//!   next turn, through routing's switch of the session's `routed` profile,
//!   recorded on `route.decided` as `source: correction`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::routing::Profiles;

/// Where a correction steers a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Steer {
    /// The profile itself.
    Profile(String),
    /// The persona (a `[personas]` key) that should answer (route.v5,
    /// theseus-c1uz).
    Persona(String),
}

/// Where the owner says the turn should have run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum To {
    /// A configured profile, by name.
    Profile(String),
    /// A dearer profile than the one it ran on.
    Stronger,
    /// A cheaper one.
    Cheaper,
    /// A persona that should have answered, by its `[personas]` key
    /// (route.v5, theseus-c1uz).
    Persona(String),
}

/// What a correction may name.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// Each configured profile, by name, with its model's id.
    pub profiles: Vec<(String, String)>,
    /// Each configured persona: its key and what it is called.
    pub personas: Vec<(String, String)>,
}

/// The `route.corrected` rows' scope.
pub const SCOPE: &str = "route.corrections";
/// The routed turns kept for a correction, a reaction or a press to find.
pub const RECENT: usize = 256;
/// The question the route pack asks of the model (route.v10, theseus-h66v4;
/// the packs before it asked `mode`, retired with the class table in row 6): a
/// correction's label says the pick was wrong (`not:<option>`).
pub const QUESTION: &str = "model";

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
    /// The verdict carried a `persona` answer (route.v5): a persona
    /// correction labels it.
    pub persona_asked: bool,
}

/// Where a turn runs ahead of the verdict, and what it follows.
#[derive(Debug, Clone, PartialEq)]
pub struct Steering {
    pub steer: Steer,
    /// The owner's label it follows, else the correction.
    pub follows: String,
}

/// A correction resolved against the turn it corrects.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The profile the session moves to, when there is one to move to.
    pub profile: Option<String>,
    pub steer: Option<Steer>,
    /// The question the label is on: `mode`, or `persona` (theseus-c1uz).
    pub question: &'static str,
    /// The label's words for `judge.label`'s check (`not:<mode>`).
    pub label: Option<String>,
    /// What the owner said, as a line names it.
    pub to: String,
}

/// What a correction may name in words: every configured profile with its
/// model, and each persona.
pub fn names(cfg: &crate::Config, book: &crate::personas::Book) -> Names {
    Names {
        profiles: cfg
            .all_profiles()
            .iter()
            .map(|(n, p)| (n.clone(), p.model.clone()))
            .collect(),
        personas: book
            .live()
            .map(|(k, v)| (k.clone(), v.name.clone()))
            .collect(),
    }
}

/// Read a correction's target from a protocol word: a profile,
/// `stronger` (`up`), or `cheaper` (`down`).
pub fn to_of(word: &str, names: &Names) -> Option<To> {
    let w = word.trim().to_lowercase();
    match w.as_str() {
        "stronger" | "up" | "more" => return Some(To::Stronger),
        "cheaper" | "down" | "less" => return Some(To::Cheaper),
        _ => {}
    }
    if let Some(who) = w.strip_prefix("persona:") {
        let who = who.trim();
        return names
            .personas
            .iter()
            .find(|(k, n)| k.to_lowercase() == who || n.to_lowercase() == who)
            .map(|(k, _)| To::Persona(k.clone()));
    }
    if let Some((p, _)) = names
        .profiles
        .iter()
        .find(|(p, m)| p.to_lowercase() == w || m.to_lowercase() == w)
    {
        return Some(To::Profile(p.clone()));
    }
    None
}

/// The correction, against the turn it corrects: where the session moves,
/// and the label, which says the verdict's pick was wrong only when the
/// profile the owner names is not the one the turn ran on.
pub fn resolve(to: &To, last: &RoutedTurn, profiles: &Profiles) -> Resolved {
    let not = |leads: bool| {
        last.mode
            .as_ref()
            .filter(|_| !leads)
            .map(|m| format!("not:{m}"))
    };
    match to {
        To::Persona(k) => Resolved {
            profile: None,
            steer: Some(Steer::Persona(k.clone())),
            question: "persona",
            label: Some(k.clone()),
            to: format!("persona {k}"),
        },
        To::Profile(p) => {
            let leads = last.profile == *p;
            Resolved {
                profile: Some(p.clone()),
                steer: Some(Steer::Profile(p.clone())),
                question: QUESTION,
                label: not(leads),
                to: p.clone(),
            }
        }
        To::Stronger | To::Cheaper => {
            let up = *to == To::Stronger;
            let profile = next(profiles, &last.profile, up);
            Resolved {
                steer: profile.clone().map(Steer::Profile),
                profile,
                question: QUESTION,
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

/// The corrections the daemon holds: the turns routing decided lately and
/// the steers waiting for a session's next turn.
#[derive(Default)]
pub struct Corrections {
    recent: Mutex<VecDeque<RoutedTurn>>,
    pending: Mutex<HashMap<String, Steering>>,
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

    /// A correction for the session's next turn (a reaction, a press, the CLI).
    pub fn set_pending(&self, session: &str, s: Steering) {
        lock(&self.pending).insert(session.to_string(), s);
    }

    pub fn take_pending(&self, session: &str) -> Option<Steering> {
        lock(&self.pending).remove(session)
    }

    /// The session's pending steer, taken only when `want` says so.
    pub fn take_pending_if(
        &self,
        session: &str,
        want: impl Fn(&Steering) -> bool,
    ) -> Option<Steering> {
        let mut m = lock(&self.pending);
        match m.get(session) {
            Some(s) if want(s) => m.remove(session),
            _ => None,
        }
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
            persona_asked: false,
        }
    }

    /// The label says the pick was wrong only when the profile the owner
    /// names is not the one the turn ran on.
    #[test]
    fn a_correction_labels_the_mode_only_when_the_mode_was_wrong() {
        let ps = profiles();
        let r = resolve(
            &To::Profile("fable".into()),
            &last(Some("chat"), "sonnet"),
            &ps,
        );
        assert_eq!(r.label.as_deref(), Some("not:chat"));
        assert_eq!(r.steer, Some(Steer::Profile("fable".into())));
        // It ran on opus, and the owner names opus: the pick was right.
        let r = resolve(
            &To::Profile("opus".into()),
            &last(Some("opus"), "opus"),
            &ps,
        );
        assert_eq!(r.label, None);
        // No verdict, no label.
        let r = resolve(&To::Stronger, &last(None, "sonnet"), &ps);
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
    fn a_protocol_word_names_a_profile_or_a_direction() {
        let n = Names {
            profiles: vec![("fable".into(), "claude-fable-5-2".into())],
            personas: vec![("birch".into(), "Birch".into())],
        };
        assert_eq!(to_of("Fable", &n), Some(To::Profile("fable".into())));
        assert_eq!(
            to_of("claude-fable-5-2", &n),
            Some(To::Profile("fable".into()))
        );
        assert_eq!(to_of("deep_coding", &n), None, "a mode is no name");
        assert_eq!(to_of("up", &n), Some(To::Stronger));
        assert_eq!(to_of("cheaper", &n), Some(To::Cheaper));
        assert_eq!(to_of("mars", &n), None);
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
