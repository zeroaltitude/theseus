//! Whose red is it? (theseus-pw1q.10.) A red after a self-join is that
//! join's only when the join caused it: not when it was there before the
//! join, and not when an outer-loop join (a commit on main that no
//! `self.joined` row names) did.
//!
//! A gate or bench red names a commit of main. The first-parent commits of
//! main from the earliest watched join's parent to that commit are bisected
//! with [`Prober`] (main's gate on a tree at the commit, through the door):
//! the parent red means the red was there before ([`Blame::Before`]); else
//! the first red commit is the cause, and it is a self-join ([`Blame::Join`])
//! or not ([`Blame::Outer`]). A health red cannot be replayed, so it is the
//! newest self-join before it, unless an outer-loop commit landed after that
//! join and before the red ([`Blame::Outer`]).

use std::future::Future;
use std::pin::Pin;

use anyhow::{Context, Result};

use super::signals::Red;
use crate::rsi::join::git;
use crate::rsi::tree::{self, Place, Verdict};

/// A self-join in the window, as its `self.joined` row says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    pub merge: String,
    pub branch: String,
    pub item: String,
    pub head: String,
    pub at_ms: u64,
}

/// Whose red it is.
#[derive(Debug, Clone, PartialEq)]
pub enum Blame {
    /// This self-join's, and the commits tested to find it.
    Join(Joined, u32),
    /// An outer-loop commit's (not a self-join): the commit.
    Outer(String),
    /// Red before the earliest watched join.
    Before,
    /// Not decidable, and why.
    Unknown(String),
}

pub type ProbeFut<'a> = Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;

/// Is the gate red on a tree at `commit`?
pub trait Prober: Send + Sync {
    /// `benches`: the benches are part of the verdict (a bench red).
    fn red_at<'a>(&'a self, p: &'a Place, commit: &'a str, benches: bool) -> ProbeFut<'a>;
}

/// Main's gate on a detached tree at the commit, through the door.
pub struct MainsGate;

impl Prober for MainsGate {
    fn red_at<'a>(&'a self, p: &'a Place, commit: &'a str, benches: bool) -> ProbeFut<'a> {
        Box::pin(async move {
            let (pc, c) = (p.clone(), commit.to_string());
            let t =
                tokio::task::spawn_blocking(move || git::detached(&pc, "probe", &c, &c)).await??;
            let run = if benches {
                tree::gate_with_benches(p, &t).await
            } else {
                tree::gate(p, &t).await
            };
            let _ = tree::close(p, &t);
            // A probe a stop cut short says nothing about the commit: an error,
            // so the watch records nothing and the red is looked at again.
            Ok(run?.ran()?.verdict != Verdict::Green)
        })
    }
}

/// The first-parent commits of `tip` after `base`, oldest first.
pub fn chain(p: &Place, base: &str, tip: &str) -> Result<Vec<String>> {
    let out = p.git_ok(
        &p.repo,
        &[
            "rev-list",
            "--first-parent",
            "--reverse",
            &format!("{base}..{tip}"),
        ],
    )?;
    Ok(out.lines().map(str::to_string).collect())
}

/// A commit's full name, from the abbreviation a label holds.
pub fn resolve(p: &Place, short: &str) -> Option<String> {
    p.git_ok(
        &p.repo,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("{short}^{{commit}}"),
        ],
    )
    .ok()
}

fn commit_ms(p: &Place, c: &str) -> Result<u64> {
    let t = p.git_ok(&p.repo, &["show", "-s", "--format=%ct", c])?;
    Ok(t.trim().parse::<u64>().context("a commit time")? * 1000)
}

/// The red's blame; `main` is the remote's main.
pub async fn blame(
    p: &Place,
    prober: &dyn Prober,
    main: &str,
    joins: &[Joined],
    red: &Red,
) -> Result<Blame> {
    let Some(first) = joins.iter().min_by_key(|j| j.at_ms) else {
        return Ok(Blame::Unknown("no self-join is in the window".into()));
    };
    let (pc, f) = (p.clone(), first.merge.clone());
    let parent = tokio::task::spawn_blocking(move || {
        pc.git_ok(&pc.repo, &["rev-parse", "--verify", &format!("{f}^1")])
    })
    .await?
    .context("the earliest join's parent")?;
    let (pc, par, m) = (p.clone(), parent.clone(), main.to_string());
    let commits = tokio::task::spawn_blocking(move || chain(&pc, &par, &m)).await??;
    let is_join = |c: &str| joins.iter().find(|j| j.merge == c).cloned();
    if red.signal == "health" {
        return health_blame(p, &commits, joins, red).await;
    }
    let tip = match red.commit.as_deref() {
        Some(short) => resolve(p, short),
        None => commits.last().cloned(),
    };
    let Some(pos) = tip.and_then(|t| commits.iter().position(|c| *c == t)) else {
        return Ok(Blame::Unknown(
            "the red's commit is not on main since the earliest join".into(),
        ));
    };
    let benches = red.signal == "bench";
    if prober.red_at(p, &parent, benches).await? {
        return Ok(Blame::Before);
    }
    // `lo` is green (the parent), `hi` red (the signal's own testimony).
    let (mut lo, mut hi, mut tested) = (None::<usize>, pos, 0u32);
    loop {
        let next = lo.map_or(0, |l| l + 1);
        if next >= hi {
            break;
        }
        let mid = next + (hi - next) / 2;
        tested += 1;
        if prober.red_at(p, &commits[mid], benches).await? {
            hi = mid;
        } else {
            lo = Some(mid);
        }
    }
    let first_red = &commits[hi];
    Ok(match is_join(first_red) {
        Some(j) => Blame::Join(j, tested),
        None => Blame::Outer(first_red.clone()),
    })
}

/// A health red: the newest join before it, unless an outer commit came between.
async fn health_blame(p: &Place, commits: &[String], joins: &[Joined], red: &Red) -> Result<Blame> {
    let Some(j) = joins
        .iter()
        .filter(|j| j.at_ms <= red.at_ms)
        .max_by_key(|j| j.at_ms)
    else {
        return Ok(Blame::Before);
    };
    let Some(at) = commits.iter().position(|c| *c == j.merge) else {
        return Ok(Blame::Unknown("the join is not on main".into()));
    };
    for c in &commits[at + 1..] {
        let (pc, cc) = (p.clone(), c.clone());
        let ms = tokio::task::spawn_blocking(move || commit_ms(&pc, &cc)).await??;
        if ms <= red.at_ms && !joins.iter().any(|j| j.merge == *c) {
            return Ok(Blame::Outer(c.clone()));
        }
    }
    Ok(Blame::Join(j.clone(), 0))
}
