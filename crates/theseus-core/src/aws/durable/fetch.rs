//! What a restore fetches from S3 (step 16), and in what order: the rows
//! say what is current, never a listing. For each segment the `<dep>#wal`
//! rows name, its sealed object when there is one, else its tails stitched
//! from byte 0, each starting where the last ended. Every object is checked
//! against its row's SHA-256. What does not join is said, never filled: a
//! tail past where the stitch stopped (a gap, or a tail of a log since
//! rewound, which stays in S3), a segment with no row, or one whose first
//! position does not follow the last. The restore stops at the first gap,
//! and the local restore's open checks every frame of what it fetched.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use theseus_store::wal;

use super::read::{Digest, ReadError, Reader, Row};

/// A sealed segment's row: its object, whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sealed {
    pub key: String,
    pub bytes: u64,
    pub sha256_hex: String,
    pub last_position: u64,
}

/// A tail's row: the open segment's bytes `from..to`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tail {
    pub key: String,
    pub from: u64,
    pub to: u64,
    pub last_position: u64,
    pub sha256_b64: String,
}

/// One segment's rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub sealed: Option<Sealed>,
    /// By where each starts.
    pub tails: BTreeMap<u64, Tail>,
}

/// A blob's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub digest: String,
    pub key: String,
    pub bytes: u64,
}

/// A deployment's rows, as the restore reads them.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    pub segments: BTreeMap<u32, Segment>,
    pub blobs: Vec<Blob>,
    /// Rows that do not read as the tender writes them, said.
    pub unread: Vec<String>,
}

fn text(r: &Row, k: &str) -> Option<String> {
    r.get(k)?.as_str().map(String::from)
}

fn num(r: &Row, k: &str) -> Option<u64> {
    r.get(k)?.as_u64()
}

impl Rows {
    /// From the `<dep>#wal` and `<dep>#blob` rows.
    pub fn read(wal_rows: &[Row], blob_rows: &[Row]) -> Self {
        let mut rows = Rows::default();
        for r in wal_rows {
            let sk = text(r, "sk").unwrap_or_default();
            let parsed = match sk.split_once(".tail.") {
                None => sk.parse::<u32>().ok().and_then(|n| {
                    Some((
                        n,
                        None,
                        Some(Sealed {
                            key: text(r, "key")?,
                            bytes: num(r, "bytes")?,
                            sha256_hex: text(r, "sha256")?,
                            last_position: num(r, "last_position")?,
                        }),
                    ))
                }),
                Some((n, _)) => n.parse::<u32>().ok().and_then(|n| {
                    Some((
                        n,
                        Some(Tail {
                            key: text(r, "key")?,
                            from: num(r, "from")?,
                            to: num(r, "to")?,
                            last_position: num(r, "last_position")?,
                            sha256_b64: text(r, "sha256")?,
                        }),
                        None,
                    ))
                }),
            };
            match parsed {
                Some((n, Some(t), _)) if t.to > t.from => {
                    rows.segments.entry(n).or_default().tails.insert(t.from, t);
                }
                Some((n, None, Some(s))) => rows.segments.entry(n).or_default().sealed = Some(s),
                _ => rows.unread.push(format!(
                    "the row {sk:?} does not read as a segment's or a tail's"
                )),
            }
        }
        for r in blob_rows {
            let digest = text(r, "sk").unwrap_or_default();
            match (text(r, "key"), num(r, "bytes")) {
                (Some(key), Some(bytes)) if super::is_digest(&digest) => {
                    rows.blobs.push(Blob { digest, key, bytes })
                }
                _ => rows
                    .unread
                    .push(format!("the blob row {digest:?} does not read as one")),
            }
        }
        rows
    }
}

/// Where a restored segment came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Source {
    /// Its sealed object.
    Object,
    /// Its tails, joined.
    Tails { count: usize },
}

/// One restored segment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fetched {
    pub segment: u32,
    #[serde(flatten)]
    pub source: Source,
    pub bytes: u64,
    pub last_position: u64,
    /// Tails past where its stitch stopped: left in S3, not restored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unjoined: Option<String>,
}

/// What a fetch did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Fetch {
    pub segments: Vec<Fetched>,
    /// Where the restore stopped short of what the rows name, and why.
    pub gap: Option<String>,
    pub blobs: u32,
    /// Blobs a row names that S3 does not hold.
    pub blobs_missing: Vec<String>,
    /// Bytes read from S3.
    pub bytes: u64,
    /// Rows that did not read.
    pub unread: Vec<String>,
}

impl Fetch {
    /// The last position the fetched segments hold.
    pub fn last_position(&self) -> u64 {
        self.segments.last().map_or(0, |s| s.last_position)
    }
}

/// A segment's tails from byte 0, each starting where the last ended: the
/// tails joined, and what was left past the join. None when no tail starts
/// at 0. A tail whose object is gone ends the join there.
async fn stitch(
    reader: &Reader,
    n: u32,
    tails: &BTreeMap<u64, Tail>,
) -> Result<Option<(Vec<u8>, usize, u64, Option<String>)>, String> {
    let mut bytes = Vec::new();
    let mut count = 0usize;
    let mut last = 0u64;
    let mut at = 0u64;
    let mut why_stopped = None;
    while let Some(t) = tails.get(&at) {
        match reader
            .get(&t.key, t.to - t.from, &Digest::Base64(t.sha256_b64.clone()))
            .await
        {
            Ok(b) => bytes.extend(b),
            Err(ReadError::Missing) => {
                why_stopped = Some(format!("the tail {} is not in S3", t.key));
                break;
            }
            Err(e) => return Err(format!("segment {n}'s tail: {e}")),
        }
        count += 1;
        last = t.last_position;
        at = t.to;
    }
    if count == 0 {
        return Ok(None);
    }
    let past: Vec<&Tail> = tails.values().filter(|t| t.from >= at).collect();
    let unjoined = match (why_stopped, past.first()) {
        (Some(why), _) => Some(format!(
            "{why}: the join stops at byte {at}, and {} tail(s) from there are not restored",
            past.len()
        )),
        (None, Some(first)) => Some(format!(
            "{} tail(s) past byte {at} (the first from byte {}) do not start where the join \
             ended: a gap, or a log since rewound; left in S3, not restored",
            past.len(),
            first.from
        )),
        (None, None) => None,
    };
    Ok(Some((bytes, count, last, unjoined)))
}

/// Fetch what `rows` name into `into`, laid out as a store's (`wal/`,
/// `blobs/`): each segment from its object, else its tails, until the first
/// gap. An object whose bytes are not its row's is refused, and so is the
/// whole fetch.
pub async fn fetch(reader: &Reader, rows: &Rows, into: &Path) -> Result<Fetch, String> {
    let io = |what: &str, e: std::io::Error| format!("{what}: {e}");
    let wal_dir = into.join("wal");
    std::fs::create_dir_all(&wal_dir).map_err(|e| io("creating the staging WAL", e))?;
    let mut out = Fetch {
        unread: rows.unread.clone(),
        ..Fetch::default()
    };
    let mut expect: Option<(u32, u64)> = None;
    for (&n, seg) in &rows.segments {
        let want = expect.map_or(1, |(s, _)| s + 1);
        if n != want {
            out.gap = Some(format!(
                "segment {want} has no row in the table; segments {n} and after are not restored"
            ));
            break;
        }
        let (bytes, source, last, unjoined) = match &seg.sealed {
            Some(s) => {
                let b = reader
                    .get(&s.key, s.bytes, &Digest::Hex(s.sha256_hex.clone()))
                    .await
                    .map_err(|e| match e {
                        ReadError::Missing => format!(
                            "segment {n}'s object {} is not in S3, though its row names it",
                            s.key
                        ),
                        e => format!("segment {n}: {e}"),
                    })?;
                (b, Source::Object, s.last_position, None)
            }
            None => match stitch(reader, n, &seg.tails).await? {
                Some((b, count, last, unjoined)) => (b, Source::Tails { count }, last, unjoined),
                None => {
                    out.gap = Some(format!(
                        "segment {n} has no object, and none of its tails starts at byte 0; \
                         segments {n} and after are not restored"
                    ));
                    break;
                }
            },
        };
        let first = wal::first_position(&bytes, 0);
        let follows = expect.map_or(1, |(_, l)| l + 1);
        if first != Some(follows) {
            out.gap = Some(format!(
                "segment {n} begins at position {}, not {follows}; segments {n} and after are \
                 not restored",
                first.map_or("(none)".into(), |p| p.to_string())
            ));
            break;
        }
        std::fs::write(wal::segment_path(&wal_dir, n), &bytes)
            .map_err(|e| io(&format!("writing segment {n}"), e))?;
        out.bytes += bytes.len() as u64;
        out.segments.push(Fetched {
            segment: n,
            source,
            bytes: bytes.len() as u64,
            last_position: last,
            unjoined: unjoined.clone(),
        });
        expect = Some((n, last));
        if unjoined.is_some() && n < rows.segments.keys().next_back().copied().unwrap_or(n) {
            // A segment that a later one follows, cut short: what follows it
            // cannot join it.
            out.gap = Some(format!(
                "segment {n} ends at its join, and later segments follow it; segments {} and \
                 after are not restored",
                n + 1
            ));
            break;
        }
    }
    if out.segments.is_empty() {
        return Err(match out.gap.take() {
            Some(g) => format!("nothing to restore: {g}"),
            None => "nothing to restore: the table holds no segment's row for it".into(),
        });
    }
    let blob_dir = into.join("blobs");
    if !rows.blobs.is_empty() {
        std::fs::create_dir_all(&blob_dir).map_err(|e| io("creating the staging blobs", e))?;
    }
    for b in &rows.blobs {
        match reader
            .get(&b.key, b.bytes, &Digest::Hex(b.digest.clone()))
            .await
        {
            Ok(bytes) => {
                std::fs::write(blob_dir.join(&b.digest), &bytes)
                    .map_err(|e| io(&format!("writing blob {}", b.digest), e))?;
                out.blobs += 1;
                out.bytes += bytes.len() as u64;
            }
            Err(ReadError::Missing) => out.blobs_missing.push(b.digest.clone()),
            Err(e) => return Err(format!("blob {}: {e}", b.digest)),
        }
    }
    Ok(out)
}
