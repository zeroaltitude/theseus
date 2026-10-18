//! The sealed hold (theseus-pw1q.21.7): where the sealed scorer is used. A
//! self branch that changes how well Theseus answers can learn to please the
//! exams it can see while the real quality falls, so a branch whose diff
//! touches a *behaviour path* ([`BEHAVIOUR`]) is scored on every sealed set
//! beside main, and held when it scores worse ([`judge`]).
//!
//! **The rule.** In each set, a family of at least [`MIN_FAMILY`] items that
//! loses more than [`SLACK`] item holds the join, and so does a total that
//! falls at all. A smaller family, or a loss within the slack, is reported
//! and never held on: n is noise. The verdict carries counts, family names
//! and the opaque ids of the items that newly fail, never an item.
//!
//! **Main's score** is cached, one for each (main commit, set digest), in
//! META (`Core::sealed_main`), and the first one after switch-on is kept as
//! the baseline the weekly drift check compares the installed main to
//! (`Core::sealed_drift`).
//!
//! **The seam** is [`Scorer`]: the daemon's is [`Scratch`], the scratch
//! runner of `runner.rs` over each set. Every spend goes through its relay.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::Scores;
use crate::rpc::Core;

/// The paths whose change moves how Theseus answers, as patterns where `*`
/// stands for any run of characters: prompt and system-text assembly, tool
/// descriptions and schemas, routing and the profiles' defaults, Jev's pack
/// code (the packs' data is the ladder's), recall, rerank and memory ranking,
/// context assembly and the compiler, and the turn loop's stop rules. A test
/// file is never one (`touches`), and a test holds that each pattern matches
/// a file, so the list cannot rot.
pub const BEHAVIOUR: &[&str] = &[
    // Prompt and system-text assembly.
    "crates/theseus-core/src/turn.rs",
    "crates/theseus-core/src/turn/spec_step.rs",
    "crates/theseus-core/src/turn/standing_step.rs",
    "crates/theseus-core/src/context_files.rs",
    "crates/theseus-core/src/standing.rs",
    "crates/theseus-core/src/persona.rs",
    // Tool descriptions and schemas.
    "crates/theseus-tools/src/*",
    "crates/theseus-core/src/tools_defer.rs",
    "crates/theseus-core/src/tools_defer/*",
    "crates/theseus-core/src/native.rs",
    // Routing, and the profiles' defaults.
    "crates/theseus-core/src/routing.rs",
    "crates/theseus-core/src/routing/*",
    "crates/theseus-core/src/turn/route_step.rs",
    "crates/theseus-core/src/turn/route_step/*",
    "crates/theseus-core/src/config/routing.rs",
    "crates/theseus-core/config/theseus.example.toml",
    // Jev's pack code.
    "crates/theseus-judge/src/*",
    "crates/theseus-core/src/judge/*",
    // Recall, rerank and memory ranking.
    "crates/theseus-memory/src/*",
    "crates/theseus-core/src/recall.rs",
    "crates/theseus-core/src/recall/*",
    "crates/theseus-core/src/turn/recall_step.rs",
    "crates/theseus-core/src/turn/rerank_step.rs",
    "crates/theseus-core/src/memory_lookup.rs",
    "crates/theseus-core/src/memory_lookup/*",
    // Context assembly and the compiler.
    "crates/theseus-core/src/compiler.rs",
    "crates/theseus-core/src/compiler/*",
    "crates/theseus-core/src/prefix.rs",
    "crates/theseus-core/src/prefix/*",
    // The turn loop's stop rules.
    "crates/theseus-core/src/advancer.rs",
    "crates/theseus-core/src/turn/advance_step.rs",
    "crates/theseus-core/src/turn/nudge_step.rs",
    "crates/theseus-core/src/turn/budget_step.rs",
];

/// A family needs this many items before a loss in it holds a join.
pub const MIN_FAMILY: u64 = 5;
/// The items a family may lose before it holds.
pub const SLACK: u64 = 1;

/// Whether `pattern` (with `*` for any run) matches all of `text`.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(mut tail) = text.strip_prefix(head) else {
                return false;
            };
            // Greedy runs from the shortest, so `a*b*c` finds its `b`.
            loop {
                if wildcard(rest, tail) {
                    return true;
                }
                let mut chars = tail.chars();
                if chars.next().is_none() {
                    return false;
                }
                tail = chars.as_str();
            }
        }
    }
}

/// The first of `paths` that is a behaviour path; none when the change
/// touches none. A test file is no behaviour: it measures it.
pub fn touches<S: AsRef<str>>(paths: &[S]) -> Option<String> {
    paths
        .iter()
        .map(AsRef::as_ref)
        .filter(|p| {
            let name = p.rsplit('/').next().unwrap_or(p);
            !(name.starts_with("tests") || name.ends_with("_tests.rs"))
        })
        .find(|p| BEHAVIOUR.iter().any(|b| wildcard(b, p)))
        .map(str::to_string)
}

/// A loss: a family of a set, or a set's total (`family` none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loss {
    pub set: String,
    pub family: Option<String>,
    pub before: u64,
    pub after: u64,
    /// The items the candidate was asked.
    pub items: u64,
}

/// What a candidate's scores say against main's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Verdict {
    /// Items passed, main's and the candidate's, and the items asked.
    pub before: u64,
    pub after: u64,
    pub total: u64,
    /// Losses that hold the join.
    pub holds: Vec<Loss>,
    /// Losses that are reported and held on for nothing.
    pub reported: Vec<Loss>,
    /// The opaque ids of items that fail now and passed on main.
    pub broken: Vec<String>,
}

impl Verdict {
    pub fn holds(&self) -> bool {
        !self.holds.is_empty()
    }

    /// `sealed 38/40 → 39/40`.
    pub fn line(&self) -> String {
        crate::rsi::log::sealed_line(self.before, self.after, self.total)
    }

    /// The hold's reason, in words: which losses and by how much.
    pub fn why(&self) -> String {
        let said: Vec<String> = self
            .holds
            .iter()
            .map(|l| {
                let (b, a, n) = (l.before, l.after, l.items);
                match &l.family {
                    Some(f) => format!("family {f} of set {}: {b} → {a} of {n}", l.set),
                    None => format!("set {} total: {b} → {a} of {n}", l.set),
                }
            })
            .collect();
        format!(
            "the sealed sets score worse than main ({})",
            said.join("; ")
        )
    }
}

/// The candidate's scores against main's, set by set (a set main has no score
/// for is not judged).
pub fn judge(main: &[Scores], candidate: &[Scores]) -> Verdict {
    let mut v = Verdict::default();
    for c in candidate {
        let Some(m) = main.iter().find(|m| m.set == c.set) else {
            continue;
        };
        v.before += m.passed;
        v.after += c.passed;
        v.total += c.items;
        let loss = |family: Option<&str>, before: u64, after: u64, items: u64| Loss {
            set: c.set.clone(),
            family: family.map(str::to_string),
            before,
            after,
            items,
        };
        for (name, f) in &c.families {
            let before = m.families.get(name).map_or(0, |f| f.passed);
            if f.passed >= before {
                continue;
            }
            let l = loss(Some(name), before, f.passed, f.items);
            if f.items >= MIN_FAMILY && before - f.passed > SLACK {
                v.holds.push(l);
            } else {
                v.reported.push(l);
            }
        }
        if c.passed < m.passed {
            v.holds.push(loss(None, m.passed, c.passed, c.items));
        }
        v.broken.extend(
            c.failures
                .iter()
                .filter(|f| !m.failures.contains(f))
                .cloned(),
        );
    }
    v
}

/// Whose build is scored: the line its spend is booked on, the commit its
/// row names, and the directory holding its `theseusd` and `theseus`.
#[derive(Debug, Clone)]
pub struct Whose {
    pub branch: String,
    pub head: String,
    pub bins: PathBuf,
}

pub type Fut<'a> = Pin<Box<dyn Future<Output = Result<Vec<Scores>>> + Send + 'a>>;

/// Who scores a build on every sealed set: the daemon's scratch runner, or a
/// test's stand-in.
pub trait Scorer: Send + Sync {
    fn score<'a>(&'a self, core: &'a Arc<Core>, whose: &'a Whose) -> Fut<'a>;
}

/// The scratch runner over each set (`Core::sealed_score`).
pub struct Scratch;

impl Scorer for Scratch {
    fn score<'a>(&'a self, core: &'a Arc<Core>, whose: &'a Whose) -> Fut<'a> {
        Box::pin(core.sealed_score(whose))
    }
}

/// Main's score, as the META cache keeps it for a (commit, set digest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kept {
    pub commit: String,
    pub digest: String,
    pub at_ms: u64,
    pub scores: Vec<Scores>,
}
