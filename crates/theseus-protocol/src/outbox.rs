//! A binding's outbox as health reports it (theseus-q4v), apart from `lib.rs`,
//! whose length the shape budget caps (`scripts/long-files.txt`).

use serde::{Deserialize, Serialize};

/// A binding's outbox (theseus-q4v): what waits to reach its channels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OutboxStatus {
    /// Posts written and not yet delivered.
    pub pending: u64,
    /// Posts delivered, in this store's life.
    pub sent: u64,
    /// Posts the channel refused for good (a deleted channel, lost access).
    pub failed: u64,
    /// When the oldest pending post was written (unix ms); 0 with none.
    #[serde(default)]
    pub oldest_pending_ms: u64,
    /// The last delivery error, and when (unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_error_ms: u64,
}

/// What became of a post, as its delivery receipt says (theseus-vug; M5's
/// delivery clause, spec §10's M5 list: "delivery recorded as `posted |
/// transport_failed | never_posted`, never `seen`").
///
/// The vocabulary is total over settled posts and it is written by whoever
/// settled the post — the surface that tried to deliver it, or the core when
/// it refuses one — never rebuilt afterwards from an outcome, a count or a
/// guess. `theseus_kernel`'s `outbox_settle` refuses a settle that names no
/// receipt, or one its outcome contradicts (`agrees`), so no post ends in no
/// state.
///
/// A receipt is not an outcome: the outcome says whether the delivery attempt
/// ended in an error, the receipt says whether anything reached the channel.
/// A post with nothing to send (the card its settle would have edited was
/// never posted) ends well and is `never_posted`.
///
/// There is deliberately no `seen`: a channel's acknowledgement says a
/// message exists, never that a person read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Receipt {
    /// The channel took it and gave its message ids back.
    Posted,
    /// It was sent and the channel refused it, for good: no retry can change
    /// that answer.
    TransportFailed,
    /// Nothing was ever sent: the core settled it instead (its place is no
    /// longer bound, so the post would have waited for ever).
    NeverPosted,
}

impl Receipt {
    /// Every receipt, so a reader can hold the vocabulary to this list.
    pub const ALL: &'static [Receipt] = &[
        Receipt::Posted,
        Receipt::TransportFailed,
        Receipt::NeverPosted,
    ];

    /// The one spelling of each, as the record keeps it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Receipt::Posted => "posted",
            Receipt::TransportFailed => "transport_failed",
            Receipt::NeverPosted => "never_posted",
        }
    }

    /// A stored or wired spelling read back; `None` for anything else, so a
    /// reader never invents a receipt.
    #[must_use]
    pub fn parse(s: &str) -> Option<Receipt> {
        Receipt::ALL.iter().copied().find(|r| r.as_str() == s)
    }

    /// The one wording every surface shows (`never posted`), so no surface
    /// writes its own.
    #[must_use]
    pub fn words(self) -> &'static str {
        match self {
            Receipt::Posted => "posted",
            Receipt::TransportFailed => "transport failed",
            Receipt::NeverPosted => "never posted",
        }
    }

    /// Whether the post reached its channel. When it did not, which of the
    /// other two it is is the honest part: it says whether anything was sent
    /// at all.
    #[must_use]
    pub fn delivered(self) -> bool {
        self == Receipt::Posted
    }

    /// Whether this receipt can stand with a settle that failed (or did not):
    /// `posted` only with one that did not fail, `transport_failed` only with
    /// one that did, and `never_posted` with either — a post that sent
    /// nothing was refused (failed) or had nothing to send (did not).
    #[must_use]
    pub fn agrees(self, failed: bool) -> bool {
        match self {
            Receipt::Posted => !failed,
            Receipt::TransportFailed => failed,
            Receipt::NeverPosted => true,
        }
    }
}
