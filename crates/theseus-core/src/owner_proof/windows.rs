//! A good code opens a window, and the seam a question's answer is judged by.
//!
//! The owner (2026-11-16, chat message 1572108762301603924): "does the
//! enrollment cause the channel to be trusted forever? it seems to me that
//! each successful 2fa should last for 24 hours". So:
//!
//! - **Enrolment trusts nothing.** It pairs the app and opens no window.
//! - **A good code opens a window over the PLACE it came from** (`cli`,
//!   `web`, `discord:dm`, `discord:<channel>`) for `[owner_proof]
//!   window_hours` (24 by default; 0, a code every time; at most 24). For
//!   that long a question that needs the proof, answered from that same
//!   place, goes through on the card's yes alone. Another place needs a code
//!   of its own.
//! - **The window is the daemon's record**, in the file only the daemon
//!   writes. No tool sets it. A use does not extend it (it ends 24 hours from
//!   the code, not from the last use); only another good code opens a new
//!   one. The single-use rule and the rate limit on codes are untouched.
//! - **What a window never covers:** a reset or a re-enrolment (they take a
//!   fresh code, `Proofs::reset`), and a question marked `needs_fresh_proof`.
//!   A good code given for such a question opens no window either.
//! - **The card always says which it is** (`Proofs::standing`): "second
//!   proof: needed, enter the code from your authenticator app", or "second
//!   proof: given here 3 hours ago, good for 21 hours more".
//! - **Ending one early:** `Proofs::lock` ends every open window at once.
//!   A restart keeps the windows (they are in the file); a config rollback's
//!   restart ends them (`Proofs::end_windows`, which the rollback calls).

use theseus_protocol::owner_proof::ProofStanding;
use theseus_protocol::work::Question;

use super::file::{self, Window};
use super::proofs::{held, Proofs, Refused};
use crate::fact::owner_proof::{WindowOpened, WindowUsed, WindowsEnded};

const HOUR_MS: u64 = 3_600_000;

/// What an answer carries, as the seam reads it: the code typed into the
/// surface's own form (if any), and the place the answer came from, as the
/// connection says it (`Answerer::via`), never as the client claims.
pub struct Answer<'a> {
    pub code: Option<&'a str>,
    pub place: &'a str,
}

/// What the proof said of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    /// The question does not ask for one.
    NotRequired,
    /// A good code came with this answer.
    Given,
    /// No code, and none needed: this place gave one earlier.
    GivenEarlier { at_ms: u64, until_ms: u64 },
    /// It needs one and none came: the answer changes nothing.
    Missing,
    /// A code came and did not count, or none can be enrolled or checked.
    Refused(Refused),
}

impl Proofs {
    /// **The seam.** Whether `answer` to `question` carries the proof the
    /// question asks for. A question that does not ask for one is
    /// `NotRequired`; one that does is approved only on a good code
    /// (`Given`) or on an open window of the answer's own place
    /// (`GivenEarlier`), and every other way is `Missing` or `Refused`.
    pub fn required_and_given(&self, question: &Question, answer: &Answer<'_>) -> Proof {
        self.judge(
            &question.id,
            question.needs_proof,
            question.needs_fresh_proof,
            answer,
        )
    }

    /// The seam's judgment over what it reads of a question: its id, and
    /// whether the daemon's record marks it as needing the proof (and a fresh
    /// one). The answer path reads the marks from the store and calls this.
    pub fn judge(&self, id: &str, needs: bool, fresh: bool, answer: &Answer<'_>) -> Proof {
        if !needs && !fresh {
            return Proof::NotRequired;
        }
        let code = answer.code.map(str::trim).filter(|c| !c.is_empty());
        if let Some(code) = code {
            return match self.check(code, answer.place) {
                Ok(()) => {
                    if !fresh {
                        self.open_window(answer.place);
                    }
                    Proof::Given
                }
                Err(why) => Proof::Refused(why),
            };
        }
        if !fresh {
            if let Some(w) = self.window_of(answer.place) {
                self.record_use(id, answer.place, &w);
                return Proof::GivenEarlier {
                    at_ms: w.opened_at_ms,
                    until_ms: w.until_ms,
                };
            }
        }
        let status = self.status();
        let why = match (status.enrolled, status.unavailable) {
            (_, true) => Some(Refused::Unavailable),
            (false, _) => Some(Refused::NotEnrolled),
            _ => None,
        };
        self.checked_row(
            match why {
                Some(Refused::Unavailable) => "unreadable",
                Some(_) => "not_enrolled",
                None => "missing",
            },
            answer.place,
        );
        why.map_or(Proof::Missing, Proof::Refused)
    }

    /// The open window over `place`, if any.
    fn window_of(&self, place: &str) -> Option<Window> {
        let _io = held(&self.io);
        let h = file::read(&self.path).ok().flatten()?;
        h.windows
            .iter()
            .filter(|w| w.place == place)
            .find_map(|w| self.capped(w))
    }

    /// A good code from `place`: its window, from now. Another window over
    /// the same place is replaced, not stacked.
    fn open_window(&self, place: &str) {
        if self.window_hours == 0 {
            return;
        }
        let _io = held(&self.io);
        let now = self.now();
        let Ok(Some(mut h)) = file::read(&self.path) else {
            return;
        };
        let until_ms = now + u64::from(self.window_hours) * HOUR_MS;
        h.windows.retain(|w| w.place != place && w.until_ms > now);
        h.windows.push(Window {
            place: place.to_string(),
            opened_at_ms: now,
            until_ms,
        });
        // A window that could not be saved is no window: the next answer asks for a code.
        if file::write(&self.path, &h).is_ok() {
            if let Err(e) = self.record(&WindowOpened { place, until_ms }) {
                tracing::warn!(error = %e, "an owner proof window could not be recorded");
            }
        }
    }

    fn record_use(&self, question: &str, place: &str, w: &Window) {
        let fact = WindowUsed {
            question,
            place,
            until_ms: w.until_ms,
        };
        if let Err(e) = self.record(&fact) {
            tracing::warn!(error = %e, "an owner proof window's use could not be recorded");
        }
    }

    /// What an answer from `place` would be asked for, in the words a card
    /// shows. `fresh` is the question's `needs_fresh_proof`.
    pub fn standing(&self, place: &str, fresh: bool) -> ProofStanding {
        let status = self.status();
        if !status.enrolled {
            return ProofStanding {
                code_needed: true,
                not_enrolled: !status.unavailable,
                given_at_ms: None,
                until_ms: None,
                line: match status.unavailable {
                    true => {
                        "second proof: needed, and Theseus cannot read the authenticator's file"
                            .into()
                    }
                    false => "second proof: needed, and no authenticator app is enrolled yet: run \
                              `theseus owner proof enroll` in your own terminal"
                        .into(),
                },
            };
        }
        let now = self.now();
        let open = status
            .windows
            .iter()
            .find(|w| w.place == place && w.until_ms > now);
        match (open, fresh) {
            (Some(w), false) => ProofStanding {
                code_needed: false,
                not_enrolled: false,
                given_at_ms: Some(w.opened_at_ms),
                until_ms: Some(w.until_ms),
                line: format!(
                    "second proof: given here {} ago, good for {} more",
                    span(now.saturating_sub(w.opened_at_ms)),
                    span(w.until_ms.saturating_sub(now)),
                ),
            },
            _ => ProofStanding {
                code_needed: true,
                not_enrolled: false,
                given_at_ms: None,
                until_ms: None,
                line: match fresh {
                    true => "second proof: needed every time for this one, enter the code from \
                             your authenticator app"
                        .into(),
                    false => {
                        "second proof: needed, enter the code from your authenticator app".into()
                    }
                },
            },
        }
    }

    /// The owner's `lock`: every open window ends now. Returns how many.
    pub fn lock(&self, by: &str, place: &str) -> Result<usize, Refused> {
        self.end(by, place)
    }

    /// The daemon ends every window itself: a config rollback's restart calls
    /// this, since the settings a window was given under are gone.
    pub fn end_windows(&self, why: &str) -> Result<usize, Refused> {
        self.end("daemon", why)
    }

    fn end(&self, by: &str, place: &str) -> Result<usize, Refused> {
        let _io = held(&self.io);
        let mut h = match file::read(&self.path) {
            Ok(Some(h)) => h,
            Ok(None) => return Ok(0),
            Err(_) => return Err(Refused::Unavailable),
        };
        let closed = h.windows.len();
        h.windows.clear();
        file::write(&self.path, &h).map_err(|_| Refused::Unavailable)?;
        if let Err(e) = self.record(&WindowsEnded { by, place, closed }) {
            tracing::warn!(error = %e, "ending the proof's windows could not be recorded");
        }
        Ok(closed)
    }
}

/// A span in words: `3 hours`, `21 hours`, `5 minutes`, `under a minute`.
pub fn span(ms: u64) -> String {
    let (hours, minutes) = (ms / HOUR_MS, ms / 60_000);
    let many = |n: u64, unit: &str| match n {
        1 => format!("1 {unit}"),
        n => format!("{n} {unit}s"),
    };
    match (hours, minutes) {
        (0, 0) => "under a minute".into(),
        (0, m) => many(m, "minute"),
        (h, _) => many(h, "hour"),
    }
}

#[cfg(test)]
mod tests;
