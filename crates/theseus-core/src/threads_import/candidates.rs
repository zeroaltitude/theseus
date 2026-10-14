//! The candidates (theseus-71ib): pure over what the scan read. No store, no
//! clock (`now` is the caller's), no model.

use std::collections::{BTreeMap, BTreeSet};

use theseus_protocol::threads::{slug_ok, MAX_TOPICS, SLUG_MAX};
use theseus_protocol::threads_import::ThreadCandidate;

use super::extract::working_set;
use super::{Msg, Seen, Summary};

const DAY_MS: u64 = 86_400_000;
/// What a stream's slug starts with when every episode of it is partner text.
const CONFIDENTIAL: &str = "confidential";

/// What the scan is asked for.
pub struct Opts<'a> {
    pub now_ms: u64,
    pub since_days: u32,
    pub min_cluster: u32,
    /// The configured personas' names.
    pub personas: &'a BTreeSet<String>,
    /// The slugs of threads that exist.
    pub existing: &'a BTreeSet<String>,
}

/// One episode's part in a candidate: what the card may read of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub session_id: String,
    pub end_ms: u64,
    pub summary: Option<Summary>,
    /// Its owner messages in the window.
    pub owner: Vec<Msg>,
}

/// A candidate, and the episodes behind it. A partner-confidential episode
/// is counted in the candidate but is not among the inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub cand: ThreadCandidate,
    pub inputs: Vec<Input>,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub drafts: Vec<Draft>,
    pub notes: Vec<String>,
}

/// A stream's grouping key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Channel(String, String),
    Cluster(String),
}

pub fn candidates(seen: &[Seen], o: &Opts<'_>) -> Plan {
    let since = o.now_ms.saturating_sub(u64::from(o.since_days) * DAY_MS);
    let mut groups: BTreeMap<Key, Vec<&Seen>> = BTreeMap::new();
    let (mut unplaced, mut untopiced) = (0u32, 0u32);
    for s in seen {
        match s.place_kind.as_str() {
            "slack-channel" | "discord-channel" => {
                match s.place_id.as_deref().or(s.place_name.as_deref()) {
                    Some(id) => groups
                        .entry(Key::Channel(s.place_kind.clone(), id.to_string()))
                        .or_default()
                        .push(s),
                    None => unplaced += 1,
                }
            }
            "dm" => match cluster_of(s) {
                Some(c) => groups.entry(Key::Cluster(c)).or_default().push(s),
                None => untopiced += 1,
            },
            _ => {}
        }
    }
    let (mut quiet, mut small) = (0u32, 0u32);
    let mut drafts = Vec::new();
    for (key, group) in groups {
        let owner_messages: u32 = group.iter().map(|s| in_window(s, since)).sum();
        if owner_messages == 0 {
            quiet += 1;
            continue;
        }
        if matches!(key, Key::Cluster(_)) && owner_messages < o.min_cluster {
            small += 1;
            continue;
        }
        drafts.push(draft(&key, &group, since, owner_messages, o));
    }
    drafts.sort_by(|a, b| {
        (b.cand.last_active_ms, &a.cand.slug).cmp(&(a.cand.last_active_ms, &b.cand.slug))
    });
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for (i, d) in drafts.iter_mut().enumerate() {
        let base = d.cand.slug.clone();
        let (mut slug, mut n) = (base.clone(), 2);
        while taken.contains(&slug) {
            slug = suffixed(&base, n);
            n += 1;
        }
        taken.insert(slug.clone());
        d.cand.exists = o.existing.contains(&slug);
        d.cand.slug = slug;
        d.cand.n = i as u32 + 1;
    }
    let mut notes = Vec::new();
    for (n, what) in [
        (quiet, "streams with no owner message in the window"),
        (small, "DM topic clusters under the minimum"),
        (untopiced, "DM episodes with no topic label, in no cluster"),
        (unplaced, "channel episodes naming no channel"),
    ] {
        if n > 0 {
            notes.push(format!("{n} {what}"));
        }
    }
    Plan { drafts, notes }
}

fn in_window(s: &Seen, since: u64) -> u32 {
    s.owner.iter().filter(|m| m.at_ms >= since).count() as u32
}

/// A DM episode's cluster: its first topic label, cut to two path parts.
fn cluster_of(s: &Seen) -> Option<String> {
    let t = s.topics.first()?.trim().to_lowercase();
    let parts: Vec<&str> = t.split('/').filter(|p| !p.is_empty()).take(2).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn draft(key: &Key, group: &[&Seen], since: u64, owner_messages: u32, o: &Opts<'_>) -> Draft {
    let open: Vec<&Seen> = group.iter().copied().filter(|s| !s.partner).collect();
    let mut sensitivity: BTreeMap<String, u32> = BTreeMap::new();
    let mut last = 0u64;
    for s in group {
        let n = in_window(s, since);
        if n > 0 {
            *sensitivity.entry(s.sensitivity.clone()).or_default() += n;
            last = last.max(s.owner.iter().map(|m| m.at_ms).max().unwrap_or(0));
        }
    }
    let mut inputs: Vec<Input> = open
        .iter()
        .map(|s| Input {
            session_id: s.session_id.clone(),
            end_ms: s.end_ms,
            summary: s.summary.clone(),
            owner: s
                .owner
                .iter()
                .filter(|m| m.at_ms >= since)
                .cloned()
                .collect(),
        })
        .filter(|i| i.summary.is_some() || !i.owner.is_empty())
        .collect();
    inputs.sort_by(|a, b| (b.end_ms, &a.session_id).cmp(&(a.end_ms, &b.session_id)));
    let texts: Vec<&str> = inputs
        .iter()
        .flat_map(|i| {
            i.owner
                .iter()
                .map(|m| m.text.as_str())
                .chain(i.summary.iter().map(|s| s.text.as_str()))
        })
        .collect();
    let (title, kind, channel, slug) = names(key, &open);
    let notes = channel
        .iter()
        .map(|ch| {
            format!(
                "from {ch}: it can be this thread's place once Discord threads bind \
                 (thread-discord); until then `theseus ask --thread` talks to it"
            )
        })
        .collect();
    let persona = most_common(group.iter().filter_map(|s| s.agent.clone())).and_then(|a| {
        o.personas
            .iter()
            .find(|p| p.eq_ignore_ascii_case(&a))
            .cloned()
    });
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for s in &open {
        for t in s.topics.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            *counts.entry(t.to_string()).or_default() += 1;
        }
    }
    let mut topics: Vec<(String, u32)> = counts.into_iter().collect();
    topics.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Draft {
        cand: ThreadCandidate {
            n: 0,
            slug,
            title,
            kind: kind.into(),
            persona,
            topics: topics
                .into_iter()
                .take(MAX_TOPICS)
                .map(|(t, _)| t)
                .collect(),
            working_set: working_set(texts),
            owner_messages,
            episodes: group.len() as u32,
            last_active_ms: last,
            sensitivity,
            exists: false,
            channel,
            notes,
        },
        inputs,
    }
}

/// A stream's title, kind, Discord channel and slug. The title is the
/// channel's label from the episodes that give text, so a stream of partner
/// episodes alone names nothing.
fn names(key: &Key, open: &[&Seen]) -> (String, &'static str, Option<String>, String) {
    match key {
        Key::Channel(kind, id) => {
            let name = most_common(open.iter().filter_map(|s| s.place_name.clone()))
                .map(|n| n.trim_start_matches('#').trim().to_string())
                .filter(|n| !n.is_empty());
            let slug = match &name {
                Some(t) => slugify(t),
                None => slugify(&format!("{CONFIDENTIAL} {id}")),
            };
            let title = name.unwrap_or_else(|| format!("confidential {kind} conversation"));
            let channel = (kind == "discord-channel").then(|| format!("discord:channel:{id}"));
            (title, "channel", channel, slug)
        }
        Key::Cluster(c) => {
            let tail = c.rsplit('/').next().unwrap_or(c).replace(['-', '_'], " ");
            (
                format!("DMs: {tail}"),
                "dm",
                None,
                format!("dm-{}", slugify(c)),
            )
        }
    }
}

fn most_common(items: impl Iterator<Item = String>) -> Option<String> {
    let mut m: BTreeMap<String, u32> = BTreeMap::new();
    for i in items {
        *m.entry(i).or_default() += 1;
    }
    m.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(k, _)| k)
}

/// A slug from words: lower-case letters and digits joined by single
/// hyphens, at most `SLUG_MAX - 4` long (room for a `-2` suffix).
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.chars().take(SLUG_MAX - 4).collect();
    let out = out.trim_matches('-').to_string();
    if out.is_empty() || !slug_ok(&out) {
        "stream".into()
    } else {
        out
    }
}

fn suffixed(base: &str, n: u32) -> String {
    let tail = format!("-{n}");
    let keep: String = base.chars().take(SLUG_MAX - tail.len()).collect();
    format!("{}{tail}", keep.trim_end_matches('-'))
}
