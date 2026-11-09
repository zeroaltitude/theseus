//! A sweep says so before it stops for the day (the owner: "for the low dollar
//! budget sweeps (ontology, people, etc): lets raise to 5$ and it should
//! notify if it's going to stop").
//!
//! Seven background sweeps each hold their own day budget, read back from
//! their own rows (consolidation, people, ontology, the learn writer, the
//! owner card, the thread import, the route bench's replay), so their
//! arithmetic cannot be one function. What can be one is the notice: every
//! site that is about to refuse a call for want of its day's money first asks
//! [`TurnRunner::sweep_heads_up`] with its [`SweepId`], and that is the only
//! way a sweep's stop is told. It says so twice a local day at most: when the
//! day's spend, with the next call's worst case, reaches [`NEAR_PERCENT`] of
//! the cap ("going to stop"), and when the call that would pass the cap is
//! refused ("stopped"). One META mark per sweep holds the day and the stage
//! said, in the frame of the row and the owner's post.
//!
//! [`SWEEPS`] lists them, and `tests` walks the list and the source: a sweep
//! with no call site fails it.

use serde_json::json;
use theseus_judge::price::Micros;

use crate::fact::limits::SweepStopping;
use crate::turn::TurnRunner;

/// The share of its day's cap at which a sweep says it is going to stop.
pub const NEAR_PERCENT: u64 = 80;

/// A sweep whose day budget the owner raised and wants told of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepId {
    Consolidation,
    People,
    Ontology,
    LearnWriter,
    Card,
    ThreadImport,
    RouteBench,
}

/// One sweep: its row in the owner's spending table, its words, its key.
#[derive(Debug, Clone, Copy)]
pub struct Sweep {
    pub id: SweepId,
    /// Its row in the spending table.
    pub row: &'static str,
    /// What the owner calls it.
    pub name: &'static str,
    /// The config key that is its day's cap.
    pub key: &'static str,
}

/// Every sweep that has a day budget of its own.
pub const SWEEPS: [Sweep; 7] = [
    Sweep {
        id: SweepId::Consolidation,
        row: "D7",
        name: "consolidation",
        key: "[memory] synth_limit_usd_per_day",
    },
    Sweep {
        id: SweepId::People,
        row: "D8",
        name: "people",
        key: "[people] sweep_usd_per_day",
    },
    Sweep {
        id: SweepId::Ontology,
        row: "D9",
        name: "ontology",
        key: "[ontology] sweep_usd_per_day",
    },
    Sweep {
        id: SweepId::LearnWriter,
        row: "D10",
        name: "learn writer",
        key: "[judge.learn] writer_limit_usd_per_day",
    },
    Sweep {
        id: SweepId::Card,
        row: "D16",
        name: "card",
        key: "[card] day_usd",
    },
    Sweep {
        id: SweepId::ThreadImport,
        row: "D17",
        name: "thread import",
        key: "[threads] import_day_usd",
    },
    Sweep {
        id: SweepId::RouteBench,
        row: "D18",
        name: "route-bench replay",
        key: "[route_bench] replay_day_usd",
    },
];

impl SweepId {
    pub fn sweep(self) -> &'static Sweep {
        SWEEPS
            .iter()
            .find(|s| s.id == self)
            .expect("every sweep is listed")
    }
}

/// How near its stop a sweep is: what a notice says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Under the near line: nothing to say.
    Quiet,
    /// Reaching the near line: it is going to stop.
    Near,
    /// The next call would pass the cap: it has stopped.
    Stopped,
}

impl Stage {
    pub fn word(self) -> &'static str {
        match self {
            Stage::Quiet => "quiet",
            Stage::Near => "near",
            Stage::Stopped => "stopped",
        }
    }
}

/// The stage of a sweep with `spent` today under `cap`, about to reserve
/// `need`. A cap of 0 is the sweep off, never a stop.
pub fn stage(cap: Micros, spent: Micros, need: Micros) -> Stage {
    if cap == 0 {
        return Stage::Quiet;
    }
    let after = spent.saturating_add(need);
    if after > cap {
        Stage::Stopped
    } else if after.saturating_mul(100) >= cap.saturating_mul(NEAR_PERCENT) {
        Stage::Near
    } else {
        Stage::Quiet
    }
}

/// The META key of a sweep's mark: the day and the stage last said.
pub fn mark_key(id: SweepId) -> String {
    format!("sweep.notice.{}", id.sweep().row.to_lowercase())
}

/// The owner's words for a stage.
pub fn words(s: &Sweep, stage: Stage, cap: Micros, spent: Micros, need: Micros) -> String {
    use crate::narrative::dollars;
    let (title, what) = match stage {
        Stage::Near => (
            format!("The {} sweep is going to stop for today", s.name),
            format!(
                "It has spent {} of its {} day budget, and its next call would reserve {}.",
                dollars(spent),
                dollars(cap),
                dollars(need)
            ),
        ),
        _ => (
            format!("The {} sweep has stopped for today", s.name),
            format!(
                "Its next call would reserve {} on top of the {} spent, past its {} day budget.",
                dollars(need),
                dollars(spent),
                dollars(cap)
            ),
        ),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &title,
        &what,
        &format!(
            "It starts again when the local day turns. `{}` is the budget; a higher one applies at \
             the daemon's next start.",
            s.key
        ),
    )
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Mark {
    day: String,
    stage: u8,
}

impl TurnRunner {
    /// A sweep is about to reserve `need` with `spent` of today's `cap` spent
    /// (all in micros): says so once if it is going to stop, and once if it
    /// has. Quiet, and no frame, otherwise; never fails the sweep.
    pub fn sweep_heads_up(&self, id: SweepId, cap: Micros, spent: Micros, need: Micros) -> Stage {
        let at = stage(cap, spent, need);
        if at == Stage::Quiet {
            return at;
        }
        let s = id.sweep();
        let today = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
        let key = mark_key(id);
        let said: Mark = self.store.get_meta(&key).ok().flatten().unwrap_or_default();
        if said.day == today && said.stage >= at as u8 {
            return at;
        }
        let text = words(s, at, cap, spent, need);
        let dm = self.place_rule.owner_dm_bound();
        let written = (|| -> anyhow::Result<()> {
            let mut frame = Vec::new();
            let post = match dm {
                true => {
                    let body = json!({"kind": "notice", "text": text});
                    let (a, records) = self.outbox.stage_to_operator(None, body)?;
                    frame.extend(records);
                    Some(a)
                }
                false => None,
            };
            let f = SweepStopping {
                sweep: s.name,
                row: s.row,
                key: s.key,
                stage: at.word(),
                cap_micros: cap,
                spent_micros: spent,
                needed_micros: need,
                posted: post.is_some(),
                text: &text,
            };
            frame.push(crate::fact::row(&f, None, None)?);
            frame.push(theseus_store::NewRecord::json(
                theseus_store::kinds::META,
                Some(&key),
                &Mark {
                    day: today.clone(),
                    stage: at as u8,
                },
            )?);
            self.store.append(&frame)?;
            if let Some(a) = post {
                self.outbox.posted(&a);
            }
            Ok(())
        })();
        if let Err(e) = written {
            tracing::warn!(sweep = s.name, error = %format!("{e:#}"), "a sweep's notice was not written");
        }
        at
    }
}

#[cfg(test)]
mod tests;
