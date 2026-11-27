//! Why Google's consent could not begin, as the owner can act on it (issue 62). The kind is read
//! from the daemon's own sentence (`auth.rs`, and `rpc/google.rs` for an account not bound), by
//! its fixed words; a Discord press is told the kind alone ([`BeginKind::marker`]), the CLI the
//! sentence.

use crate::approval::Surface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginKind {
    /// The settings bind no Google account.
    NoAccount,
    /// The client secret did not resolve, is not a `[secrets]` entry, or holds no client.
    Client,
    /// The client secret is still resolving.
    Resolving,
    /// Anything else: the loopback listener.
    Other,
}

/// How the daemon's sentence for each kind begins.
const NO_ACCOUNT: &str = "no Google account is bound";
const CLIENT: &str = "the client secret `";
const RESOLVING: &str = "is still resolving";
const NOT_AN_ENTRY: &str = "` is not a [secrets] entry";

impl BeginKind {
    pub const ALL: [BeginKind; 4] = [Self::NoAccount, Self::Client, Self::Resolving, Self::Other];

    /// The whole message a Discord press's failed begin carries: fixed words.
    pub fn marker(self) -> &'static str {
        match self {
            Self::NoAccount => "the consent could not begin: no account",
            Self::Client => "the consent could not begin: client secret",
            Self::Resolving => "the consent could not begin: still resolving",
            Self::Other => "the consent could not begin",
        }
    }

    /// The kind a message is exactly the marker of.
    pub fn of_marker(message: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.marker() == message)
    }

    /// The kind of the daemon's sentence. A secret's name sits between backquotes: what follows
    /// the first closing one tells the kind, so a reason after it tells nothing.
    pub fn of_words(words: &str) -> Self {
        if words.starts_with(NO_ACCOUNT) {
            return Self::NoAccount;
        }
        if let Some(rest) = words.strip_prefix(CLIENT) {
            return match rest.split_once("` ") {
                Some((_, tail)) if tail.starts_with(RESOLVING) => Self::Resolving,
                _ => Self::Client,
            };
        }
        if words.starts_with('`') && words.ends_with(NOT_AN_ENTRY) {
            return Self::Client;
        }
        Self::Other
    }

    /// What the surface that asked is told: the marker on Discord, the sentence elsewhere.
    pub fn said_to(self, surface: Surface, words: String) -> String {
        match surface {
            Surface::Discord => self.marker().to_string(),
            _ => words,
        }
    }
}
