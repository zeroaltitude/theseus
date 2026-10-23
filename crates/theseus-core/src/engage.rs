//! Which persona answers a message (theseus-c1uz, T10a): the pure rules.
//!
//! A persona is a per-message verdict, not a property of a thread (the
//! owner, 2026-10-09). With two or more `[personas]`:
//!
//! 1. **Explicit address wins at confidence 1**, ahead of Jev: a name at the
//!    message's start followed by a comma or colon, `@name` anywhere, or
//!    `/as name`. Jev is still asked, in shadow, so the label is free.
//! 2. Else Jev's `persona` answer (route.v5): at the pack's `act` bar the
//!    persona is engaged silently, from `confirm` to `act` engaged with a
//!    visible why, below `confirm`, or when the verdict is late or absent,
//!    the prior stands. A persona never asks.
//! 3. The prior is the persona that answered the session's last turn, else
//!    the session's own (`TurnRunner::persona_of`).
//!
//! The engaged persona is named in one tail line beside the new message
//! (`line`), so a switch changes no byte of the cached prefix.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use theseus_judge::band::Band;

use crate::config::PersonaView;
use crate::personas::Book;

/// One persona a message may be answered as.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The `[personas]` key; none for the built-in default.
    pub key: Option<String>,
    pub name: String,
    pub card: Option<String>,
    /// What Jev's `none` engages: the default persona, else the built-in.
    pub is_default: bool,
}

impl Choice {
    pub fn of(view: &PersonaView) -> Self {
        Self {
            key: view.key.clone(),
            name: view.name.clone(),
            card: view.card.clone(),
            is_default: false,
        }
    }

    /// The built-in default, which answers where no persona is the default.
    pub fn built_in() -> Self {
        Self {
            is_default: true,
            ..Self::of(&PersonaView::built_in())
        }
    }
}

/// The route option that engages the default persona (route.v7's persona
/// question has `prior` as its one static option; this one is added with the
/// personas, theseus-utfnl).
pub const NONE: &str = "none";

/// The card `none` shows when the default persona has none.
pub const NONE_CARD: &str =
    "No specialist: the general assistant, for anything the other cards do not cover.";

/// The personas a place may be answered as: every live one in a private
/// place; in a shared place its own and those marked `shareable`. The
/// default persona is marked, and the built-in default is among them only
/// where no persona is the default, so `none` always has one to engage.
pub fn choices(book: &Book, shared: bool, own: Option<&str>) -> Vec<Choice> {
    let default = book.default_live();
    let mut out: Vec<Choice> = book
        .live()
        .filter(|(k, v)| !shared || v.shareable || own == Some(k.as_str()))
        .map(|(k, v)| Choice {
            is_default: default == Some(k.as_str()),
            ..Choice::of(v)
        })
        .collect();
    if default.is_none() {
        out.push(Choice::built_in());
    }
    out
}

/// route.v7's persona items: every option with a card, the default persona
/// not among them, and `none` (the default persona's name and card, else the
/// general assistant's) in its place. A persona without a card is never
/// offered but as the default.
pub fn inputs(options: &[Choice]) -> Vec<theseus_judge::builders::PersonaInput> {
    let default = options.iter().find(|o| o.is_default);
    let mut out: Vec<theseus_judge::builders::PersonaInput> = options
        .iter()
        .filter(|o| !o.is_default)
        .filter_map(|o| {
            Some(theseus_judge::builders::PersonaInput {
                id: o.key.clone()?,
                name: o.name.clone(),
                card: o.card.clone()?,
            })
        })
        .collect();
    out.push(theseus_judge::builders::PersonaInput {
        id: NONE.into(),
        name: default.map_or_else(|| crate::config::DEFAULT_NAME.into(), |d| d.name.clone()),
        card: default
            .and_then(|d| d.card.clone())
            .unwrap_or_else(|| NONE_CARD.into()),
    });
    out
}

/// How the persona came to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The message addressed it.
    Explicit,
    /// Jev's pick at the act bar.
    Jev,
    /// Jev's pick between confirm and act: engaged, with its why shown.
    JevShown,
    /// The owner's correction named it.
    Corrected,
    /// A message the owner corrected to it, close to this one, did.
    Learned,
    /// The prior stands.
    Prior,
}

impl How {
    pub fn as_str(self) -> &'static str {
        match self {
            How::Explicit => "explicit",
            How::Jev => "jev",
            How::JevShown => "jev_shown",
            How::Corrected => "corrected",
            How::Learned => "learned",
            How::Prior => "prior",
        }
    }
}

/// Jev's `persona` answer, as the verdict carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonaAnswer {
    /// An option's id, or `prior`.
    pub choice: String,
    pub confidence: f64,
    pub band: Band,
    /// The next-best option and its probability.
    pub runner_up: Option<(String, f64)>,
    /// Whether the judgment acts (canary or live) or is only recorded.
    pub acts: bool,
}

/// Jev's `seal` answer: whether the message warrants sealing its thread,
/// and how sure.
#[derive(Debug, Clone, PartialEq)]
pub struct SealAnswer {
    /// The top option is `yes`.
    pub yes: bool,
    pub p_yes: f64,
    pub band: Band,
    pub acts: bool,
}

impl SealAnswer {
    /// A recommendation: `yes` at the confirm bar or above.
    pub fn recommends(&self) -> bool {
        self.yes && self.band != Band::Escalate
    }
}

/// The persona that answers a turn, and what the route line says of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Engaged {
    pub key: Option<String>,
    pub name: String,
    pub how: How,
    /// 1.0 for an explicit address; Jev's probability; none for the prior.
    pub p: Option<f64>,
    pub runner_up: Option<(String, f64)>,
    /// Terms of the persona's card the message matched.
    pub terms: Vec<String>,
    /// Jev's pick when it differs from what engaged (`shadow`, or too sure
    /// of a prior): recorded so the label is free.
    pub jev: Option<String>,
}

impl Engaged {
    pub fn shown(&self) -> bool {
        !matches!(self.how, How::Jev | How::Prior | How::Explicit)
    }

    /// The tail line: who answers this turn.
    pub fn line(&self) -> String {
        format!("{} answers this turn", self.name)
    }

    /// The route line's why: the band and probability, the runner-up, the
    /// card terms the message matched (`"CVE", "sandbox escape" · 0.94 ·
    /// next builder 0.04`).
    pub fn why(&self) -> String {
        let mut parts = Vec::new();
        match self.how {
            How::Explicit => return "addressed by name".into(),
            How::Corrected => return "as you corrected".into(),
            How::Learned => return "like a message you corrected".into(),
            _ => {}
        }
        if !self.terms.is_empty() {
            let t: Vec<String> = self.terms.iter().map(|t| format!("\"{t}\"")).collect();
            parts.push(t.join(", "));
        }
        if let Some(p) = self.p {
            parts.push(format!("{p:.2}"));
        }
        if let Some((o, p)) = &self.runner_up {
            parts.push(format!("next {o} {p:.2}"));
        }
        parts.join(" \u{b7} ")
    }
}

/// The prior persona: who answered last, as an engaged value with no why.
pub fn prior(c: &Choice) -> Engaged {
    Engaged {
        key: c.key.clone(),
        name: c.name.clone(),
        how: How::Prior,
        p: None,
        runner_up: None,
        terms: Vec::new(),
        jev: None,
    }
}

/// Decide the persona: an explicit address, else Jev's answer by its band
/// (only when it `acts` and `allowed`), else the prior.
pub fn decide(
    prior_of: &Choice,
    options: &[Choice],
    explicit: Option<&Choice>,
    steered: Option<(&Choice, How)>,
    answer: Option<&PersonaAnswer>,
    allowed: bool,
    message: &str,
) -> Engaged {
    let jev = answer
        .filter(|a| a.choice != "prior")
        .map(|a| a.choice.clone());
    if let Some(c) = explicit {
        return Engaged {
            how: How::Explicit,
            p: Some(1.0),
            jev,
            ..prior(c)
        };
    }
    if let Some((c, how)) = steered {
        return Engaged {
            how,
            p: Some(1.0),
            jev,
            ..prior(c)
        };
    }
    let Some(a) = answer else {
        return prior(prior_of);
    };
    let picked = options.iter().find(|o| match a.choice.as_str() {
        NONE => o.is_default,
        id => o.key.as_deref() == Some(id),
    });
    let (Some(c), true) = (picked, allowed && a.acts) else {
        return Engaged {
            jev,
            ..prior(prior_of)
        };
    };
    let how = match a.band {
        Band::Act => How::Jev,
        Band::Confirm => How::JevShown,
        Band::Escalate => {
            return Engaged {
                jev,
                ..prior(prior_of)
            }
        }
    };
    if c.key == prior_of.key {
        return Engaged {
            jev: None,
            ..prior(prior_of)
        };
    }
    Engaged {
        how,
        p: Some(a.confidence),
        runner_up: a.runner_up.clone(),
        terms: matched_terms(c.card.as_deref().unwrap_or(""), message),
        jev,
        ..prior(c)
    }
}

/// The phrases of a card that the message holds, at most three: a card's
/// phrases are its comma-, semicolon- and colon-separated parts, matched
/// without case and without a plural s.
pub fn matched_terms(card: &str, message: &str) -> Vec<String> {
    let msg = message.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for raw in card.split([',', ';', ':', '.']) {
        let phrase = raw
            .trim()
            .trim_start_matches("and ")
            .trim_matches(|c: char| !c.is_alphanumeric());
        if phrase.chars().count() < 3 || phrase.split_whitespace().count() > 3 {
            continue;
        }
        let lower = phrase.to_lowercase();
        let stem = lower.strip_suffix('s').unwrap_or(&lower);
        if stem.len() >= 3 && msg.contains(stem) && !out.iter().any(|o| o == phrase) {
            out.push(phrase.to_string());
        }
        if out.len() == 3 {
            break;
        }
    }
    out
}

/// An explicit address in a message, if it names one of `options` (the
/// built-in default is addressable by its name too). Returns the persona
/// addressed and the message with a `/as name` prefix removed.
pub fn address<'a>(text: &'a str, options: &[Choice]) -> Option<(Choice, &'a str)> {
    let t = text.trim_start();
    let find = |word: &str| {
        let w = word
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .to_lowercase();
        options
            .iter()
            .find(|o| !w.is_empty() && (o.key.as_deref() == Some(&w) || o.name.to_lowercase() == w))
    };
    if let Some(rest) = t.strip_prefix("/as ") {
        let rest = rest.trim_start();
        let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        return find(word).map(|c| (c.clone(), after.trim_start()));
    }
    // A name at the start, then a comma or colon: "Birch, can you...".
    let end = t
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(t.len());
    let (word, after) = t.split_at(end);
    if matches!(after.chars().next(), Some(',' | ':')) {
        if let Some(c) = find(word) {
            return Some((c.clone(), text));
        }
    }
    // @name anywhere, at a word's start.
    for w in t.split_whitespace() {
        if let Some(name) = w.strip_prefix('@') {
            if let Some(c) = find(name) {
                return Some((c.clone(), text));
            }
        }
    }
    None
}

/// The most turns of a session remembered, and sessions.
const TURNS: usize = 8;
const SESSIONS: usize = 512;

/// Which persona answered each recent turn of a session, in memory (lost at
/// a restart, when the prior falls back to the session's own persona).
#[derive(Default)]
pub struct Engagements {
    by_session: Mutex<HashMap<String, Turns>>,
}

/// A session's recent turns and who answered each (a `[personas]` key, none
/// for the built-in default).
type Turns = VecDeque<(String, Option<String>)>;

impl Engagements {
    /// Turn `turn` of `session` was answered as `key`.
    pub fn note(&self, session: &str, turn: &str, key: Option<String>) {
        let mut g = self.by_session.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= SESSIONS && !g.contains_key(session) {
            if let Some(k) = g.keys().next().cloned() {
                g.remove(&k);
            }
        }
        let q = g.entry(session.to_string()).or_default();
        q.retain(|(t, _)| t != turn);
        q.push_back((turn.to_string(), key));
        while q.len() > TURNS {
            q.pop_front();
        }
    }

    /// The persona that answered the session's last turn: `Some(key)` (a
    /// `None` key is the built-in default), or none when nothing is known.
    pub fn last(&self, session: &str) -> Option<Option<String>> {
        let g = self.by_session.lock().unwrap_or_else(|e| e.into_inner());
        g.get(session)?.back().map(|(_, k)| k.clone())
    }

    /// The session's recent turns and who answered each, oldest first.
    pub fn recent(&self, session: &str) -> Vec<(String, Option<String>)> {
        let g = self.by_session.lock().unwrap_or_else(|e| e.into_inner());
        g.get(session)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Who answered `turn`.
    pub fn of_turn(&self, session: &str, turn: &str) -> Option<Option<String>> {
        let g = self.by_session.lock().unwrap_or_else(|e| e.into_inner());
        g.get(session)?
            .iter()
            .find(|(t, _)| t == turn)
            .map(|(_, k)| k.clone())
    }
}

/// The `Answer` of a judgment's `persona` question, from its banded form.
pub fn persona_answer(j: &theseus_judge::Judgment) -> Option<PersonaAnswer> {
    let a = j.answer("persona")?;
    let theseus_judge::Answer::Choice {
        choice,
        probabilities,
        confidence,
    } = &a.answer
    else {
        return None;
    };
    let mut rest: Vec<&(String, f64)> = probabilities.iter().filter(|(o, _)| o != choice).collect();
    rest.sort_by(|x, y| y.1.total_cmp(&x.1));
    Some(PersonaAnswer {
        choice: choice.clone(),
        confidence: *confidence,
        band: a.band.band,
        runner_up: rest.first().map(|(o, p)| (o.clone(), *p)),
        acts: j.mode != theseus_judge::Mode::Shadow,
    })
}

/// The `Answer` of a judgment's `seal` question.
pub fn seal_answer(j: &theseus_judge::Judgment) -> Option<SealAnswer> {
    let a = j.answer("seal")?;
    let p_yes = a.answer.probability_of("yes")?;
    let top = matches!(&a.answer, theseus_judge::Answer::Choice { choice, .. } if choice == "yes");
    Some(SealAnswer {
        yes: top,
        p_yes,
        band: a.band.band,
        acts: j.mode != theseus_judge::Mode::Shadow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(key: Option<&str>, name: &str, card: &str) -> Choice {
        Choice {
            key: key.map(str::to_string),
            name: name.into(),
            card: (!card.is_empty()).then(|| card.into()),
            is_default: false,
        }
    }

    fn options() -> Vec<Choice> {
        vec![
            choice(
                Some("birch"),
                "Birch",
                "Security engineer: CVEs, sandbox escapes, threat models and disclosure.",
            ),
            choice(
                Some("alder"),
                "Alder",
                "Release manager: changelogs, version bumps, packaging and CI.",
            ),
            choice(None, "Theseus", ""),
        ]
    }

    fn answer(choice: &str, p: f64, band: Band, acts: bool) -> PersonaAnswer {
        PersonaAnswer {
            choice: choice.into(),
            confidence: p,
            band,
            runner_up: Some(("alder".into(), 0.04)),
            acts,
        }
    }

    fn who(explicit: Option<&Choice>, a: Option<&PersonaAnswer>, acts: bool, msg: &str) -> Engaged {
        let o = options();
        decide(&o[1], &o, explicit, None, a, acts, msg)
    }

    /// A name at the start with a comma or colon, `@name`, and `/as name`
    /// address a persona; the same word in a sentence does not.
    #[test]
    fn an_address_is_a_name_at_the_start_an_at_or_as() {
        let o = options();
        let key = |t: &str| address(t, &o).map(|(c, _)| c.key.unwrap_or_default());
        assert_eq!(key("Birch, is this a CVE?").as_deref(), Some("birch"));
        assert_eq!(key("  alder: bump the version").as_deref(), Some("alder"));
        assert_eq!(key("what do you think @birch").as_deref(), Some("birch"));
        assert_eq!(key("/as alder what changed?").as_deref(), Some("alder"));
        assert_eq!(key("Theseus, summarise").as_deref(), Some(""));
        for not in [
            "Birch trees are lovely",
            "ask the birch about it",
            "mail me at x@birch.example",
            "/as nobody hello",
            "alderman, hello",
        ] {
            assert!(address(not, &o).is_none(), "{not}");
        }
        let (_, rest) = address("/as alder what changed?", &o).unwrap();
        assert_eq!(rest, "what changed?", "the /as prefix is taken off");
    }

    /// Jev's pick engages at the act band silently, shows its why between
    /// confirm and act, and below confirm, in shadow, or with no answer the
    /// prior stands; the prior is never asked about.
    #[test]
    fn the_bands_engage_show_or_keep_the_prior() {
        let msg = "Is this CVE a sandbox escape?";
        let sure = answer("birch", 0.94, Band::Act, true);
        let e = who(None, Some(&sure), true, msg);
        assert_eq!(
            (e.key.as_deref(), e.how, e.shown()),
            (Some("birch"), How::Jev, false)
        );
        assert_eq!(e.terms, ["CVEs", "sandbox escapes"]);
        assert_eq!(
            e.why(),
            "\"CVEs\", \"sandbox escapes\" \u{b7} 0.94 \u{b7} next alder 0.04"
        );
        let less = answer("birch", 0.7, Band::Confirm, true);
        let e = who(None, Some(&less), true, msg);
        assert_eq!((e.how, e.shown()), (How::JevShown, true));
        let unsure = answer("birch", 0.4, Band::Escalate, true);
        let e = who(None, Some(&unsure), true, msg);
        assert_eq!((e.key.as_deref(), e.how), (Some("alder"), How::Prior));
        assert_eq!(e.jev.as_deref(), Some("birch"), "recorded for the label");
        // Shadow: recorded, never acted on.
        let e = who(None, Some(&sure), false, msg);
        assert_eq!((e.key.as_deref(), e.how), (Some("alder"), How::Prior));
        assert_eq!(e.jev.as_deref(), Some("birch"));
        // `prior` keeps the prior; a late or absent verdict does too.
        let keep = answer("prior", 0.99, Band::Act, true);
        assert_eq!(who(None, Some(&keep), true, msg).how, How::Prior);
        assert_eq!(who(None, None, true, msg).how, How::Prior);
        // An option that is not offered cannot be picked.
        let ghost = answer("cedar", 0.99, Band::Act, true);
        assert_eq!(who(None, Some(&ghost), true, msg).how, How::Prior);
    }

    fn with_default(default: Option<&str>) -> Vec<Choice> {
        let mut o = options();
        o.pop();
        for c in &mut o {
            c.is_default = c.key.as_deref() == default;
        }
        if default.is_none() {
            o.push(Choice::built_in());
        }
        o
    }

    /// `none` is the default persona: Jev picking it engages it.
    #[test]
    fn none_engages_the_default() {
        let o = with_default(Some("alder"));
        let sure = answer(NONE, 0.95, Band::Act, true);
        let e = decide(&o[0], &o, None, None, Some(&sure), true, "hello");
        assert_eq!((e.key.as_deref(), e.how), (Some("alder"), How::Jev));
        assert_eq!(e.jev.as_deref(), Some("none"));
    }

    /// With no persona marked default `none` is the built-in.
    #[test]
    fn none_with_no_default_is_the_built_in() {
        let o = with_default(None);
        let sure = answer(NONE, 0.95, Band::Act, true);
        let e = decide(&o[1], &o, None, None, Some(&sure), true, "hello");
        assert_eq!((e.key, e.name.as_str(), e.how), (None, "Theseus", How::Jev));
    }

    /// The default persona is Jev's `none` item, with its own name and card
    /// (else the general assistant's), and not an item of its own too.
    #[test]
    fn the_default_is_not_offered_twice() {
        let ids = |o: &[Choice]| -> Vec<String> { inputs(o).into_iter().map(|p| p.id).collect() };
        let mut o = with_default(Some("alder"));
        assert_eq!(ids(&o), ["birch", "none"]);
        let card = o[1].card.take();
        let none = inputs(&o).pop().unwrap();
        assert_eq!(none.name, "Alder");
        assert_eq!(none.card, NONE_CARD, "a default with no card");
        o[1].card = card.clone();
        assert_eq!(inputs(&o).pop().unwrap().card, card.unwrap());
        let o = with_default(None);
        assert_eq!(ids(&o), ["birch", "alder", "none"]);
        assert_eq!(inputs(&o).pop().unwrap().name, "Theseus");
    }

    /// An explicit address wins at confidence 1, over Jev's pick, which is
    /// kept as a label.
    #[test]
    fn an_explicit_address_wins_at_one_and_keeps_jevs_pick() {
        let o = options();
        let sure = answer("alder", 0.99, Band::Act, true);
        let e = decide(&o[1], &o, Some(&o[0]), None, Some(&sure), true, "Birch, hi");
        assert_eq!(
            (e.key.as_deref(), e.how, e.p),
            (Some("birch"), How::Explicit, Some(1.0))
        );
        assert_eq!(e.jev.as_deref(), Some("alder"));
        assert_eq!(e.line(), "Birch answers this turn");
        assert!(!e.shown(), "an address needs no why");
        assert_eq!(e.why(), "addressed by name");
    }

    /// The owner's correction places a persona ahead of Jev's pick.
    #[test]
    fn a_correction_places_the_persona_ahead_of_jev() {
        let o = options();
        let sure = answer("alder", 0.99, Band::Act, true);
        let e = decide(
            &o[1],
            &o,
            None,
            Some((&o[0], How::Corrected)),
            Some(&sure),
            true,
            "x",
        );
        assert_eq!((e.key.as_deref(), e.how), (Some("birch"), How::Corrected));
        assert!(e.shown() && e.why() == "as you corrected");
    }

    /// A shared place offers its own persona and the shareable ones only.
    #[test]
    fn a_shared_place_offers_its_own_and_the_shareable() {
        use crate::config::PersonaConfig;
        let mut cfg = crate::config::Config::example();
        let ps = &mut cfg.personas;
        ps.insert("birch".to_string(), PersonaConfig::default());
        ps.insert(
            "alder".to_string(),
            PersonaConfig {
                shareable: true,
                ..Default::default()
            },
        );
        ps.insert("cedar".to_string(), PersonaConfig::default());
        let book = Book::of_config(&cfg);
        let keys = |shared, own: Option<&str>| -> Vec<String> {
            choices(&book, shared, own)
                .into_iter()
                .filter_map(|c| c.key)
                .collect()
        };
        assert_eq!(keys(false, None), ["alder", "birch", "cedar"]);
        assert_eq!(keys(true, None), ["alder"]);
        assert_eq!(keys(true, Some("cedar")), ["alder", "cedar"]);
    }

    /// The engagement memory keeps who answered the recent turns, bounded.
    #[test]
    fn the_engagement_memory_is_bounded_and_ordered() {
        let m = Engagements::default();
        assert_eq!(m.last("s"), None);
        for n in 0..12 {
            m.note(
                "s",
                &format!("t{n}"),
                (n % 2 == 0).then(|| "birch".to_string()),
            );
        }
        assert_eq!(m.last("s"), Some(None));
        assert_eq!(m.recent("s").len(), TURNS);
        assert_eq!(m.of_turn("s", "t10"), Some(Some("birch".to_string())));
        assert_eq!(m.of_turn("s", "t0"), None, "the oldest out");
    }
}
