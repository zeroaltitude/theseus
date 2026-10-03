//! The world the simulator makes from its seed: people, guild channels whose
//! viewers change, the work tree's files, and the context files; and the state
//! the sim's model and stand-in tools share with the run while a turn runs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};

use super::atoms::{Atoms, Origin, R};

/// The owner on Discord, and the first id of everyone else (invented ids).
pub const OWNER: u64 = 7_100_000_000_000_001;
const FIRST_PERSON: u64 = 7_100_000_000_000_002;
const FIRST_CHANNEL: u64 = 7_200_000_000_000_001;
const CHANNEL_NAMES: [&str; 4] = ["harbour", "lab", "workshop", "lounge"];
const PEOPLE_NAMES: [&str; 6] = ["ana", "ben", "cy", "dee", "eli", "fay"];

/// A guild channel: who can view it now (the truth), whether the bot can
/// read that (the Server Members intent), and what it last told the core.
#[derive(Debug, Clone)]
pub struct Channel {
    pub id: u64,
    pub name: String,
    pub truth: BTreeSet<u64>,
    pub readable: bool,
    /// What the binding last pushed: None before the first read; Some(None)
    /// when it could not read who views it.
    pub pushed: Option<Option<BTreeSet<u64>>>,
    /// The step of the last push, for the binding's "a minute old" rule.
    pub pushed_at: u32,
}

impl Channel {
    /// What a read of the channel finds now.
    pub fn read(&self) -> Option<BTreeSet<u64>> {
        self.readable.then(|| self.truth.clone())
    }
}

/// A file the work tree holds, and its atom.
#[derive(Debug, Clone)]
pub struct File {
    /// Relative to the work tree.
    pub path: String,
    pub public: bool,
    pub text: String,
}

/// What the model does in one loop of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Read the owner's file `k` (a private tree).
    ReadPrivate(usize),
    /// Read file `k` of the public tree.
    ReadOpen(usize),
    /// Fetch a page: outside text, anyone's.
    Fetch,
    /// Run a job that connects out (18c: untrusted, the owner's), or one that
    /// connects nowhere (the owner's).
    RunEgress,
    RunLocal,
    /// Start a task, its brief written from what the request carried.
    Task {
        wake: bool,
    },
    /// Answer, with no call.
    Say,
}

/// A change of who can view a channel that the model's call brings mid-turn,
/// for the run to push to the core outside the shared state's lock.
#[derive(Debug, Clone)]
pub struct Push {
    pub channel: u64,
    pub name: String,
    pub viewers: Option<BTreeSet<u64>>,
}

/// One request the model answered.
#[derive(Debug, Clone)]
pub struct Call {
    pub session: String,
    pub digest: String,
    /// The request's system blocks, and its messages, as JSON text.
    pub system: String,
    pub text: String,
    pub messages: Vec<serde_json::Value>,
    /// The store's last position when the model was called: every node of
    /// the session at or before it was compiled into the request.
    pub pos: u64,
}

/// What the run and its model and tools share.
pub struct Shared {
    pub rng: StdRng,
    pub atoms: Atoms,
    pub people: Vec<u64>,
    pub names: BTreeMap<u64, String>,
    pub channels: Vec<Channel>,
    pub files: Vec<File>,
    /// The context files: (absolute path is the run's), text, public.
    pub context: Vec<File>,
    /// The session whose turn runs now: the model answers for it.
    pub current: Option<String>,
    /// The channel that session posts to, if it is a guild channel's.
    pub turn_channel: Option<u64>,
    /// Each session's acts for its turn, in order; empty means `Say`.
    pub plans: BTreeMap<String, VecDeque<Act>>,
    /// Every request the model answered since the run last took them.
    pub calls: Vec<Call>,
    /// Pushes the model asked for mid-turn, for the run to make.
    pub pushes: Vec<Push>,
    /// The share of a model's calls that change a channel's viewers first.
    pub p_mid_turn: f64,
    /// Counters the model and the tools keep.
    pub tool_calls: u64,
    pub fetches: u64,
    pub egress: u64,
    pub mid_turn_changes: u64,
    pub mid_turn_pushed: u64,
    pub step: u32,
    next_call: u64,
}

impl Shared {
    /// The world for `seed`: 4 to 6 people besides the owner, 2 to 4
    /// channels, each viewed by the owner and some of the others (a third of
    /// them by the owner alone, at first), a private and a public tree of
    /// files, and a context file of each kind.
    pub fn generate(seed: u64, p_mid_turn: f64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let others = rng.random_range(4..=6usize);
        let mut people = vec![OWNER];
        let mut names = BTreeMap::from([(OWNER, "owner".to_string())]);
        for (i, name) in PEOPLE_NAMES.iter().enumerate().take(others) {
            let id = FIRST_PERSON + i as u64;
            people.push(id);
            names.insert(id, name.to_string());
        }
        let n = rng.random_range(2..=4usize);
        let mut channels = Vec::new();
        for (j, name) in CHANNEL_NAMES.iter().enumerate().take(n) {
            let mut truth = BTreeSet::from([OWNER]);
            // The first channel is the owner's alone at first; a third of
            // the rest too.
            if j > 0 && !rng.random_bool(1.0 / 3.0) {
                for p in &people[1..] {
                    if rng.random_bool(0.4) {
                        truth.insert(*p);
                    }
                }
            }
            channels.push(Channel {
                id: FIRST_CHANNEL + j as u64,
                name: name.to_string(),
                truth,
                // One channel in eight cannot be read: it counts as anyone.
                readable: !rng.random_bool(0.125),
                pushed: None,
                pushed_at: 0,
            });
        }
        let mut atoms = Atoms::default();
        let mut files = Vec::new();
        for k in 0..4 {
            let path = format!("private/notes-{k}.txt");
            let (_, m) = atoms.mint(R::Owner, Origin::File(path.clone()));
            files.push(File {
                text: format!("Private notes {k}: the harbour code is {m}.\n"),
                path,
                public: false,
            });
        }
        for k in 0..3 {
            let path = format!("open/readme-{k}.txt");
            let (_, m) = atoms.mint(R::Public, Origin::File(path.clone()));
            files.push(File {
                text: format!("Readme {k} of the open tree: {m}.\n"),
                path,
                public: true,
            });
        }
        let mut context = Vec::new();
        for (path, public) in [("private/context.md", false), ("open/context.md", true)] {
            let readers = if public { R::Public } else { R::Owner };
            let (_, m) = atoms.mint(readers, Origin::Context(path.into()));
            context.push(File {
                text: format!("Context ({path}): {m}\n"),
                path: path.into(),
                public,
            });
        }
        Self {
            rng,
            atoms,
            people,
            names,
            channels,
            files,
            context,
            current: None,
            turn_channel: None,
            plans: BTreeMap::new(),
            calls: Vec::new(),
            pushes: Vec::new(),
            p_mid_turn,
            tool_calls: 0,
            fetches: 0,
            egress: 0,
            mid_turn_changes: 0,
            mid_turn_pushed: 0,
            step: 0,
            next_call: 0,
        }
    }

    /// A tool call's id, unique in the run.
    pub fn call_id(&mut self) -> String {
        self.next_call += 1;
        format!("c{}", self.next_call)
    }

    pub fn channel(&self, id: u64) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// What the core was last told about every channel: who views it, or
    /// None when that could not be read. A channel never read is left out.
    pub fn pushed_views(&self) -> super::atoms::Views {
        self.channels
            .iter()
            .filter_map(|c| c.pushed.clone().map(|v| (c.id, v)))
            .collect()
    }

    /// Who can view each channel now, whether or not the bot can read it.
    pub fn truth(&self) -> super::atoms::Views {
        self.channels
            .iter()
            .map(|c| (c.id, Some(c.truth.clone())))
            .collect()
    }

    /// Someone joins `channel`, or leaves it (never the owner): the truth
    /// changes, and the binding hears of it or not. Leaving is a little more
    /// likely than joining, so channels keep coming back to the owner alone,
    /// where the owner's material is admitted and a newcomer makes a post
    /// wait.
    pub fn change_viewers(&mut self, channel: usize) -> (u64, bool) {
        let truth = &self.channels[channel].truth;
        let inside: Vec<u64> = self.people[1..]
            .iter()
            .copied()
            .filter(|p| truth.contains(p))
            .collect();
        let outside: Vec<u64> = self.people[1..]
            .iter()
            .copied()
            .filter(|p| !truth.contains(p))
            .collect();
        let leave = !inside.is_empty() && (outside.is_empty() || self.rng.random_bool(0.55));
        let pool = if leave { &inside } else { &outside };
        let who = *pool.choose(&mut self.rng).unwrap_or(&OWNER);
        let c = &mut self.channels[channel];
        if leave {
            c.truth.remove(&who);
        } else {
            c.truth.insert(who);
        }
        (who, !leave)
    }

    /// The binding read every channel (a channel or role change): what each
    /// read finds, to push.
    pub fn read_all(&mut self) -> Vec<Push> {
        let step = self.step;
        self.channels
            .iter_mut()
            .map(|c| {
                c.pushed = Some(c.read());
                c.pushed_at = step;
                Push {
                    channel: c.id,
                    name: c.name.clone(),
                    viewers: c.read(),
                }
            })
            .collect()
    }

    /// The binding read one channel.
    pub fn read_one(&mut self, channel: u64) -> Option<Push> {
        let step = self.step;
        let c = self.channels.iter_mut().find(|c| c.id == channel)?;
        c.pushed = Some(c.read());
        c.pushed_at = step;
        Some(Push {
            channel: c.id,
            name: c.name.clone(),
            viewers: c.read(),
        })
    }
}
