//! The proof's keeper: enrolment, status, reset, and the check of a code with
//! its rate limit. Every check, good or bad, is a ledger row
//! (`owner.proof.checked`), and a row says the outcome and the place, never
//! the code.
//!
//! **Fails closed.** A check that cannot save its own counters, or cannot
//! write its row, is a refusal: a code is never accepted on a footing that
//! would let it be used twice or tried without limit.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use zeroize::Zeroize;

use super::file::{self, Held};
use super::totp::{self, Mismatch, Secret, SECRET_BYTES};
use crate::fact::owner_proof::{ProofChanged, ProofChecked};
use crate::store::Store;

/// Wrong tries before a lock: five in [`TRIES_WITHIN_MS`].
pub const TRIES: usize = 5;
/// The window the tries are counted in: ten minutes.
pub const TRIES_WITHIN_MS: u64 = 10 * 60 * 1000;
/// How long a lock holds: ten minutes.
pub const LOCK_MS: u64 = 10 * 60 * 1000;
pub use crate::config::owner_proof::MAX_WINDOW_HOURS;
/// How long an enrolment waits for its first code before it is dropped.
pub const ENROL_WAIT_MS: u64 = 10 * 60 * 1000;

/// Unix milliseconds, from the daemon's clock; a test's clock is its own.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Why a code did not count, in the words a person is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No authenticator app is enrolled yet.
    NotEnrolled,
    /// Not the code the app shows now.
    Wrong,
    /// A code the app showed, already taken once.
    Used,
    /// Too many wrong codes: checking waits until `until_ms`.
    TooMany { until_ms: u64 },
    /// The proof's file cannot be read or kept, so nothing is taken from it.
    Unavailable,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEnrolled => f.write_str(
                "no authenticator app is enrolled yet: run `theseus owner proof enroll` in your own terminal",
            ),
            Self::Wrong => f.write_str(
                "that is not the code your authenticator app shows now: check it and try again",
            ),
            Self::Used => f.write_str(
                "that code was already used: wait for the app to show the next one",
            ),
            Self::TooMany { .. } => f.write_str(
                "too many wrong codes: codes are not checked for ten minutes. Nothing changed",
            ),
            Self::Unavailable => f.write_str(
                "Theseus cannot read or keep the authenticator's file, so no code can be checked \
                 (`theseus owner proof status` says more)",
            ),
        }
    }
}

impl std::error::Error for Refused {}

/// What `status` says: never the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub enrolled: bool,
    pub enrolled_at_ms: Option<u64>,
    /// Set while codes are not being checked.
    pub locked_until_ms: Option<u64>,
    /// The file is there and cannot be used.
    pub unavailable: bool,
    /// The windows still open: where, since when, until when.
    pub windows: Vec<file::Window>,
}

/// What an enrolment offers the owner, once: the address and the secret for
/// the app. It goes to the terminal that asked and nowhere else.
pub struct Offer {
    pub uri: String,
    pub base32: String,
}

impl std::fmt::Debug for Offer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Offer(..)")
    }
}

/// Why an enrolment step did not go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrolError {
    /// One is enrolled already: reset it first.
    Enrolled,
    /// Nothing is waiting for a code: begin again.
    NothingPending,
    /// The first code did not count; the enrolment keeps nothing.
    Code(Refused),
    /// The file could not be written.
    Unsaved,
}

impl std::fmt::Display for EnrolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enrolled => f.write_str(
                "an authenticator app is enrolled already: `theseus owner proof reset` takes it away first",
            ),
            Self::NothingPending => f.write_str(
                "no enrolment is waiting for a code (it waits ten minutes): start again with `theseus owner proof enroll`",
            ),
            Self::Code(why) => write!(f, "{why}. Nothing was enrolled"),
            Self::Unsaved => f.write_str(
                "Theseus could not write the authenticator's file, so nothing was enrolled",
            ),
        }
    }
}

impl std::error::Error for EnrolError {}

/// An enrolment waiting for the app's first code. In memory only: it is kept
/// when a code from the app proves the owner has the secret, and not before.
pub(super) struct Pending {
    pub(super) secret: Secret,
    started_ms: u64,
    tries: usize,
}

/// The keeper. One per daemon, over its state dir.
pub struct Proofs {
    pub(super) path: PathBuf,
    store: Store,
    clock: Clock,
    /// How long a good code covers the place it came from, in hours
    /// (`[owner_proof] window_hours`, 0 to [`MAX_WINDOW_HOURS`]).
    pub(super) window_hours: u32,
    /// One check at a time: the counters are read, changed and written
    /// under it, so two surfaces checking at once share one rate limit.
    pub(super) io: Mutex<()>,
    pub(super) pending: Mutex<Option<Pending>>,
}

impl std::fmt::Debug for Proofs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proofs")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// A lock on `m` that a panic elsewhere does not take away.
pub(super) fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Proofs {
    /// The keeper over `state`'s own file, writing its rows to `store`. A
    /// good code covers its place for `window_hours`, which is held to
    /// [`MAX_WINDOW_HOURS`].
    pub fn open(state: &Path, store: Store, clock: Clock, window_hours: u32) -> Self {
        Self {
            path: state.join(file::FILE),
            store,
            clock,
            window_hours: window_hours.min(MAX_WINDOW_HOURS),
            io: Mutex::new(()),
            pending: Mutex::new(None),
        }
    }

    pub(super) fn now(&self) -> u64 {
        (self.clock)()
    }

    /// `w` as the setting in force now allows it: a window's end is never
    /// later than `window_hours` after it opened, and with the setting at 0
    /// there is none. So lowering `[owner_proof] window_hours` shortens a
    /// window already open, and `None` once it is over.
    pub(super) fn capped(&self, w: &file::Window) -> Option<file::Window> {
        let allowed = u64::from(self.window_hours) * 3_600_000;
        let until_ms = w.until_ms.min(w.opened_at_ms.saturating_add(allowed));
        (allowed > 0 && until_ms > self.now()).then(|| file::Window {
            until_ms,
            ..w.clone()
        })
    }

    /// Whether one is enrolled, and since when; never the secret.
    pub fn status(&self) -> Status {
        let _io = held(&self.io);
        let now = self.now();
        match file::read(&self.path) {
            Ok(None) => Status {
                enrolled: false,
                enrolled_at_ms: None,
                locked_until_ms: None,
                unavailable: false,
                windows: Vec::new(),
            },
            Ok(Some(h)) => Status {
                enrolled: true,
                enrolled_at_ms: Some(h.enrolled_at_ms),
                locked_until_ms: (h.locked_until_ms > now).then_some(h.locked_until_ms),
                unavailable: false,
                windows: h.windows.iter().filter_map(|w| self.capped(w)).collect(),
            },
            Err(_) => Status {
                enrolled: false,
                enrolled_at_ms: None,
                locked_until_ms: None,
                unavailable: true,
                windows: Vec::new(),
            },
        }
    }

    /// Check `code`, made at `place`. A code is used once, wrong codes are
    /// counted toward a lock, and the check is a ledger row whatever came of
    /// it.
    pub fn check(&self, code: &str, place: &str) -> Result<(), Refused> {
        let _io = held(&self.io);
        let now = self.now();
        let (result, outcome, until) = self.checked(code, now);
        let recorded = self.record(&ProofChecked {
            outcome,
            place,
            locked_until_ms: until,
        });
        match (result, recorded) {
            (Ok(()), Err(e)) => {
                tracing::warn!(error = %e, "an owner proof check could not be recorded");
                Err(Refused::Unavailable)
            }
            (result, _) => result,
        }
    }

    /// The check itself, under the lock: the result, the outcome the row
    /// names, and when a lock ends if one is in play.
    fn checked(&self, code: &str, now: u64) -> (Result<(), Refused>, &'static str, Option<u64>) {
        let mut h = match file::read(&self.path) {
            Ok(Some(h)) => h,
            Ok(None) => return (Err(Refused::NotEnrolled), "not_enrolled", None),
            Err(_) => return (Err(Refused::Unavailable), "unreadable", None),
        };
        if now < h.locked_until_ms {
            let until = h.locked_until_ms;
            return (
                Err(Refused::TooMany { until_ms: until }),
                "locked",
                Some(until),
            );
        }
        let Ok(secret) = h.secret() else {
            return (Err(Refused::Unavailable), "unreadable", None);
        };
        h.wrong_ms
            .retain(|t| now.saturating_sub(*t) < TRIES_WITHIN_MS);
        let verdict = totp::verify(&secret, code, now / 1000, h.last_step);
        let (result, outcome) = match verdict {
            Ok(step) => {
                h.last_step = Some(step);
                h.wrong_ms.clear();
                (Ok(()), "ok")
            }
            Err(m) => {
                h.wrong_ms.push(now);
                if h.wrong_ms.len() >= TRIES {
                    h.locked_until_ms = now + LOCK_MS;
                    h.wrong_ms.clear();
                }
                match m {
                    Mismatch::Wrong => (Err(Refused::Wrong), "wrong"),
                    Mismatch::Used => (Err(Refused::Used), "used"),
                }
            }
        };
        let until = (h.locked_until_ms > now).then_some(h.locked_until_ms);
        if file::write(&self.path, &h).is_err() {
            return (Err(Refused::Unavailable), "unsaved", None);
        }
        // The try that caused the lock says so: the person is told the lock, not "wrong".
        match (result, until) {
            (Err(Refused::Wrong | Refused::Used), Some(until_ms)) => {
                (Err(Refused::TooMany { until_ms }), outcome, Some(until_ms))
            }
            (result, _) => (result, outcome, until),
        }
    }

    pub(super) fn record<F: crate::fact::Fact>(&self, f: &F) -> anyhow::Result<()> {
        self.store.append(&[crate::fact::row(f, None, None)?])?;
        Ok(())
    }

    /// Begin an enrolment: a fresh secret from the OS, kept in memory only
    /// until a code from the app proves it was taken down. Returns what the
    /// app is told, once. Refused when one is enrolled already.
    pub fn begin_enrol(&self, place: &str) -> Result<Offer, EnrolError> {
        let _io = held(&self.io);
        if !matches!(file::read(&self.path), Ok(None)) {
            return Err(EnrolError::Enrolled);
        }
        let mut bytes = [0u8; SECRET_BYTES];
        os_random(&mut bytes).map_err(|_| EnrolError::Unsaved)?;
        let secret = Secret::from_bytes(bytes);
        bytes.zeroize();
        let offer = Offer {
            uri: totp::otpauth_uri(&secret, "Theseus", "owner"),
            base32: secret.base32(),
        };
        *held(&self.pending) = Some(Pending {
            secret,
            started_ms: self.now(),
            tries: 0,
        });
        let _ = place;
        Ok(offer)
    }

    /// The first code from the app: when it counts, the secret is kept (and
    /// that step is used), and not otherwise. Five wrong codes drop the
    /// enrolment, so a guess has no more tries here than anywhere.
    pub fn confirm_enrol(&self, code: &str, place: &str) -> Result<(), EnrolError> {
        let _io = held(&self.io);
        let now = self.now();
        let mut slot = held(&self.pending);
        let Some(p) = slot.as_mut() else {
            return Err(EnrolError::NothingPending);
        };
        if now.saturating_sub(p.started_ms) > ENROL_WAIT_MS {
            *slot = None;
            return Err(EnrolError::NothingPending);
        }
        match totp::verify(&p.secret, code, now / 1000, None) {
            Ok(step) => {
                let h = Held::new(&p.secret, now, step);
                file::write(&self.path, &h).map_err(|_| EnrolError::Unsaved)?;
                *slot = None;
                drop(slot);
                self.checked_row("ok", place);
                self.changed("enrolled", place, None);
                Ok(())
            }
            Err(m) => {
                p.tries += 1;
                if p.tries >= TRIES {
                    *slot = None;
                }
                drop(slot);
                self.checked_row("wrong", place);
                Err(EnrolError::Code(match m {
                    Mismatch::Wrong => Refused::Wrong,
                    Mismatch::Used => Refused::Used,
                }))
            }
        }
    }

    /// Drop an enrolment that never confirmed a code.
    pub fn abandon_enrol(&self) {
        *held(&self.pending) = None;
    }

    /// Whether an enrolment waits for its first code.
    pub fn enrolling(&self) -> bool {
        held(&self.pending).is_some()
    }

    /// Take the enrolment away, on a valid code from the app it is.
    pub fn reset(&self, code: &str, place: &str) -> Result<(), Refused> {
        self.check(code, place)?;
        let _io = held(&self.io);
        file::remove(&self.path).map_err(|_| Refused::Unavailable)?;
        self.changed("reset", place, None);
        Ok(())
    }

    /// A check's row, for the enrolment's first code, which is a check of a
    /// secret not yet kept.
    pub(super) fn checked_row(&self, outcome: &str, place: &str) {
        let fact = ProofChecked {
            outcome,
            place,
            locked_until_ms: None,
        };
        if let Err(e) = self.record(&fact) {
            tracing::warn!(error = %e, "an owner proof check could not be recorded");
        }
    }

    /// A row of `owner.proof.changed`: what was done, or refused, and where.
    pub fn changed(&self, what: &str, place: &str, why: Option<&str>) {
        if let Err(e) = self.record(&ProofChanged { what, place, why }) {
            tracing::warn!(error = %e, "an owner proof change could not be recorded");
        }
    }
}

/// Bytes from the OS's random source.
fn os_random(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(test)]
mod tests;
