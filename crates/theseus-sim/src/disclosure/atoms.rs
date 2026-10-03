//! The oracle's model of who may read what (design m4-boundaries §2.5, rebuilt
//! here on purpose, apart from the core's `labels.rs`).
//!
//! Every piece of content the world makes carries one **atom**: a marker,
//! `zq<n>qz`, unique in the run. An atom's readers come from where the world
//! made it (who said it and where, which tree a file is in, a fetched page),
//! never from the label the core wrote. The model repeats every atom its
//! request carried, so an atom found in a request, a streamed edit, or a post
//! is content that left for that audience, and the oracle judges it with its
//! own `covers`.

use std::collections::{BTreeMap, BTreeSet};

/// Who may read an atom; the owner always may.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum R {
    /// Anyone: a fetched page, a file in a public tree.
    Public,
    /// Whoever can view the guild channel: what was said there.
    Place(u64),
    /// Named people: what was said in a DM.
    People(BTreeSet<u64>),
    /// The owner alone: the CLI's words, the owner's files and programs.
    Owner,
}

impl R {
    pub fn describe(&self) -> String {
        match self {
            R::Public => "public".into(),
            R::Place(c) => format!("channel {c}'s"),
            R::People(p) => format!("for {p:?}"),
            R::Owner => "owner-only".into(),
        }
    }
}

/// An audience, as the oracle evaluates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aud {
    /// The CLI, the web UI, a task that reports nowhere.
    Owner,
    /// A DM's person.
    Person(u64),
    /// A guild channel, and who can view it; None when that cannot be read,
    /// and then the channel counts as anyone.
    Place {
        channel: u64,
        members: Option<BTreeSet<u64>>,
    },
}

impl Aud {
    pub fn describe(&self) -> String {
        match self {
            Aud::Owner => "the owner".into(),
            Aud::Person(u) => format!("the DM with {u}"),
            Aud::Place {
                channel,
                members: Some(m),
            } => format!("channel {channel} ({} viewers: {m:?})", m.len()),
            Aud::Place {
                channel,
                members: None,
            } => format!("channel {channel} (anyone: its viewers cannot be read)"),
        }
    }
}

/// Who can view each channel: what the binding last told the core, or the
/// ground truth. None for a channel whose viewers cannot be read.
pub type Views = BTreeMap<u64, Option<BTreeSet<u64>>>;

/// The people an audience names; None for one nobody can name (a channel
/// whose viewers cannot be read), and none for the owner alone.
fn members(aud: &Aud) -> Option<BTreeSet<u64>> {
    match aud {
        Aud::Owner => Some(BTreeSet::new()),
        Aud::Person(u) => Some(BTreeSet::from([*u])),
        Aud::Place { members, .. } => members.clone(),
    }
}

/// `covers` (§2.5), the oracle's own: every member of the audience is a
/// reader or the owner, and a channel's own words always cover it.
pub fn covers(r: &R, aud: &Aud, owner: u64, views: &Views) -> bool {
    let members: BTreeSet<u64> = match (r, aud) {
        (R::Public, _) | (_, Aud::Owner) => return true,
        (R::Place(c), Aud::Place { channel, .. }) if c == channel => return true,
        (_, Aud::Place { members: None, .. }) => return false,
        (
            _,
            Aud::Place {
                members: Some(m), ..
            },
        ) => m.clone(),
        (_, Aud::Person(u)) => BTreeSet::from([*u]),
    };
    let readers: BTreeSet<u64> = match r {
        R::People(p) => p.clone(),
        R::Place(c) => views.get(c).cloned().flatten().unwrap_or_default(),
        _ => BTreeSet::new(),
    };
    members.iter().all(|m| *m == owner || readers.contains(m))
}

/// Readers that may leave for any audience a place can have: the public, and
/// what was said in the place itself (19c's quiet loops and the post's fast
/// path).
pub fn fits_any(r: &R, channel: Option<u64>) -> bool {
    match r {
        R::Public => true,
        R::Place(c) => channel == Some(*c),
        _ => false,
    }
}

/// Where an atom came from, for a failure's words and the latch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The operator's words through the CLI.
    Cli,
    /// Through Discord: a DM with a person, or a guild channel.
    Dm(u64),
    Channel(u64),
    /// A file in the work tree, and whether its tree is public.
    File(String),
    /// A context file in the system block.
    Context(String),
    /// A fetched page: outside text.
    Page,
    /// What a job that connected out returned (18c): outside text.
    Egress,
    /// What a job that connected nowhere returned.
    Run,
}

impl Origin {
    pub fn external(&self) -> bool {
        matches!(self, Origin::Page | Origin::Egress)
    }
}

#[derive(Debug, Clone)]
pub struct Atom {
    pub readers: R,
    pub origin: Origin,
}

/// Gaps the simulator found in the core, each filed. A run counts each case
/// instead of failing on it, until the step that fixes the gap deletes its
/// entry; `--strict` fails on them too.
pub const KNOWN_GAPS: &[(&str, &str)] = &[(
    "theseus-42ub",
    "the meet that labels an answer leaves out the context files its request carried, so an \
     owner-only context file reaches the model's words labeled for the channel",
)];

/// The known gap that explains `a` reaching an audience it may not, when one
/// does: an owner-only context file's atom in the model's words (never in the
/// system block itself, which the context filter judges on its own).
pub fn known_gap(a: &Atom, in_words: bool) -> Option<&'static str> {
    (in_words && matches!(a.origin, Origin::Context(_)) && a.readers != R::Public)
        .then_some(KNOWN_GAPS[0].0)
}

/// Every atom of a run, the operator's graduations, and who has been shown
/// each: an atom in a node the operator graduated may be read by the
/// graduated readers too, and an atom a person was shown while they could
/// read it may be shown to them again (labels are fixed when a node is
/// written, so what was said to someone stays theirs: ana, shown #harbour's
/// words in her DM while she viewed #harbour, keeps the answer that repeated
/// them after she leaves).
#[derive(Debug, Default)]
pub struct Atoms {
    list: Vec<Atom>,
    grants: BTreeMap<u32, BTreeSet<R>>,
    shown: BTreeMap<u32, BTreeSet<u64>>,
}

pub fn marker(id: u32) -> String {
    format!("zq{id}qz")
}

/// The atoms in `text`, in order of first appearance, each once.
pub fn scan(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("zq") {
        let after = &rest[at + 2..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with("qz") {
            if let Ok(id) = after[..digits].parse::<u32>() {
                if seen.insert(id) {
                    out.push(id);
                }
            }
            rest = &after[digits + 2..];
        } else {
            rest = after;
        }
    }
    out
}

impl Atoms {
    /// A new atom, and its marker.
    pub fn mint(&mut self, readers: R, origin: Origin) -> (u32, String) {
        let id = self.list.len() as u32;
        self.list.push(Atom { readers, origin });
        (id, marker(id))
    }

    pub fn get(&self, id: u32) -> Option<&Atom> {
        self.list.get(id as usize)
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// The operator graduated a node that carries `id` to `r`.
    pub fn grant(&mut self, id: u32, r: R) {
        self.grants.entry(id).or_default().insert(r);
    }

    pub fn grants(&self) -> usize {
        self.grants.values().map(BTreeSet::len).sum()
    }

    /// May `id` reach `aud`? By its own readers, by a graduation's, or
    /// because everyone in the audience was shown it while they could read
    /// it.
    pub fn allowed(&self, id: u32, aud: &Aud, owner: u64, views: &Views) -> bool {
        let Some(a) = self.get(id) else {
            return false;
        };
        covers(&a.readers, aud, owner, views)
            || self
                .grants
                .get(&id)
                .is_some_and(|g| g.iter().any(|r| covers(r, aud, owner, views)))
            || self.shown.get(&id).is_some_and(|seen| {
                members(aud).is_some_and(|m| m.iter().all(|p| *p == owner || seen.contains(p)))
            })
    }

    /// `aud` was shown `id`, and was allowed to be: each of its members may
    /// be shown it again. An audience nobody can name adds nobody.
    pub fn saw(&mut self, id: u32, aud: &Aud) {
        if let Some(m) = members(aud) {
            self.shown.entry(id).or_default().extend(m);
        }
    }

    /// May `id` stream into `channel`, whoever comes to view it?
    pub fn fits_any(&self, id: u32, channel: u64) -> bool {
        let Some(a) = self.get(id) else {
            return false;
        };
        fits_any(&a.readers, Some(channel))
            || self
                .grants
                .get(&id)
                .is_some_and(|g| g.iter().any(|r| fits_any(r, Some(channel))))
    }

    /// One atom in words, for a failure: its marker, its readers, its origin.
    pub fn describe(&self, id: u32) -> String {
        match self.get(id) {
            Some(a) => format!(
                "{} ({}, from {:?}{})",
                marker(id),
                a.readers.describe(),
                a.origin,
                match self.grants.get(&id) {
                    Some(g) => format!(", graduated to {g:?}"),
                    None => String::new(),
                }
            ),
            None => format!("{} (unknown)", marker(id)),
        }
    }
}
