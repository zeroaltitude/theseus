//! Addressing: which thread a message names (theseus-rha2; the owner,
//! 2026-11-21: a message goes to a thread by address only).
//!
//! Pure functions over the threads in memory and a message; the flow that
//! runs a turn where its address says is `rpc/thread_flow.rs`. A message
//! goes to a thread only by its address, first match wins, certain: the
//! **place** a thread alone lists in its `surfaces`; an **explicit** word
//! (`--thread`, a leading `#slug`, `/t slug`, or "in the <title> thread:");
//! a **reply** to a message that a thread's turn wrote. A message with no
//! address runs in the place's own conversation ("home"), always: nothing
//! here guesses from what it says.
//!
//! The entity readers ([`entity_hits`], [`new_entities`]) are not routing:
//! they name what a message says of a working set, for Jev's line about a
//! thread, a working set's growth, and the generic-folder check.

use std::path::{Component, Path, PathBuf};

use theseus_protocol::thread_route::RouteHow;
use theseus_protocol::threads::{ThreadInfo, WorkingSet};

use crate::generic_folder::is_generic_folder;

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

/// An address given that cannot be honoured: the words a refusal may show
/// (none: it may show none) and the thread it names when this reader may be
/// told of it. It is refused, never run anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub word: Option<String>,
    pub slug: Option<String>,
}

/// The word an explicit address gives, as typed, and whether it came after
/// `/t`: after a leading `#`, after `/t `, or between "in the " and "
/// thread:" (or " thread,").
fn address_word(text: &str) -> Option<(String, bool)> {
    let t = text.trim_start();
    if let Some(rest) = t.strip_prefix('#') {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == ':' || c == ',')
            .unwrap_or(rest.len());
        return Some((rest[..end].to_string(), false));
    }
    let lower = t.to_lowercase();
    if lower.starts_with("/t ") || lower.starts_with("/t\t") {
        let word = t[3..].split_whitespace().next()?;
        return Some((word.to_string(), true));
    }
    if lower.starts_with("in the ") {
        let head: String = t.chars().take(120).collect();
        let lh = head.to_ascii_lowercase();
        let at = [" thread:", " thread,"]
            .iter()
            .filter_map(|stop| lh.find(stop))
            .min()?;
        return Some((head[7..at].trim().to_string(), false));
    }
    None
}

/// The explicit address of `text` that failed, read BEFORE the place and
/// the reply, so a failed address never falls through to another thread
/// (issue 44): its word names no `live` thread, and names one of `told` (a
/// thread that is not live and that this reader may be told of: the gate's
/// call, by the caller), or came after `/t`. A `#word` or "in the …" that
/// names none of them is no address at all, whatever else it names: a
/// thread the reader may not be told of reads as no thread.
pub fn failed_explicit(
    text: &str,
    live: &[&ThreadInfo],
    told: &[&ThreadInfo],
) -> Option<Unresolved> {
    let (word, slash) = address_word(text)?;
    if word.is_empty() || explicit(text, live).is_some() {
        return None;
    }
    match named(&word, told) {
        Some(t) => Some(Unresolved {
            word: Some(word),
            slug: Some(t.slug.clone()),
        }),
        None => slash.then_some(Unresolved {
            word: Some(word),
            slug: None,
        }),
    }
}

/// The word an explicit address in `text` gives, as typed, if it has one.
pub fn typed_word(text: &str) -> Option<String> {
    address_word(text).map(|(w, _)| w).filter(|w| !w.is_empty())
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

// ---------------------------------------------------------------- the place's memory

/// The place's last exchange: the thread it ran in (none: home), when the
/// message arrived, and when the thread's reply ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Last {
    pub slug: Option<String>,
    pub at_ms: u64,
    pub replied_ms: Option<u64>,
}

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

/// The message's concrete entities (refs, branches, files, hosts, repos, as
/// the working set's extractor finds them) that no live thread's working set holds.
pub fn new_entities(text: &str, threads: &[&ThreadInfo]) -> usize {
    let named = crate::thread_grow::extract::working_set([text]);
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

/// `path` with `~` expanded and `.`/`..` folded, no filesystem read.
fn normal(path: &str, home: Option<&Path>) -> PathBuf {
    let expanded = match (path.strip_prefix("~/"), path == "~", home) {
        (Some(rest), _, Some(h)) => h.join(rest),
        (_, true, Some(h)) => h.to_path_buf(),
        _ => PathBuf::from(path),
    };
    fold(&expanded)
}

fn fold(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests;
