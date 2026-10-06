//! The ladder (M5 step 26a; design `m5-judgment.md` §2.7): each pack
//! version's mode, in the store, and the one answer every point asks.
//!
//! - **The mode in the store.** A `pack.mode` row per change, scoped per
//!   pack id on its own (`pack:<id>`), so a first read is a few rows: the
//!   version, its mode (`off`, `shadow`, `canary` with a share, `live`,
//!   `rolled_back`), the mode before, who (`owner` or `system`) and through
//!   what, why, the report it cites with its holdout's bounds, `forced` with
//!   the numbers, and a rollback's rule and words. A version's latest row is
//!   its mode; with none, the line this build wires it at (`WIRED`). Read
//!   once after serving (`Core::warm_ladder`), or by an RPC or the nightly
//!   check, then kept in memory and changed as rows are written. A point
//!   never reads it (theseus-289c): before the warm read, [`Ladder::given`]
//!   answers from the wired line under the config, a pack that would act in
//!   shadow, since a row the ladder has not read may have rolled it back.
//! - **One answer** ([`Ladder::given`], `JudgeService::mode_for`): off,
//!   shadow, or live for one session, under the config's ceiling
//!   (`JudgeConfig::mode_of`, unchanged: it lowers, never raises). A canary
//!   acts where `learn::arm(session, pack, share)` is canary and judges the
//!   control in shadow; nothing is stored on the session, and each
//!   judgment's context records its arm. `rolled_back` acts as shadow.
//! - **Rollback** (`rules`): each event a pack's rules count is a
//!   `pack.event` row scoped to its pack id and day, so a restart reads the
//!   day's events back before it counts; the rules are checked as each
//!   lands, and again by the nightly run.
//! - **Adoption** (`adopt`): the packs live before the ladder stand as the
//!   owner's promotions, written once after the first read: the warm read
//!   writes them between turns (`Core::warm_ladder`), an RPC's first read at
//!   once.
//! - **Promotion** (`promote`): the bar an automatic promotion must clear,
//!   and the numbers an owner's forced one carries.

pub mod adopt;
pub mod promote;
pub mod rules;

use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use theseus_judge::learn::{self, CanaryEvent};
use theseus_protocol::packs::PackModeRow;
use theseus_protocol::LedgerKind;

use crate::config::{JudgeConfig, PackMode};
use crate::ledger::LedgerRow;
use crate::store::Store;

/// A pack's modes' scope: `pack:loop`.
pub fn scope(pack: &str) -> String {
    format!("pack:{}", id_of(pack))
}

/// A pack's events of one local day: `pack.event:security:2026-10-04`.
pub fn events_scope(pack: &str, day: &str) -> String {
    format!("pack.event:{}:{day}", id_of(pack))
}

/// A pack's id, from its name (`loop.v1`) or its id (`loop`).
pub fn id_of(pack: &str) -> &str {
    pack.split('.').next().unwrap_or(pack)
}

/// A rung of the ladder: [`PackMode`]'s four, and `rolled_back`, which acts
/// as shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rung {
    Off,
    #[default]
    Shadow,
    Canary,
    Live,
    RolledBack,
}

impl Rung {
    pub fn as_str(self) -> &'static str {
        match self {
            Rung::Off => "off",
            Rung::Shadow => "shadow",
            Rung::Canary => "canary",
            Rung::Live => "live",
            Rung::RolledBack => "rolled_back",
        }
    }

    pub fn parse(s: &str) -> Option<Rung> {
        Some(match s {
            "off" => Rung::Off,
            "shadow" => Rung::Shadow,
            "canary" => Rung::Canary,
            "live" => Rung::Live,
            "rolled_back" => Rung::RolledBack,
            _ => return None,
        })
    }

    pub fn of(m: PackMode) -> Rung {
        match m {
            PackMode::Off => Rung::Off,
            PackMode::Shadow => Rung::Shadow,
            PackMode::Canary => Rung::Canary,
            PackMode::Live => Rung::Live,
        }
    }

    /// What it does, before the config's ceiling.
    pub fn acts_as(self) -> PackMode {
        match self {
            Rung::Off => PackMode::Off,
            Rung::Shadow | Rung::RolledBack => PackMode::Shadow,
            Rung::Canary => PackMode::Canary,
            Rung::Live => PackMode::Live,
        }
    }

    /// Whether its pack acts somewhere: the rungs a rollback moves down from.
    pub fn acts(self) -> bool {
        matches!(self, Rung::Canary | Rung::Live)
    }
}

/// The arm a judgment records (design §2.5: canary, control, all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmOf {
    Canary,
    Control,
    All,
}

impl ArmOf {
    pub fn as_str(self) -> &'static str {
        match self {
            ArmOf::Canary => "canary",
            ArmOf::Control => "control",
            ArmOf::All => "all",
        }
    }
}

/// The ladder's answer for one session: what the pack does there, and the
/// arm its judgment records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Given {
    /// `Off`, `Shadow`, or `Live` (a canary's canary arm acts, so `Live`).
    pub mode: PackMode,
    pub arm: ArmOf,
}

impl Given {
    pub const OFF: Given = Given {
        mode: PackMode::Off,
        arm: ArmOf::All,
    };

    pub fn on(self) -> bool {
        self.mode != PackMode::Off
    }

    /// The judgment's mode, as its row records it.
    pub fn judge_mode(self) -> theseus_judge::Mode {
        match (self.mode, self.arm) {
            (PackMode::Live, ArmOf::Canary) => theseus_judge::Mode::Canary,
            (PackMode::Live, _) => theseus_judge::Mode::Live,
            _ => theseus_judge::Mode::Shadow,
        }
    }
}

/// A version's standing: its rows folded, as of a time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Standing {
    pub rung: Rung,
    pub share: Option<f64>,
    /// `owner: decision of 2026-10-04`, `wired`, …
    pub why: String,
    /// A day's brake lapses then.
    pub until_ms: Option<u64>,
    /// The rule that rolled it back.
    pub rule: Option<String>,
    /// The latest row that set it: the events a rule counts come after it.
    pub position: u64,
    /// When it was last rolled back: a move up cites a report after it.
    pub rolled_back_at: Option<u64>,
}

impl Standing {
    /// Health's words for it: `live (owner: decision of 2026-10-04)`,
    /// `canary 0.2 (owner: …)`, `rolled back until 00:00 (notices_per_day)`.
    pub fn words(&self) -> String {
        match self.rung {
            Rung::RolledBack => format!(
                "rolled back{} ({})",
                crate::fact::ladder::until_words(self.until_ms),
                self.rule.as_deref().unwrap_or(&self.why)
            ),
            r if self.why == WIRED_WHY => r.as_str().to_string(),
            r => format!(
                "{} ({})",
                crate::fact::ladder::mode_words(r.as_str(), self.share),
                self.why
            ),
        }
    }
}

/// A version with no row stands at its wired line, for this reason.
pub const WIRED_WHY: &str = "wired";

/// Fold a version's rows (oldest first, declined ones skipped) onto its
/// wired line, as of `now`: a brake whose `until` passed leaves what it
/// stood on before.
pub fn fold(wired: PackMode, rows: &[PackModeRow], now: u64) -> Standing {
    let mut cur = Standing {
        rung: Rung::of(wired),
        why: WIRED_WHY.into(),
        ..Standing::default()
    };
    let mut before: Option<Standing> = None;
    let lapse = |cur: &mut Standing, before: &mut Option<Standing>, at: u64| {
        if cur.rung == Rung::RolledBack && cur.until_ms.is_some_and(|u| u <= at) {
            if let Some(b) = before.take() {
                let rolled_back_at = cur.rolled_back_at;
                *cur = Standing {
                    rolled_back_at,
                    ..b
                };
            }
        }
    };
    for r in rows.iter().filter(|r| !r.declined) {
        lapse(&mut cur, &mut before, r.at_unix_ms);
        let Some(rung) = Rung::parse(&r.mode) else {
            continue;
        };
        if rung == Rung::RolledBack && r.until_ms.is_some() {
            before = Some(cur.clone());
        } else {
            before = None;
        }
        cur = Standing {
            rung,
            share: (rung == Rung::Canary).then_some(r.share).flatten(),
            why: format!("{}: {}", r.who, r.why),
            until_ms: r.until_ms,
            rule: r.rule.clone(),
            position: r.position,
            rolled_back_at: match rung {
                Rung::RolledBack => Some(r.at_unix_ms),
                _ => cur.rolled_back_at,
            },
        };
    }
    lapse(&mut cur, &mut before, now);
    cur
}

/// What the first read loaded, kept current as rows are written.
#[derive(Debug, Default)]
pub(crate) struct Loaded {
    /// Each version's `pack.mode` rows, oldest first.
    pub(crate) rows: BTreeMap<String, Vec<PackModeRow>>,
    /// The local day `events` and `brakes` are of.
    pub(crate) day: String,
    /// Today's events, by pack id, with their positions.
    pub(crate) events: BTreeMap<String, Vec<(u64, CanaryEvent)>>,
    /// Pack ids braked today by their own step (a `judge.paused` row,
    /// `what: "notices"`, read by its day's key: `rules::brakes_today`).
    pub(crate) brakes: BTreeSet<String>,
}

/// A clock in unix ms: the wall's, or a test's on tokio's paused clock.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Where a written row's sentence goes (the judge's narrator).
pub type Said = Arc<dyn Fn(&PackModeRow) + Send + Sync>;

pub struct Ladder {
    store: Store,
    /// The wired lines: `WIRED`, or a test's.
    wired: Mutex<Vec<(String, PackMode)>>,
    clock: Mutex<Clock>,
    loaded: Mutex<Option<Loaded>>,
    said: Mutex<Option<Said>>,
    /// Tests: the day's reads the ladder made (each load's, and any other).
    #[cfg(test)]
    reads: AtomicUsize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Ladder {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            wired: Mutex::new(
                super::WIRED
                    .iter()
                    .map(|(p, m)| ((*p).to_string(), *m))
                    .collect(),
            ),
            clock: Mutex::new(Arc::new(theseus_protocol::now_unix_ms)),
            loaded: Mutex::new(None),
            said: Mutex::new(None),
            #[cfg(test)]
            reads: AtomicUsize::new(0),
        }
    }

    /// Tests: the day's reads the ladder has made (each load reads one).
    #[cfg(test)]
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    /// Where each row it writes is said.
    pub fn say_to(&self, said: Said) {
        *lock(&self.said) = Some(said);
    }

    /// Say a row written (by the ladder, or in a caller's frame).
    pub fn say(&self, row: &PackModeRow) {
        let said = lock(&self.said).clone();
        if let Some(s) = said {
            s(row);
        }
    }

    /// Tests: the clock the ladder reads.
    pub fn set_clock(&self, clock: Clock) {
        *lock(&self.clock) = clock;
    }

    /// Tests: wire `pack` at `mode`, before the first read (a build whose
    /// live pack has joined).
    pub fn wire(&self, pack: &str, mode: PackMode) {
        let mut w = lock(&self.wired);
        w.retain(|(p, _)| p != pack);
        w.push((pack.to_string(), mode));
    }

    pub fn now(&self) -> u64 {
        (lock(&self.clock))()
    }

    /// The line this build wires `pack` at; shadow for a pack it does not.
    pub fn wired(&self, pack: &str) -> PackMode {
        lock(&self.wired)
            .iter()
            .find(|(p, _)| p == pack)
            .map_or(PackMode::Shadow, |(_, m)| *m)
    }

    pub fn wired_packs(&self) -> Vec<(String, PackMode)> {
        lock(&self.wired).clone()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Whether the first read has happened.
    pub fn is_loaded(&self) -> bool {
        lock(&self.loaded).is_some()
    }

    /// Run `f` on what is loaded, loading it first (and writing the
    /// adoptions it finds missing). A store that cannot be read leaves it
    /// unloaded, and `f` sees an empty ladder: every pack at its wired line.
    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Loaded, &Ladder) -> T) -> T {
        let mut g = lock(&self.loaded);
        if g.is_none() {
            match self.load() {
                Ok(l) => *g = Some(l),
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "judge: the ladder cannot be read; every pack stands at its wired line");
                    let mut empty = Loaded::default();
                    return f(&mut empty, self);
                }
            }
            if let Some(l) = g.as_mut() {
                adopt::adopt_missing(self, l);
            }
        }
        let l = g.as_mut().expect("loaded");
        let today = crate::judge::spend::local_day(self.now());
        if l.day != today {
            // A new local day: its events and brakes start empty, and nothing
            // is read (theseus-289c). Every event since midnight in this
            // process landed through `land`, and a brake the notices write
            // reloads the ladder (`notice::pause_notices`); a restart's are
            // read by its first read.
            l.day = today;
            l.events.clear();
            l.brakes.clear();
        }
        f(l, self)
    }

    /// The warm read (`Core::warm_ladder`): the rows, today's events and
    /// brakes, read now, and nothing written. The adoptions it finds missing
    /// are [`Ladder::adopt`]'s, written between turns. A store that cannot
    /// be read leaves it unloaded, and every point keeps the pre-read rule.
    pub fn read(&self) {
        let mut g = lock(&self.loaded);
        if g.is_some() {
            return;
        }
        match self.load() {
            Ok(l) => *g = Some(l),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the ladder cannot be read; every pack stands at its pre-read line");
            }
        }
    }

    /// Whether an adoption the store lacks would be written (read, so
    /// nothing when it is not).
    pub fn adoption_missing(&self) -> bool {
        match lock(&self.loaded).as_ref() {
            Some(l) => adopt::missing(self, l),
            None => false,
        }
    }

    /// Write the adoptions the store lacks, reading the ladder first if it
    /// is not.
    pub fn adopt(&self) {
        self.with(|l, me| adopt::adopt_missing(me, l));
    }

    /// Drop what is loaded: the next read is from the store (the nightly
    /// check's).
    pub fn forget(&self) {
        *lock(&self.loaded) = None;
    }

    /// Read it again from the store, now (a row written in a caller's
    /// frame), so health never falls back to the wired lines meanwhile.
    pub fn reload(&self) {
        self.forget();
        self.with(|_, _| ());
    }

    fn load(&self) -> Result<Loaded> {
        let mut l = Loaded {
            day: crate::judge::spend::local_day(self.now()),
            ..Loaded::default()
        };
        let mut ids: BTreeSet<String> = self
            .wired_packs()
            .iter()
            .map(|(p, _)| id_of(p).to_string())
            .collect();
        ids.extend(adopt::ADOPTED.iter().map(|a| id_of(a.pack).to_string()));
        for id in &ids {
            for r in self.store.scope_after(&scope(id), 0)? {
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                if row.kind != LedgerKind::PackMode.as_str() {
                    continue;
                }
                if let Ok(mut m) = serde_json::from_value::<PackModeRow>(row.data) {
                    m.position = r.position;
                    m.at_unix_ms = row.at_unix_ms;
                    l.rows.entry(m.pack.clone()).or_default().push(m);
                }
            }
        }
        self.read_day(&mut l);
        Ok(l)
    }

    /// Today's events and brakes, from their scopes.
    fn read_day(&self, l: &mut Loaded) {
        #[cfg(test)]
        self.reads.fetch_add(1, Ordering::SeqCst);
        let ids: BTreeSet<String> = l
            .rows
            .keys()
            .map(|p| id_of(p).to_string())
            .chain(self.wired_packs().iter().map(|(p, _)| id_of(p).to_string()))
            .collect();
        for id in &ids {
            let Ok(records) = self.store.scope_after(&events_scope(id, &l.day), 0) else {
                continue;
            };
            for r in records {
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                if let Ok(ev) = serde_json::from_value::<CanaryEvent>(row.data["event"].clone()) {
                    l.events
                        .entry(id.clone())
                        .or_default()
                        .push((r.position, ev));
                }
            }
        }
        l.brakes = rules::brakes_today(&self.store, &l.day);
    }

    /// A version's standing now.
    pub fn standing(&self, pack: &str) -> Standing {
        self.with(|l, me| me.standing_in(l, pack))
    }

    pub(crate) fn standing_in(&self, l: &Loaded, pack: &str) -> Standing {
        let now = self.now();
        let rows = l.rows.get(pack).map_or(&[][..], Vec::as_slice);
        let mut s = fold(self.wired(pack), rows, now);
        // A brake the notices' own step recorded today reads as a rollback
        // until midnight.
        if s.rung.acts() && l.brakes.contains(id_of(pack)) {
            s = Standing {
                rung: Rung::RolledBack,
                why: "the notices' brake".into(),
                until_ms: Some(rules::next_midnight(now)),
                rule: Some("notices".into()),
                ..s
            };
        }
        s
    }

    /// The ladder's answer for `pack` in `session`, under `cfg`'s ceiling.
    /// A ceiling at shadow or below needs no read: rows never give `off`.
    /// Nor does any point (theseus-289c): before the warm read, the answer
    /// is [`Ladder::unread`]'s, and nothing is read or written on the
    /// asking thread.
    pub fn given(&self, cfg: &JudgeConfig, pack: &str, session: &str) -> Given {
        let ceiling = cfg.mode_of(pack, PackMode::Live);
        if ceiling <= PackMode::Shadow {
            return match ceiling.min(self.wired(pack)) {
                PackMode::Off => Given::OFF,
                _ => Given {
                    mode: PackMode::Shadow,
                    arm: ArmOf::All,
                },
            };
        }
        if !self.is_loaded() {
            return given_of(self.unread(cfg, pack), None, pack, session);
        }
        let s = self.standing(pack);
        given_of(cfg.mode_of(pack, s.rung.acts_as()), s.share, pack, session)
    }

    /// What `pack` does before the warm read: its wired line under the
    /// config, a pack that would act in shadow. A stored row may have put it
    /// below its line (a rollback, the owner's or a rule's), and an unread
    /// ladder never acts on a pack the owner rolled back: it judges, and
    /// nothing acts, until the read a moment after serving.
    pub fn unread(&self, cfg: &JudgeConfig, pack: &str) -> PackMode {
        cfg.mode_of(pack, self.wired(pack)).min(PackMode::Shadow)
    }

    /// Every version's rows, oldest first (`pack.list`), from what is
    /// loaded.
    pub fn rows_of(&self, pack: &str) -> Vec<PackModeRow> {
        self.with(|l, _| l.rows.get(pack).cloned().unwrap_or_default())
    }

    /// Write a `pack.mode` row (scoped per pack id) and keep it: the row
    /// with its position and time. Its sentence is the caller's to say.
    pub fn write(&self, row: PackModeRow) -> Result<PackModeRow> {
        self.with(|l, me| me.write_in(l, row))
    }

    pub(crate) fn write_in(&self, l: &mut Loaded, row: PackModeRow) -> Result<PackModeRow> {
        let rec = self.row_record(&row)?;
        self.commit_row(l, row, &[rec])
    }

    /// Several rows in one frame (the adoptions), each kept with its
    /// position and said.
    pub(crate) fn write_all_in(&self, l: &mut Loaded, rows: Vec<PackModeRow>) -> Result<()> {
        let records = rows
            .iter()
            .map(|r| self.row_record(r))
            .collect::<Result<Vec<_>>>()?;
        let at = self.now();
        let positions = self.store.append(&records)?;
        for (mut row, position) in rows.into_iter().zip(positions) {
            row.position = position;
            row.at_unix_ms = at;
            l.rows
                .entry(row.pack.clone())
                .or_default()
                .push(row.clone());
            self.say(&row);
        }
        Ok(())
    }

    /// The row's record, for a frame its caller builds (an answer's).
    pub fn row_record(&self, row: &PackModeRow) -> Result<theseus_store::NewRecord> {
        let mut r = LedgerRow::new(
            LedgerKind::PackMode,
            None,
            None,
            crate::fact::ladder::data(row),
        );
        r.at_unix_ms = self.now();
        Ok(
            theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &r)?
                .scoped(&scope(&row.pack)),
        )
    }

    fn commit_row(
        &self,
        l: &mut Loaded,
        mut row: PackModeRow,
        records: &[theseus_store::NewRecord],
    ) -> Result<PackModeRow> {
        let at = self.now();
        let positions = self.store.append(records)?;
        row.position = positions.first().copied().unwrap_or_default();
        row.at_unix_ms = at;
        l.rows
            .entry(row.pack.clone())
            .or_default()
            .push(row.clone());
        self.say(&row);
        Ok(row)
    }
}

/// The answer from a mode under the ceiling, the canary's share, and the
/// session's arm.
pub fn given_of(mode: PackMode, share: Option<f64>, pack: &str, session: &str) -> Given {
    match mode {
        PackMode::Off => Given::OFF,
        PackMode::Shadow => Given {
            mode: PackMode::Shadow,
            arm: ArmOf::All,
        },
        PackMode::Live => Given {
            mode: PackMode::Live,
            arm: ArmOf::All,
        },
        PackMode::Canary => match learn::arm(session, pack, share.unwrap_or(0.0)) {
            learn::Arm::Canary => Given {
                mode: PackMode::Live,
                arm: ArmOf::Canary,
            },
            learn::Arm::Control => Given {
                mode: PackMode::Shadow,
                arm: ArmOf::Control,
            },
        },
    }
}

#[cfg(test)]
mod tests;
