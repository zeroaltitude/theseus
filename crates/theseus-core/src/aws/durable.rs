//! The durability tender (AWS design §5, step 15; spec §6, "Tenders" and
//! the 5–60 s target): the store shipped off the machine, so a lost machine
//! loses at most the last few seconds.
//!
//! - **Where it runs.** In the daemon, as a task after serving (2 s after,
//!   as the index tender starts), on the WAL follower the index lane built
//!   (`theseus-follow`), not inside `theseus-index`: the tender signs with a
//!   role session, and AWS's credentials stay in the core (§3.5: no program
//!   or child holds them). Only the socket daemon runs it, and only for the
//!   account whose `[aws.accounts.<id>]` says `durability = true`.
//! - **What it ships**, under `s3://theseus-<account>-<region>/durability/
//!   <deployment>/`: each sealed WAL segment whole (`wal/<n>.seg`, one
//!   `PutObject`, or a multipart upload past 8 MiB), the open segment's new
//!   whole frames as tails (`wal/<n>.seg.tail/<from>-<to>`: byte ranges, so a
//!   restore joins them), and each new blob (`blobs/<sha256>`). Every object
//!   carries its SHA-256, which S3 checks. It reads the WAL only through the
//!   follower, and ships bytes, never frames re-encoded, so a change to the
//!   frame's layout ships as it is.
//! - **The index rows** ([`rows`]): a row per object and per keyed record's
//!   latest position, `BatchWriteItem`s into the durability table, retrying
//!   unprocessed items.
//! - **Its cursor is durable** ([`cursor`]): after every object, so a restart
//!   resumes without a duplicate or a gap. The object in flight is saved
//!   before it is sent; a restart that finds it asks S3 for it (by its
//!   checksum, or a multipart upload's parts) instead of sending it again.
//! - **The session** `theseus-durability`, whose inline policy ([`policy`])
//!   allows writing objects under its own prefix, reading them back, and
//!   writing rows whose keys start with its deployment. Nothing else.
//! - **When**: woken by the WAL's directory (inotify), it waits [`SETTLE`]
//!   for the writes around it, then ships; a minute's backstop besides. A
//!   failed pass is tried again after a backoff that doubles to a minute,
//!   and health says why.
//! - **Visibility**: health's `durability` line on the account, with
//!   `oldest_unshipped_unix_ms` (the recovery point's exposure) and its lag;
//!   a `durability.shipped` row per sealed segment; telemetry's bytes shipped
//!   and the lag each time it catches up.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read as _;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_follow::{Batch, Cursor, FollowError, Stop, Wake, Waker, WalFollower};
use theseus_protocol::{now_unix_ms, AwsDurabilityStatus};
use theseus_store::{blocking, wal};

use crate::config::AwsAccountConfig;
use crate::fact::durability::SegmentShipped;
use crate::fact::Fact;
use crate::ledger::LedgerRow;

use super::Account;

pub mod cursor;
pub mod fetch;
pub mod read;
pub mod restore;
pub mod rows;
pub mod s3;

use cursor::{Inflight, Part, Paths, Saved, Upload};
use rows::Table;
use s3::{Bucket, S3Error};

/// The tender's name: its session is `theseus-durability`.
pub const TENDER: &str = "durability";
/// The table the foundation stack makes.
pub const TABLE: &str = "theseus-durability";
/// After serving, before the first pass: the start's aftermath stays quiet.
pub const START_AFTER: Duration = Duration::from_secs(2);
/// After the WAL changes, before shipping: the writes around it ride along.
pub const SETTLE: Duration = Duration::from_secs(5);
/// A pass this long after the last, whatever the WAL did.
pub const BACKSTOP: Duration = Duration::from_secs(60);
/// The longest wait between failed passes.
pub const RETRY_MAX: Duration = Duration::from_secs(60);

/// The foundation's bucket in `region`.
pub fn bucket(account: &str, region: &str) -> String {
    format!("theseus-{account}-{region}")
}

/// A deployment's prefix in the bucket.
pub fn prefix(deployment: &str) -> String {
    format!("durability/{deployment}/")
}

/// The tender session's inline policy (§3.5): objects under its prefix
/// (`s3:PutObject` covers a multipart upload's create, parts, and
/// completion; `s3:GetObject` a head), an upload's parts listed or aborted,
/// and rows whose partition key starts with its deployment.
pub fn policy(tender: &str, account: &str, cfg: &AwsAccountConfig) -> Option<Value> {
    (tender == TENDER).then(|| {
        let (region, deployment) = (&cfg.region, cfg.deployment());
        json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Sid": "ShipToItsPrefix",
                    "Effect": "Allow",
                    "Action": [
                        "s3:PutObject", "s3:GetObject",
                        "s3:ListMultipartUploadParts", "s3:AbortMultipartUpload",
                    ],
                    "Resource": format!(
                        "arn:aws:s3:::{}/{}*", bucket(account, region), prefix(deployment)
                    ),
                },
                {
                    "Sid": "ItsIndexRows",
                    "Effect": "Allow",
                    "Action": "dynamodb:BatchWriteItem",
                    "Resource": format!("arn:aws:dynamodb:{region}:{account}:table/{TABLE}"),
                    "Condition": {"ForAllValues:StringLike": {
                        "dynamodb:LeadingKeys": [format!("{deployment}#*")],
                    }},
                },
            ],
        })
    })
}

/// The line with its lag as of now.
pub fn with_lag(mut s: AwsDurabilityStatus) -> AwsDurabilityStatus {
    s.lag_ms = s
        .oldest_unshipped_unix_ms
        .map_or(0, |t| now_unix_ms().saturating_sub(t));
    s
}

/// The tender's sizes and waits; tests make them small.
#[derive(Clone, Debug)]
pub struct Tuning {
    /// A multipart upload's part (S3's least is 5 MiB, but for the last).
    pub part_bytes: u64,
    /// A sealed segment this size or smaller goes in one `PutObject`.
    pub single_max: u64,
    /// The WAL read in one batch, about.
    pub batch_bytes: usize,
    pub settle: Duration,
    pub row_tries: u32,
    pub row_backoff: Duration,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            part_bytes: 8 << 20,
            single_max: 8 << 20,
            batch_bytes: 8 << 20,
            settle: SETTLE,
            row_tries: 8,
            row_backoff: Duration::from_millis(50),
        }
    }
}

/// What telemetry hears: bytes shipped, by object (`segment`, `tail`,
/// `blob`, `rows`: rows count one each), and the lag each catch-up closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    Shipped { object: &'static str, bytes: u64 },
    CaughtUp { lag_ms: u64 },
}

/// Where the tender's rows and measures go.
#[derive(Clone, Default)]
pub struct Hooks {
    pub ledger: Option<Arc<dyn Fn(LedgerRow) + Send + Sync>>,
    pub measure: Option<Arc<dyn Fn(Measure) + Send + Sync>>,
}

/// Why a pass stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum Halt {
    /// It is tried again after a backoff: AWS, the network, the disk.
    Retry(String),
    /// It cannot go on: the WAL does not read as a log.
    Stop(String),
}

fn retry(what: &str) -> impl Fn(std::io::Error) -> Halt + '_ {
    move |e| Halt::Retry(format!("{what}: {e}"))
}

fn s3_halt(e: S3Error) -> Halt {
    Halt::Retry(e.to_string())
}

/// Bytes `from..to` of a file.
fn read_range(path: &Path, from: u64, to: u64) -> std::io::Result<Vec<u8>> {
    let f = File::open(path)?;
    let mut buf = vec![0u8; usize::try_from(to - from).map_err(std::io::Error::other)?];
    f.read_exact_at(&mut buf, from)?;
    Ok(buf)
}

/// A file's length and SHA-256, read in pieces.
fn file_sha256(path: &Path) -> std::io::Result<(u64, [u8; 32])> {
    use sha2::{Digest, Sha256};
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut len = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        len += n as u64;
    }
    Ok((len, h.finalize().into()))
}

fn is_digest(d: &str) -> bool {
    d.len() == 64
        && d.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The tender's state between passes.
pub struct Shipper {
    account: Arc<Account>,
    deployment: String,
    prefix: String,
    bucket: Bucket,
    table: Table,
    paths: Paths,
    tuning: Tuning,
    saved: Saved,
    follower: WalFollower,
    blobs: BTreeSet<String>,
    status: AwsDurabilityStatus,
    hooks: Hooks,
    /// A test's crash: fail just after S3 took this part, before the cursor
    /// records it.
    #[cfg(test)]
    pub crash_after_part: Option<u32>,
    /// A test's crash: fail just after S3 took an object, before the cursor
    /// records it.
    #[cfg(test)]
    pub crash_after_put: bool,
}

impl Shipper {
    /// Open from the saved cursor, or from the log's start: a cursor the log
    /// no longer holds (a restore replaced it) starts again too, and every
    /// object is checked against S3 before it is sent. Reads the disk: call
    /// it off the runtime's workers.
    pub fn open(
        account: Arc<Account>,
        paths: Paths,
        tuning: Tuning,
        hooks: Hooks,
    ) -> Result<Self, Halt> {
        let saved = cursor::load(&paths).unwrap_or_default();
        let (follower, saved) = match WalFollower::open(&paths.wal, saved.follow.clone()) {
            Ok(f) => (f, saved),
            Err(FollowError::Rewound(why)) => {
                tracing::warn!(%why, "durability: the WAL is not the one the cursor was taken on; shipping it again from the start");
                let f = WalFollower::open(&paths.wal, Cursor::start())
                    .map_err(|e| Halt::Stop(e.to_string()))?;
                (f, Saved::default())
            }
            Err(FollowError::Io(e)) => return Err(Halt::Retry(format!("opening the WAL: {e}"))),
            Err(e) => return Err(Halt::Stop(e.to_string())),
        };
        let blobs = cursor::shipped_blobs(&paths).map_err(retry("reading blobs.shipped"))?;
        let cfg = &account.cfg;
        let deployment = cfg.deployment().to_string();
        let region = cfg.region.clone();
        let status = AwsDurabilityStatus {
            state: "shipping".into(),
            bucket: bucket(&account.id, &region),
            prefix: prefix(&deployment),
            table: TABLE.into(),
            shipped_to_position: saved.follow.position,
            ..Default::default()
        };
        Ok(Self {
            bucket: Bucket {
                account: account.clone(),
                name: status.bucket.clone(),
                region: region.clone(),
            },
            table: Table {
                account: account.clone(),
                name: TABLE.into(),
                region,
                tries: tuning.row_tries,
                backoff: tuning.row_backoff,
            },
            prefix: status.prefix.clone(),
            account,
            deployment,
            paths,
            tuning,
            saved,
            follower,
            blobs,
            status,
            hooks,
            #[cfg(test)]
            crash_after_part: None,
            #[cfg(test)]
            crash_after_put: false,
        })
    }

    pub fn status(&self) -> &AwsDurabilityStatus {
        &self.status
    }

    /// Health's line, on the account.
    fn publish(&self) {
        self.account.tended.lock().unwrap().durability = Some(self.status.clone());
    }

    fn measure(&self, m: Measure) {
        if let Some(h) = &self.hooks.measure {
            h(m);
        }
    }

    fn shipped(&mut self, object: &'static str, bytes: u64) {
        self.status.bytes += bytes;
        self.status.last_shipped_unix_ms = Some(now_unix_ms());
        self.measure(Measure::Shipped { object, bytes });
    }

    /// The WAL changed at `at_ms`: something is unshipped since then, unless
    /// something older is already.
    pub fn changed(&mut self, at_ms: u64) {
        self.oldest(at_ms);
        self.publish();
    }

    fn oldest(&mut self, at_ms: u64) {
        let o = &mut self.status.oldest_unshipped_unix_ms;
        *o = Some(o.map_or(at_ms, |t| t.min(at_ms)));
    }

    fn save(&self) -> Result<(), Halt> {
        blocking(|| cursor::save(&self.paths, &self.saved)).map_err(retry("saving the cursor"))
    }

    /// Ship what the WAL and `blobs/` hold that S3 does not yet, and their
    /// rows. Ok once caught up.
    pub async fn pass(&mut self) -> Result<(), Halt> {
        let r = self.ship_all().await;
        match &r {
            Ok(()) => {
                self.status.state = "caught_up".into();
                self.status.error = None;
            }
            Err(Halt::Retry(e)) => {
                self.status.state = "failing".into();
                self.status.error = Some(e.clone());
            }
            Err(Halt::Stop(e)) => {
                self.status.state = "stopped".into();
                self.status.error = Some(e.clone());
            }
        }
        self.publish();
        r
    }

    async fn ship_all(&mut self) -> Result<(), Halt> {
        self.status.state = "shipping".into();
        self.ship_blobs().await?;
        loop {
            let before = self.follower.cursor().clone();
            let batch_bytes = self.tuning.batch_bytes;
            let batch = match blocking(|| self.follower.read(batch_bytes)) {
                Ok(b) => b,
                Err(FollowError::Rewound(why)) => {
                    // A restore replaced the log: start again, and let S3's
                    // checksums say what it holds already.
                    tracing::warn!(%why, "durability: the WAL was replaced; shipping it again from the start");
                    self.saved = Saved::default();
                    self.save()?;
                    self.follower =
                        blocking(|| WalFollower::open(&self.paths.wal, Cursor::start()))
                            .map_err(|e| Halt::Stop(e.to_string()))?;
                    return Err(Halt::Retry(format!("the WAL was replaced ({why})")));
                }
                Err(FollowError::Io(e)) => {
                    return Err(Halt::Retry(format!("reading the WAL: {e}")))
                }
                Err(e) => return Err(Halt::Stop(e.to_string())),
            };
            if batch.is_empty() {
                break;
            }
            if let Err(e) = self.ship_batch(&batch, before.position).await {
                // The next pass reads the batch again from the cursor on
                // disk: what this one shipped, it finds there.
                let from = self.saved.follow.clone();
                if let Ok(f) = blocking(|| WalFollower::open(&self.paths.wal, from)) {
                    self.follower = f;
                }
                return Err(e);
            }
            if *self.follower.stop() != Stop::Budget {
                break;
            }
        }
        if let Some(t) = self.status.oldest_unshipped_unix_ms.take() {
            let lag_ms = now_unix_ms().saturating_sub(t);
            self.measure(Measure::CaughtUp { lag_ms });
        }
        Ok(())
    }

    /// One batch the follower read: the segments it sealed, its tails, and
    /// its rows, then the cursor past it.
    async fn ship_batch(&mut self, batch: &Batch, before: u64) -> Result<(), Halt> {
        if let Some(r) = batch.records.first() {
            self.oldest(r.at_unix_ms);
        }
        // The sealed segments first: a restart meets the upload it left
        // where it left it.
        for &seg in &batch.sealed {
            if seg > self.saved.sealed_to {
                let last = batch
                    .spans
                    .iter()
                    .zip(&batch.ends)
                    .filter(|((s, ..), _)| *s == seg)
                    .map(|(_, e)| *e)
                    .next_back()
                    .unwrap_or(before);
                self.ship_segment(seg, last).await?;
            }
        }
        for (i, &(seg, from, to)) in batch.spans.iter().enumerate() {
            let sealed = seg <= self.saved.sealed_to
                || batch.sealed.contains(&seg)
                || wal::segment_path(&self.paths.wal, seg + 1).exists();
            // A restart between a tail's save and the batch's reads that
            // tail's bytes again: ship only what follows it, so the tails
            // tile the segment with no overlap (their ends are frames' ends).
            let from = match self.saved.tail {
                Some((s, t)) if s == seg && t > from => t,
                _ => from,
            };
            if !sealed && from < to {
                self.ship_tail(seg, from, to, batch.ends[i]).await?;
            }
        }
        let records = rows::record_rows(
            &self.deployment,
            &batch.records,
            &batch.spans,
            &batch.ends,
            self.saved.rows_to,
        );
        let last = batch.records.last().map_or(before, |r| r.position);
        self.flush_rows(records).await?;
        self.saved.rows_to = self.saved.rows_to.max(last);
        self.saved.follow = self.follower.cursor().clone();
        self.save()?;
        self.status.shipped_to_position = self.saved.follow.position;
        self.publish();
        Ok(())
    }

    /// The rows the cursor holds, and `more`: written, then dropped from it.
    async fn flush_rows(&mut self, more: Vec<Value>) -> Result<(), Halt> {
        let mut items = std::mem::take(&mut self.saved.pending);
        items.extend(more);
        // A batch must not name one key twice: the last of each stands.
        let mut seen = BTreeSet::new();
        let mut unique: Vec<Value> = items
            .into_iter()
            .rev()
            .filter(|i| seen.insert((i["pk"].to_string(), i["sk"].to_string())))
            .collect();
        unique.reverse();
        if unique.is_empty() {
            return Ok(());
        }
        let count = unique.len();
        match self.table.write(unique.clone()).await {
            Ok(n) => {
                self.status.rows += n;
                self.measure(Measure::Shipped {
                    object: "rows",
                    bytes: n,
                });
                self.save()
            }
            Err(e) => {
                // Kept for the next pass.
                self.saved.pending = unique;
                Err(Halt::Retry(format!("{e} ({count} rows)")))
            }
        }
    }

    /// Put one object whole, unless the cursor says it was in flight and S3
    /// holds it with this checksum already. Whether it was sent.
    async fn put_once(&mut self, key: &str, bytes: &[u8], sha: &str) -> Result<bool, Halt> {
        if let Some(i) = self.saved.inflight(key) {
            if i.upload.is_none()
                && i.sha256 == sha
                && self.bucket.checksum(key).await.map_err(s3_halt)?.as_deref() == Some(sha)
            {
                return Ok(false);
            }
        }
        self.saved.set_inflight(Inflight {
            key: key.into(),
            sha256: sha.into(),
            upload: None,
        });
        self.save()?;
        self.bucket.put(key, bytes, sha).await.map_err(s3_halt)?;
        #[cfg(test)]
        if self.crash_after_put {
            return Err(Halt::Retry(format!("a test's crash after putting {key}")));
        }
        Ok(true)
    }

    /// A part's SHA-256 (base64), from the file.
    fn part_sha(path: &Path, part_bytes: u64, n: u32, len: u64) -> std::io::Result<String> {
        let from = u64::from(n - 1) * part_bytes;
        let bytes = read_range(path, from, (from + part_bytes).min(len))?;
        Ok(s3::b64(&s3::sha256(&bytes)))
    }

    /// The upload the cursor holds for `key`, with the parts S3 took that
    /// it did not record (their checksums match the file's): None when there
    /// is none, or S3 no longer has it. Ok(Err(parts)) when S3 completed it
    /// already.
    async fn resume_upload(
        &mut self,
        key: &str,
        path: &Path,
        len: u64,
    ) -> Result<Result<Option<Upload>, u32>, Halt> {
        let Some(mut up) = self.saved.inflight(key).and_then(|i| i.upload.clone()) else {
            return Ok(Ok(None));
        };
        match self.bucket.list_parts(key, &up.id).await {
            Ok(listed) => {
                for (n, etag, sha) in listed {
                    if n as usize != up.parts.len() + 1 {
                        continue;
                    }
                    let local = blocking(|| Self::part_sha(path, up.part_bytes, n, len))
                        .map_err(retry("reading a segment"))?;
                    if sha.as_deref() != Some(local.as_str()) {
                        break;
                    }
                    up.parts.push(Part {
                        n,
                        etag,
                        sha256: local,
                    });
                }
                Ok(Ok(Some(up)))
            }
            Err(S3Error::Missing) => {
                // Completed before the cursor said so, or aborted.
                let count = u32::try_from(len.div_ceil(up.part_bytes)).unwrap_or(u32::MAX);
                let mut parts = Vec::new();
                for n in 1..=count {
                    let sha = blocking(|| Self::part_sha(path, up.part_bytes, n, len))
                        .map_err(retry("reading a segment"))?;
                    parts.push(Part {
                        n,
                        etag: String::new(),
                        sha256: sha,
                    });
                }
                let held = self.bucket.checksum(key).await.map_err(s3_halt)?;
                if held.is_some() && held == s3::composite(&parts) {
                    Ok(Err(count))
                } else {
                    Ok(Ok(None))
                }
            }
            Err(e) => Err(s3_halt(e)),
        }
    }

    /// Put a large object in parts, resuming an upload the cursor holds: the
    /// parts it took.
    async fn put_large(
        &mut self,
        key: &str,
        path: &Path,
        len: u64,
        sha: &str,
    ) -> Result<u32, Halt> {
        let mut up = match self.resume_upload(key, path, len).await? {
            Err(parts) => return Ok(parts),
            Ok(Some(up)) => up,
            Ok(None) => {
                let id = self.bucket.create_upload(key).await.map_err(s3_halt)?;
                Upload {
                    id,
                    part_bytes: self.tuning.part_bytes,
                    parts: Vec::new(),
                }
            }
        };
        self.saved.set_inflight(Inflight {
            key: key.into(),
            sha256: sha.into(),
            upload: Some(up.clone()),
        });
        self.save()?;
        let count = u32::try_from(len.div_ceil(up.part_bytes)).unwrap_or(u32::MAX);
        let first = u32::try_from(up.parts.len()).unwrap_or(u32::MAX) + 1;
        for n in first..=count {
            let from = u64::from(n - 1) * up.part_bytes;
            let to = (from + up.part_bytes).min(len);
            let bytes =
                blocking(|| read_range(path, from, to)).map_err(retry("reading a segment"))?;
            let part_sha = s3::b64(&s3::sha256(&bytes));
            let etag = self
                .bucket
                .upload_part(key, &up.id, n, &bytes, &part_sha)
                .await
                .map_err(s3_halt)?;
            #[cfg(test)]
            if self.crash_after_part == Some(n) {
                return Err(Halt::Retry(format!("a test's crash after part {n}")));
            }
            up.parts.push(Part {
                n,
                etag,
                sha256: part_sha,
            });
            self.saved.set_inflight(Inflight {
                key: key.into(),
                sha256: sha.into(),
                upload: Some(up.clone()),
            });
            self.save()?;
        }
        match self.bucket.complete(key, &up.id, &up.parts).await {
            Ok(()) => Ok(count),
            Err(S3Error::Missing) => {
                // Completed by an attempt whose answer was lost, or gone.
                let held = self.bucket.checksum(key).await.map_err(s3_halt)?;
                if held.is_some() && held == s3::composite(&up.parts) {
                    Ok(count)
                } else {
                    self.saved.done(key);
                    self.save()?;
                    Err(Halt::Retry(format!(
                        "the upload of {key} is gone; it starts again"
                    )))
                }
            }
            Err(e) => Err(s3_halt(e)),
        }
    }

    /// A tail of the open segment: its whole frames `from..to`.
    async fn ship_tail(&mut self, seg: u32, from: u64, to: u64, last: u64) -> Result<(), Halt> {
        let path = wal::segment_path(&self.paths.wal, seg);
        let bytes = blocking(|| read_range(&path, from, to)).map_err(retry("reading a segment"))?;
        let sha = s3::b64(&s3::sha256(&bytes));
        let key = format!("{}wal/{seg:09}.seg.tail/{from:012}-{to:012}", self.prefix);
        self.put_once(&key, &bytes, &sha).await?;
        self.saved.pending.push(rows::item(
            format!("{}#wal", self.deployment),
            rows::wal_sk(seg, Some(from)),
            &[
                ("key", json!(key)),
                ("from", json!(from)),
                ("to", json!(to)),
                ("last_position", json!(last)),
                ("sha256", json!(sha)),
            ],
        ));
        self.saved.tail = Some((seg, to));
        self.saved.done(&key);
        self.save()?;
        self.status.tails += 1;
        self.shipped("tail", to - from);
        Ok(())
    }

    /// A sealed segment, whole, and its `durability.shipped` row.
    async fn ship_segment(&mut self, seg: u32, last: u64) -> Result<(), Halt> {
        let t0 = Instant::now();
        let path = wal::segment_path(&self.paths.wal, seg);
        let (len, raw) = blocking(|| file_sha256(&path)).map_err(retry("reading a segment"))?;
        let sha = s3::b64(&raw);
        let hex = hex::encode(raw);
        let key = format!("{}wal/{seg:09}.seg", self.prefix);
        let parts = if len <= self.tuning.single_max {
            let bytes =
                blocking(|| read_range(&path, 0, len)).map_err(retry("reading a segment"))?;
            self.put_once(&key, &bytes, &sha).await?;
            1
        } else {
            self.put_large(&key, &path, len, &sha).await?
        };
        self.saved.pending.push(rows::item(
            format!("{}#wal", self.deployment),
            rows::wal_sk(seg, None),
            &[
                ("key", json!(key)),
                ("bytes", json!(len)),
                ("sha256", json!(hex)),
                ("last_position", json!(last)),
                ("parts", json!(parts)),
            ],
        ));
        self.saved.sealed_to = seg;
        if matches!(self.saved.tail, Some((s, _)) if s <= seg) {
            self.saved.tail = None;
        }
        self.saved.done(&key);
        self.save()?;
        self.status.segments += 1;
        self.shipped("segment", len);
        let fact = SegmentShipped {
            segment: seg,
            key: &key,
            bytes: len,
            sha256: &hex,
            last_position: last,
            parts,
            took_ms: t0.elapsed().as_millis() as u64,
        };
        if let (Some(ledger), Some(kind)) = (&self.hooks.ledger, SegmentShipped::KIND) {
            ledger(LedgerRow::new(kind, None, None, fact.row()));
        }
        Ok(())
    }

    /// Each blob `blobs.shipped` does not name, and its row.
    async fn ship_blobs(&mut self) -> Result<(), Halt> {
        let dir = self.paths.blobs.clone();
        let names: Vec<String> = match blocking(|| std::fs::read_dir(&dir)) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| is_digest(n) && !self.blobs.contains(n))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(Halt::Retry(format!("listing blobs: {e}"))),
        };
        for d in names {
            let path = dir.join(&d);
            let bytes = blocking(|| std::fs::read(&path)).map_err(retry("reading a blob"))?;
            let raw = s3::sha256(&bytes);
            if hex::encode(raw) != d {
                tracing::warn!(blob = %d, "durability: a blob whose bytes are not its name; not shipped");
                continue;
            }
            let sha = s3::b64(&raw);
            let key = format!("{}blobs/{d}", self.prefix);
            self.put_once(&key, &bytes, &sha).await?;
            self.saved.pending.push(rows::item(
                format!("{}#blob", self.deployment),
                d.clone(),
                &[("key", json!(key)), ("bytes", json!(bytes.len()))],
            ));
            // Saved with the object still in flight, then marked: a crash
            // between the two finds it in S3 by its checksum.
            self.save()?;
            blocking(|| cursor::mark_blob(&self.paths, &d)).map_err(retry("marking a blob"))?;
            self.saved.done(&key);
            self.blobs.insert(d);
            self.status.blobs += 1;
            self.shipped("blob", bytes.len() as u64);
        }
        self.flush_rows(Vec::new()).await
    }
}

/// The line before the tender has opened: `waiting` (or `state`), and why.
fn before_open(account: &Account, state: &str, why: &str) {
    account.tended.lock().unwrap().durability = Some(AwsDurabilityStatus {
        state: state.into(),
        bucket: bucket(&account.id, &account.cfg.region),
        prefix: prefix(account.cfg.deployment()),
        table: TABLE.into(),
        error: Some(why.into()),
        ..Default::default()
    });
}

/// The tender's life: wait out the start, open, then ship at each change
/// (after [`SETTLE`]) and at each backstop, until `alive` says the core is
/// gone. A pass that fails is tried again after a backoff.
pub async fn run(
    account: Arc<Account>,
    paths: Paths,
    tuning: Tuning,
    hooks: Hooks,
    alive: impl Fn() -> bool + Send + 'static,
) {
    before_open(&account, "waiting", "for the start to settle");
    tokio::time::sleep(START_AFTER).await;
    if !account.settled().await {
        before_open(
            &account,
            "waiting",
            "for the account's check (its calls fail closed until it passes)",
        );
    }
    let Some(shipper) = open(&account, &paths, &tuning, &hooks, &alive).await else {
        return;
    };
    follow(shipper, &paths, &tuning, &alive).await;
}

/// Open the shipper, trying again after a backoff while the disk refuses.
/// None once it cannot go on, or the core is gone.
async fn open(
    account: &Arc<Account>,
    paths: &Paths,
    tuning: &Tuning,
    hooks: &Hooks,
    alive: &impl Fn() -> bool,
) -> Option<Shipper> {
    let mut backoff = tuning.settle;
    loop {
        if !alive() {
            return None;
        }
        let (a, p, t, h) = (
            account.clone(),
            paths.clone(),
            tuning.clone(),
            hooks.clone(),
        );
        match tokio::task::spawn_blocking(move || Shipper::open(a, p, t, h)).await {
            Ok(Ok(s)) => {
                s.publish();
                return Some(s);
            }
            Ok(Err(Halt::Stop(why))) => {
                tracing::error!(%why, "durability: stopped");
                before_open(account, "stopped", &why);
                return None;
            }
            Ok(Err(Halt::Retry(why))) => {
                tracing::warn!(%why, "durability: cannot open yet");
                before_open(account, "failing", &why);
            }
            Err(e) => tracing::warn!(error = %e, "durability: the open's task failed"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(RETRY_MAX);
    }
}

/// A failed pass, logged: false when it stops for good, else true after
/// its backoff (doubled for the next).
async fn after_failure(halt: Halt, backoff: &mut Duration) -> bool {
    match halt {
        Halt::Stop(why) => {
            tracing::error!(%why, "durability: stopped");
            false
        }
        Halt::Retry(why) => {
            tracing::warn!(%why, retry_in_ms = backoff.as_millis() as u64, "durability: a pass failed");
            tokio::time::sleep(*backoff).await;
            *backoff = (*backoff * 2).min(RETRY_MAX);
            true
        }
    }
}

/// Watch the WAL's directory on a thread of its own: each change is a
/// message. A thread, not the runtime's blocking pool, so a stop never waits
/// for its wait (the daemon's runtime gives blocking work 500 ms at a stop);
/// it holds only the watch, never the store, and ends at the first wake
/// after the tender is gone.
fn watch(dir: &Path) -> std::io::Result<tokio::sync::mpsc::Receiver<()>> {
    let mut waker = Waker::new(dir)?;
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    std::thread::Builder::new()
        .name("durability-watch".into())
        .spawn(move || loop {
            match waker.wait(BACKSTOP) {
                // Full: a change is waiting already, which this one joins.
                Ok(Wake::Changed) => {
                    if let Err(tokio::sync::mpsc::error::TrySendError::Closed(())) = tx.try_send(())
                    {
                        return;
                    }
                }
                Ok(_) if tx.is_closed() => return,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "durability: the WAL's watch failed; ending it");
                    return;
                }
            }
        })?;
    Ok(rx)
}

/// Ship, then wait for the WAL to change (or the backstop), and again.
async fn follow(mut shipper: Shipper, paths: &Paths, tuning: &Tuning, alive: &impl Fn() -> bool) {
    let mut changes = match watch(&paths.wal) {
        Ok(rx) => rx,
        Err(e) => {
            tracing::error!(error = %e, "durability: cannot watch the WAL; stopped");
            return;
        }
    };
    let mut backoff = tuning.settle;
    while alive() {
        match shipper.pass().await {
            Ok(()) => backoff = tuning.settle,
            Err(halt) => {
                if !after_failure(halt, &mut backoff).await {
                    return;
                }
                continue;
            }
        }
        match tokio::time::timeout(BACKSTOP, changes.recv()).await {
            Ok(Some(())) => {
                shipper.changed(now_unix_ms());
                tokio::time::sleep(tuning.settle).await;
            }
            Ok(None) => return,
            Err(_) => {}
        }
    }
}

impl crate::Core {
    /// The durability tender after serving, for the account that ships
    /// (`durability = true`). It holds the core only weakly: its rows and
    /// measures upgrade for a moment, and it ends once the core is gone.
    pub fn tend_durability_after_serving(self: &Arc<Self>) {
        let Some(aws) = self.tools.aws.clone() else {
            return;
        };
        let Some(account) = aws.accounts().find(|a| a.cfg.durability).cloned() else {
            return;
        };
        let paths = Paths::for_store(self.store.dir());
        let weak = Arc::downgrade(self);
        let ledger = {
            let core = weak.clone();
            Arc::new(move |row: LedgerRow| {
                let Some(c) = core.upgrade() else { return };
                if let Err(e) = blocking(|| c.store.append_ledger(&row)) {
                    tracing::warn!(error = %e, "ledger append failed");
                }
            }) as Arc<dyn Fn(LedgerRow) + Send + Sync>
        };
        let measure = {
            let core = weak.clone();
            Arc::new(move |m: Measure| {
                if let Some(c) = core.upgrade() {
                    c.telemetry().record_durability(m);
                }
            }) as Arc<dyn Fn(Measure) + Send + Sync>
        };
        let hooks = Hooks {
            ledger: Some(ledger),
            measure: Some(measure),
        };
        let alive = move || weak.strong_count() > 0;
        tokio::spawn(run(account, paths, Tuning::default(), hooks, alive));
    }
}
