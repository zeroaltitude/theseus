//! How often the daemon reads the vault (issue 44): a count of its own
//! reads ([`VaultReads`]): each `op` process the daemon starts, and the
//! entries it asks for (one per `op read`, each reference of an `op inject`),
//! per local day. Counts only: never a name or a value. Health's
//! `secrets.vault_today` reads it.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::future::BoxFuture;

/// The entries the daemon reads from the vault in a local day past which a
/// failing entry is not fetched again (issue 44: the vault's whole account
/// allows 50,000 requests a day, shared by every reader). A constant, the
/// owner's choice of fewer settings; it is a `default_*_per_day` function so
/// that the keel's cap rule reads its number and a raise is a finding.
pub fn default_vault_reads_per_day() -> u32 {
    2000
}

/// The time the pace reads: the daemon's clock, or a test's.
pub trait Clock: Send + Sync {
    /// Unix milliseconds now.
    fn now_ms(&self) -> u64;
    /// Wait `d`.
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()>;
}

/// The system's clock, and tokio's timer.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        theseus_protocol::now_unix_ms()
    }
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(d))
    }
}

#[derive(Default)]
struct Day {
    day: String,
    processes: u32,
    references: u32,
}

/// The daemon's own vault reads in the local day.
pub struct VaultReads {
    clock: Arc<dyn Clock>,
    day: Mutex<Day>,
}

impl VaultReads {
    pub fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            clock,
            day: Mutex::default(),
        })
    }

    /// The process's count, which every reader `OpReader::from_env` makes
    /// shares: the daemon's secrets, its config note, its looks and writes.
    pub fn process() -> Arc<Self> {
        static PROCESS: OnceLock<Arc<VaultReads>> = OnceLock::new();
        PROCESS
            .get_or_init(|| Self::new(Arc::new(SystemClock)))
            .clone()
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    fn with_today<T>(&self, f: impl FnOnce(&mut Day) -> T) -> T {
        let today = crate::judge::spend::local_day(self.clock.now_ms());
        let mut d = self.day.lock().unwrap_or_else(|p| p.into_inner());
        if d.day != today {
            *d = Day {
                day: today,
                ..Day::default()
            };
        }
        f(&mut d)
    }

    /// One `op` process started, asking for `references` entries.
    pub fn note(&self, references: usize) {
        let n = u32::try_from(references).unwrap_or(u32::MAX);
        self.with_today(|d| {
            d.processes = d.processes.saturating_add(1);
            d.references = d.references.saturating_add(n);
        });
    }

    /// Today's processes and entries.
    pub fn today(&self) -> (u32, u32) {
        self.with_today(|d| (d.processes, d.references))
    }
}
