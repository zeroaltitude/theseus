//! The shadow day budget (design §2.6): shadow judgments are the system's
//! experiment, not a session's work, so they spend from the judge's own
//! budget, `[judge] shadow_limit_usd_per_day`, by the local day.
//!
//! The budget is held in blocks: a judgment's reservation draws from the
//! block in memory, and a new block ($0.01) is written to the store, in a
//! frame of its own, before any judgment spends past the last one. What the
//! judgments settle rides in the sink's frames, beside their rows (the same
//! META record, `judge.budget`). So after a crash, today's reserved blocks
//! past what was settled are booked as spent at the next judgment, never
//! before serving: conservative, and never more than a block's rest.
//!
//! A judgment that would pass the limit is skipped and counted, never
//! queued, and the first such skip of a day writes one `judge.paused` row.
//! The next local day starts afresh.

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
    paused_said: bool,
    calls: u64,
    failed: u64,
    skipped: u64,
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
                day,
                limit,
                spent,
                needed,
            } => rec.announce(&JudgePaused {
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
}

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
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Day> {
        self.day.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn limit_micros(&self) -> Micros {
        self.limit
    }

    /// Reserve `need` for a judgment of `today`. Reads the store once per
    /// process, at its first use (never before serving).
    pub fn reserve(&self, store: &Store, today: &str, need: Micros) -> Reserve {
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
            if d.paused_said && !d.stored.day.is_empty() {
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
        if held + need > self.limit {
            d.skipped += 1;
            if !d.paused_said {
                d.paused_said = true;
                let paused = JudgePaused {
                    day: today,
                    limit_micros: self.limit,
                    spent_micros: d.stored.spent_micros,
                    needed_micros: need,
                };
                if let Ok(r) = crate::fact::row(&paused, None, None) {
                    out.push(r);
                    said.push(Said::Paused {
                        day: today.into(),
                        limit: self.limit,
                        spent: d.stored.spent_micros,
                        needed: need,
                    });
                }
            }
            if !out.is_empty() {
                out.push(meta(&d.stored));
            }
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

    /// A judgment came back: its reservation is released and what it cost
    /// (`spent`) is counted. `called`: it reached Jev; `failed`: it failed
    /// there.
    pub fn settle(&self, today: &str, reserved: Micros, spent: Micros, called: bool, failed: bool) {
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
            paused: d.paused_said,
        }
    }
}

fn meta(s: &Stored) -> NewRecord {
    NewRecord::json(kinds::META, Some(META_KEY), s).expect("the budget's record encodes")
}

/// The local day of `unix_ms`, as `2026-10-04`.
pub fn local_day(unix_ms: u64) -> String {
    let l = crate::wake::local(unix_ms);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}
