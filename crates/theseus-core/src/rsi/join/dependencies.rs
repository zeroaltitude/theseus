//! The keel check's dependency class (issue 20, "The builder cannot try a
//! library the machine does not already hold"; the owner's decision of
//! 2026-11-15 17:46:56): a branch whose change adds, removes or changes the
//! version or source of a package stops at the owner's card before it joins,
//! and the card lists each package: its name, its old and new version, and
//! where it comes from.
//!
//! It reads the two lock files (`Cargo.lock`, `cockpit/package-lock.json`) as
//! the branch left main and as the branch has them, because a manifest's
//! dependency line that names a new package must change the lock it agrees with.
//! A lock file change that changes no package (a reorder, a workspace member's
//! own entry) is no hit. A lock file that cannot be read, or an entry from a
//! source that may not be fetched, is a hit too: this check never passes by
//! default.

use anyhow::Result;

use super::keel::{Hit, DEPENDENCY};
use crate::rsi::deps::hosts::Endpoints;
use crate::rsi::deps::lock::{self, Lock};
use crate::rsi::deps::{CARGO_LOCK, NPM_LOCK};
use crate::rsi::tree::Place;

/// The most packages one card lists; the rest are counted in a last line.
const MAX_LISTED: usize = 300;

/// A lock file's text at `rev`, or none when `rev` has none.
fn blob(p: &Place, rev: &str, rel: &str) -> Option<String> {
    let o = p.git(&p.repo, &["show", &format!("{rev}:{rel}")]).ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The packages `base..head` changed, as one hit each.
pub fn hits(p: &Place, base: &str, head: &str) -> Result<Vec<Hit>> {
    let ends = Endpoints::registries();
    let mut lines = Vec::new();
    for (eco, rel) in [("cargo", CARGO_LOCK), ("npm", NPM_LOCK)] {
        let (old, new) = (blob(p, base, rel), blob(p, head, rel));
        if old == new {
            continue;
        }
        lines.extend(read_changes(
            eco,
            rel,
            old.as_deref(),
            new.as_deref(),
            &ends,
        ));
    }
    let more = lines.len().saturating_sub(MAX_LISTED);
    lines.truncate(MAX_LISTED);
    if more > 0 {
        lines.push(format!("and {more} more package changes"));
    }
    Ok(lines
        .into_iter()
        .map(|at| Hit {
            class: DEPENDENCY.into(),
            at,
        })
        .collect())
}

/// What one lock file's change says, as lines.
fn read_changes(
    eco: &str,
    rel: &str,
    old: Option<&str>,
    new: Option<&str>,
    ends: &Endpoints,
) -> Vec<String> {
    let parse = |t: Option<&str>| -> Result<Lock, String> {
        match t {
            None => Ok(Lock::default()),
            Some(t) if eco == "cargo" => lock::cargo(t),
            Some(t) => lock::npm(t, ends),
        }
    };
    let (old, new) = match (parse(old), parse(new)) {
        (Ok(o), Ok(n)) => (o, n),
        (Err(e), _) | (_, Err(e)) => {
            return vec![format!(
                "{rel} could not be read, so what it changes is not known: {e}"
            )];
        }
    };
    let mut lines = lock::changes(eco, &old.pkgs, &new.pkgs);
    lines.extend(
        new.problems
            .into_iter()
            .filter(|p| !old.problems.contains(p))
            .map(|p| format!("{eco}: {p}")),
    );
    lines
}
