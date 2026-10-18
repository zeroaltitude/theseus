//! The self budget (theseus-pw1q.3): one spend account for all
//! self-improvement, the owner's hard keel. Two lines, read at the start
//! (`[self.budget]`): `branch_usd` ($10) for one self branch, all its loops
//! together, and `day_usd` ($30) for the local day, across every branch and
//! every loop (the backlog's reading, a branch's model calls, the reviewer,
//! self-bench, self-exams). Raising either is the owner's.
//!
//! **Reserve before spending.** A self loop calls [`Budget::reserve`] with its
//! worst case before it spends, and [`Budget::settle`] with what it spent
//! after, so a loop never overruns a line by the cost of its last call. A
//! reservation that would take either line past its limit is refused before
//! any spend ([`Reserved::Stopped`]: which line, where it stands, what is
//! left) and writes a `self.budget.stopped` row, once for each branch and
//! line. The branch is kept as it is. A turn of a `"self"`-principal session
//! reserves its every model call this way (`turn/self_budget_step.rs`); the
//! per-loop caps (the pack writer's, consolidation's, the replay's) are
//! unchanged and their spend is not counted here twice: a loop that has its
//! own cap reserves here only what it spends outside a turn.
//!
//! **The account is the record.** One META record (`self.budget`) holds the
//! local day's settled spend, each branch's, and every open reservation. Each
//! reserve and each settle is one frame with its row, so a restart reads the
//! day back from the store. A reservation left open by a process that ended
//! is settled at its worst case, by the first use after the start (a frame
//! that writes a `self.budget.settled` row for each), never released: money
//! that may have been spent counts as spent.
//!
//! **Off writes nothing.** With `[self] mode = "off"` every call returns
//! [`Reserved::Stopped`] with [`Line::Off`] and reads and writes nothing.
//! The kill switch and the mode stay [`super::gate`]'s: the budget is the
//! money, not the permission.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_kernel::{micros_to_usd, usd_to_micros, Micros};
use theseus_protocol::rsi::{SelfBranchSpend, SelfBudgetView, SelfMode};
use theseus_store::{kinds, NewRecord};

use crate::config::self_improve::SelfBudgetConfig;
use crate::fact::rsi_budget::{BudgetReserved, BudgetSettled, BudgetStopped};
use crate::store::Store;

/// The account's META record.
pub const BUDGET_KEY: &str = "self.budget";

/// The line a refused reservation ran into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The one branch's line.
    Branch,
    /// The local day's line.
    Day,
    /// `[self] mode = "off"`: nothing self-directed spends.
    Off,
}

impl Line {
    pub fn name(self) -> &'static str {
        match self {
            Line::Branch => "branch",
            Line::Day => "day",
            Line::Off => "off",
        }
    }
}

/// What a refused reservation says: the line, where it stands, and what is
/// left under it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stopped {
    pub line: Line,
    /// The line itself, in dollars.
    pub line_usd: f64,
    /// What the line's account holds, settled and reserved together.
    pub spent_usd: f64,
    /// What is left under the line (never negative).
    pub left_usd: f64,
    /// What the refused reservation asked for.
    pub asked_usd: f64,
}

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Line::Off => f.write_str(
                "self-improvement is off ([self] mode = \"off\"), so nothing self-directed spends",
            ),
            l => write!(
                f,
                "the self budget's {} line, ${:.2}, stops this: ${:.2} is spent or reserved, \
                 ${:.2} is left, and the call asked for ${:.2}. The work is kept as it is; \
                 raising the line is the owner's.",
                l.name(),
                self.line_usd,
                self.spent_usd,
                self.left_usd,
                self.asked_usd
            ),
        }
    }
}

/// A worst case held on the account until [`Budget::settle`].
#[derive(Debug)]
#[must_use = "a reservation is settled with Budget::settle"]
pub struct Reservation {
    id: String,
    branch: Option<String>,
    micros: Micros,
}

impl Reservation {
    /// What it holds, in dollars.
    pub fn usd(&self) -> f64 {
        micros_to_usd(self.micros)
    }
}

/// What [`Budget::reserve`] answers.
#[derive(Debug)]
pub enum Reserved {
    Granted(Reservation),
    Stopped(Stopped),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Open {
    branch: Option<String>,
    kind: String,
    micros: Micros,
    at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct BranchAcct {
    /// Settled spend.
    spent: Micros,
    last_ms: u64,
    /// The line this branch last stopped at (the row is written once for it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stopped: Option<String>,
}

/// The stored account.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct State {
    /// The local day `day_spent` is for.
    pub(super) day: String,
    /// The day's settled spend, every branch and every loop together.
    day_spent: Micros,
    branches: BTreeMap<String, BranchAcct>,
    open: BTreeMap<String, Open>,
    /// The line a reservation with no branch last stopped at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stopped: Option<String>,
}

impl State {
    fn roll(&mut self, day: &str) {
        if self.day != day {
            self.day = day.to_string();
            self.day_spent = 0;
        }
    }

    fn open_total(&self) -> Micros {
        self.open.values().map(|o| o.micros).sum()
    }

    fn branch_open(&self, branch: &str) -> Micros {
        self.open
            .values()
            .filter(|o| o.branch.as_deref() == Some(branch))
            .map(|o| o.micros)
            .sum()
    }

    fn branch_total(&self, branch: &str) -> Micros {
        self.branches.get(branch).map_or(0, |b| b.spent) + self.branch_open(branch)
    }

    fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(BUDGET_KEY), self)
    }

    /// Settle every open reservation at its worst case (a restart's): the
    /// rows to write. A reservation of an earlier day counts to its branch
    /// and not to today.
    fn settle_all_open(&mut self, now_ms: u64) -> Vec<(Open, String)> {
        let open = std::mem::take(&mut self.open);
        let mut out = Vec::new();
        for (id, o) in open {
            if let Some(b) = &o.branch {
                let acct = self.branches.entry(b.clone()).or_default();
                acct.spent += o.micros;
                acct.last_ms = now_ms;
            }
            if day_of(o.at_ms) == self.day {
                self.day_spent += o.micros;
            }
            out.push((o, id));
        }
        out
    }
}

fn day_of(ms: u64) -> String {
    crate::judge::spend::local_day(ms)
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    st: State,
}

/// The self budget: its lines, and the account.
pub struct Budget {
    mode: SelfMode,
    branch_line: Micros,
    day_line: Micros,
    store: Store,
    inner: Mutex<Inner>,
    /// Each session's branch (`None`: not a self session), found once.
    sessions: Mutex<HashMap<String, Option<String>>>,
    /// The same for sessions with no task of their own, by their principal.
    principals: Mutex<HashMap<String, Option<String>>>,
}

impl Budget {
    pub fn new(mode: SelfMode, cfg: &SelfBudgetConfig, store: Store) -> Self {
        Self {
            mode,
            branch_line: usd_to_micros(cfg.branch_usd),
            day_line: usd_to_micros(cfg.day_usd),
            store,
            inner: Mutex::new(Inner::default()),
            sessions: Mutex::new(HashMap::new()),
            principals: Mutex::new(HashMap::new()),
        }
    }

    /// Whether self steps may spend at all (`[self] mode`).
    pub fn on(&self) -> bool {
        self.mode == SelfMode::Act
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read the account at the first use after the start, and settle what an
    /// earlier process left open, at its worst case.
    fn load(&self, inner: &mut Inner, now_ms: u64) -> Result<()> {
        if inner.loaded {
            return Ok(());
        }
        let mut st = self
            .store
            .get_meta::<State>(BUDGET_KEY)?
            .unwrap_or_default();
        st.roll(&day_of(now_ms));
        if !st.open.is_empty() {
            let settled = st.settle_all_open(now_ms);
            let mut records = Vec::with_capacity(settled.len() + 1);
            for (o, id) in &settled {
                let f = BudgetSettled {
                    reservation: id,
                    branch: o.branch.as_deref(),
                    kind: &o.kind,
                    spent_usd: micros_to_usd(o.micros),
                    released_usd: 0.0,
                    recovered: true,
                };
                records.push(crate::fact::row(&f, None, None)?);
            }
            records.push(st.record()?);
            self.store.append(&records)?;
        }
        inner.st = st;
        inner.loaded = true;
        Ok(())
    }

    /// Hold `worst_case_usd` for a self step of `kind` (`"turn"`,
    /// `"backlog"`, `"review"`, `"bench"`, `"exam"`, …) on the day and, when it
    /// is on a branch, on the branch. Refused, with nothing spent, when either
    /// line would pass.
    pub fn reserve(
        &self,
        kind: &str,
        branch: Option<&str>,
        worst_case_usd: f64,
    ) -> Result<Reserved> {
        let asked = usd_to_micros(worst_case_usd);
        if !self.on() {
            return Ok(Reserved::Stopped(Stopped {
                line: Line::Off,
                line_usd: 0.0,
                spent_usd: 0.0,
                left_usd: 0.0,
                asked_usd: micros_to_usd(asked),
            }));
        }
        let now = theseus_protocol::now_unix_ms();
        let mut g = self.lock();
        self.load(&mut g, now)?;
        let st = &mut g.st;
        st.roll(&day_of(now));
        // The branch's line first, then the day's.
        let line = branch
            .map(|b| (Line::Branch, self.branch_line, st.branch_total(b)))
            .filter(|(_, line, held)| held.saturating_add(asked) > *line)
            .or_else(|| {
                let held = st.day_spent + st.open_total();
                (held.saturating_add(asked) > self.day_line).then_some((
                    Line::Day,
                    self.day_line,
                    held,
                ))
            });
        if let Some((line, limit, held)) = line {
            let stopped = Stopped {
                line,
                line_usd: micros_to_usd(limit),
                spent_usd: micros_to_usd(held),
                left_usd: micros_to_usd(limit.saturating_sub(held)),
                asked_usd: micros_to_usd(asked),
            };
            let key = match line {
                Line::Day => format!("day:{}", st.day),
                _ => line.name().to_string(),
            };
            let seen = match branch {
                Some(b) => st
                    .branches
                    .entry(b.to_string())
                    .or_default()
                    .stopped
                    .as_ref(),
                None => st.stopped.as_ref(),
            };
            if seen != Some(&key) {
                match branch {
                    Some(b) => st.branches.entry(b.to_string()).or_default().stopped = Some(key),
                    None => st.stopped = Some(key),
                }
                let f = BudgetStopped {
                    branch,
                    line: line.name(),
                    line_usd: stopped.line_usd,
                    spent_usd: stopped.spent_usd,
                    asked_usd: stopped.asked_usd,
                    kind,
                };
                self.store
                    .append(&[crate::fact::row(&f, None, None)?, st.record()?])?;
            }
            return Ok(Reserved::Stopped(stopped));
        }
        let id = crate::new_id("sbr");
        st.open.insert(
            id.clone(),
            Open {
                branch: branch.map(str::to_string),
                kind: kind.to_string(),
                micros: asked,
                at_ms: now,
            },
        );
        let f = BudgetReserved {
            reservation: &id,
            branch,
            kind,
            reserved_usd: micros_to_usd(asked),
            day_usd: micros_to_usd(st.day_spent + st.open_total()),
        };
        let rows = crate::fact::row(&f, None, None).and_then(|r| Ok([r, st.record()?]));
        if let Err(e) = rows.and_then(|r| self.store.append(&r)) {
            st.open.remove(&id);
            return Err(e);
        }
        Ok(Reserved::Granted(Reservation {
            id,
            branch: branch.map(str::to_string),
            micros: asked,
        }))
    }

    /// Settle a reservation at what the step spent: the spend joins the day
    /// and the branch, and the rest of the hold is released. Spend past the
    /// hold counts in full (it was spent). A reservation a restart already
    /// settled changes nothing.
    pub fn settle(&self, r: Reservation, actual_usd: f64) -> Result<()> {
        if !self.on() {
            return Ok(());
        }
        let actual = usd_to_micros(actual_usd);
        let now = theseus_protocol::now_unix_ms();
        let mut g = self.lock();
        self.load(&mut g, now)?;
        let st = &mut g.st;
        let Some(open) = st.open.remove(&r.id) else {
            return Ok(());
        };
        st.roll(&day_of(now));
        st.day_spent += actual;
        if let Some(b) = &r.branch {
            let acct = st.branches.entry(b.clone()).or_default();
            acct.spent += actual;
            acct.last_ms = now;
        }
        let f = BudgetSettled {
            reservation: &r.id,
            branch: r.branch.as_deref(),
            kind: &open.kind,
            spent_usd: micros_to_usd(actual),
            released_usd: micros_to_usd(open.micros.saturating_sub(actual)),
            recovered: false,
        };
        let rows = crate::fact::row(&f, None, None).and_then(|r| Ok([r, st.record()?]));
        if let Err(e) = rows.and_then(|r| self.store.append(&r)) {
            // The settle's frame failed: the hold stays open, at its worst
            // case, which is the safe side; a restart settles it.
            st.open.insert(r.id, open);
            st.day_spent -= actual;
            if let Some(acct) = r.branch.as_ref().and_then(|b| st.branches.get_mut(b)) {
                acct.spent -= actual;
            }
            return Err(e);
        }
        Ok(())
    }

    /// The branch a session's spend counts to, when it is a `"self"` task's
    /// session: the root task of its tree (a subtask's spend is its branch's).
    /// Found once and kept; `None` for any other session.
    pub fn branch_of(&self, session_id: &str) -> Option<String> {
        if let Some(b) = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
        {
            return b.clone();
        }
        let found = self.find_branch(session_id);
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session_id.to_string(), found.clone());
        found
    }

    /// The branch of a session with no task of its own, opened under the
    /// `"self"` principal (`principal` reads its execution's, once): the
    /// backlog starts an item this way (seam S3), so its spend counts to the
    /// item whose `session` it is, else to the session. `None` for any other
    /// principal. Found once and kept, as [`Budget::branch_of`] does.
    pub fn branch_of_principal(
        &self,
        session_id: &str,
        principal: impl FnOnce() -> String,
    ) -> Option<String> {
        if let Some(b) = self
            .principals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session_id)
        {
            return b.clone();
        }
        let found = (principal() == super::SELF_PRINCIPAL).then(|| {
            crate::task_graph::all(&self.store)
                .ok()
                .and_then(|all| {
                    all.into_iter().find(|t| {
                        t.session.as_deref() == Some(session_id)
                            && t.origin.principal == super::SELF_PRINCIPAL
                    })
                })
                .map_or_else(|| session_id.to_string(), |t| self.root_of(t))
        });
        self.principals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session_id.to_string(), found.clone());
        found
    }

    fn find_branch(&self, session_id: &str) -> Option<String> {
        use crate::task_graph as g;
        let rec = g::get(&self.store, &g::of_session(session_id))
            .ok()
            .flatten()?;
        if rec.origin.principal != super::SELF_PRINCIPAL {
            return None;
        }
        Some(self.root_of(rec))
    }

    /// The root of a task's tree: a bounded walk up, through the ancestors
    /// that have a session (a parent with none, as the backlog's, is a
    /// folder of branches, not a branch).
    fn root_of(&self, mut rec: crate::task_graph::TaskRecord) -> String {
        use crate::task_graph as g;
        for _ in 0..16 {
            let Some(parent) = rec.parent.clone() else {
                break;
            };
            match g::get(&self.store, &parent).ok().flatten() {
                Some(p) if p.session.is_some() => rec = p,
                _ => break,
            }
        }
        rec.id
    }

    /// The account as `self.log` and health show it: none while the mode is
    /// off. Reads the store once if nothing has used the budget since the
    /// start, and writes nothing (a reservation an earlier process left open
    /// counts as spent here, as the first use will settle it).
    pub fn view(&self) -> Option<SelfBudgetView> {
        if !self.on() {
            return None;
        }
        let now = theseus_protocol::now_unix_ms();
        let day = day_of(now);
        let kept = {
            let g = self.lock();
            g.loaded.then(|| g.st.clone())
        };
        let mut st = kept.unwrap_or_else(|| {
            self.store
                .get_meta::<State>(BUDGET_KEY)
                .ok()
                .flatten()
                .unwrap_or_default()
        });
        st.roll(&day);
        let mut branches: Vec<(u64, SelfBranchSpend)> = st
            .branches
            .iter()
            .map(|(name, a)| {
                let open = st.branch_open(name);
                (
                    a.last_ms,
                    SelfBranchSpend {
                        branch: name.clone(),
                        spent_usd: micros_to_usd(a.spent + open),
                        open_usd: micros_to_usd(open),
                        stopped: a.stopped.is_some(),
                    },
                )
            })
            .collect();
        branches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.branch.cmp(&b.1.branch)));
        Some(SelfBudgetView {
            day,
            day_spent_usd: micros_to_usd(st.day_spent + st.open_total()),
            day_line_usd: micros_to_usd(self.day_line),
            branch_line_usd: micros_to_usd(self.branch_line),
            branches: branches.into_iter().map(|(_, b)| b).collect(),
        })
    }
}
