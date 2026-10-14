//! The router: which thread an unaddressed message is about (theseus-rha2).
//!
//! Pure functions over the threads in memory, a message, and the place's
//! last exchange; the orchestration (the Jev wait, the ask, the turn) is
//! `rpc/thread_flow.rs`. In the order a message meets them:
//!
//! 1. **Addressing**, first match wins, certain (confidence 1), before any
//!    judge: the **place** a thread alone lists in its `surfaces`; an
//!    **explicit** word (`--thread`, a leading `#slug`, `/t slug`, or "in the
//!    <title> thread:"); a **reply** to a message that a thread's turn wrote.
//! 2. **The pre-router**, free: each live thread scores from what the message
//!    names of its working set (a repo, PR or issue number, branch, path,
//!    host, Beads-style id: a unique hit is strong), from **stickiness** to
//!    the place's last thread (0.6 at once, halving every
//!    `[threads] sticky_half_life_secs`: strong for a minute, weak after an
//!    hour) and from the **reply chain** (right after a thread's reply, with no
//!    thread's entity named, the message is likelier its follow-up: 0.7 at
//!    once). The three combine as independent evidence, `1 - Π(1 - p)`. Only a
//!    named entity decides alone: stickiness and the chain are priors, capped
//!    below the act bar together (0.88), since in the owner's DMs a third to
//!    a half of consecutive messages switch concern, and a follow-up that
//!    reads like a fragment ("and the tests too") is Jev's to tell.
//! 3. **Fusion** with `classify.v2`'s `thread` answer (the pick `p`): the
//!    picked thread scores `1 - (1 - 0.8·pre)(1 - p)`; every other thread
//!    keeps `0.85·pre`; `new_thread` and `home` score `p` when picked. A weighted
//!    max where Jev agrees or not, and the evidence adds where it agrees.
//! 4. **The decision**, at the pack's bars (`act` 0.90, `confirm` 0.60): the
//!    top choice at 0.90 or more and 0.15 clear of the next routes with a quiet
//!    tag; 0.60 to 0.90, clear, routes with a tag and a move handle; anything
//!    else, or two choices within 0.15, asks in one line. Jev not answering in
//!    time (or not there) leaves the pre-router's pick standing, tagged a
//!    guess, and asks only when two threads are close.
//!
//! A `new_thread` pick at 0.60 offers a new thread and waits for one tap; a
//! one-off runs in the place's own conversation ("home") and never makes a
//! thread.

use theseus_protocol::thread_route::{AskOption, RouteHow, ThreadTag};
use theseus_protocol::threads::{ThreadInfo, WorkingSet};

/// The classify pack's bars for the `thread` question.
pub const ACT: f64 = 0.90;
pub const CONFIRM: f64 = 0.60;
/// Two choices this close are a tie, and a tie is asked.
pub const CLOSE: f64 = 0.15;
/// The guess floor: below it the router has no pick without Jev.
const GUESS: f64 = 0.30;
/// What stickiness is at the moment of the place's last message.
const STICKY: f64 = 0.60;
/// The owner moved a message like this one to the thread: the evidence.
pub const LEARNED: f64 = 0.92;
/// The follow-up of a thread's reply, at the moment the reply ended.
const CHAIN: f64 = 0.70;
/// A unique hit on a specific entity (a number, a branch, a path, a host).
const STRONG: f64 = 0.92;
/// A hit on a repo or a root, which several threads often share.
const WEAK: f64 = 0.80;
/// A thread's entity score when other threads hit the message too.
const SHARED: f64 = 0.60;
/// Jev's pick adds to the router's agreeing pick at this share of its score,
/// and the router keeps this share of a thread Jev did not pick.
const AGREE: f64 = 0.80;
const OTHERS: f64 = 0.85;
const MAX: f64 = 0.99;

/// The kinds of entity a working set names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ref,
    File,
    Branch,
    Host,
    Repo,
    Root,
}

impl Kind {
    fn strength(self) -> f64 {
        match self {
            Kind::Ref | Kind::File | Kind::Branch | Kind::Host => STRONG,
            Kind::Repo | Kind::Root => WEAK,
        }
    }
}

/// What addressed a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    /// The place a thread alone lists in its `surfaces`.
    Place(String),
    /// A word that names the thread; the message with it taken off.
    Explicit { slug: String, text: String },
    /// A reply to a thread's reply.
    Reply(String),
}

impl Address {
    pub fn slug(&self) -> &str {
        match self {
            Address::Place(s) | Address::Reply(s) | Address::Explicit { slug: s, .. } => s,
        }
    }

    pub fn how(&self) -> RouteHow {
        match self {
            Address::Reply(_) => RouteHow::Reply,
            _ => RouteHow::Addressed,
        }
    }
}

fn norm(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The live thread a word names: its slug, or its title (hyphens and
/// spaces alike, case folded).
fn named<'a>(word: &str, threads: &[&'a ThreadInfo]) -> Option<&'a ThreadInfo> {
    let w = norm(word);
    if w.is_empty() {
        return None;
    }
    threads
        .iter()
        .copied()
        .find(|t| norm(&t.slug) == w || norm(&t.title) == w)
}

/// The explicit address of `text`, if it has one: a leading `#slug`, `/t
/// slug`, or "in the <title> thread:" (a `:` or `,` after `thread`). Only a
/// live thread is named; the message comes back with the address taken off.
pub fn explicit(text: &str, threads: &[&ThreadInfo]) -> Option<Address> {
    let t = text.trim_start();
    let strip = |rest: &str| {
        rest.trim_start_matches([':', ',', ' ', '\t'])
            .trim()
            .to_string()
    };
    if let Some(rest) = t.strip_prefix('#') {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ':' || c == ',')
            .unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        if let Some(th) = named(word, threads) {
            return Some(Address::Explicit {
                slug: th.slug.clone(),
                text: strip(after),
            });
        }
        return None;
    }
    let lower = t.to_lowercase();
    if lower.starts_with("/t ") || lower.starts_with("/t\t") {
        let rest = t[3..].trim_start();
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        return named(word, threads).map(|th| Address::Explicit {
            slug: th.slug.clone(),
            text: strip(after),
        });
    }
    if lower.starts_with("in the ") {
        // "in the <title> thread:" within the first 120 characters.
        let head: String = t.chars().take(120).collect();
        let lh = head.to_ascii_lowercase();
        for stop in [" thread:", " thread,"] {
            if let Some(at) = lh.find(stop) {
                let word = &head[7..at];
                if let Some(th) = named(word, threads) {
                    let after = &t[at + stop.len()..];
                    return Some(Address::Explicit {
                        slug: th.slug.clone(),
                        text: strip(after),
                    });
                }
            }
        }
    }
    None
}

/// The thread whose `surfaces` list `place` alone (`discord:channel:<id>`,
/// `discord:dm:<id>`, or `cli` for no place): two that list it are
/// candidates, not an address.
pub fn by_place(place: Option<&str>, threads: &[&ThreadInfo]) -> Option<Address> {
    let key = place.unwrap_or("cli");
    let mut listed = threads
        .iter()
        .filter(|t| t.surfaces.iter().any(|s| s == key));
    let first = listed.next()?;
    match listed.next() {
        None => Some(Address::Place(first.slug.clone())),
        Some(_) => None,
    }
}

/// Everything addressing reads: the explicit flag or word, the reply's
/// thread, the place.
pub struct Addressing<'a> {
    pub text: &'a str,
    pub place: Option<&'a str>,
    /// The live thread a replied-to turn ran in.
    pub reply_thread: Option<&'a str>,
}

/// Addressing, in order: place, explicit, reply. The first match wins.
pub fn address(a: &Addressing<'_>, threads: &[&ThreadInfo]) -> Option<Address> {
    by_place(a.place, threads)
        .or_else(|| explicit(a.text, threads))
        .or_else(|| {
            let slug = a.reply_thread?;
            threads
                .iter()
                .any(|t| t.slug == slug)
                .then(|| Address::Reply(slug.to_string()))
        })
}

/// "That was for the security fix": the owner's words that the last message
/// was placed wrong. The live thread they name (its slug), or `home`; none for
/// any other message. Narrow on purpose: the whole message is the correction.
pub fn moved_words(text: &str, threads: &[&ThreadInfo]) -> Option<String> {
    let t = text.trim().trim_end_matches(['.', '!']).to_lowercase();
    let mut t = t.as_str();
    for lead in [
        "no, ", "no ", "oops, ", "oops ", "sorry, ", "sorry ", "wait, ", "wait ",
    ] {
        if let Some(rest) = t.strip_prefix(lead) {
            t = rest.trim_start();
        }
    }
    const SAYS: [&str; 9] = [
        "that was for ",
        "this was for ",
        "that was about ",
        "that was meant for ",
        "that belongs in ",
        "that goes in ",
        "that goes to ",
        "that should have gone to ",
        "that should go to ",
    ];
    let rest = SAYS.iter().find_map(|p| t.strip_prefix(p))?;
    let rest = rest.strip_prefix("the ").unwrap_or(rest);
    let rest = rest.strip_suffix(" thread").unwrap_or(rest).trim();
    if rest == "home" || rest == "home conversation" {
        return Some("home".into());
    }
    named(rest, threads).map(|th| th.slug.clone())
}

// ---------------------------------------------------------------- entities

/// Words too generic to name a thread's work.
const GENERIC: &[&str] = &[
    "main", "master", "develop", "dev", "src", "lib", "test", "tests", "docs", "doc", "home",
    "tmp", "bin", "app", "api", "web", "core", "build", "target", "node", "git", "the", "and",
];

fn boundary(c: Option<char>) -> bool {
    c.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
}

/// Whether `needle` (lower case) appears in `hay` (lower case) as a whole
/// token: the characters around it are not word characters.
fn has_token(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let at = from + i;
        let before = hay[..at].chars().next_back();
        let after = hay[at + needle.len()..].chars().next();
        if boundary(before) && boundary(after) {
            return true;
        }
        from = at + needle.len();
        if from >= hay.len() {
            break;
        }
    }
    false
}

/// The numbers a message gives as a PR or issue (`#412`, `PR 412`, `issue
/// 412`, `pull/412`).
fn numbers(lower: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let b: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit()) {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let digits: String = b[i..j].iter().collect();
            let before: String = b[..i].iter().collect();
            let marked = before.ends_with('#')
                || ["pr", "pull", "issue", "mr", "bug", "ticket"]
                    .iter()
                    .any(|w| {
                        let t = before.trim_end_matches([' ', '#', '/', '-']);
                        t.ends_with(w) && boundary(t[..t.len() - w.len()].chars().next_back())
                    });
            if marked {
                if let Ok(n) = digits.parse() {
                    out.push(n);
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn ref_number(r: &str) -> Option<u64> {
    let digits = r.trim().trim_start_matches(|c: char| !c.is_ascii_digit());
    let prefix = &r.trim()[..r.trim().len() - digits.len()];
    let ok = prefix
        .to_lowercase()
        .chars()
        .all(|c| c == '#' || c == '-' || c == ' ' || c == '/')
        || ["pr", "pull", "issue", "mr"]
            .iter()
            .any(|w| norm(prefix) == *w);
    (ok && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

fn base_of(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// What `text` names of one working set, as (kind, entity).
pub fn entity_hits(text: &str, w: &WorkingSet) -> Vec<(Kind, String)> {
    let lower = text.to_lowercase();
    let nums = numbers(&lower);
    let mut out = Vec::new();
    let specific = |e: &str| e.chars().count() >= 3 && !GENERIC.contains(&e);
    for r in &w.refs {
        let hit = match ref_number(r) {
            Some(n) => nums.contains(&n),
            None => specific(&r.to_lowercase()) && has_token(&lower, &r.to_lowercase()),
        };
        if hit {
            out.push((Kind::Ref, r.clone()));
        }
    }
    for f in &w.files {
        let (full, base) = (f.to_lowercase(), base_of(f).to_lowercase());
        if has_token(&lower, &full)
            || (base.chars().count() >= 4 && specific(&base) && has_token(&lower, &base))
        {
            out.push((Kind::File, f.clone()));
        }
    }
    for b in &w.branches {
        let e = b.to_lowercase();
        if specific(&e) && has_token(&lower, &e) {
            out.push((Kind::Branch, b.clone()));
        }
    }
    for h in &w.hosts {
        let e = h.to_lowercase();
        if specific(&e) && lower.contains(&e) {
            out.push((Kind::Host, h.clone()));
        }
    }
    for r in &w.repos {
        let e = r.to_lowercase();
        let short = e.rsplit('/').next().unwrap_or(&e);
        if (specific(&e) && has_token(&lower, &e)) || (specific(short) && has_token(&lower, short))
        {
            out.push((Kind::Repo, r.clone()));
        }
    }
    for r in &w.roots {
        let e = r.to_lowercase();
        let base = base_of(&e);
        if has_token(&lower, e.trim_end_matches('/'))
            || (base.chars().count() >= 4 && specific(base) && has_token(&lower, base))
        {
            out.push((Kind::Root, r.clone()));
        }
    }
    out
}

/// The top entities of a working set, for Jev's line about a thread: refs,
/// repos, branches, files, hosts, roots.
pub fn top_entities(w: &WorkingSet, n: usize) -> Vec<String> {
    w.refs
        .iter()
        .chain(&w.repos)
        .chain(&w.branches)
        .chain(&w.files)
        .chain(&w.hosts)
        .chain(&w.roots)
        .take(n)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------- the pre-router

/// The place's last exchange: the thread it ran in (none: home), when the
/// message arrived, and when the thread's reply ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Last {
    pub slug: Option<String>,
    pub at_ms: u64,
    pub replied_ms: Option<u64>,
}

fn noisy_or(ps: &[f64]) -> f64 {
    (1.0 - ps
        .iter()
        .fold(1.0, |acc, p| acc * (1.0 - p.clamp(0.0, 1.0))))
    .min(MAX)
}

/// Stickiness: the share of [`STICKY`] left after `gap_ms`.
pub fn sticky(gap_ms: u64, half_life_ms: u64) -> f64 {
    STICKY * 0.5f64.powf(gap_ms as f64 / half_life_ms.max(1) as f64)
}

/// The follow-up of a reply that ended `gap_ms` ago: it fades four times
/// faster than stickiness.
pub fn chain(gap_ms: u64, half_life_ms: u64) -> f64 {
    CHAIN * 0.5f64.powf(gap_ms as f64 / (half_life_ms.max(4) / 4) as f64)
}

/// One thread's pre-router score and what it stands on.
#[derive(Debug, Clone, PartialEq)]
pub struct Pre {
    pub slug: String,
    pub title: String,
    pub entity: f64,
    pub sticky: f64,
    pub chain: f64,
    pub score: f64,
    pub hits: Vec<String>,
}

/// Score every thread from the message, the place's last exchange and the
/// clock.
pub fn pre_route(
    text: &str,
    threads: &[&ThreadInfo],
    last: Option<&Last>,
    now_ms: u64,
    half_life_ms: u64,
) -> Vec<Pre> {
    let hits: Vec<Vec<(Kind, String)>> = threads
        .iter()
        .map(|t| entity_hits(text, &t.working_set))
        .collect();
    let hitting = hits.iter().filter(|h| !h.is_empty()).count();
    threads
        .iter()
        .zip(&hits)
        .map(|(t, h)| {
            // Each specific hit is evidence; a repo and a root are one (they
            // name the same place in two words).
            let mut strengths: Vec<f64> = h
                .iter()
                .filter(|(k, _)| k.strength() >= STRONG)
                .map(|(k, _)| k.strength())
                .collect();
            if h.iter().any(|(k, _)| k.strength() < STRONG) {
                strengths.push(WEAK);
            }
            let mut entity = noisy_or(&strengths);
            if hitting > 1 {
                entity *= SHARED;
            }
            // A thread the message names nothing of loses stickiness and the
            // chain when another thread's entity is named.
            let other_named = hitting > 0 && h.is_empty();
            let mine = last.filter(|l| l.slug.as_deref() == Some(&t.slug));
            let sticky = mine.map_or(0.0, |l| {
                let s = sticky(now_ms.saturating_sub(l.at_ms), half_life_ms);
                if other_named {
                    s * 0.5
                } else {
                    s
                }
            });
            let chain = mine
                .and_then(|l| l.replied_ms)
                .filter(|_| hitting == 0)
                .map_or(0.0, |r| chain(now_ms.saturating_sub(r), half_life_ms));
            Pre {
                slug: t.slug.clone(),
                title: t.title.clone(),
                entity,
                sticky,
                chain,
                score: noisy_or(&[entity, sticky, chain]),
                hits: h.iter().map(|(_, e)| e.clone()).collect(),
            }
        })
        .collect()
}

/// The two best of `scores`: (index, score), (index, score).
type Ranked = Option<(usize, f64)>;

fn top_two(scores: &[f64]) -> (Ranked, Ranked) {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    let pick = |i: usize| order.get(i).map(|&k| (k, scores[k]));
    (pick(0), pick(1))
}

/// Whether the pre-router alone decides: the best thread at the act bar and
/// clear of the next, so nothing waits for Jev.
pub fn decisive(pre: &[Pre]) -> bool {
    let scores: Vec<f64> = pre.iter().map(|p| p.score).collect();
    match top_two(&scores) {
        (Some((_, a)), Some((_, b))) => a >= ACT && a - b >= CLOSE,
        (Some((_, a)), None) => a >= ACT,
        _ => false,
    }
}

// ---------------------------------------------------------------- fusion and the decision

/// What `classify.v2`'s `thread` question picked.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Thread(String),
    New,
    Home,
}

/// Jev's pick and the probability it was banded at.
#[derive(Debug, Clone, PartialEq)]
pub struct Jev {
    pub pick: Pick,
    pub p: f64,
}

/// A choice the decision weighs.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    Thread(String),
    New,
    Home,
}

/// Every choice's fused score, best first.
pub fn fuse(pre: &[Pre], jev: Option<&Jev>) -> Vec<(Choice, f64)> {
    let mut out: Vec<(Choice, f64)> = pre
        .iter()
        .map(|p| {
            let score = match jev {
                None => p.score,
                Some(j) if j.pick == Pick::Thread(p.slug.clone()) => {
                    noisy_or(&[AGREE * p.score, j.p])
                }
                Some(_) => p.score * OTHERS,
            };
            (Choice::Thread(p.slug.clone()), score)
        })
        .collect();
    let home = match jev {
        Some(Jev {
            pick: Pick::Home,
            p,
        }) => *p,
        _ => 0.0,
    };
    out.push((Choice::Home, home));
    if let Some(Jev { pick: Pick::New, p }) = jev {
        out.push((Choice::New, *p));
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

/// What to do with the message.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Run it in the thread (none: at home).
    Route(ThreadTag),
    /// Offer a new thread, one tap; any other answer is the options'.
    OfferNew { fused: f64 },
    /// Ask in one line.
    Ask { options: Vec<AskChoice> },
}

/// One option of an ask, with the choice it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct AskChoice {
    pub choice: Choice,
    pub option: AskOption,
}

fn tag(
    threads: &[&ThreadInfo],
    choice: &Choice,
    how: RouteHow,
    confidence: f64,
    movable: bool,
) -> ThreadTag {
    let (slug, title) = match choice {
        Choice::Thread(s) => (
            Some(s.clone()),
            threads
                .iter()
                .find(|t| &t.slug == s)
                .map_or_else(|| s.clone(), |t| t.title.clone()),
        ),
        _ => (None, "home".to_string()),
    };
    ThreadTag {
        slug,
        title,
        how,
        confidence,
        movable,
    }
}

/// The ask: the two best threads (or one), `new` when Jev offered it, and
/// home. Keys are `1`, `2`, `new`, `home`.
pub fn ask_options(
    threads: &[&ThreadInfo],
    ranked: &[(Choice, f64)],
    new_title: Option<&str>,
) -> Vec<AskChoice> {
    let mut out = Vec::new();
    let mut n = 0;
    for (c, _) in ranked {
        if let Choice::Thread(s) = c {
            if n >= 2 {
                break;
            }
            n += 1;
            let title = threads
                .iter()
                .find(|t| &t.slug == s)
                .map_or(s.as_str(), |t| t.title.as_str());
            out.push(AskChoice {
                choice: c.clone(),
                option: AskOption {
                    key: n.to_string(),
                    label: title.to_string(),
                },
            });
        }
    }
    if let Some(t) = new_title {
        out.push(AskChoice {
            choice: Choice::New,
            option: AskOption {
                key: "new".into(),
                label: format!("New thread \u{ab}{t}\u{bb}"),
            },
        });
    }
    out.push(AskChoice {
        choice: Choice::Home,
        option: AskOption {
            key: "home".into(),
            label: "home".into(),
        },
    });
    out
}

/// The offer of a new thread: one tap on it, or home.
pub fn offer_new(title: &str) -> Vec<AskChoice> {
    vec![
        AskChoice {
            choice: Choice::New,
            option: AskOption {
                key: "new".into(),
                label: format!("New thread \u{ab}{title}\u{bb}"),
            },
        },
        AskChoice {
            choice: Choice::Home,
            option: AskOption {
                key: "home".into(),
                label: "home".into(),
            },
        },
    ]
}

/// The question in one line, from an ask's thread options.
pub fn question(options: &[AskChoice]) -> String {
    let titles: Vec<&str> = options
        .iter()
        .filter(|o| matches!(o.choice, Choice::Thread(_)))
        .map(|o| o.option.label.as_str())
        .collect();
    match titles.as_slice() {
        [] => "Which thread is this about?".to_string(),
        [one] => format!("Is this about the {one} thread?"),
        [a, b, ..] => format!("Is this about the {a} or the {b}?"),
    }
}

/// How the decision was reached: Jev answered, or the router stands alone
/// (Jev timed out, or is not there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// Jev answered in time.
    Jev,
    /// Jev did not answer in `route_wait_ms`, or cannot.
    Alone,
}

/// The decision table. `threads` are the live ones the scores are of.
pub fn decide(
    threads: &[&ThreadInfo],
    pre: &[Pre],
    jev: Option<&Jev>,
    voice: Voice,
    new_title: Option<&str>,
) -> Decision {
    let ranked = fuse(pre, jev);
    let top = ranked[0].clone();
    let second = ranked
        .iter()
        .skip(1)
        .find(|(c, _)| *c != Choice::New)
        .map_or(0.0, |(_, s)| *s);
    let gap = top.1 - second;
    if voice == Voice::Alone {
        // The pre-router's pick stands, a guess; torn between two, ask.
        return match &top {
            (Choice::Thread(_), s) if *s >= GUESS && gap >= CLOSE => {
                Decision::Route(tag(threads, &top.0, RouteHow::Guess, *s, true))
            }
            (Choice::Thread(_), s) if *s >= GUESS => Decision::Ask {
                options: ask_options(threads, &ranked, None),
            },
            _ => Decision::Route(tag(threads, &Choice::Home, RouteHow::Guess, top.1, true)),
        };
    }
    match &top.0 {
        Choice::New if top.1 >= CONFIRM => Decision::OfferNew { fused: top.1 },
        Choice::Home if top.1 >= ACT => {
            Decision::Route(tag(threads, &top.0, RouteHow::Home, top.1, false))
        }
        c if top.1 >= ACT && gap >= CLOSE => {
            Decision::Route(tag(threads, c, RouteHow::Quiet, top.1, false))
        }
        c if top.1 >= CONFIRM && gap >= CLOSE => {
            Decision::Route(tag(threads, c, RouteHow::Visible, top.1, true))
        }
        _ => {
            // A new thread is an option only when Jev raised it.
            let raised = ranked.iter().any(|(c, _)| *c == Choice::New);
            Decision::Ask {
                options: ask_options(threads, &ranked, new_title.filter(|_| raised)),
            }
        }
    }
}

/// The router's pick with no Jev, for a place with a single live thread: no
/// wait and no ask. The thread when the router is at the confirm bar and clear
/// of home, else home.
pub fn decide_single(threads: &[&ThreadInfo], pre: &[Pre]) -> ThreadTag {
    match pre.first() {
        Some(p) if p.score >= ACT => tag(
            threads,
            &Choice::Thread(p.slug.clone()),
            RouteHow::Quiet,
            p.score,
            false,
        ),
        Some(p) if p.score >= CONFIRM => tag(
            threads,
            &Choice::Thread(p.slug.clone()),
            RouteHow::Visible,
            p.score,
            true,
        ),
        _ => tag(threads, &Choice::Home, RouteHow::Home, 1.0, false),
    }
}

/// A title and a slug for a new thread, from the message: its first few
/// content words.
pub fn propose(text: &str, taken: &[&str]) -> (String, String) {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric()))
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .filter(|w| !COMMON.contains(&w.as_str()))
        .take(3)
        .collect();
    let title = if words.is_empty() {
        "new thread".to_string()
    } else {
        words.join(" ")
    };
    let base: String = words.join("-").chars().take(36).collect();
    let base = if base.is_empty() {
        "thread".to_string()
    } else {
        base
    };
    let mut slug = base.clone();
    let mut n = 2;
    while taken.contains(&slug.as_str()) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    (title, slug)
}

const COMMON: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "you", "your", "are", "was", "can", "could",
    "would", "should", "will", "what", "when", "where", "which", "who", "how", "why", "from",
    "about", "just", "also", "some", "please", "let", "lets", "want", "need", "make", "new",
    "start", "begin", "thread", "now", "then", "have", "has", "had", "not", "but",
];

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::threads::ThreadState;

    fn th(slug: &str, title: &str, ws: WorkingSet) -> ThreadInfo {
        ThreadInfo {
            slug: slug.into(),
            title: title.into(),
            goal: String::new(),
            topics: vec![],
            working_set: ws,
            card: vec![],
            persona: None,
            state: ThreadState::Active,
            sealed: None,
            seal_pending: None,
            surfaces: vec![],
            parent: None,
            merged_into: None,
            split_from: None,
            session_id: None,
            last_active_ms: 1,
            created_at_ms: 1,
        }
    }

    fn ws(
        refs: &[&str],
        repos: &[&str],
        branches: &[&str],
        files: &[&str],
        hosts: &[&str],
        roots: &[&str],
    ) -> WorkingSet {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect();
        WorkingSet {
            roots: v(roots),
            repos: v(repos),
            branches: v(branches),
            refs: v(refs),
            files: v(files),
            hosts: v(hosts),
        }
    }

    fn pr_round() -> ThreadInfo {
        th(
            "pr-round",
            "pr round",
            ws(
                &["#412"],
                &["lanterns/tidewatch"],
                &["lane/pr-round"],
                &["src/lighthouse.rs"],
                &[],
                &["/work/tidewatch"],
            ),
        )
    }

    fn security_fix() -> ThreadInfo {
        th(
            "security-fix",
            "security fix",
            ws(
                &["#977", "tidewatch-9x2k"],
                &["lanterns/tidewatch"],
                &["fix/token-leak"],
                &["src/vault.rs"],
                &["login.example.net"],
                &["/work/vault"],
            ),
        )
    }

    fn refs(v: &[ThreadInfo]) -> Vec<&ThreadInfo> {
        v.iter().collect()
    }

    // ---- addressing

    /// A leading `#slug`, `/t slug`, and "in the <title> thread:" name a live
    /// thread and are taken off; an unknown name is no address.
    #[test]
    fn explicit_words_name_a_live_thread_and_come_off_the_message() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        let ex = |s: &str| explicit(s, &t);
        assert_eq!(
            ex("#pr-round check the lint run"),
            Some(Address::Explicit {
                slug: "pr-round".into(),
                text: "check the lint run".into()
            })
        );
        assert_eq!(
            ex("/t security-fix: rotate the key"),
            Some(Address::Explicit {
                slug: "security-fix".into(),
                text: "rotate the key".into()
            })
        );
        assert_eq!(
            ex("In the security fix thread: rotate the key"),
            Some(Address::Explicit {
                slug: "security-fix".into(),
                text: "rotate the key".into()
            })
        );
        assert_eq!(
            ex("in the PR round thread, rerun it"),
            Some(Address::Explicit {
                slug: "pr-round".into(),
                text: "rerun it".into()
            })
        );
        assert_eq!(ex("#412 is failing"), None, "a number is no slug");
        assert_eq!(ex("#nothing-here do it"), None);
        assert_eq!(ex("in the garden thread: water it"), None);
        assert_eq!(
            ex("please /t pr-round later"),
            None,
            "only a leading word addresses"
        );
    }

    /// The place addresses only when one thread alone lists it; the first
    /// match wins: place, then explicit, then reply.
    #[test]
    fn addressing_goes_place_then_explicit_then_reply() {
        let mut a = pr_round();
        a.surfaces = vec!["discord:channel:7".into()];
        let mut b = security_fix();
        b.surfaces = vec!["discord:channel:8".into(), "cli".into()];
        let all = [a, b];
        let t = refs(&all);
        let at = |text: &'static str, place: Option<&'static str>, reply: Option<&'static str>| {
            address(
                &Addressing {
                    text,
                    place,
                    reply_thread: reply,
                },
                &t,
            )
        };
        assert_eq!(
            at("hello", Some("discord:channel:7"), None),
            Some(Address::Place("pr-round".into()))
        );
        assert_eq!(
            at("hello", None, None),
            Some(Address::Place("security-fix".into())),
            "cli is a place"
        );
        assert_eq!(
            at(
                "#security-fix hello",
                Some("discord:channel:7"),
                Some("security-fix")
            ),
            Some(Address::Place("pr-round".into())),
            "the place comes first"
        );
        assert_eq!(
            at(
                "#security-fix hello",
                Some("discord:dm:1"),
                Some("pr-round")
            ),
            Some(Address::Explicit {
                slug: "security-fix".into(),
                text: "hello".into()
            }),
            "explicit before reply"
        );
        assert_eq!(
            at("hello", Some("discord:dm:1"), Some("pr-round")),
            Some(Address::Reply("pr-round".into()))
        );
        assert_eq!(
            at("hello", Some("discord:dm:1"), Some("gone")),
            None,
            "a reply to a thread no longer live"
        );
        // Two threads listing a place is a pair of candidates.
        let mut c = security_fix();
        c.surfaces = vec!["discord:channel:7".into()];
        let both = [
            {
                let mut p = pr_round();
                p.surfaces = vec!["discord:channel:7".into()];
                p
            },
            c,
        ];
        assert_eq!(by_place(Some("discord:channel:7"), &refs(&both)), None);
    }

    /// The owner's "that was for ..." names a live thread or home, and only
    /// that: any other message is a message.
    #[test]
    fn the_owners_words_that_a_message_was_placed_wrong_are_read_narrowly() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        for (said, want) in [
            ("that was for the security fix", Some("security-fix")),
            (
                "No, that was for the security fix thread.",
                Some("security-fix"),
            ),
            ("Oops, that was for pr-round!", Some("pr-round")),
            ("that was about the PR round", Some("pr-round")),
            ("that should have gone to home", Some("home")),
            ("that was for the garden", None),
            ("that was for the security fix, and rotate the key", None),
            ("was that for the security fix?", None),
            ("rotate the key", None),
        ] {
            assert_eq!(moved_words(said, &t).as_deref(), want, "{said:?}");
        }
    }

    // ---- entities

    #[test]
    fn a_message_names_a_threads_entities_as_whole_tokens() {
        let w = pr_round().working_set;
        let hit = |s: &str| {
            entity_hits(s, &w)
                .into_iter()
                .map(|(_, e)| e)
                .collect::<Vec<_>>()
        };
        assert_eq!(hit("what is failing in PR 412?"), ["#412"]);
        assert_eq!(hit("look at pull/412 again"), ["#412"]);
        assert_eq!(hit("issue #412 again"), ["#412"]);
        assert!(
            hit("we have 412 users").is_empty(),
            "a bare number is no PR"
        );
        assert!(hit("see #4120").is_empty());
        assert_eq!(
            hit("open src/lighthouse.rs and read it"),
            ["src/lighthouse.rs"]
        );
        assert_eq!(
            hit("the lighthouse.rs file"),
            ["src/lighthouse.rs"],
            "a basename counts"
        );
        assert_eq!(hit("push lane/pr-round"), ["lane/pr-round"]);
        assert_eq!(
            hit("tidewatch builds slowly"),
            ["lanterns/tidewatch", "/work/tidewatch"],
            "a repo's own name, and a root's last part"
        );
        assert!(
            hit("the main branch builds").is_empty(),
            "generic words name nothing"
        );
        let s = security_fix().working_set;
        assert_eq!(
            entity_hits("is login.example.net down", &s)[0].1,
            "login.example.net"
        );
        assert_eq!(
            entity_hits("what about tidewatch-9x2k", &s)[0].1,
            "tidewatch-9x2k"
        );
    }

    #[test]
    fn a_unique_hit_is_strong_and_a_shared_one_is_not() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        let p = pre_route("what is failing in PR 412?", &t, None, 1_000, 600_000);
        assert!(p[0].score >= ACT, "{:?}", p[0]);
        assert_eq!(p[1].score, 0.0);
        assert!(decisive(&p));
        // The shared repo name hits both: neither is sure.
        let p = pre_route("tidewatch builds slowly", &t, None, 1_000, 600_000);
        assert!(
            p[0].score < CONFIRM + 0.1 && p[1].score < CONFIRM + 0.1,
            "{p:?}"
        );
        assert!(!decisive(&p));
        // No entity, no history: nothing.
        let p = pre_route("what time is it", &t, None, 1_000, 600_000);
        assert!(p.iter().all(|x| x.score == 0.0));
    }

    // ---- stickiness and the chain

    #[test]
    fn stickiness_is_strong_for_a_minute_and_weak_after_an_hour() {
        let hl = 600_000;
        assert!(sticky(0, hl) > 0.59);
        assert!(sticky(60_000, hl) > 0.55, "a minute");
        assert!(
            (sticky(600_000, hl) - 0.3).abs() < 1e-9,
            "one half-life halves it"
        );
        assert!(sticky(3_600_000, hl) < 0.01, "an hour");
        assert!(chain(0, hl) > 0.69 && chain(150_000, hl) < 0.36 && chain(3_600_000, hl) < 0.001);
        // Together they never reach the act bar: a prior is not a decision.
        assert!(noisy_or(&[sticky(0, hl), chain(0, hl)]) < ACT);
    }

    #[test]
    fn right_after_a_threads_reply_its_follow_up_stays_and_a_new_entity_breaks_it() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        let last = Last {
            slug: Some("security-fix".into()),
            at_ms: 100_000,
            replied_ms: Some(110_000),
        };
        let now = 130_000;
        let p = pre_route("and the tests too", &t, Some(&last), now, 600_000);
        assert!(p[1].score >= CONFIRM && p[1].score < ACT, "{:?}", p[1]);
        assert_eq!(p[0].score, 0.0);
        assert!(
            !decisive(&p),
            "a follow-up is a prior, Jev tells a fragment"
        );
        // A message naming the other thread's PR goes there, and the last
        // thread's stickiness and chain fall away.
        let p = pre_route("and PR 412 too", &t, Some(&last), now, 600_000);
        assert!(p[0].score >= ACT && p[1].score < 0.35, "{p:?}");
        assert_eq!(p[1].chain, 0.0);
        // An hour later the thread is not the place's any more.
        let late = Last {
            slug: Some("security-fix".into()),
            at_ms: 100_000,
            replied_ms: Some(110_000),
        };
        let p = pre_route("and the tests too", &t, Some(&late), 3_800_000, 600_000);
        assert!(p.iter().all(|x| x.score < GUESS), "{p:?}");
    }

    // ---- fusion and the decision table

    fn tags(d: &Decision) -> (RouteHow, Option<&str>, bool) {
        match d {
            Decision::Route(t) => (t.how, t.slug.as_deref(), t.movable),
            other => panic!("not a route: {other:?}"),
        }
    }

    fn pre_with(scores: &[(&str, f64)]) -> Vec<Pre> {
        scores
            .iter()
            .map(|(s, p)| Pre {
                slug: (*s).into(),
                title: s.replace('-', " "),
                entity: *p,
                sticky: 0.0,
                chain: 0.0,
                score: *p,
                hits: vec![],
            })
            .collect()
    }

    /// Every band of the table, with Jev answering.
    #[test]
    fn the_decision_table_by_band() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        let jev = |s: &str, p: f64| Jev {
            pick: Pick::Thread(s.into()),
            p,
        };
        // Jev sure, the router silent: quiet.
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
            Some(&jev("pr-round", 0.95)),
            Voice::Jev,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Quiet, Some("pr-round"), false));
        // Jev likely: a visible tag and a move handle.
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
            Some(&jev("security-fix", 0.75)),
            Voice::Jev,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Visible, Some("security-fix"), true));
        // Jev unsure: ask, with both threads and home.
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
            Some(&jev("security-fix", 0.45)),
            Voice::Jev,
            None,
        );
        let Decision::Ask { options } = &d else {
            panic!("{d:?}")
        };
        let keys: Vec<&str> = options.iter().map(|o| o.option.key.as_str()).collect();
        assert_eq!(keys, ["1", "2", "home"]);
        // Two threads within 0.15 of each other: ask, whatever the top.
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.80), ("security-fix", 0.0)]),
            Some(&jev("security-fix", 0.70)),
            Voice::Jev,
            None,
        );
        assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
        // Jev and the router agree: the evidence adds (Jev's 0.85 alone is
        // only likely).
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
            Some(&jev("pr-round", 0.85)),
            Voice::Jev,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Visible, Some("pr-round"), true));
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.50), ("security-fix", 0.0)]),
            Some(&jev("pr-round", 0.85)),
            Voice::Jev,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Quiet, Some("pr-round"), false));
        // Jev overrules a sticky router when it is sure.
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.80), ("security-fix", 0.0)]),
            Some(&jev("security-fix", 0.96)),
            Voice::Jev,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Quiet, Some("security-fix"), false));
        // Home, sure: no thread, no tag. Home, likely: asks nothing but offers a move.
        let home = |p: f64| Jev {
            pick: Pick::Home,
            p,
        };
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.2), ("security-fix", 0.0)]),
            Some(&home(0.95)),
            Voice::Jev,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Home, None, false));
        // A new thread at the confirm bar is offered; below it, asked.
        let new = |p: f64| Jev { pick: Pick::New, p };
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.1), ("security-fix", 0.0)]),
            Some(&new(0.7)),
            Voice::Jev,
            Some("tide gauge"),
        );
        assert!(matches!(d, Decision::OfferNew { .. }), "{d:?}");
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.1), ("security-fix", 0.0)]),
            Some(&new(0.5)),
            Voice::Jev,
            Some("tide gauge"),
        );
        let Decision::Ask { options } = &d else {
            panic!("{d:?}")
        };
        assert!(options
            .iter()
            .any(|o| o.option.key == "new" && o.option.label.contains("tide gauge")));
    }

    /// Without Jev (a timeout or none), the router's pick stands as a guess,
    /// and two close threads are asked.
    #[test]
    fn without_jev_the_routers_pick_stands_as_a_guess() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.0), ("security-fix", 0.70)]),
            None,
            Voice::Alone,
            None,
        );
        assert_eq!(tags(&d), (RouteHow::Guess, Some("security-fix"), true));
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.5), ("security-fix", 0.55)]),
            None,
            Voice::Alone,
            None,
        );
        assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
        let d = decide(
            &t,
            &pre_with(&[("pr-round", 0.05), ("security-fix", 0.0)]),
            None,
            Voice::Alone,
            None,
        );
        assert_eq!(
            tags(&d),
            (RouteHow::Guess, None, true),
            "nothing to go on: home, said as a guess"
        );
    }

    #[test]
    fn a_single_threads_place_waits_for_nothing() {
        let all = [pr_round()];
        let t = refs(&all);
        assert_eq!(
            decide_single(&t, &pre_with(&[("pr-round", 0.95)])).how,
            RouteHow::Quiet
        );
        let v = decide_single(&t, &pre_with(&[("pr-round", 0.7)]));
        assert_eq!((v.how, v.movable), (RouteHow::Visible, true));
        let h = decide_single(&t, &pre_with(&[("pr-round", 0.2)]));
        assert_eq!((h.how, h.slug), (RouteHow::Home, None));
    }

    #[test]
    fn the_question_is_one_line_and_the_options_are_keyed() {
        let all = [pr_round(), security_fix()];
        let t = refs(&all);
        let ranked = vec![
            (Choice::Thread("pr-round".into()), 0.5),
            (Choice::Thread("security-fix".into()), 0.45),
            (Choice::Home, 0.0),
        ];
        let opts = ask_options(&t, &ranked, Some("tide gauge"));
        assert_eq!(
            question(&opts),
            "Is this about the pr round or the security fix?"
        );
        let keys: Vec<&str> = opts.iter().map(|o| o.option.key.as_str()).collect();
        assert_eq!(keys, ["1", "2", "new", "home"]);
        assert_eq!(opts[2].option.label, "New thread \u{ab}tide gauge\u{bb}");
        assert_eq!(question(&opts[..1]), "Is this about the pr round thread?");
    }

    #[test]
    fn a_new_threads_title_and_slug_come_from_the_message() {
        assert_eq!(
            propose("Let's look at the tide gauge calibration", &[]),
            ("look tide gauge".to_string(), "look-tide-gauge".to_string())
        );
        let (_, slug) = propose("tide gauge calibration notes", &["tide-gauge-calibration"]);
        assert_eq!(slug, "tide-gauge-calibration-2");
        assert_eq!(propose("ok", &[]).1, "thread");
    }
}
