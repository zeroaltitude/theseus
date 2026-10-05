//! `theseusd restore --from s3://<bucket>/durability/<deployment>/` (AWS
//! design §5, step 16): a store rebuilt from what the durability tender
//! shipped. Refused while a daemon serves the store, before anything is
//! fetched. The bucket names its account, which the config binds with
//! `durability = true`; the reads go in that account's restore session
//! ([`super::read`]). The rows name each segment and blob, which are fetched
//! into a staging directory beside the store, laid out as a store's
//! ([`super::fetch`]), and the local restore (`crate::restore`), unchanged,
//! restores from it: its open checks every frame, cuts a torn tail, and
//! refuses rot a synced mark proves. The staging copy is removed after.
//!
//! **After it** ([`seed`]): when the config's deployment is the restored
//! one, the tender's cursor is seeded at the last restored frame, so the
//! next start ships only what comes after it (the `store.restored` row
//! first), never the whole store again. A tender cursor that was beside the
//! store before is moved aside, never deleted.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use theseus_follow::{Cursor, WalFollower};
use theseus_store::wal;

use super::bucket;
use super::cursor::{self, Paths, Saved};
use super::fetch::{self, Fetch, Rows, Source};
use super::read::{self, Reader};
use crate::aws::{Account, Aws};
use crate::restore::RestoreReport;

/// A durability prefix's URL, read: its bucket and deployment.
pub fn parse_url(url: &str) -> Result<(String, String)> {
    let shape = "the source is s3://<bucket>/durability/<deployment>/";
    let Some(rest) = url.strip_prefix("s3://") else {
        bail!("{url:?} is not an s3:// URL: {shape}");
    };
    let (bucket, path) = rest.split_once('/').unwrap_or((rest, ""));
    let deployment = path
        .strip_prefix("durability/")
        .map(|d| d.strip_suffix('/').unwrap_or(d))
        .unwrap_or_default();
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+=,.@_-".contains(c))
    };
    if !ok(bucket) || !ok(deployment) {
        bail!("{url:?} names no deployment's prefix: {shape}");
    }
    Ok((bucket.to_string(), deployment.to_string()))
}

/// The account whose foundation bucket is `name`, as the config binds it
/// with `durability = true`.
pub fn account_for<'a>(aws: &'a Aws, name: &str) -> Result<&'a Arc<Account>> {
    let Some(a) = aws
        .accounts()
        .find(|a| bucket(&a.id, &a.cfg.region) == name)
    else {
        bail!(
            "no bound account's durability bucket is {name}: the config's [aws.accounts.<id>] \
             each ship to theseus-<id>-<region> ({})",
            aws.accounts()
                .map(|a| bucket(&a.id, &a.cfg.region))
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    if !a.cfg.durability {
        bail!(
            "account {}'s table in the config does not say durability = true, so the restore \
             does not read its bucket",
            a.id
        );
    }
    Ok(a)
}

/// What a restore from S3 did.
#[derive(Debug, Clone, Serialize)]
pub struct S3Report {
    pub url: String,
    pub account: String,
    pub deployment: String,
    pub fetch: Fetch,
    pub restore: RestoreReport,
    /// The config's own deployment is the restored one: a daemon started on
    /// it ships into the restored prefix.
    pub same_deployment: bool,
    /// The tender's cursor, seeded at this position (only with the same
    /// deployment).
    pub seeded_at: Option<u64>,
    /// Where a tender cursor that was beside the store went.
    pub cursor_aside: Option<String>,
}

/// Seams a test sets: how many rows a `Query` page asks for, and the most
/// bytes one `GetObject` reads.
#[derive(Clone, Debug, Default)]
pub struct Knobs {
    pub page: Option<u32>,
    pub chunk: Option<u64>,
}

/// Restore the store in `state_dir` from `url`. Refused while a daemon
/// answers on `socket`, and before anything is fetched when a store is in
/// place and `force` is not given.
pub async fn from_s3(
    aws: &Aws,
    url: &str,
    state_dir: &Path,
    socket: &Path,
    force: bool,
    knobs: &Knobs,
) -> Result<S3Report> {
    let (bucket_name, deployment) = parse_url(url)?;
    if tokio::net::UnixStream::connect(socket).await.is_ok() {
        bail!(
            "a theseusd is serving on {}; stop it first (`theseus shutdown`): the store is \
             single-process, and nothing was fetched",
            socket.display()
        );
    }
    let account = account_for(aws, &bucket_name)?.clone();
    let target = state_dir.join("store");
    let occupied = std::fs::read_dir(&target).is_ok_and(|mut d| d.next().is_some());
    if occupied && !force {
        bail!(
            "{} already holds a store; pass --force to move it aside (it is kept, never \
             deleted); nothing was fetched",
            target.display()
        );
    }
    let creds = read::session(&account, &deployment)
        .await
        .map_err(anyhow::Error::msg)
        .context("the restore's session")?;
    let reader = Reader {
        account: account.clone(),
        bucket: bucket_name,
        region: account.cfg.region.clone(),
        creds,
        chunk: knobs.chunk.unwrap_or(read::CHUNK),
        page: knobs.page,
    };
    let wal_rows = reader
        .query(&format!("{deployment}#wal"))
        .await
        .map_err(anyhow::Error::msg)?;
    let blob_rows = reader
        .query(&format!("{deployment}#blob"))
        .await
        .map_err(anyhow::Error::msg)?;
    let rows = Rows::read(&wal_rows, &blob_rows);

    std::fs::create_dir_all(state_dir)?;
    let staging = state_dir.join(format!("store.from-s3-{}", theseus_protocol::now_unix_ms()));
    let fetched = fetch::fetch(&reader, &rows, &staging).await;
    let fetched = match fetched {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            bail!("{e}; nothing was restored");
        }
    };
    let (from, dir) = (staging.clone(), state_dir.to_path_buf());
    let local = tokio::task::spawn_blocking(move || crate::restore::restore(&from, &dir, force))
        .await
        .context("the local restore's task")?;
    // The staging copy is S3's, fetched again at will: removed either way.
    let _ = std::fs::remove_dir_all(&staging);
    let restore = local?;

    let same_deployment = account.cfg.deployment() == deployment;
    let paths = Paths::for_store(&target);
    let cursor_aside = aside(&paths, state_dir)?;
    let seeded_at = if same_deployment {
        let (paths, f) = (paths.clone(), fetched.clone());
        let last = restore.last_position;
        tokio::task::spawn_blocking(move || seed(&paths, last, &f))
            .await
            .context("seeding the tender's cursor")?
            .context("seeding the tender's cursor")?
    } else {
        None
    };
    Ok(S3Report {
        url: url.to_string(),
        account: account.id.clone(),
        deployment,
        fetch: fetched,
        restore,
        same_deployment,
        seeded_at,
        cursor_aside,
    })
}

/// The tender's directory beside the store, moved aside: it was the
/// replaced store's.
fn aside(paths: &Paths, state_dir: &Path) -> Result<Option<String>> {
    if !paths.state.exists() {
        return Ok(None);
    }
    let name = paths
        .state
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "durability".into());
    let to: PathBuf = state_dir.join(format!(
        "{name}.before-restore-{}",
        theseus_protocol::now_unix_ms()
    ));
    std::fs::rename(&paths.state, &to)
        .with_context(|| format!("moving {} aside", paths.state.display()))?;
    Ok(Some(to.display().to_string()))
}

/// The cursor just past the last frame whose positions are all at or
/// before `last`: the restored history, before the `store.restored` row.
fn cursor_through(wal_dir: &Path, last: u64) -> std::io::Result<Cursor> {
    let err = |e: theseus_follow::FollowError| std::io::Error::other(e.to_string());
    let mut c = Cursor::start();
    let mut f = WalFollower::open(wal_dir, c.clone()).map_err(err)?;
    loop {
        let b = f.read(8 << 20).map_err(err)?;
        if b.is_empty() {
            return Ok(c);
        }
        if b.records.last().map_or(0, |r| r.position) <= last {
            c = f.cursor().clone();
            continue;
        }
        // Past it in this batch: again from the batch's start, a frame at a
        // time (a read takes at least one whole frame).
        let mut f = WalFollower::open(wal_dir, c.clone()).map_err(err)?;
        loop {
            let b = f.read(1).map_err(err)?;
            if b.is_empty() || b.records.first().map_or(0, |r| r.position) > last {
                return Ok(c);
            }
            c = f.cursor().clone();
        }
    }
}

/// Seed the tender's cursor beside the restored store: shipped through the
/// last restored frame (its bytes, its sealed segments, and its rows), and
/// every restored blob, so the next start ships what follows and nothing
/// again. The cursor's segment is shipped as tails from there, unless the
/// restore's own row went into a new segment, sealing it: then it ships
/// whole, unless S3 holds it whole already. The position seeded at.
pub fn seed(paths: &Paths, last: u64, fetched: &Fetch) -> std::io::Result<Option<u64>> {
    let c = cursor_through(&paths.wal, last)?;
    if c.segment == 0 {
        return Ok(None);
    }
    let rotated = wal::segment_path(&paths.wal, c.segment + 1).exists();
    // Its sealed object in S3 holds every byte of it the restore read: only
    // then is it shipped already (with tails after the object, it is not
    // whole there, and ships whole again).
    let whole = fetched
        .segments
        .iter()
        .any(|s| s.segment == c.segment && s.source == Source::Object);
    let saved = Saved {
        sealed_to: if rotated && whole {
            c.segment
        } else {
            c.segment - 1
        },
        tail: (!rotated).then_some((c.segment, c.offset)),
        rows_to: c.position,
        follow: c.clone(),
        ..Saved::default()
    };
    cursor::save(paths, &saved)?;
    let mut digests: Vec<String> = std::fs::read_dir(&paths.blobs)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| super::is_digest(n))
                .collect()
        })
        .unwrap_or_default();
    digests.sort();
    for d in &digests {
        cursor::mark_blob(paths, d)?;
    }
    Ok(Some(c.position))
}

/// The restore's lines, as `theseusd restore` prints them before the local
/// restore's own.
pub fn lines(r: &S3Report) -> String {
    let f = &r.fetch;
    let mut text = format!(
        "fetched {} (account {}, in the session theseus-{}): {} segment(s), {} blob(s), {} \
         bytes\n",
        r.url,
        r.account,
        read::RESTORE,
        f.segments.len(),
        f.blobs,
        f.bytes
    );
    let words: Vec<String> = f
        .segments
        .iter()
        .map(|s| match s.source {
            Source::Object => format!("segment {} from its object", s.segment),
            Source::Tails { count } => format!("segment {} from {count} tail(s)", s.segment),
            Source::ObjectAndTails { count } => format!(
                "segment {} from its object and {count} tail(s) after it",
                s.segment
            ),
        })
        .collect();
    text.push_str(&format!("  {}\n", words.join(", ")));
    for s in &f.segments {
        if let Some(u) = &s.unjoined {
            text.push_str(&format!("  segment {}: {u}\n", s.segment));
        }
    }
    if let Some(g) = &f.gap {
        text.push_str(&format!("  a gap, not filled: {g}\n"));
    }
    if !f.blobs_missing.is_empty() {
        text.push_str(&format!(
            "  {} blob(s) a row names are not in S3, so not restored: {}\n",
            f.blobs_missing.len(),
            f.blobs_missing.join(", ")
        ));
    }
    for u in &f.unread {
        text.push_str(&format!("  not read: {u}\n"));
    }
    text
}

/// What follows the local restore's lines: the tender's cursor, and the
/// warning when the config ships into the restored prefix.
pub fn after_lines(r: &S3Report) -> String {
    let mut text = String::new();
    if let Some(a) = &r.cursor_aside {
        text.push_str(&format!(
            "the durability tender's cursor that was beside the store is kept at {a}\n"
        ));
    }
    if r.same_deployment {
        text.push_str(&format!(
            "warning: the config's deployment is {}, the one restored: a daemon started on this \
             store ships into {} (its cursor is seeded at position {}, so it ships only what \
             comes after; the store.restored row first). Give this config a deployment of its \
             own to keep it out of that prefix.\n",
            r.deployment,
            r.url,
            r.seeded_at.map_or("(none)".into(), |p| p.to_string())
        ));
    } else {
        text.push_str(&format!(
            "the config's deployment is not {}: a daemon started on this store ships it whole \
             into its own prefix\n",
            r.deployment
        ));
    }
    text
}
