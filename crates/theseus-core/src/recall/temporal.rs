//! A question about time (theseus-q1tbn): "when did we switch the cache
//! TTL?", "what was the first time the build broke?", "how long since the
//! migration?", read from the turn's own words by a small deterministic
//! reader, with no model call on the path (as `when` reads a span).
//!
//! - **What it reads**, as a [`Kind`]: `When` ("when did / was / is", "what
//!   day / date / time did …"), `Duration` ("how long ago", "how long
//!   since", "how many days between"), `Earliest` ("the first time", "the
//!   earliest …", "when did we first …"), `Latest` ("the last time", "the
//!   most recent …", "when did we last …"), and `Order` ("what came first",
//!   "before / after <event>" in a question).
//! - **What it leaves alone**: a time that is only the question's subject
//!   ("what time zone do I use?", "what date format does it print?"), a
//!   question about now ("what day is it?", "what time does the backup run
//!   now?"), a present-tense "what time does …" (a value, not an event), "latest" as an adjective of a
//!   thing ("the latest version"), and "before" / "after" a date (that is a
//!   span, `when`'s).
//! - **The subject** is the question without its temporal words
//!   ([`subject`]): they match every dated note, so the index searches on
//!   the event alone.
//! - **The order** ([`order`]): among the subject's best matches, a note
//!   that states the event (it carries a time expression, or it is the
//!   change itself) goes before one that only mentions it, and each tier is
//!   ordered by the kind: oldest first for `When` and `Earliest`, newest
//!   first for `Latest`, both ends first for `Order` and `Duration`.

use theseus_memory::{rerank, Asker, Candidate, MemoryScience, Pack, Params};

use super::when;

/// What a question about time asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The date or time of one event.
    When,
    /// Which of two events came first (or after which).
    Order,
    /// The time between two events, or since one.
    Duration,
    /// The last time, or the most recent.
    Latest,
    /// The first time, or the earliest.
    Earliest,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::When => "when",
            Kind::Order => "order",
            Kind::Duration => "duration",
            Kind::Latest => "latest",
            Kind::Earliest => "earliest",
        }
    }
}

/// How many of the best matches the order reaches over: the subject's
/// matches. Past them the relevance order stands.
pub const SUBJECT_TOP: usize = 12;

#[derive(Debug, Clone)]
struct Tok {
    w: String,
    at: (usize, usize),
}

fn tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut push = |s: usize, e: usize| {
        out.push(Tok {
            w: text[s..e].to_lowercase().replace('’', "'"),
            at: (s, e),
        });
    };
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() || c == '\'' || c == '’' || c == '-' {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            push(s, i);
        }
    }
    if let Some(s) = start {
        push(s, text.len());
    }
    out
}

/// What "when" takes to ask about an event; "when does the backup start"
/// asks for a value as it stands, not for an event.
const AUX: [&str; 5] = ["did", "was", "were", "is", "had"];
/// What "what day / date / time" takes to ask about the past: "what time
/// does the backup run" asks for a value, as it is now.
const PAST: [&str; 4] = ["did", "was", "were", "had"];
/// Words that make a question about how things stand, not how they were.
const CURRENT: [&str; 4] = ["now", "currently", "nowadays", "anymore"];
const NOW: [&str; 5] = ["it", "today", "now", "tomorrow", "tonight"];
const DAY_WORDS: [&str; 5] = ["day", "date", "time", "year", "month"];
const UNITS: [&str; 12] = [
    "day", "days", "week", "weeks", "month", "months", "year", "years", "hour", "hours", "minute",
    "minutes",
];
const DURATION_AFTER: [&str; 8] = [
    "between",
    "since",
    "ago",
    "apart",
    "passed",
    "elapsed",
    "separate",
    "separated",
];
/// The things a question asks the latest or earliest of.
const NOUNS: [&str; 18] = [
    "time",
    "mention",
    "discussion",
    "decision",
    "change",
    "conversation",
    "session",
    "message",
    "note",
    "update",
    "meeting",
    "incident",
    "deploy",
    "deployment",
    "run",
    "failure",
    "outage",
    "commit",
];
const INTERROGATIVE: [&str; 17] = [
    "what", "which", "when", "who", "how", "did", "was", "were", "do", "does", "is", "are", "have",
    "has", "can", "could", "should",
];
const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
/// What follows `before` / `after` when it names a date or a span, which
/// `when` reads, not an event.
const DATE_WORDS: [&str; 23] = [
    "today",
    "yesterday",
    "tomorrow",
    "last",
    "this",
    "next",
    "week",
    "month",
    "year",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "noon",
    "midnight",
];

/// A question about time, read: its kind, and the byte ranges of its
/// temporal words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub kind: Kind,
    cut: Vec<(usize, usize)>,
}

fn is_date_word(w: &str) -> bool {
    w.starts_with(|c: char| c.is_ascii_digit())
        || DATE_WORDS.contains(&w)
        || WEEKDAYS.contains(&w)
        || (w.len() == 3
            && [
                "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
            ]
            .contains(&w))
}

/// The reading of one text: what the phrases found so far say, and where.
struct Reader<'a> {
    t: Vec<Tok>,
    text: &'a str,
    cut: Vec<(usize, usize)>,
    duration: bool,
    when: bool,
    earliest: bool,
    latest: bool,
    order: bool,
}

impl Reader<'_> {
    fn w(&self, i: usize) -> Option<&str> {
        self.t.get(i).map(|x| x.w.as_str())
    }

    fn is(&self, i: usize, set: &[&str]) -> bool {
        self.w(i).is_some_and(|w| set.contains(&w))
    }

    /// Words `[from, to)` are the question's temporal phrase.
    fn span(&mut self, from: usize, to: usize) {
        if let (Some(a), Some(b)) = (self.t.get(from), self.t.get(to.saturating_sub(1))) {
            self.cut.push((a.at.0, b.at.1));
        }
    }

    /// The phrase at word `i`, if one starts there.
    fn phrase(&mut self, i: usize) {
        match self.t[i].w.as_str() {
            "how" => self.duration_at(i),
            "when" if self.is(i + 1, &AUX) && !self.is(i + 2, &NOW) => {
                self.when = true;
                self.span(i, i + 2);
            }
            "what" | "which" if self.is(i + 1, &DAY_WORDS) => self.day_at(i),
            "first" | "last" if self.w(i + 1) == Some("time") => self.time_at(i),
            "most" if self.is(i + 1, &["recent", "recently"]) => {
                if self.w(i + 1) == Some("recently") || self.is(i + 2, &NOUNS) {
                    self.latest = true;
                    self.span(i, i + 2);
                }
            }
            "latest" | "earliest" if self.is(i + 1, &NOUNS) => {
                self.latest |= self.t[i].w == "latest";
                self.earliest |= self.t[i].w == "earliest";
                self.span(i, i + 1);
            }
            _ => {}
        }
    }

    /// "how long ago / since", "how many days between".
    fn duration_at(&mut self, i: usize) {
        let long = self.w(i + 1) == Some("long") && self.is(i + 2, &["ago", "since"]);
        let many = self.w(i + 1) == Some("many")
            && self.is(i + 2, &UNITS)
            && self.is(i + 3, &DURATION_AFTER);
        if long || many {
            self.duration = true;
            self.span(i, i + if long { 3 } else { 4 });
        }
    }

    /// "what day / date / time did …", "what day of the week was …", never
    /// "what time is it".
    fn day_at(&mut self, i: usize) {
        if (self.is(i + 2, &PAST) || self.w(i + 2) == Some("of")) && !self.is(i + 3, &NOW) {
            self.when = true;
            self.span(i, i + 3);
        }
    }

    /// "the first time …", "the last time …".
    fn time_at(&mut self, i: usize) {
        self.earliest |= self.t[i].w == "first";
        self.latest |= self.t[i].w == "last";
        // The determiner before it goes too.
        let from = if i > 0 && self.is(i - 1, &["the", "our", "my"]) {
            i - 1
        } else {
            i
        };
        self.span(from, i + 2);
    }

    /// "when did we first / last …": the adverb of a when-question.
    fn adverb(&mut self) {
        for i in 0..self.t.len() {
            if !self.is(i, &["first", "last"]) {
                continue;
            }
            let det = i > 0 && self.is(i - 1, &["the", "this", "that", "a", "my", "our", "one"]);
            let unit = self.w(i + 1).is_some_and(|n| {
                UNITS.contains(&n) || WEEKDAYS.contains(&n) || is_date_word(n) || n == "night"
            });
            if !det && !unit {
                self.earliest |= self.t[i].w == "first";
                self.latest |= self.t[i].w == "last";
                self.span(i, i + 1);
                return;
            }
        }
    }

    /// "which came first", and, in a question, "before / after <event>".
    fn order(&mut self) {
        self.order = (1..self.t.len())
            .any(|i| self.w(i) == Some("first") && self.is(i - 1, &["came", "happened"]));
        let asks = self.text.trim_end().ends_with('?') || self.is(0, &INTERROGATIVE);
        if self.order || !asks {
            return;
        }
        for i in 1..self.t.len().saturating_sub(1) {
            if self.is(i, &["before", "after"]) && !self.w(i + 1).is_some_and(is_date_word) {
                self.order = true;
                self.span(i, i + 1);
                return;
            }
        }
    }

    fn kind(&self) -> Option<Kind> {
        let by = [
            (self.duration, Kind::Duration),
            (self.earliest, Kind::Earliest),
            (self.latest, Kind::Latest),
            (self.when, Kind::When),
            (self.order, Kind::Order),
        ];
        by.into_iter().find(|(on, _)| *on).map(|(_, k)| k)
    }
}

/// The kind of question `text` asks about time, if it does.
pub fn read(text: &str) -> Option<Asked> {
    let t = tokens(text);
    if t.iter().any(|x| CURRENT.contains(&x.w.as_str())) {
        return None;
    }
    let mut r = Reader {
        t,
        text,
        cut: Vec::new(),
        duration: false,
        when: false,
        earliest: false,
        latest: false,
        order: false,
    };
    for i in 0..r.t.len() {
        r.phrase(i);
    }
    if r.when && !r.earliest && !r.latest {
        r.adverb();
    }
    // The order of two events is the last resort: a date after "before" is
    // a span, and a kind found above is the better reading.
    if r.kind().is_none() || r.t.iter().any(|x| x.w == "came" || x.w == "happened") {
        r.order();
    }
    let kind = r.kind()?;
    let mut cut = r.cut;
    cut.sort_unstable();
    Some(Asked { kind, cut })
}

/// The question without its temporal words: what the index searches on
/// (they match every dated note). `text` unchanged when it is no question
/// about time, or when too little would be left to search on.
pub fn subject(text: &str) -> String {
    let Some(asked) = read(text) else {
        return text.to_string();
    };
    let mut out = String::new();
    let mut at = 0;
    for (a, b) in &asked.cut {
        if *a >= at {
            out.push_str(&text[at..*a]);
            at = *b;
        }
    }
    out.push_str(&text[at..]);
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.split_whitespace().filter(|w| w.len() > 2).count() < 2 {
        return text.to_string();
    }
    out
}

/// The question the turn's new messages ask about time, if one does.
pub fn of_turn(nodes: &crate::store::Transcript, turn_id: &str) -> Option<Asked> {
    let words: Vec<&str> = nodes
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn_id))
        .filter_map(|(_, n)| match &n.body {
            crate::node::Body::UserMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let words = words.join("\n");
    // A question, not a pasted document that happens to say "when was".
    (words.chars().count() <= MAX_QUESTION_CHARS)
        .then(|| read(&words))
        .flatten()
}

/// The longest text read as a question about time.
pub const MAX_QUESTION_CHARS: usize = 400;

/// Words of an event that changed (the past, and the imperative a person
/// gives): a note that says one states the event itself.
const CHANGED: &[&str] = &[
    "switched",
    "switch",
    "changed",
    "change",
    "moved",
    "move",
    "migrated",
    "migrate",
    "set",
    "decided",
    "decide",
    "chose",
    "choose",
    "picked",
    "pick",
    "upgraded",
    "upgrade",
    "bumped",
    "bump",
    "raised",
    "raise",
    "lowered",
    "lower",
    "cut",
    "dropped",
    "drop",
    "added",
    "increased",
    "decreased",
    "reduced",
    "reduce",
    "extended",
    "shortened",
    "updated",
    "update",
    "swapped",
    "renamed",
    "rename",
    "replaced",
    "replace",
    "deployed",
    "shipped",
    "launched",
    "started",
    "stopped",
    "finished",
    "completed",
    "fixed",
    "merged",
    "released",
    "enabled",
    "disabled",
    "installed",
    "removed",
    "adopted",
    "reverted",
    "rolled",
    "cancelled",
    "canceled",
    "scheduled",
    "signed",
    "hired",
    "joined",
    "left",
    "broke",
    "agreed",
    "deleted",
    "created",
    "paused",
    "resumed",
    "turned",
];
/// Phrases that mark a change however it is worded.
const CHANGE_PHRASES: [&str; 5] = [
    "from now on",
    "going forward",
    "no longer",
    "instead of",
    "used to be",
];

/// Whether `c` states an event: its text carries a time expression, read
/// as of its own time, or says a change happened. A note that only mentions
/// the subject does neither.
pub fn states_event(c: &Candidate) -> bool {
    if when::read_local(&c.text, c.time_ms).is_some() {
        return true;
    }
    let lower = c.text.to_lowercase();
    CHANGE_PHRASES.iter().any(|p| lower.contains(p))
        || tokens(&c.text)
            .iter()
            .any(|t| CHANGED.contains(&t.w.as_str()))
}

/// `kept` (the candidates that passed every filter, most relevant first)
/// in the order a question of `kind` asks for: the keys, best first, over
/// every candidate. The subject's matches are the first [`SUBJECT_TOP`].
pub fn order(kind: Kind, kept: &[Candidate]) -> Vec<String> {
    let top = kept.len().min(SUBJECT_TOP);
    let (stating, mentions): (Vec<&Candidate>, Vec<&Candidate>) =
        kept[..top].iter().partition(|c| states_event(c));
    let mut out: Vec<String> = Vec::with_capacity(kept.len());
    for tier in [stating, mentions] {
        out.extend(by_kind(kind, tier).into_iter().map(Candidate::key));
    }
    out.extend(kept[top..].iter().map(Candidate::key));
    out
}

/// One tier, in the kind's order. Ties in time keep the relevance order.
fn by_kind(kind: Kind, mut tier: Vec<&Candidate>) -> Vec<&Candidate> {
    match kind {
        Kind::When | Kind::Earliest => {
            tier.sort_by_key(|c| c.time_ms);
            tier
        }
        Kind::Latest => {
            tier.sort_by_key(|c| std::cmp::Reverse(c.time_ms));
            tier
        }
        Kind::Order | Kind::Duration => {
            // Both ends, then the rest as the relevance had them.
            let mut ends: Vec<&Candidate> = Vec::new();
            let oldest = tier.iter().copied().min_by_key(|c| c.time_ms);
            let newest = tier
                .iter()
                .copied()
                .max_by_key(|c| (c.time_ms, std::cmp::Reverse(c.index_rank)));
            ends.extend(oldest);
            ends.extend(newest.filter(|n| oldest.is_none_or(|o| o.key() != n.key())));
            tier.retain(|c| !ends.iter().any(|e| e.key() == c.key()));
            ends.extend(tier);
            ends
        }
    }
}

/// What recall packs for a question of `kind`: the candidates that pass every
/// filter (the place rule first) in [`order`], then the same pack as any
/// recall's, under the science's own filters.
pub fn pack(
    kind: Kind,
    science: &dyn MemoryScience,
    asker: &Asker<'_>,
    candidates: &[Candidate],
    params: &Params,
) -> Pack {
    let eligible = rerank::eligible(science, asker, candidates, params);
    let keys = order(kind, &eligible);
    rerank::repack(science, asker, candidates.to_vec(), params, keys)
}
