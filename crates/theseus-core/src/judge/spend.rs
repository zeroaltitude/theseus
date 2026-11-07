//! The shadow day budget (design §2.6): shadow judgments are the system's
//! experiment, not a session's work, so they spend from the judge's own
//! budget, `[judge] shadow_limit_usd_per_day`, by the local day.
//!
//! The budget is held in blocks: a judgment's reservation draws from the
//! block in memory, and a new block ($0.01) is written to the store before
//! any judgment spends past the last one. What the judgments settle rides
//! in the sink's frames, beside their rows (the same META record,
//! `judge.budget`), and so does the next block once the current one is
//! half spent ([`ShadowBudget::ahead`], theseus-xkbs), so a judgment seldom
//! needs a frame of its own; when one does, a judgment no turn waits on
//! writes it between turns (`judge::reserve_between`). So after a crash,
//! today's reserved blocks past what was settled are booked as spent at the
//! next judgment, never before serving: conservative, and never more than
//! two blocks' rest (a block's, before blocks were written ahead).
//!
//! A judgment that would pass its line is skipped and counted, never
//! queued, and the first such skip of a day writes one `judge.paused` row.
//! The next local day starts afresh.
//!
//! There are two lines (theseus-0j2.16). Live judgments (route, rerank,
//! the gate's security pack: what the product acts on) may spend the whole
//! limit; shadow judgments stop at [`SHADOW_STOP_PERCENT`] of it, so the
//! experiments can never starve the judging the product depends on. Each
//! line says its own pause, once a day.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use theseus_judge::price::Micros;
use theseus_store::{kinds, NewRecord};

use crate::fact::judge::{JudgeBlockBooked, JudgePaused, JudgeResumed};
use crate::fact::Rec;
use crate::store::Store;

/// The META record that holds today's blocks and settled spend.
pub const META_KEY: &str = "judge.budget";

/// One block: $0.01.
pub const BLOCK_MICROS: Micros = 10_000;

/// The part of the day's limit shadow may spend, in percent: past it shadow
/// stops, and the rest is the live judgments' alone.
pub const SHADOW_STOP_PERCENT: u64 = 70;

/// Whose spend a reservation is: a judgment that acts (`Live`, a canary's
/// arm included) or one that only watches (`Shadow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Live,
    Shadow,
}

impl Class {
    /// Its word in a `judge.paused` row.
    fn what(self) -> &'static str {
        match self {
            Class::Live => "live",
            Class::Shadow => "shadow",
        }
    }

    /// The class of a request of `asks`: live when any of them acts (one
    /// judgment the product waits on is worth the whole request's spend).
    pub fn of_asks<'a>(asks: impl IntoIterator<Item = &'a theseus_judge::Ask>) -> Self {
        match asks.into_iter().any(|a| Self::of(a.mode) == Class::Live) {
            true => Class::Live,
            false => Class::Shadow,
        }
    }

    /// The class of a judgment asked in `mode`.
    pub fn of(mode: theseus_judge::Mode) -> Self {
        match mode {
            theseus_judge::Mode::Shadow => Class::Shadow,
            theseus_judge::Mode::Canary | theseus_judge::Mode::Live => Class::Live,
        }
    }
}

/// What the store keeps of the day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub day: String,
    /// Every block reserved today.
    pub reserved_micros: Micros,
    /// What today's judgments settled (and any rest booked after a crash).
    pub spent_micros: Micros,
}

#[derive(Debug, Default)]
struct Day {
    /// Read from the store at the first judgment of this process.
    loaded: bool,
    stored: Stored,
    /// Reserved by judgments in flight.
    in_flight: Micros,
    /// The day's limit said (live is refused too).
    paused_said: bool,
    /// The shadow line said (shadow is refused; live goes on).
    shadow_said: bool,
    calls: u64,
    failed: u64,
    skipped: u64,
}

impl Day {
    /// Whether `class`'s pause is said today.
    fn said(&self, class: Class) -> bool {
        match class {
            Class::Live => self.paused_said,
            Class::Shadow => self.shadow_said || self.paused_said,
        }
    }

    fn set_said(&mut self, class: Class) {
        match class {
            Class::Live => self.paused_said = true,
            Class::Shadow => self.shadow_said = true,
        }
    }
}

/// A reservation's answer, with the records to write in one frame before
/// anything is sent: a new block, a crash's booked rest, the day's first
/// `judge.paused` row.
#[derive(Debug)]
pub enum Reserve {
    Granted(Vec<NewRecord>, Vec<Said>),
    /// The day's limit is reached: the judgment is skipped, never queued.
    Paused(Vec<NewRecord>, Vec<Said>),
}

/// A fact a reservation's records hold, said once their frame is written
/// (23b: their sentences).
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    Paused {
        class: Class,
        day: String,
        limit: Micros,
        spent: Micros,
        needed: Micros,
    },
    Booked {
        day: String,
        reserved: Micros,
        settled: Micros,
    },
    Resumed {
        day: String,
        paused_day: String,
    },
}

impl Said {
    /// Its sentences: its row rode in the reservation's frame.
    pub fn announce(&self, rec: &Rec<'_>) {
        match self {
            Said::Paused {
                class,
                day,
                limit,
                spent,
                needed,
            } => rec.announce(&JudgePaused {
                what: class.what(),
                day,
                limit_micros: *limit,
                spent_micros: *spent,
                needed_micros: *needed,
            }),
            Said::Booked {
                day,
                reserved,
                settled,
            } => rec.announce(&JudgeBlockBooked {
                day,
                reserved_micros: *reserved,
                settled_micros: *settled,
            }),
            Said::Resumed { day, paused_day } => rec.announce(&JudgeResumed { day, paused_day }),
        }
    }
}

pub struct ShadowBudget {
    limit: Micros,
    day: Mutex<Day>,
    /// The daemon's day ceiling (theseus-kp20), above this budget: a
    /// judgment holds on it as it reserves here, and is skipped when it
    /// refuses; what the judgment settles is spent there too. The hook says
    /// a refusal (outside this budget's lock).
    ceiling: std::sync::OnceLock<(std::sync::Arc<theseus_kernel::DayCeiling>, CeilingHook)>,
}

/// What the core does with a judgment the day ceiling refused.
pub type CeilingHook = Box<dyn Fn(&theseus_kernel::Reached) + Send + Sync>;

/// The judge's key on the day ceiling: it holds and settles by amount.
const CEILING_KEY: &str = "judge";

/// Today's counts, for health.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Today {
    pub day: String,
    pub spent_micros: Micros,
    pub calls: u64,
    pub failed: u64,
    pub skipped: u64,
    pub paused: bool,
}

impl ShadowBudget {
    pub fn new(limit_usd: f64) -> Self {
        Self {
            limit: theseus_judge::price::usd_to_micros(limit_usd),
            day: Mutex::default(),
            ceiling: std::sync::OnceLock::new(),
        }
    }

    /// Hold every judgment on the daemon's day ceiling, too (once).
    pub fn set_ceiling(
        &self,
        ceiling: std::sync::Arc<theseus_kernel::DayCeiling>,
        hook: CeilingHook,
    ) {
        let _ = self.ceiling.set((ceiling, hook));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Day> {
        self.day.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn limit_micros(&self) -> Micros {
        self.limit
    }

    /// The most `class` may hold today: the limit, or for shadow its line.
    pub fn cap(&self, class: Class) -> Micros {
        match class {
            Class::Live => self.limit,
            Class::Shadow => self.limit * SHADOW_STOP_PERCENT / 100,
        }
    }

    /// Whether a reservation of `need` now would ask for a frame: the
    /// first of this process (it may book a crash's rest), a new day's, a
    /// new block's, or the day's first pause (theseus-xkbs). A read of
    /// memory, never the store's.
    pub fn needs_frame(&self, today: &str, need: Micros, class: Class) -> bool {
        let d = self.lock();
        if !d.loaded || d.stored.day != today {
            return true;
        }
        let held = d.stored.spent_micros + d.in_flight;
        if held + need > self.cap(class) {
            return !d.said(class);
        }
        held + need > d.stored.reserved_micros
    }

    /// The next block, raised in memory for the sink's frame to write with
    /// the record (theseus-xkbs): once less than half of the current block
    /// is left, and while the day's limit allows. The caller writes the
    /// record before any reservation can read the raise (the service's
    /// `blocks` lock). A crash then books up to two blocks' rest.
    pub fn ahead(&self, today: &str) {
        let mut d = self.lock();
        if !d.loaded || d.stored.day != today || d.paused_said {
            return;
        }
        let held = d.stored.spent_micros + d.in_flight;
        let left = d.stored.reserved_micros.saturating_sub(held);
        if left * 2 < BLOCK_MICROS && d.stored.reserved_micros < self.limit {
            d.stored.reserved_micros = (d.stored.reserved_micros + BLOCK_MICROS).min(self.limit);
        }
    }

    /// Reserve `need` for a judgment of `today`. Reads the store once per
    /// process, at its first use (never before serving).
    pub fn reserve(&self, store: &Store, today: &str, need: Micros, class: Class) -> Reserve {
        let mut d = self.lock();
        let mut out = Vec::new();
        let mut said = Vec::new();
        if !d.loaded {
            d.loaded = true;
            if let Ok(Some(s)) = store.get_meta::<Stored>(META_KEY) {
                if s.day == today && s.reserved_micros > s.spent_micros {
                    if let Ok(r) = crate::fact::row(
                        &JudgeBlockBooked {
                            day: today,
                            reserved_micros: s.reserved_micros,
                            settled_micros: s.spent_micros,
                        },
                        None,
                        None,
                    ) {
                        out.push(r);
                        said.push(Said::Booked {
                            day: today.into(),
                            reserved: s.reserved_micros,
                            settled: s.spent_micros,
                        });
                    }
                    d.stored = Stored {
                        spent_micros: s.reserved_micros,
                        ..s
                    };
                } else if s.day == today {
                    d.stored = s;
                }
            }
        }
        if d.stored.day != today {
            // The first judgment of a new day after a paused one (23b).
            if (d.paused_said || d.shadow_said) && !d.stored.day.is_empty() {
                let resumed = JudgeResumed {
                    day: today,
                    paused_day: &d.stored.day,
                };
                if let Ok(r) = crate::fact::row(&resumed, None, None) {
                    out.push(r);
                    said.push(Said::Resumed {
                        day: today.into(),
                        paused_day: d.stored.day.clone(),
                    });
                }
            }
            *d = Day {
                loaded: true,
                in_flight: d.in_flight,
                stored: Stored {
                    day: today.to_string(),
                    ..Stored::default()
                },
                ..Day::default()
            };
        }
        let held = d.stored.spent_micros + d.in_flight;
        let cap = self.cap(class);
        if held + need > cap {
            d.skipped += 1;
            if !d.said(class) {
                d.set_said(class);
                if let Some((row, say)) = pause(&d.stored, class, cap, need) {
                    out.push(row);
                    said.push(say);
                }
            }
            if !out.is_empty() {
                out.push(meta(&d.stored));
            }
            return Reserve::Paused(out, said);
        }
        if let Some((r, hook)) = self.ceiling_refuses(need) {
            d.skipped += 1;
            out.extend((!out.is_empty()).then(|| meta(&d.stored)));
            drop(d);
            hook(&r);
            return Reserve::Paused(out, said);
        }
        if held + need > d.stored.reserved_micros {
            let blocks = (held + need - d.stored.reserved_micros).div_ceil(BLOCK_MICROS);
            d.stored.reserved_micros = (d.stored.reserved_micros + blocks * BLOCK_MICROS)
                .min(self.limit)
                .max(held + need);
            out.push(meta(&d.stored));
        } else if !out.is_empty() {
            out.push(meta(&d.stored));
        }
        d.in_flight += need;
        Reserve::Granted(out, said)
    }

    /// Whether the day ceiling refuses a judgment of `need` (it holds it
    /// otherwise), with the hook that says so.
    fn ceiling_refuses(&self, need: Micros) -> Option<(theseus_kernel::Reached, &CeilingHook)> {
        let (c, hook) = self.ceiling.get()?;
        c.hold(c.now(), CEILING_KEY, need).err().map(|r| (r, hook))
    }

    /// A judgment came back: its reservation is released and what it cost
    /// (`spent`) is counted. `called`: it reached Jev; `failed`: it failed
    /// there.
    pub fn settle(&self, today: &str, reserved: Micros, spent: Micros, called: bool, failed: bool) {
        if let Some((c, _)) = self.ceiling.get() {
            c.settle_part(c.now(), CEILING_KEY, reserved, spent);
        }
        let mut d = self.lock();
        d.in_flight = d.in_flight.saturating_sub(reserved);
        if d.stored.day != today {
            return;
        }
        d.stored.spent_micros += spent;
        if called {
            d.calls += 1;
        } else {
            d.skipped += 1;
        }
        if failed {
            d.failed += 1;
        }
    }

    /// Whether today's limit has paused the judge's spend: a read of what
    /// this process knows, never the store's.
    pub fn paused(&self, today: &str) -> bool {
        let d = self.lock();
        d.loaded && d.paused_said && d.stored.day == today
    }

    /// Whether shadow is stopped today, at its line or at the limit (live
    /// goes on past its line, so `paused` is the limit's alone).
    pub fn shadow_paused(&self, today: &str) -> bool {
        let d = self.lock();
        d.loaded && (d.shadow_said || d.paused_said) && d.stored.day == today
    }

    /// A judgment skipped before it reserved (shed, the breaker, no key,
    /// sampling aside).
    pub fn skipped(&self, today: &str) {
        let mut d = self.lock();
        if d.stored.day == today {
            d.skipped += 1;
        }
    }

    /// The META record as it stands, for the sink's frame.
    pub fn record(&self) -> Option<NewRecord> {
        let d = self.lock();
        (d.loaded && !d.stored.day.is_empty()).then(|| meta(&d.stored))
    }

    /// Today's counts. Before this process's first judgment, the store's
    /// record of today (a read, never a write).
    pub fn today(&self, store: &Store, today: &str) -> Today {
        let d = self.lock();
        if !d.loaded {
            let s = store
                .get_meta::<Stored>(META_KEY)
                .ok()
                .flatten()
                .filter(|s| s.day == today)
                .unwrap_or_default();
            return Today {
                day: today.into(),
                spent_micros: s.spent_micros,
                ..Today::default()
            };
        }
        if d.stored.day != today {
            return Today {
                day: today.into(),
                ..Today::default()
            };
        }
        Today {
            day: today.into(),
            spent_micros: d.stored.spent_micros,
            calls: d.calls,
            failed: d.failed,
            skipped: d.skipped,
            paused: d.paused_said || d.shadow_said,
        }
    }
}

/// `class`'s pause at `cap`, its row and its sentence.
fn pause(s: &Stored, class: Class, cap: Micros, need: Micros) -> Option<(NewRecord, Said)> {
    let paused = JudgePaused {
        what: class.what(),
        day: &s.day,
        limit_micros: cap,
        spent_micros: s.spent_micros,
        needed_micros: need,
    };
    let row = crate::fact::row(&paused, None, None).ok()?;
    let say = Said::Paused {
        class,
        day: s.day.clone(),
        limit: cap,
        spent: s.spent_micros,
        needed: need,
    };
    Some((row, say))
}

fn meta(s: &Stored) -> NewRecord {
    NewRecord::json(kinds::META, Some(META_KEY), s).expect("the budget's record encodes")
}

/// The local day of `unix_ms`, as `2026-10-04`.
pub fn local_day(unix_ms: u64) -> String {
    let l = crate::wake::local(unix_ms);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}
