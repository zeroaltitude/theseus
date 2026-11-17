//! The seam for the second proof (issue 22, line 1 step 6).
//!
//! A change to a sensitive setting is never made on the card's yes alone: it
//! also needs a second proof, a code from the owner's authenticator app,
//! built on its own branch (`owner_proof`). Until that module is on main,
//! this is the one function the change path asks, and it answers "not
//! installed", so such a change is refused before a card is made, and again
//! on a yes (`answer.rs`), whatever else happened. The later commit that wires
//! the proof in replaces the body of [`required_and_given`] with a call to
//! `owner_proof::required_and_given`, and nothing else here moves.

use theseus_kernel::Action;

/// What the second proof is, for a question and its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof {
    /// The proof is not built into this daemon: a sensitive change is refused.
    NotInstalled,
    /// Required, and a good proof came with the answer. Only a build with the
    /// proof in returns it (a test pretends, `pretend`).
    Given,
}

/// The words a refused sensitive change says.
pub const NOT_INSTALLED: &str = "this setting needs the second proof, which is not installed yet";

/// Whether the second proof is required for `question` and what was given
/// with its answer `via`. Until the proof is installed: not installed.
pub fn required_and_given(_question: &Action, _via: &str) -> Proof {
    #[cfg(test)]
    if pretending() {
        return Proof::Given;
    }
    Proof::NotInstalled
}

/// Whether a sensitive change may go to the owner at all: not while the
/// proof is not installed.
pub fn installed() -> bool {
    #[cfg(test)]
    if pretending() {
        return true;
    }
    false
}

#[cfg(test)]
thread_local! {
    static PRETEND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn pretending() -> bool {
    PRETEND.with(std::cell::Cell::get)
}

/// A test that follows a sensitive change past the proof: on this thread, the
/// proof is built in and was given, until the guard drops.
#[cfg(test)]
pub(crate) fn pretend() -> impl Drop {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            PRETEND.with(|p| p.set(false));
        }
    }
    PRETEND.with(|p| p.set(true));
    Guard
}
