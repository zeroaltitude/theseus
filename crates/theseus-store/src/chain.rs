//! The ledger's hash chain (theseus-bmkxk; the killer-features plan's R25).
//!
//! Every ledger record the writer appends carries a link: `seq` (its place
//! in the chain, from 0) and a SHA-256 digest over the previous link's digest,
//! the seq, and the record's **canonical bytes**. The chain is computed in one
//! place, the store's single writer (`Inner::commit`), in WAL order, so
//! concurrent appenders cannot interleave it differently. A reader never
//! trusts a stored link: `Walk` recomputes every one.
//!
//! **The canonical bytes** are the payload exactly as the appender handed it
//! to the store (`{"at_unix_ms":…,"kind":…,…}`), never re-serialized. The
//! writer splices the link in by bytes, `canonical[..len-1] + ,"chain":"<seq>.<hex>"}`,
//! and a reader takes it back out by bytes, so no serde version, field order or
//! float format can change what was hashed. The link is the row's `chain`
//! field (`LedgerRow::chain`, which decodes it and is never written by a
//! row's own builder).
//!
//! What it catches: an edit of a row, a removed row, a reordered pair, and (with a head
//! recorded earlier) a truncated tail or a chain rewritten from some row onward. What it
//! cannot: a forger who rewrites every later row with this code and no earlier head to
//! compare against.

use sha2::{Digest, Sha256};

/// The chain's first row's predecessor.
pub const GENESIS: [u8; 32] = [0; 32];

/// The ledger kind of the row that starts a chain.
pub const STARTED_KIND: &str = theseus_protocol::LedgerKind::LedgerChainStarted.as_str();

/// The ledger kind of the row that pins the history from before the chain.
pub const PINNED_KIND: &str = theseus_protocol::LedgerKind::LedgerHistoryPinned.as_str();

const KEY: &[u8] = b"\"chain\":\"";

/// One link: its place in the chain and its digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub seq: u64,
    pub digest: [u8; 32],
}

/// The chain's head: its last link, and where that row is in the WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub link: Link,
    pub position: u64,
}

pub fn hex(d: &[u8; 32]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// The digest of a row: over the previous digest, the seq, and the canonical bytes.
pub fn digest(prev: &[u8; 32], seq: u64, canonical: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(seq.to_le_bytes());
    h.update(canonical);
    h.finalize().into()
}

/// Whether a payload is a JSON object the chain can be spliced into.
pub fn chainable(payload: &[u8]) -> bool {
    payload.len() >= 2 && payload[0] == b'{' && payload[payload.len() - 1] == b'}'
}

/// `canonical` with its link spliced in. `None` when `canonical` is not an object.
pub fn seal(canonical: &[u8], prev: &[u8; 32], seq: u64) -> Option<(Vec<u8>, Link)> {
    if !chainable(canonical) {
        return None;
    }
    let d = digest(prev, seq, canonical);
    let mut out = Vec::with_capacity(canonical.len() + KEY.len() + 90);
    out.extend_from_slice(&canonical[..canonical.len() - 1]);
    if canonical.len() > 2 {
        out.push(b',');
    }
    out.extend_from_slice(KEY);
    out.extend_from_slice(format!("{seq}.{}\"}}", hex(&d)).as_bytes());
    Some((out, Link { seq, digest: d }))
}

/// What a stored payload holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// A link at its end: the canonical bytes it was computed over, and the link as stored.
    Chained { canonical: Vec<u8>, link: Link },
    /// No link (a row from before the chain, or not a JSON object).
    Unchained,
}

pub fn open(stored: &[u8]) -> Opened {
    let Some(body) = stored.strip_suffix(b"\"}") else {
        return Opened::Unchained;
    };
    // The link's own text holds no quote, so the last key before the end is it.
    let Some(at) = body.windows(KEY.len()).rposition(|w| w == KEY) else {
        return Opened::Unchained;
    };
    let Ok(text) = std::str::from_utf8(&body[at + KEY.len()..]) else {
        return Opened::Unchained;
    };
    let Some((seq, hexd)) = text.split_once('.') else {
        return Opened::Unchained;
    };
    let (Ok(seq), Some(digest)) = (seq.parse::<u64>(), unhex(hexd)) else {
        return Opened::Unchained;
    };
    let mut canonical = body[..at].to_vec();
    match canonical.last() {
        Some(b',') => {
            canonical.pop();
            canonical.push(b'}');
        }
        Some(b'{') => canonical.push(b'}'),
        _ => return Opened::Unchained,
    }
    Opened::Chained {
        canonical,
        link: Link { seq, digest },
    }
}

/// The row that starts a chain: its data says how much history came before it.
/// `through` is the position of the last unchained row (0 with none).
pub fn started_row(at_unix_ms: u64, unchained: u64, through: u64) -> Vec<u8> {
    format!(
        "{{\"at_unix_ms\":{at_unix_ms},\"kind\":\"{STARTED_KIND}\",\"data\":{{\"unchained_rows\":{unchained},\"through_position\":{through},\"chain\":1}}}}"
    )
    .into_bytes()
}

/// A running digest over the rows from before the chain, in WAL order: each
/// payload's length, then its bytes. `ledger.history_pinned` records it.
#[derive(Default)]
pub struct History {
    h: Sha256,
    pub rows: u64,
    pub through: u64,
}

impl History {
    pub fn push(&mut self, position: u64, payload: &[u8]) {
        self.h.update((payload.len() as u64).to_le_bytes());
        self.h.update(payload);
        self.rows += 1;
        self.through = position;
    }
    pub fn digest(&self) -> [u8; 32] {
        self.h.clone().finalize().into()
    }
}
