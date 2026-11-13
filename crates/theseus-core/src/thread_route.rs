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
//!    once). A thread whose **title** the message names (every content word of
//!    it, [`thread_title`](crate::thread_title)) adds evidence of 0.35, not
//!    shared out, so two titles named make two contenders (theseus-4qdh8).
//!    The four combine as independent evidence, `1 - Π(1 - p)`. Only a
//!    named entity decides alone: stickiness and the chain are priors, capped
//!    below the act bar together (0.88), since in the owner's DMs a third to
//!    a half of consecutive messages switch concern, and a follow-up that
//!    reads like a fragment ("and the tests too") is Jev's to tell. Then
//!    **the directory rule** ([`apart_by_dir`], theseus-vw8k7): when the
//!    turn's directory is known and a thread has roots, none of which holds
//!    it or lies under it, that thread keeps only its entity score: the
//!    priors and a learned correction name nothing of it, and the message
//!    comes from somewhere else; a file or root it names by a bare or
//!    relative name is another directory's file, and counts only when named
//!    by its absolute path. A thread with no roots, or a turn with no
//!    directory, is scored as before.
//! 3. **Fusion** with `classify.v2`'s `thread` answer (the pick `p`): the
//!    picked thread scores `1 - (1 - 0.8·pre)(1 - p)`; every other thread
//!    keeps `0.85·pre`; `new_thread` and `home` score `p` when picked. A weighted
//!    max where Jev agrees or not, and the evidence adds where it agrees.
//! 4. **The decision**, at the pack's bars (`act` 0.90, `confirm` 0.60): the
//!    top choice at 0.90 or more and 0.15 clear of the next routes with a quiet
//!    tag; 0.60 to 0.90, clear, routes with a tag and a move handle; anything
//!    else asks in one line only when two threads contend (each at
//!    `CONTEND`, 0.25, or more) or a new thread is raised; a message no two
//!    threads contend for asks nothing (theseus-e31an) and runs, movable,
//!    in the lone thread when that thread is the top choice and at the
//!    confirm bar, else at home (theseus-m7fwf). Jev not answering in time
//!    (or not there) leaves the pre-router's pick standing, tagged a guess,
//!    and asks only when two contending threads are close -- an ask that
//!    offers a new thread when the message names two entities no live thread
//!    holds, since no Jev ranked one.
//!
//! A `new_thread` pick at 0.60 offers a new thread and waits for one tap; a
//! one-off runs in the place's own conversation ("home") and never makes a
//! thread.

use std::path::{Path, PathBuf};

use theseus_protocol::thread_route::{AskOption, RouteHow, ThreadTag};
use theseus_protocol::threads::{ThreadInfo, WorkingSet};

use crate::toolrun::misroute::{fold, normal};
use crate::{generic_folder::is_generic_folder, thread_title::title_hit};

/// The classify pack's bars for the `thread` question.
pub const ACT: f64 = 0.90;
pub const CONFIRM: f64 = 0.60;
/// Two choices this close are a tie, and a tie is asked.
pub const CLOSE: f64 = 0.15;
/// The guess floor: below it the router has no pick without Jev.
const GUESS: f64 = 0.30;
/// A thread with this much fused evidence contends: an ask needs two of them
/// (theseus-e31an), so a lone weak lean runs at home instead of asking.
const CONTEND: f64 = 0.25;
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
pub(crate) const GENERIC: &[&str] = &[
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
    for r in w.roots.iter().filter(|r| !is_generic_folder(base_of(r))) {
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
    /// The owner moved a message like this one to the thread: its score was
    /// lifted to [`LEARNED`] (`rpc/thread_flow.rs`'s `learned_thread`).
    pub learned: bool,
    /// The raw (stickiness, chain), before another thread's hit took any
    /// of it: what the directory rule rescores from.
    standing: (f64, f64),
}

/// A thread's entity score from its hits, `hitting` threads hit in all.
fn entity_of(h: &[(Kind, String)], hitting: usize) -> f64 {
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
    let entity = noisy_or(&strengths);
    match hitting > 1 {
        true => entity * SHARED,
        false => entity,
    }
}

/// Every thread's (entity, stickiness, chain) from its hits and its raw
/// (stickiness, chain): one thread's entity counts less when another is
/// hit too, and a thread the message names nothing of loses half its
/// stickiness and its chain when another thread is named. A thread is named
/// by an entity hit or by its title (`titled`, theseus-7nx13): right after
/// one thread's reply, a message naming another by title no longer leaves
/// the replier its whole chain. A title naming the thread itself leaves its
/// own chain alone; an entity hit anywhere still takes every chain.
fn settle(
    hits: &[Vec<(Kind, String)>],
    titled: &[bool],
    raw: &[(f64, f64)],
) -> Vec<(f64, f64, f64)> {
    let hitting = hits.iter().filter(|h| !h.is_empty()).count();
    let named: Vec<bool> = hits
        .iter()
        .zip(titled)
        .map(|(h, &t)| t || !h.is_empty())
        .collect();
    let naming = named.iter().filter(|n| **n).count();
    let titles = titled.iter().filter(|t| **t).count();
    hits.iter()
        .zip(raw)
        .zip(named.iter().zip(titled))
        .map(|((h, &(sticky, chain)), (&named, &titled))| {
            let other_named = naming > 0 && !named;
            let other_titled = titles > usize::from(titled);
            (
                entity_of(h, hitting),
                if other_named { sticky * 0.5 } else { sticky },
                if hitting == 0 && !other_titled {
                    chain
                } else {
                    0.0
                },
            )
        })
        .collect()
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
    let standing: Vec<(f64, f64)> = threads
        .iter()
        .map(|t| {
            let mine = last.filter(|l| l.slug.as_deref() == Some(&t.slug));
            (
                mine.map_or(0.0, |l| {
                    sticky(now_ms.saturating_sub(l.at_ms), half_life_ms)
                }),
                mine.and_then(|l| l.replied_ms)
                    .map_or(0.0, |r| chain(now_ms.saturating_sub(r), half_life_ms)),
            )
        })
        .collect();
    // A title the message names is evidence beside the entity, never shared
    // out (theseus-4qdh8); it names the thread to `settle` too (theseus-7nx13).
    let titles: Vec<Option<f64>> = threads.iter().map(|t| title_hit(text, &t.title)).collect();
    let titled: Vec<bool> = titles.iter().map(Option::is_some).collect();
    let settled = settle(&hits, &titled, &standing);
    threads
        .iter()
        .zip(hits)
        .zip(standing.into_iter().zip(settled))
        .zip(titles)
        .map(
            |(((t, h), (standing, (entity, sticky, chain))), title)| Pre {
                slug: t.slug.clone(),
                title: t.title.clone(),
                entity,
                sticky,
                chain,
                score: noisy_or(&[entity, title.unwrap_or(0.0), sticky, chain]),
                hits: (h.into_iter().map(|(_, e)| e))
                    .chain(title.map(|_| t.title.clone()))
                    .collect(),
                learned: false,
                standing,
            },
        )
        .collect()
}

// ---------------------------------------------------------------- the directory rule

/// A working set's path entry as a path, read with no filesystem: `~`
/// expanded by `home`, `.` and `..` folded; a relative entry lies under
/// `dir`, and is none without it.
pub fn entry_path(e: &str, dir: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    let e = e.trim();
    if e.is_empty() {
        return None;
    }
    let p = normal(e, home);
    match p.is_absolute() {
        true => Some(p),
        false => dir.map(|d| fold(&Path::new(d).join(&p))),
    }
}

/// Whether `p` is one of the thread's `files` or lies under one of its
/// `roots` (by components), relative entries read under `dir`.
pub fn holds_path(t: &ThreadInfo, p: &Path, dir: Option<&str>, home: Option<&Path>) -> bool {
    let w = &t.working_set;
    w.files
        .iter()
        .filter_map(|f| entry_path(f, dir, home))
        .any(|f| f == p)
        || w.roots
            .iter()
            .filter_map(|r| entry_path(r, dir, home))
            .any(|r| p.starts_with(r))
}

/// The directory rule's silence, in words (theseus-2gtyi): when the turn's
/// directory lies above every root, `apart_by_dir` finds each thread near
/// (a root lies under the directory), drops nothing, and so says nothing.
/// This says it: none of the threads with roots has a root holding the
/// directory, and at least one has a root under it. Under a root, apart
/// from all of them, with no roots or no directory: none (the rule speaks
/// for itself, or has nothing to say).
pub fn above_every_root<'b>(
    threads: &[&ThreadInfo],
    dir: Option<&str>,
    base: impl Fn(&ThreadInfo) -> Option<&'b str>,
    home: Option<&Path>,
) -> Option<String> {
    let here = dir.and_then(|d| entry_path(d, None, home))?;
    let mut above = Vec::new();
    for t in threads {
        let roots: Vec<PathBuf> = (t.working_set.roots.iter())
            .filter_map(|r| entry_path(r, base(t), home))
            .collect();
        if roots.iter().any(|r| here.starts_with(r)) {
            return None;
        }
        if roots.iter().any(|r| r.starts_with(&here)) {
            above.push(t.slug.as_str());
        }
    }
    (!above.is_empty()).then(|| {
        format!(
            "This turn ran in {}, which lies above the roots of {} and inside none of them, so the directory \
             placed the message in none of those; a thread whose roots lie elsewhere is still set apart. \
             To place a message by directory, cd into a root, name a file by its absolute path, or /move it.",
            here.display(),
            above.join(", ")
        )
    })
}

/// The directory rule (theseus-vw8k7): a thread whose roots are known keeps
/// only the evidence that names it when the turn runs in a directory apart
/// from all of them. With the turn's directory known (absolute after `~`),
/// a thread that has roots (each read under `base(thread)`, the directory
/// its relative roots were taken in; a relative one with none is unread)
/// none of which holds the directory or lies under it loses stickiness, the
/// reply chain and a learned correction, and a file or root hit counts only
/// where the message names the entry's absolute path whole: a bare or
/// relative name (`slow.sh`, which a working set grows from a message) names
/// another file from another directory. Its score is what entity score is
/// left. Jev's pick still fuses afterwards. A thread with no readable root,
/// or a turn with no directory, is left as it was. Returns the threads whose
/// evidence was dropped.
pub fn apart_by_dir<'b>(
    pre: &mut [Pre],
    threads: &[&ThreadInfo],
    text: &str,
    dir: Option<&str>,
    base: impl Fn(&ThreadInfo) -> Option<&'b str>,
    home: Option<&Path>,
) -> Vec<String> {
    let Some(here) = dir.and_then(|d| entry_path(d, None, home)) else {
        return Vec::new();
    };
    let lower = text.to_lowercase();
    let mut dropped = Vec::new();
    let mut apart = vec![false; pre.len()];
    let mut hits = Vec::with_capacity(pre.len());
    for (p, apart) in pre.iter().zip(&mut apart) {
        let Some(t) = threads.iter().find(|t| t.slug == p.slug) else {
            hits.push(Vec::new());
            continue;
        };
        let roots: Vec<PathBuf> = t
            .working_set
            .roots
            .iter()
            .filter_map(|r| entry_path(r, base(t), home))
            .collect();
        let near = roots
            .iter()
            .any(|r| here.starts_with(r) || r.starts_with(&here));
        let mut h = entity_hits(text, &t.working_set);
        if !roots.is_empty() && !near {
            let named_whole = |e: &str| {
                entry_path(e, None, home).is_some() && has_token(&lower, &e.trim().to_lowercase())
            };
            let before = h.len();
            h.retain(|(k, e)| !matches!(k, Kind::File | Kind::Root) || named_whole(e));
            *apart = true;
            // Entity hits alone: `p.hits` also holds a title the message names,
            // which the rule keeps (theseus-axno0).
            if p.sticky > 0.0 || p.chain > 0.0 || p.learned || h.len() < before {
                dropped.push(p.slug.clone());
            }
        }
        hits.push(h);
    }
    if !apart.contains(&true) {
        return dropped;
    }
    // What is left is scored as `pre_route` would have, had the dropped hits
    // never been there: another thread's entity counts less only while
    // another thread is still hit, and stickiness is halved only while
    // another thread is still named.
    let raw: Vec<(f64, f64)> = pre
        .iter()
        .zip(&apart)
        .map(|(p, &a)| if a { (0.0, 0.0) } else { p.standing })
        .collect();
    // A title the message names counts for every thread, an apart one too:
    // the rule drops what only looks like naming (a bare file name from
    // another directory), and a title said whole names the thread wherever
    // the turn runs (theseus-4qdh8, theseus-7nx13).
    let titles: Vec<Option<f64>> = pre.iter().map(|p| title_hit(text, &p.title)).collect();
    let titled: Vec<bool> = titles.iter().map(Option::is_some).collect();
    let settled = settle(&hits, &titled, &raw);
    for ((((p, h), (entity, sticky, chain)), apart), title) in
        pre.iter_mut().zip(hits).zip(settled).zip(apart).zip(titles)
    {
        p.entity = entity;
        p.sticky = sticky;
        p.chain = chain;
        p.hits = (h.into_iter().map(|(_, e)| e))
            .chain(title.map(|_| p.title.clone()))
            .collect();
        p.score = noisy_or(&[entity, title.unwrap_or(0.0), sticky, chain]);
        match apart {
            true => p.learned = false,
            // The owner's correction lifted this score past what it stands on.
            false if p.learned => p.score = p.score.max(LEARNED),
            false => {}
        }
    }
    dropped
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
    /// A thread the ask does not list: answered by its slug (theseus-98bz.4).
    Other,
}

/// What the decision reads beside the scores (theseus-eqp1v).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rules {
    /// `[threads] sticky_alone_asks`: a thread that stands on stickiness and
    /// the reply chain alone (no entity named, no learned correction) cannot
    /// overrule Jev's other pick; the message is asked about instead.
    pub sticky_alone_asks: bool,
    /// The message's concrete entities that no live thread's working set
    /// holds ([`new_entities`]): with two or more, an ask offers a new thread.
    pub new_entities: usize,
    /// `[threads] ask_every_thread` (theseus-98bz.4): an ask lists every live,
    /// unsealed thread (four at most, else three and `other`), not the best two.
    pub ask_every_thread: bool,
    /// `[threads] unsure = "guess"` (theseus-ttlze): an ask or a new-thread
    /// offer becomes a [`RouteHow::Guess`] placement ([`guessed`]).
    pub guess_unsure: bool,
}

/// The most threads an ask lists by name; past it, [`LISTED_PAST`] and `other`.
const LISTED: usize = 4;
const LISTED_PAST: usize = 3;

/// Just below the confirm bar: where a sticky-only thread Jev disagrees with
/// is held, so the decision falls to the ask.
const STICKY_CAP: f64 = CONFIRM - 0.01;

/// The message's concrete entities (refs, branches, files, hosts, repos, as
/// the import's extractor finds them) that no live thread's working set holds.
pub fn new_entities(text: &str, threads: &[&ThreadInfo]) -> usize {
    let named = crate::threads_import::extract::working_set([text]);
    let held = |e: &str| {
        threads.iter().any(|t| {
            let w = &t.working_set;
            [&w.refs, &w.branches, &w.files, &w.hosts, &w.repos, &w.roots]
                .iter()
                .any(|l| l.iter().any(|x| x.eq_ignore_ascii_case(e)))
        })
    };
    [
        &named.refs,
        &named.branches,
        &named.files,
        &named.hosts,
        &named.repos,
    ]
    .iter()
    .flat_map(|l| l.iter())
    .filter(|e| !held(e))
    .count()
}

/// Stickiness alone does not overrule Jev (theseus-eqp1v): a top thread whose
/// score stands on no entity and no learned correction, which Jev did not
/// pick, is held just below the confirm bar.
/// Whether it held one: the ask that follows is the cap's (theseus-q3ggn).
fn hold_sticky(ranked: &mut [(Choice, f64)], pre: &[Pre], jev: &Jev) -> bool {
    let Some((Choice::Thread(slug), score)) = ranked.first_mut() else {
        return false;
    };
    let alone = pre
        .iter()
        .find(|p| &p.slug == slug)
        .is_some_and(|p| p.entity <= 0.0 && !p.learned);
    let held = alone && jev.pick != Pick::Thread(slug.clone()) && *score > STICKY_CAP;
    if held {
        *score = STICKY_CAP;
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    held
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

/// The threads an ask may name: the live ones not sealed.
pub fn askable<'a>(threads: &[&'a ThreadInfo]) -> Vec<&'a ThreadInfo> {
    threads
        .iter()
        .copied()
        .filter(|t| t.sealed.is_none())
        .collect()
}

/// The ask: its threads by fused score (every live, unsealed one when there
/// are four or fewer, else the best three and `other`, with `every`; else the
/// best two), `new` when raised, and home. Keys are `1`, `2`, …, `other`,
/// `new`, `home`.
pub fn ask_options(
    threads: &[&ThreadInfo],
    ranked: &[(Choice, f64)],
    new_title: Option<&str>,
    every: bool,
) -> Vec<AskChoice> {
    let mut out = Vec::new();
    let named: Vec<&ThreadInfo> = match every {
        true => askable(threads),
        false => threads.to_vec(),
    };
    let listed = |s: &str| named.iter().any(|t| t.slug == s);
    let total = ranked
        .iter()
        .filter(|(c, _)| matches!(c, Choice::Thread(s) if listed(s)))
        .count();
    let most = match (every, total > LISTED) {
        (false, _) => 2,
        (true, false) => LISTED,
        (true, true) => LISTED_PAST,
    };
    let mut n = 0;
    for (c, _) in ranked {
        if let Choice::Thread(s) = c {
            if n >= most {
                break;
            }
            if !listed(s) {
                continue;
            }
            n += 1;
            let title = named
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
    if every && total > n {
        out.push(AskChoice {
            choice: Choice::Other,
            option: AskOption {
                key: "other".into(),
                label: "another thread".into(),
            },
        });
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

/// The question in one line, from an ask's thread options: every thread it
/// lists, and "or another thread" when it does not list them all.
pub fn question(options: &[AskChoice]) -> String {
    let titles: Vec<String> = options
        .iter()
        .filter(|o| matches!(o.choice, Choice::Thread(_)))
        .map(|o| format!("the {}", o.option.label))
        .collect();
    let other = options.iter().any(|o| o.choice == Choice::Other);
    match (titles.as_slice(), other) {
        ([], _) => "Which thread is this about?".to_string(),
        ([one], false) => format!("Is this about {one} thread?"),
        (all, false) => {
            let (last, rest) = all.split_last().expect("two or more");
            format!("Is this about {} or {last}?", rest.join(", "))
        }
        (all, true) => format!("Is this about {} or another thread?", all.join(", ")),
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

/// The decision table. `threads` are the live ones the scores are of. With
/// `rules.guess_unsure` no path answers an ask or an offer: see [`guessed`].
pub fn decide(
    threads: &[&ThreadInfo],
    pre: &[Pre],
    jev: Option<&Jev>,
    voice: Voice,
    new_title: Option<&str>,
    rules: Rules,
) -> Decision {
    let d = decide_table(threads, pre, jev, voice, new_title, rules);
    match rules.guess_unsure {
        true => guessed(d, threads, pre, jev),
        false => d,
    }
}

/// Under `unsure = "guess"` every `Ask` and `OfferNew` is a `Route` tagged
/// `Guess`: Jev's pick when it answered, else the router's top, else home.
/// A guess never lands in a sealed or seal-pending thread (it goes home), and
/// never makes a thread: a pick of `New` is a guess of home. Movable, so the
/// reply can offer the change.
pub fn guessed(d: Decision, threads: &[&ThreadInfo], pre: &[Pre], jev: Option<&Jev>) -> Decision {
    if matches!(d, Decision::Route(_)) {
        return d;
    }
    let ranked = fuse(pre, jev);
    let (choice, conf) = ranked
        .into_iter()
        .find(|(c, _)| *c != Choice::New)
        .unwrap_or((Choice::Home, 0.0));
    let open = |s: &str| {
        threads
            .iter()
            .any(|t| t.slug == s && t.sealed.is_none() && t.seal_pending.is_none())
    };
    let choice = match choice {
        Choice::Thread(s) if open(&s) => Choice::Thread(s),
        _ => Choice::Home,
    };
    Decision::Route(tag(threads, &choice, RouteHow::Guess, conf, true))
}

fn decide_table(
    threads: &[&ThreadInfo],
    pre: &[Pre],
    jev: Option<&Jev>,
    voice: Voice,
    new_title: Option<&str>,
    rules: Rules,
) -> Decision {
    let mut ranked = fuse(pre, jev);
    let held = match (rules.sticky_alone_asks, voice, jev) {
        (true, Voice::Jev, Some(j)) => hold_sticky(&mut ranked, pre, j),
        _ => false,
    };
    let top = ranked[0].clone();
    let second = ranked
        .iter()
        .skip(1)
        .find(|(c, _)| *c != Choice::New)
        .map_or(0.0, |(_, s)| *s);
    let gap = top.1 - second;
    let contenders = ranked
        .iter()
        .filter(|(c, s)| matches!(c, Choice::Thread(_)) && *s >= CONTEND)
        .count();
    if voice == Voice::Alone {
        // No Jev ranked a `New`, so the message's own entities are the only
        // evidence of a new stream: the bar `lone_offer` uses (theseus-m7fwf).
        // It only adds an option to the ask below; it never makes one.
        let raised = rules.new_entities >= 2;
        // The pre-router's pick stands, a guess; torn between two, ask.
        return match &top {
            (Choice::Thread(_), s) if *s >= GUESS && gap >= CLOSE => {
                Decision::Route(tag(threads, &top.0, RouteHow::Guess, *s, true))
            }
            (Choice::Thread(_), s) if *s >= GUESS && contenders >= 2 => Decision::Ask {
                options: ask_options(
                    threads,
                    &ranked,
                    new_title.filter(|_| raised),
                    rules.ask_every_thread,
                ),
            },
            (Choice::Thread(_), s) if *s >= GUESS => {
                Decision::Route(tag(threads, &top.0, RouteHow::Guess, *s, true))
            }
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
            // A new thread is an option when Jev raised it, or when the
            // stickiness cap made this ask and the message names two or
            // more entities no live thread holds (theseus-q3ggn).
            let raised =
                ranked.iter().any(|(c, _)| *c == Choice::New) || (held && rules.new_entities >= 2);
            if contenders < 2 && !raised {
                // No two threads contend: nothing to ask about (theseus-e31an).
                // The lead still stands, though. A lone thread at the confirm
                // bar is here only because it sits close to home, and handing
                // the turn to the choice it beat threw that away
                // (theseus-m7fwf): it is routed to the thread, visibly and
                // movable, which costs no question. Under the bar, home.
                return match &top {
                    (Choice::Thread(_), s) if *s >= CONFIRM => {
                        Decision::Route(tag(threads, &top.0, RouteHow::Visible, *s, true))
                    }
                    _ => {
                        let home = ranked
                            .iter()
                            .find(|(c, _)| *c == Choice::Home)
                            .map_or(0.0, |(_, s)| *s);
                        Decision::Route(tag(threads, &Choice::Home, RouteHow::Home, home, true))
                    }
                };
            }
            Decision::Ask {
                options: ask_options(
                    threads,
                    &ranked,
                    new_title.filter(|_| raised),
                    rules.ask_every_thread,
                ),
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

/// A place with one live thread offers a new one (theseus-98bz.7): when the
/// thread's score is below the confirm bar and the message names two or more
/// entities no working set holds (`new_entities`), the owner is asked: the
/// thread, a new one titled `new_title`, or home. Otherwise none, and
/// [`decide_single`] decides as before.
pub fn lone_offer(
    threads: &[&ThreadInfo],
    pre: &[Pre],
    new_entities: usize,
    new_title: &str,
    every: bool,
) -> Option<Decision> {
    let p = pre.first()?;
    (p.score < CONFIRM && new_entities >= 2).then(|| Decision::Ask {
        options: ask_options(threads, &fuse(pre, None), Some(new_title), every),
    })
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
mod tests;
