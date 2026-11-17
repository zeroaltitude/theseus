//! The owner's second proof (issue 22): a code from the owner's own
//! authenticator app, for the changes that matter most. Read
//! `docs/design/owner-proof.md` for the whole of it.
//!
//! **The point of it: neither the secret nor a code ever passes through a
//! turn.** The model never sees the secret, never sees a code, and cannot
//! supply one. That is why it is an app and not a code sent by mail:
//! Theseus can read the owner's mail.
//!
//! - [`totp`]: the code (RFC 6238), pure.
//! - [`file`]: the one file the secret and the counters live in, and the
//!   paths the floor holds for it.
//! - [`marks`]: a question marked as needing the proof, by the daemon's own code.
//! - [`proofs`]: enrolment, status, reset, and the check with its rate limit.
//! - [`windows`]: a good code covers the place it came from for a while, and
//!   the seam a question's answer is judged by (`Proofs::required_and_given`).

pub mod file;
pub mod marks;
pub mod proofs;
pub mod totp;
pub mod windows;

pub use file::{floor_paths, FILE};
pub use marks::{mark, marked};
pub use proofs::{EnrolError, Offer, Proofs, Refused, Status, MAX_WINDOW_HOURS};
pub use windows::{Answer, Proof};

#[cfg(test)]
pub(crate) mod stand_in;

#[cfg(test)]
mod tests_floor;

#[cfg(test)]
mod tests_answer;
