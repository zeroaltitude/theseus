//! What makes a record hot (issue 21 of 1.0.7): a score from what the store already holds, and
//! the signals the index reads off the log to have it.
//!
//! A search by meaning reads the whole **hot set** and the nearest few groups of the cold rest, so
//! what is hot is what a turn is likeliest to want: recent, used, important, or vouched for.
//!
//! | Signal | Where it comes from | In the score |
//! |---|---|---|
//! | recency | the node's *own* time (an import of old sessions is old, never new) | halves every 30 days |
//! | use | `memory.used` rows: a recall admitted it and the turn used it | a lot at the first, less at each more; a use is itself recent or not |
//! | importance | `memory.labeled` rows' durability: high is kept, floor is let go | a floor under the score |
//! | the operator's labels | `memory.label` rows | useful and should-have make it hot; wrong and stale keep it out of the hot set |
//!
//! A *record* is a text, and many nodes can say it: its score is its best node's. The log's rows
//! are read by the tender as it follows the log ([`from_ledger`]); only nodes with something to
//! say are kept (a few tens of thousands at today's size), and the file beside the vectors
//! (`heat.log`) makes them survive a restart. A signal never changes what a search returns, only
//! what it reads first: a lost one costs a record its place in the hot set, never its answer.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

/// Seconds in a day.
const DAY: f32 = 86_400.0;
/// Recency halves every this many days.
const HALF_LIFE_DAYS: f32 = 30.0;

/// An operator's label on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Label {
    #[default]
    None,
    /// `useful` or `should_have`: hot.
    Vouched,
    /// `wrong` or `stale`: never hot.
    Doubted,
}

/// A node's durability, where it moves the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Durability {
    #[default]
    Neutral,
    High,
    Floor,
}

/// What the index holds of one node's signals: eight bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Sig {
    /// Uses, saturating.
    pub used: u8,
    pub label: Label,
    pub durability: Durability,
    /// When it was last used, in seconds.
    pub last_used_s: u32,
}

impl Sig {
    fn is_default(&self) -> bool {
        *self == Sig::default()
    }

    /// Whether a cold record of this node is searched with the hot set at once: the operator
    /// vouched for it, or a turn used it (and no label doubts it).
    pub fn wants_hot(&self) -> bool {
        self.label == Label::Vouched || (self.used > 0 && self.label != Label::Doubted)
    }
}

/// A signal read off the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A recall admitted the node and its turn used it.
    Used { node: String, at_ms: u64 },
    /// The operator labelled it (`useful`, `should_have`, `wrong`, `stale`).
    Label { node: String, label: String },
    /// The gate's first-sight durability (`high`, `medium`, `low`, `floor`).
    Durability { node: String, durability: String },
}

impl Signal {
    pub fn node(&self) -> &str {
        match self {
            Signal::Used { node, .. }
            | Signal::Label { node, .. }
            | Signal::Durability { node, .. } => node,
        }
    }
}

#[derive(Deserialize)]
struct Row {
    #[serde(default)]
    at_unix_ms: u64,
    kind: String,
    #[serde(default)]
    data: Value,
}

/// The signal a ledger row of the log carries, if it is one the hot set reads. Cheap for the rest:
/// a row that names no memory kind is never parsed.
pub fn from_ledger(payload: &[u8]) -> Option<Signal> {
    const NEEDLE: &[u8] = b"\"kind\":\"memory.";
    if !payload.windows(NEEDLE.len()).any(|w| w == NEEDLE) {
        return None;
    }
    let row: Row = serde_json::from_slice(payload).ok()?;
    let node = row.data.get("node_id")?.as_str()?.to_string();
    match row.kind.as_str() {
        "memory.used" if row.data.get("used")?.as_bool()? => Some(Signal::Used {
            node,
            at_ms: row.at_unix_ms,
        }),
        "memory.label" => Some(Signal::Label {
            node,
            label: row.data.get("label")?.as_str()?.to_string(),
        }),
        "memory.labeled" => Some(Signal::Durability {
            node,
            durability: row.data.get("durability")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

/// The nodes' signals.
#[derive(Debug, Default)]
pub(super) struct Signals {
    by_node: HashMap<Box<str>, Sig>,
}

impl Signals {
    pub fn len(&self) -> usize {
        self.by_node.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_node.is_empty()
    }

    pub fn get(&self, node: &str) -> Sig {
        self.by_node.get(node).copied().unwrap_or_default()
    }

    /// The nodes whose records want to be hot.
    pub fn hot_nodes(&self) -> Vec<String> {
        self.by_node
            .iter()
            .filter(|(_, s)| s.wants_hot())
            .map(|(n, _)| n.to_string())
            .collect()
    }

    /// Take one signal in. Returns whether anything changed.
    pub fn apply(&mut self, s: &Signal) -> bool {
        let mut sig = self.get(s.node());
        let before = sig;
        match s {
            Signal::Used { at_ms, .. } => {
                sig.used = sig.used.saturating_add(1);
                sig.last_used_s = sig.last_used_s.max(super::time_secs(*at_ms));
            }
            Signal::Label { label, .. } => match label.as_str() {
                "useful" | "should_have" => sig.label = Label::Vouched,
                "wrong" | "stale" => sig.label = Label::Doubted,
                _ => {}
            },
            Signal::Durability { durability, .. } => match durability.as_str() {
                "high" => sig.durability = Durability::High,
                "floor" => sig.durability = Durability::Floor,
                _ => sig.durability = Durability::Neutral,
            },
        }
        if sig == before {
            return false;
        }
        if sig.is_default() {
            self.by_node.remove(s.node());
        } else {
            self.by_node.insert(s.node().into(), sig);
        }
        true
    }

    /// A node forgotten: nothing of it stays.
    pub fn forget(&mut self, node: &str) -> bool {
        self.by_node.remove(node).is_some()
    }

    /// Read the file `path` (a missing one is no signals), and say how many lines it had.
    pub fn load(path: &Path) -> io::Result<(Signals, usize)> {
        let mut s = Signals::default();
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((s, 0)),
            Err(e) => return Err(e),
        };
        let mut lines = 0;
        for line in BufReader::new(file).lines() {
            let line = line?;
            lines += 1;
            if let Some(sig) = parse_line(&line) {
                s.apply(&sig);
            }
        }
        Ok((s, lines))
    }

    /// Write every node's signals to `path`, new and renamed into place.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = path.with_extension("log.tmp");
        let mut out = io::BufWriter::new(File::create(&tmp)?);
        for (node, sig) in &self.by_node {
            for line in lines_of(node, sig) {
                writeln!(out, "{line}")?;
            }
        }
        out.flush()?;
        out.get_ref().sync_data()?;
        drop(out);
        fs::rename(&tmp, path)
    }

    /// Add `signals` to the end of `path`.
    pub fn append(path: &Path, signals: &[Signal]) -> io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        let mut text = String::new();
        for s in signals {
            text.push_str(&line_of(s));
            text.push('\n');
        }
        f.write_all(text.as_bytes())
    }
}

fn line_of(s: &Signal) -> String {
    match s {
        Signal::Used { node, at_ms } => format!("U {node} {at_ms}"),
        Signal::Label { node, label } => format!("L {node} {label}"),
        Signal::Durability { node, durability } => format!("D {node} {durability}"),
    }
}

fn lines_of(node: &str, sig: &Sig) -> Vec<String> {
    let mut v = Vec::new();
    for _ in 0..sig.used {
        v.push(line_of(&Signal::Used {
            node: node.into(),
            at_ms: u64::from(sig.last_used_s) * 1000,
        }));
    }
    match sig.label {
        Label::None => {}
        Label::Vouched => v.push(line_of(&Signal::Label {
            node: node.into(),
            label: "useful".into(),
        })),
        Label::Doubted => v.push(line_of(&Signal::Label {
            node: node.into(),
            label: "wrong".into(),
        })),
    }
    match sig.durability {
        Durability::Neutral => {}
        Durability::High => v.push(line_of(&Signal::Durability {
            node: node.into(),
            durability: "high".into(),
        })),
        Durability::Floor => v.push(line_of(&Signal::Durability {
            node: node.into(),
            durability: "floor".into(),
        })),
    }
    v
}

fn parse_line(line: &str) -> Option<Signal> {
    let mut it = line.splitn(3, ' ');
    let (tag, node, rest) = (it.next()?, it.next()?.to_string(), it.next()?);
    match tag {
        "U" => Some(Signal::Used {
            node,
            at_ms: rest.parse().ok()?,
        }),
        "L" => Some(Signal::Label {
            node,
            label: rest.into(),
        }),
        "D" => Some(Signal::Durability {
            node,
            durability: rest.into(),
        }),
        _ => None,
    }
}

/// A record's hot score for a node of this own time and these signals, at `now_s`. Higher is
/// hotter. Recency is 1 for a node said now and halves every 30 days; use adds 0.6 for the first
/// and 0.2 for each of three more, and 0.3 more again while the last use is recent; high
/// durability adds 0.8 and floor takes 0.5 off; a vouched label adds 10 and a doubted one takes 10
/// off, so the first is hot whatever its age and the second never is.
pub(super) fn score(now_s: u32, time_s: u32, sig: Sig) -> f32 {
    let age = |s: u32| now_s.saturating_sub(s) as f32 / DAY / HALF_LIFE_DAYS;
    let mut v = 0.5f32.powf(age(time_s));
    if sig.used > 0 {
        v += 0.6 + 0.2 * f32::from(sig.used.min(4) - 1);
        v += 0.3 * 0.5f32.powf(age(sig.last_used_s));
    }
    v += match sig.durability {
        Durability::High => 0.8,
        Durability::Floor => -0.5,
        Durability::Neutral => 0.0,
    };
    v += match sig.label {
        Label::Vouched => 10.0,
        Label::Doubted => -10.0,
        Label::None => 0.0,
    };
    v
}

/// A score at or under this is never hot (a doubted label).
pub(super) const NEVER_HOT: f32 = -5.0;

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u32 = 1_800_000_000;

    fn days(d: u32) -> u32 {
        NOW - d * 86_400
    }

    #[test]
    fn newer_is_hotter_and_an_import_of_old_sessions_is_old() {
        let s = Sig::default();
        assert!(score(NOW, days(1), s) > score(NOW, days(40), s));
        // The time is the node's own: stored today, said a year ago, it is a year old.
        assert!(score(NOW, days(365), s) < 0.01);
    }

    #[test]
    fn use_importance_and_labels_move_a_score() {
        let base = score(NOW, days(90), Sig::default());
        let used = Sig {
            used: 2,
            last_used_s: days(2),
            ..Sig::default()
        };
        assert!(score(NOW, days(90), used) > base + 0.8);
        let high = Sig {
            durability: Durability::High,
            ..Sig::default()
        };
        assert!(score(NOW, days(90), high) > base + 0.7);
        let vouched = Sig {
            label: Label::Vouched,
            ..Sig::default()
        };
        assert!(score(NOW, days(2000), vouched) > score(NOW, 0, used));
        let doubted = Sig {
            label: Label::Doubted,
            ..Sig::default()
        };
        assert!(score(NOW, days(0), doubted) <= NEVER_HOT);
    }

    fn row(kind: &str, data: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"at_unix_ms": 5_000, "kind": kind, "data": data}))
            .unwrap()
    }

    #[test]
    fn the_logs_rows_are_read_for_what_the_hot_set_wants() {
        let used = row(
            "memory.used",
            serde_json::json!({"node_id": "a", "used": true}),
        );
        assert_eq!(
            from_ledger(&used),
            Some(Signal::Used {
                node: "a".into(),
                at_ms: 5_000
            })
        );
        let shown = row(
            "memory.used",
            serde_json::json!({"node_id": "a", "used": false}),
        );
        assert_eq!(from_ledger(&shown), None);
        let label = row(
            "memory.label",
            serde_json::json!({"node_id": "b", "label": "wrong"}),
        );
        assert_eq!(
            from_ledger(&label),
            Some(Signal::Label {
                node: "b".into(),
                label: "wrong".into()
            })
        );
        let labeled = row(
            "memory.labeled",
            serde_json::json!({"node_id": "c", "durability": "high"}),
        );
        assert_eq!(
            from_ledger(&labeled),
            Some(Signal::Durability {
                node: "c".into(),
                durability: "high".into()
            })
        );
        assert_eq!(from_ledger(&row("recall.ran", serde_json::json!({}))), None);
        assert_eq!(from_ledger(b"not json at all"), None);
    }

    #[test]
    fn signals_survive_a_restart_and_a_forgotten_node_leaves_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heat.log");
        let mut s = Signals::default();
        let all = [
            Signal::Used {
                node: "a".into(),
                at_ms: 9_000,
            },
            Signal::Label {
                node: "b".into(),
                label: "useful".into(),
            },
            Signal::Durability {
                node: "c".into(),
                durability: "high".into(),
            },
            Signal::Durability {
                node: "d".into(),
                durability: "medium".into(),
            },
        ];
        for x in &all {
            s.apply(x);
        }
        Signals::append(&path, &all).unwrap();
        // A medium durability is nothing to keep.
        assert_eq!(s.len(), 3);
        let (read, lines) = Signals::load(&path).unwrap();
        assert_eq!((read.len(), lines), (3, 4));
        assert_eq!(read.get("a").used, 1);
        assert_eq!(read.get("b").label, Label::Vouched);
        let mut s = read;
        assert!(s.forget("b"));
        s.save(&path).unwrap();
        let (again, _) = Signals::load(&path).unwrap();
        assert_eq!(again.get("b"), Sig::default());
        assert_eq!(again.get("c").durability, Durability::High);
    }
}
