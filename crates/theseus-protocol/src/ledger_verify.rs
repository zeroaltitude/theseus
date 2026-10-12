//! `ledger.verify` (theseus-bmkxk): the ledger's hash chain, walked. Read only.
//! The daemon recomputes every link from the stored bytes, streaming, and
//! answers with the head and the first broken link, if there is one. It
//! returns no row's data: a row's position, time and kind, and what was
//! done to it, as far as the walk can tell.

/// `ledger.verify`'s params. No params walks the whole ledger.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerVerifyParams {
    /// Start at the first ledger row at or after this WAL position, trusting
    /// that row's own link and checking everything after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since: Option<u64>,
    /// A head recorded earlier, `SEQ:DIGEST` (the digest in hex, or its first
    /// 8 characters or more): the walk fails if the chain ends before it
    /// (a truncated tail) or differs from it (rewritten from some row on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub anchor: Option<String>,
    /// Answer with the head the writer last committed and walk nothing: a
    /// claim, not a proof.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub head_only: bool,
}

/// The chain's head: its last link, and where that row is in the WAL.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerHead {
    pub seq: u64,
    /// The digest, 64 hex characters.
    pub digest: String,
    pub position: u64,
}

impl LedgerHead {
    /// The head as health shows it: `3fa1c09e…` and its seq.
    pub fn short(&self) -> String {
        format!("{}…#{}", &self.digest[..self.digest.len().min(8)], self.seq)
    }
}

/// The first broken link.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerBreak {
    /// The row's WAL position.
    pub position: u64,
    pub at_unix_ms: u64,
    /// The row's kind, when its bytes still say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub seq: Option<u64>,
    /// `edited`, `removed`, `reordered`, `out_of_order`, `unchained`,
    /// `truncated`, `rewritten` or `history`.
    pub fault: String,
    /// Rows missing before this one, for `removed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub missing: Option<u64>,
    pub detail: String,
}

/// `ledger.verify`'s answer.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LedgerVerifyResult {
    /// The chain verified to its head, and none was broken.
    pub ok: bool,
    /// Rows whose links verified.
    pub rows: u64,
    /// When the chain started (unix ms), when the walk saw its first row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    /// The head. With `head_only` it is the writer's claim, otherwise the
    /// walk's recomputed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub head: Option<LedgerHead>,
    /// Rows from before the chain, which the chain's start row pins by count.
    #[serde(default)]
    pub unchained: u64,
    /// A pinned digest of that earlier history was checked and matched.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub history_checked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub broken: Option<LedgerBreak>,
    /// The walk's time, in milliseconds.
    #[serde(default)]
    pub took_ms: u64,
    /// The ledger's index is still being built after a start.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub building: bool,
}
