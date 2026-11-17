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

pub mod totp;
