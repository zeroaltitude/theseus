//! Auto-revert and the owner's veto (theseus-pw1q.10; the owner's decision of
//! 2026-10-09: Theseus acts first, and a regression after a join undoes
//! itself). What makes a self-join safe is this twofold: a red it caused
//! reverts it with a notice, and the owner's veto is one click.
//!
//! **The watch** ([`Core::self_revert_pass`], from the tender, only with
//! `[self] mode = "act"` and never while the switch is halted): for
//! `[self] revert_window_hours` after each `self.joined`, three signals
//! ([`signals`]): main's gates, the benches against their budgets, and the
//! error rate against the hour before the join. A red is attributed
//! ([`attribute`]): one that was there before the join, or that an outer-loop
//! join (a commit on main no `self.joined` row names) caused, is not any
//! self-join's and leaves it alone; among the self-joins in the window the
//! first red commit is found by bisecting main's gate through `rsi::tree`.
//!
//! **The revert** ([`doing`]), under the self-join's lock: `git revert -m 1`
//! in a tree, main's gate through the door, a signed commit, the push, the
//! install by the self-join's path; then `self.reverted` and the owner's
//! notice in one frame. A revert that cannot reach main (a conflict, a red or
//! keel gate, no key) is the owner's card.
//!
//! **The veto** ([`Core::self_revert`], `theseus self revert <merge>`, the
//! cockpit's Revert button; also `flow:<name>` and `ext:<name>` for a minted
//! tool, one verb, theseus-pw1q.21.8): the owner's alone, from any place, by the
//! resume's rule (`judge_act(Act::SelfRevert)`, `places::owner_anywhere`); a
//! job's shell is refused with an `approval.refused` row. It reverts at once,
//! writes `self.vetoed`, and drops the join's backlog item for good. It works
//! with the mode off and the switch halted: it is the owner's own act.

pub mod attribute;
pub(crate) mod doing;
pub mod signals;
mod tender;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use theseus_protocol::rsi::{SelfRevertParams, SelfRevertResult};

use self::attribute::{Blame, Joined, MainsGate, Prober};
use self::signals::{Files, Signals};
use super::join::install::{self, Installer};
use super::join::{blocking, git, remote_of, rows_of};
use super::Gate;
use crate::approval::Answerer;
use crate::rpc::{Act, Core};

pub use doing::Done;

/// Why a revert runs.
#[derive(Debug, Clone)]
pub enum Cause {
    /// A signal turned red after the join.
    Auto {
        signal: &'static str,
        detail: String,
        /// The commits tested to find the join (0: attributed by time).
        tested: u32,
    },
    /// The owner's word.
    Veto {
        by: String,
        place: String,
        /// Whether the backlog item was dropped by this veto.
        dropped: bool,
    },
}

/// What the watch and the revert reach outside the core.
#[derive(Clone)]
pub struct Seams {
    pub signals: Arc<dyn Signals>,
    pub prober: Arc<dyn Prober>,
    pub installer: Arc<dyn Installer>,
    pub lock_wait: Duration,
    pub lock_every: Duration,
    /// A test's clock (unix ms).
    pub now_ms: Option<u64>,
    /// Called with each phase's name before its gate is asked.
    pub on_phase: Option<super::join::PhaseHook>,
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            signals: Arc::new(Files::default()),
            prober: Arc::new(MainsGate),
            installer: Arc::new(install::Systemd),
            lock_wait: Duration::from_secs(2 * 3600),
            lock_every: Duration::from_secs(30),
            now_ms: None,
            on_phase: None,
        }
    }
}

/// What one pass did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pass {
    /// What the gate said at the start.
    pub gate: String,
    /// Self-joins watched.
    pub watched: usize,
    /// `(merge, signal)` for each revert made.
    pub reverted: Vec<(String, String)>,
    /// `(signal, why)` for each red left alone.
    pub left: Vec<(String, String)>,
    /// `(merge, phase)` for each revert that stopped.
    pub stopped: Vec<(String, String)>,
}

/// A bad merge name: the caller's mistake, not the daemon's.
#[derive(Debug)]
pub struct BadMerge(pub String);

impl std::fmt::Display for BadMerge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadMerge {}

/// What the ledger holds of the joins and their reverts.
struct Seen {
    joined: Vec<Joined>,
    /// `(merge, revert)` of each revert or veto row.
    undone: Vec<(String, String)>,
    /// Merges whose revert stopped for the owner.
    held: HashSet<String>,
}

fn seen(core: &Core) -> Result<Option<Seen>> {
    let Some(rows) = rows_of(
        &core.store,
        &[
            "self.joined",
            "self.reverted",
            "self.vetoed",
            "self.join.stopped",
        ],
    )?
    else {
        return Ok(None);
    };
    let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    let mut out = Seen {
        joined: Vec::new(),
        undone: Vec::new(),
        held: HashSet::new(),
    };
    for r in rows {
        let d = &r.data;
        match r.kind.as_str() {
            "self.joined" => out.joined.push(Joined {
                merge: s(d, "merge"),
                branch: s(d, "branch"),
                item: s(d, "item"),
                head: s(d, "head"),
                at_ms: r.at_unix_ms,
            }),
            "self.reverted" | "self.vetoed" => out.undone.push((s(d, "merge"), s(d, "revert"))),
            "self.join.stopped"
                if d["numbers"]["held"] == true && s(d, "branch").starts_with("revert ") =>
            {
                out.held.insert(s(d, "head"));
            }
            _ => {}
        }
    }
    Ok(Some(out))
}

impl Core {
    /// One pass of the watch: the module's doc. `known` holds the reds already
    /// judged, so a red left alone is not tested again.
    pub async fn self_revert_pass(
        self: &Arc<Self>,
        seams: &Seams,
        known: &mut HashSet<String>,
    ) -> Result<Pass> {
        let mut pass = Pass::default();
        let g = super::gate(&self.self_ctx());
        pass.gate = g.name().into();
        let hours = self.cfg.self_improve.revert_window_hours;
        if g != Gate::Allowed || hours == 0 {
            return Ok(pass);
        }
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Ok(pass);
        };
        let core = self.clone();
        let Some(seen) = blocking(move || seen(&core)).await? else {
            return Ok(pass);
        };
        let now = seams.now_ms.unwrap_or_else(theseus_protocol::now_unix_ms);
        let watched: Vec<Joined> = seen
            .joined
            .iter()
            .filter(|j| {
                now.saturating_sub(j.at_ms) <= hours * 3_600_000
                    && !seen.undone.iter().any(|(m, _)| *m == j.merge)
                    && !seen.held.contains(&j.merge)
            })
            .cloned()
            .collect();
        pass.watched = watched.len();
        let Some(since) = watched.iter().map(|j| j.at_ms).min() else {
            return Ok(pass);
        };
        let (core, sg) = (self.clone(), seams.signals.clone());
        let reds = blocking(move || sg.reds(&core, since, now)).await?;
        let place = self.tools.self_pr.place(&repo);
        let remote = remote_of(&self.cfg);
        for red in reds {
            let key = format!("{}:{}:{:?}", red.signal, red.at_ms, red.commit);
            // Recorded once the red has been looked at, not before: a fetch or a
            // probe that errors returns early, and the next pass must see it again.
            if known.contains(&key) {
                continue;
            }
            let (p, r) = (place.clone(), remote.clone());
            let main = blocking(move || {
                git::fetch(&p, &r)?;
                git::main_of(&p, &r)
            })
            .await?;
            let blame = attribute::blame(&place, &*seams.prober, &main, &watched, &red).await?;
            known.insert(key);
            let why = match blame {
                Blame::Join(j, tested) => {
                    if let Some(why) = super::join::refused(self) {
                        pass.stopped
                            .push((j.merge.clone(), format!("halted: {why}")));
                        break;
                    }
                    let cause = Cause::Auto {
                        signal: red.signal,
                        detail: red.detail.clone(),
                        tested,
                    };
                    match self.revert_join(seams, &j, &cause).await? {
                        Done::Reverted(_) => pass.reverted.push((j.merge, red.signal.into())),
                        Done::Held { phase, .. } => pass.stopped.push((j.merge, phase)),
                    }
                    break;
                }
                Blame::Outer(c) => {
                    format!("an outer-loop commit ({}) caused it", &c[..c.len().min(12)])
                }
                Blame::Before => "it was red before the earliest join".into(),
                Blame::Unknown(why) => why,
            };
            pass.left.push((red.signal.into(), why));
        }
        Ok(pass)
    }

    /// The owner's veto of a self-join: the module's doc.
    pub async fn self_revert(
        self: &Arc<Self>,
        p: &SelfRevertParams,
        who: &Answerer,
        seams: &Seams,
    ) -> Result<SelfRevertResult> {
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(
            who,
            Act::SelfRevert {
                what: &p.merge,
                from_job,
            },
        )?;
        let want = p.merge.trim().to_string();
        // A flow is unloaded, not reverted: the frozen copy stays (theseus-pw1q.21.4).
        if let Some(name) = want.strip_prefix(crate::flow::tool::PREFIX) {
            let info = self
                .flow_retire(name, "owner", &who.via())
                .map_err(|e| BadMerge(format!("{e:#}")))?;
            return Ok(SelfRevertResult {
                merge: want,
                revert: info.digest,
                item: None,
                changed: true,
            });
        }
        if let Some(name) = want.strip_prefix("ext:") {
            return self.revert_minted_extension(name, &want, who);
        }
        if want.len() < 7 {
            return Err(
                BadMerge("name the merge by at least its first 7 characters".into()).into(),
            );
        }
        let core = self.clone();
        let seen = blocking(move || seen(&core)).await?.ok_or_else(|| {
            anyhow!("the ledger's index is still being built: ask again in a minute")
        })?;
        let mut hits: Vec<&Joined> = seen
            .joined
            .iter()
            .filter(|j| j.merge.starts_with(&want))
            .collect();
        hits.dedup_by(|a, b| a.merge == b.merge);
        let j = match hits.as_slice() {
            [j] => (*j).clone(),
            [] => {
                return Err(BadMerge(format!(
                    "no self-join has the merge {want}: only a self-join can be reverted this \
                     way (see `theseus self log`), and an outer-loop join is never reverted by \
                     Theseus"
                ))
                .into())
            }
            more => {
                return Err(BadMerge(format!(
                    "{want} names {} self-joins: use more of it",
                    more.len()
                ))
                .into())
            }
        };
        if let Some((_, revert)) = seen.undone.iter().find(|(m, _)| *m == j.merge) {
            return Ok(SelfRevertResult {
                merge: j.merge,
                revert: revert.clone(),
                item: Some(j.item),
                changed: false,
            });
        }
        let by = who.who();
        let (core, item, by2) = (self.clone(), j.item.clone(), by.clone());
        let dropped = blocking(move || core.self_drop_vetoed(&item, &by2)).await?;
        let cause = Cause::Veto {
            by,
            place: who.via(),
            dropped,
        };
        match self.revert_join(seams, &j, &cause).await? {
            Done::Reverted(revert) => Ok(SelfRevertResult {
                merge: j.merge,
                revert,
                item: Some(j.item),
                changed: true,
            }),
            Done::Held { phase, why } => Err(anyhow!(
                "the revert of {} stopped at the {phase}: {why}. The owner's card says the same; \
                 the change is still on main",
                &j.merge[..j.merge.len().min(12)]
            )),
        }
    }
}

impl Core {
    /// A minted extension is revoked, not reverted: the frozen copy stays
    /// (theseus-pw1q.21.8). Only one the `"self"` principal loaded: an owner's
    /// extension is `theseus extend revoke`'s.
    fn revert_minted_extension(
        &self,
        name: &str,
        want: &str,
        who: &Answerer,
    ) -> Result<SelfRevertResult> {
        let minted = crate::extend::load::LoadedSet::read(&self.store)?
            .loaded
            .get(name)
            .is_some_and(|l| l.acked_by == super::pr::PRINCIPAL);
        if !minted {
            return Err(BadMerge(format!(
                "no minted extension named {name:?} is loaded (an extension the owner loaded is \
                 `theseus extend revoke {name}`)"
            ))
            .into());
        }
        let r = self
            .revoke_loaded(name, None, ("owner", &who.via()), Some("reverted"), || {
                Ok(())
            })
            .map_err(|e| BadMerge(format!("{e:#}")))?;
        Ok(SelfRevertResult {
            merge: want.into(),
            revert: r.digest,
            item: None,
            changed: true,
        })
    }
}
