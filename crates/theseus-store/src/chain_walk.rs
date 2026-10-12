//! Walking the chain (theseus-bmkxk): the one reader of the ledger's links. It
//! recomputes every digest from the canonical bytes, never trusting a stored
//! one, holds nothing but the head and a few counters (a million rows walk in
//! constant memory), and names the first broken link: its position, time and
//! kind, and whether the row was edited, removed or reordered, as far as the
//! walk can tell.

use crate::chain::{self, Head, History, Link, Opened, GENESIS};
use crate::record::{kinds, Record};
use crate::store::Store;
use anyhow::Result;

/// What went wrong at a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The row's bytes do not match its own digest.
    Edited,
    /// Rows are missing before this one (`missing` of them, by seq).
    Removed { missing: u64 },
    /// This row and the next stand in each other's places.
    Reordered,
    /// A seq the chain has already passed: a row repeated, or moved earlier.
    OutOfOrder,
    /// A row with no link after the chain started.
    Unchained,
    /// The chain ends before the head recorded earlier: rows missing from its tail.
    Truncated,
    /// The chain differs from the head recorded earlier: it was rewritten from some row onward.
    Rewritten,
    /// The rows from before the chain no longer match what the chain pinned.
    History,
}

impl Fault {
    /// The word the CLI and the wire use.
    pub fn word(&self) -> &'static str {
        match self {
            Fault::Edited => "edited",
            Fault::Removed { .. } => "removed",
            Fault::Reordered => "reordered",
            Fault::OutOfOrder => "out_of_order",
            Fault::Unchained => "unchained",
            Fault::Truncated => "truncated",
            Fault::Rewritten => "rewritten",
            Fault::History => "history",
        }
    }
}

/// The first broken link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Break {
    pub position: u64,
    pub at_unix_ms: u64,
    /// The row's kind, when its bytes still say.
    pub kind: Option<String>,
    pub seq: Option<u64>,
    pub fault: Fault,
    pub detail: String,
}

/// A head recorded earlier, to compare the walk against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub seq: u64,
    /// The digest in hex, or a prefix of it (at least 8 characters).
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Rows whose links verified.
    pub rows: u64,
    /// When the chain started (its first row's time), if the walk saw it.
    pub since_ms: Option<u64>,
    pub head: Option<Head>,
    /// Rows from before the chain, passed over.
    pub unchained: u64,
    /// The pinned history checked out (a `ledger.history_pinned` row matched).
    pub history_checked: bool,
    pub broken: Option<Break>,
}

/// One ledger record, as the walk is fed it.
pub struct Row<'a> {
    pub position: u64,
    pub at_unix_ms: u64,
    pub payload: &'a [u8],
}

struct Pending {
    position: u64,
    at_unix_ms: u64,
    canonical: Vec<u8>,
    link: Link,
    before: Link,
}

pub struct Walk {
    anchor: Option<Anchor>,
    /// Begin at the first row seen, trusting its own link (`--since`).
    since: bool,
    head: Option<Head>,
    started: bool,
    pending: Option<Pending>,
    rows: u64,
    since_ms: Option<u64>,
    history: History,
    history_checked: bool,
    pinned: bool,
    broken: Option<Break>,
}

fn kind_of(canonical: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(canonical).ok()?;
    v.get("kind")?.as_str().map(str::to_string)
}

fn mentions(canonical: &[u8], kind: &str) -> bool {
    let needle = format!("\"kind\":\"{kind}\"");
    canonical
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

impl Walk {
    pub fn new(since: bool, anchor: Option<Anchor>) -> Self {
        Self {
            anchor,
            since,
            head: None,
            started: false,
            pending: None,
            rows: 0,
            since_ms: None,
            history: History::default(),
            history_checked: false,
            pinned: false,
            broken: None,
        }
    }

    /// The walk has found its answer: stop feeding it.
    pub fn done(&self) -> bool {
        self.broken.is_some()
    }

    fn brk(
        &mut self,
        position: u64,
        at_unix_ms: u64,
        canonical: Option<&[u8]>,
        seq: Option<u64>,
        fault: Fault,
        detail: String,
    ) {
        if self.broken.is_none() {
            self.broken = Some(Break {
                position,
                at_unix_ms,
                kind: canonical.and_then(kind_of),
                seq,
                fault,
                detail,
            });
        }
    }

    /// Feed the next row, in WAL order.
    pub fn step(&mut self, row: Row<'_>) {
        if self.broken.is_some() {
            return;
        }
        if let Some(p) = self.pending.take() {
            self.resolve(p, Some(&row));
            return;
        }
        match chain::open(row.payload) {
            Opened::Unchained => self.unchained(&row),
            Opened::Chained { canonical, link } => self.chained(&row, canonical, link),
        }
    }

    fn unchained(&mut self, row: &Row<'_>) {
        if self.started {
            self.brk(
                row.position,
                row.at_unix_ms,
                Some(row.payload),
                None,
                Fault::Unchained,
                "a row with no chain after the chain started: it was replaced, or written around the chain".into(),
            );
        } else {
            self.history.push(row.position, row.payload);
        }
    }

    fn chained(&mut self, row: &Row<'_>, canonical: Vec<u8>, link: Link) {
        let Some(head) = self.head else {
            return self.first(row, canonical, link);
        };
        let seq = head.link.seq + 1;
        if link.seq == seq && link.digest == chain::digest(&head.link.digest, seq, &canonical) {
            self.accept(row, &canonical, link);
        } else {
            self.pending = Some(Pending {
                position: row.position,
                at_unix_ms: row.at_unix_ms,
                canonical,
                link,
                before: head.link,
            });
        }
    }

    fn first(&mut self, row: &Row<'_>, canonical: Vec<u8>, link: Link) {
        if self.since {
            // The first row seen is trusted: the walk checks everything after it.
            self.since_ms = Some(row.at_unix_ms);
            self.started = true;
            self.history_checked = true;
            self.head = Some(Head {
                link,
                position: row.position,
            });
            self.rows = 1;
            return self.anchored(row, link);
        }
        if link.seq != 0 {
            return self.brk(
                row.position,
                row.at_unix_ms,
                Some(&canonical),
                Some(link.seq),
                Fault::Removed { missing: link.seq },
                format!(
                    "the chain's first row is missing: the first row found is seq {}, not seq 0",
                    link.seq
                ),
            );
        }
        if link.digest != chain::digest(&GENESIS, 0, &canonical) {
            return self.brk(
                row.position,
                row.at_unix_ms,
                Some(&canonical),
                Some(0),
                Fault::Edited,
                "the chain's first row does not match its own digest".into(),
            );
        }
        self.check_start(row, &canonical);
        if self.broken.is_some() {
            return;
        }
        self.since_ms = Some(row.at_unix_ms);
        self.started = true;
        self.head = Some(Head {
            link,
            position: row.position,
        });
        self.rows = 1;
        self.anchored(row, link);
    }

    /// The first row says how many rows came before it: they must still be there.
    fn check_start(&mut self, row: &Row<'_>, canonical: &[u8]) {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(canonical) else {
            return;
        };
        let d = &v["data"];
        let (Some(n), Some(through)) =
            (d["unchained_rows"].as_u64(), d["through_position"].as_u64())
        else {
            return;
        };
        if n != self.history.rows || (n > 0 && through != self.history.through) {
            self.brk(
                row.position,
                row.at_unix_ms,
                Some(canonical),
                Some(0),
                Fault::History,
                format!(
                    "the chain started over {n} earlier rows through position {through}; {} are there now, through position {}",
                    self.history.rows, self.history.through
                ),
            );
        }
    }

    fn accept(&mut self, row: &Row<'_>, canonical: &[u8], link: Link) {
        if !self.since && !self.pinned && mentions(canonical, chain::PINNED_KIND) {
            self.check_pin(row, canonical);
            if self.broken.is_some() {
                return;
            }
        }
        self.head = Some(Head {
            link,
            position: row.position,
        });
        self.rows += 1;
        self.anchored(row, link);
    }

    fn check_pin(&mut self, row: &Row<'_>, canonical: &[u8]) {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(canonical) else {
            return;
        };
        let d = &v["data"];
        let (Some(rows), Some(through), Some(digest)) = (
            d["rows"].as_u64(),
            d["through_position"].as_u64(),
            d["digest"].as_str(),
        ) else {
            return;
        };
        self.pinned = true;
        let (n, last) = (self.history.rows, self.history.through);
        let ours = chain::hex(&self.history.digest());
        if rows == n && through == last && ours == digest {
            self.history_checked = true;
        } else {
            self.brk(
                row.position,
                row.at_unix_ms,
                Some(canonical),
                Some(row.position),
                Fault::History,
                format!(
                    "the rows from before the chain ({n}, through position {last}) no longer match the digest pinned here ({rows} rows, through position {through}): one was edited or removed"
                ),
            );
        }
    }

    fn anchored(&mut self, row: &Row<'_>, link: Link) {
        let Some(a) = &self.anchor else { return };
        if link.seq == a.seq && !chain::hex(&link.digest).starts_with(&a.digest) {
            let detail = format!(
                "seq {} has digest {}, but the head recorded earlier was {}: the chain was rewritten from here or before",
                link.seq,
                &chain::hex(&link.digest)[..12],
                &a.digest[..a.digest.len().min(12)]
            );
            self.brk(
                row.position,
                row.at_unix_ms,
                None,
                Some(link.seq),
                Fault::Rewritten,
                detail,
            );
        }
    }

    fn resolve(&mut self, p: Pending, next: Option<&Row<'_>>) {
        let want = p.before.seq + 1;
        let (fault, detail) = if p.link.seq > want {
            let swapped = next.is_some_and(|n| match chain::open(n.payload) {
                Opened::Chained { canonical, link } => {
                    link.seq == want
                        && link.digest == chain::digest(&p.before.digest, want, &canonical)
                }
                Opened::Unchained => false,
            });
            if swapped {
                let n = next.map_or(0, |n| n.position);
                (
                    Fault::Reordered,
                    format!(
                        "the rows at positions {} and {n} are in each other's places (seq {} came before seq {want})",
                        p.position, p.link.seq
                    ),
                )
            } else {
                let missing = p.link.seq - want;
                (
                    Fault::Removed { missing },
                    format!(
                        "{missing} row(s) are missing before this one: seq {want} is the next the chain needs, this is seq {}",
                        p.link.seq
                    ),
                )
            }
        } else if p.link.seq == want {
            let follows = next.is_some_and(|n| match chain::open(n.payload) {
                Opened::Chained { canonical, link } => {
                    link.seq == want + 1
                        && link.digest == chain::digest(&p.link.digest, want + 1, &canonical)
                }
                Opened::Unchained => false,
            });
            (
                Fault::Edited,
                if follows {
                    "the row does not match its own digest, and the next row still links to that digest: its bytes were changed after it was written".into()
                } else {
                    "the row does not match its own digest".to_string()
                },
            )
        } else {
            (
                Fault::OutOfOrder,
                format!(
                    "seq {} comes after seq {}: a row was repeated or moved earlier",
                    p.link.seq, p.before.seq
                ),
            )
        };
        self.brk(
            p.position,
            p.at_unix_ms,
            Some(&p.canonical),
            Some(p.link.seq),
            fault,
            detail,
        );
    }

    /// The walk's end: settle the last row, and compare the head with the anchor.
    pub fn finish(mut self) -> Report {
        if let Some(p) = self.pending.take() {
            self.resolve(p, None);
        }
        if self.broken.is_none() {
            if let (Some(a), Some(h)) = (&self.anchor, self.head) {
                if h.link.seq < a.seq {
                    let (position, at) = (h.position, 0);
                    self.brk(
                        position,
                        at,
                        None,
                        Some(h.link.seq),
                        Fault::Truncated,
                        format!(
                            "the chain ends at seq {}, but a head recorded earlier was seq {}: {} row(s) are missing from the tail",
                            h.link.seq,
                            a.seq,
                            a.seq - h.link.seq
                        ),
                    );
                }
            } else if let (Some(a), None) = (&self.anchor, self.head) {
                self.brk(
                    0,
                    0,
                    None,
                    None,
                    Fault::Truncated,
                    format!(
                        "no chain at all, but a head recorded earlier was seq {}",
                        a.seq
                    ),
                );
            }
        }
        Report {
            rows: self.rows,
            since_ms: self.since_ms,
            head: self.head,
            unchained: self.history.rows,
            history_checked: self.history_checked,
            broken: self.broken,
        }
    }
}

/// Rows read at a time: the walk's memory is one page.
pub const PAGE: usize = 2000;

/// Walk the store's ledger in WAL order, a page at a time (constant memory).
/// `since` starts at the first row at or after that position.
pub fn verify(store: &dyn Store, since: Option<u64>, anchor: Option<Anchor>) -> Result<Report> {
    let mut walk = Walk::new(since.is_some(), anchor);
    let mut after = since.map_or(0, |p| p.saturating_sub(1));
    loop {
        let page: Vec<Record> = store.of_kind_after(kinds::LEDGER, after, PAGE)?;
        let Some(last) = page.last() else { break };
        after = last.position;
        let n = page.len();
        for r in &page {
            walk.step(Row {
                position: r.position,
                at_unix_ms: r.at_unix_ms,
                payload: &r.payload,
            });
            if walk.done() {
                return Ok(walk.finish());
            }
        }
        if n < PAGE {
            break;
        }
    }
    Ok(walk.finish())
}

/// What came before the chain: a digest over the ledger rows with no link, in
/// WAL order, and the row that started the chain (`ledger.chain_started`).
pub struct Before {
    pub history: History,
    pub start: Option<Record>,
}

/// Read the unchained rows from the ledger's start to the chain's first row, a
/// page at a time, calling `between` after each page (a caller paces itself
/// there). The cost grows with the history from before the chain, never with
/// the chain: a store that started chained stops at its first row.
///
/// `page(after)` reads the ledger's next `PAGE` records past `after`: a caller
/// that must not hold the store between pages gets it afresh there.
pub fn read_before(
    mut page: impl FnMut(u64) -> Result<Vec<Record>>,
    mut between: impl FnMut(),
) -> Result<Before> {
    let mut history = History::default();
    let mut after = 0;
    loop {
        let page: Vec<Record> = page(after)?;
        let Some(last) = page.last() else { break };
        after = last.position;
        let n = page.len();
        for r in page {
            match chain::open(&r.payload) {
                Opened::Unchained => history.push(r.position, &r.payload),
                Opened::Chained { .. } => {
                    return Ok(Before {
                        history,
                        start: Some(r),
                    })
                }
            }
        }
        if n < PAGE {
            break;
        }
        between();
    }
    Ok(Before {
        history,
        start: None,
    })
}
