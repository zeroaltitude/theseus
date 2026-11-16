//! The pipeline's own fetch: the packages a branch's lock files name, from the
//! registries, each checked against the checksum the lock records, into a
//! cache its gate reads. This is the daemon's code. No build script, no code of
//! the branch and no code of a downloaded package runs here: a package is
//! downloaded, hashed, and written out (a crate unpacked with
//! `theseus_files::archive::unpack_tar_gz`, which writes plain files only; an
//! npm tarball kept whole, as npm's cache holds it). Nothing is installed.
//!
//! A package the registry does not have, or serves with other bytes than the
//! lock names, is a [`Fail::Refused`] with words the builder reads. A registry
//! that does not answer is [`Fail::Infra`]: the machine's, not the change's.
//!
//! Every request goes to one of the two endpoints ([`super::hosts::Endpoints`]),
//! and a redirect is never followed, so no answer can send the fetch to a host
//! that is not on the list.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use sha2::{Digest, Sha256, Sha384, Sha512};

use super::hosts::Endpoints;
use super::lock::Pkg;

/// How many downloads run at once.
const PARALLEL: usize = 8;
/// The largest `.crate` file taken (crates.io's own limit is 10 MiB, raised by hand for a few).
const MAX_CRATE_BYTES: u64 = 128 * 1024 * 1024;
/// The most one crate may unpack to.
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
/// The largest npm tarball taken.
const MAX_TARBALL_BYTES: u64 = 256 * 1024 * 1024;
/// Tries at one download before a registry that errs is called down.
const TRIES: u32 = 3;

/// Why a fetch stopped.
#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    /// The change's fault, in words its builder reads.
    Refused(String),
    /// The machine's or the registry's: nothing is wrong with the change.
    Infra(String),
}

/// What a fetch did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Packages downloaded.
    pub fetched: usize,
    /// Packages already there, with the checksum the lock names.
    pub kept: usize,
}

impl Stats {
    fn add(&mut self, fetched: bool) {
        if fetched {
            self.fetched += 1;
        } else {
            self.kept += 1;
        }
    }
}

/// Downloads from the two endpoints.
pub struct Fetcher {
    client: reqwest::Client,
    ends: Endpoints,
}

impl Fetcher {
    pub fn new(ends: Endpoints) -> Result<Self, Fail> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(300))
            .user_agent("theseus-self-build")
            .build()
            .map_err(|e| Fail::Infra(format!("an HTTP client could not be made: {e}")))?;
        Ok(Self { client, ends })
    }

    /// `url`'s body, at most `cap` bytes, retried when the registry errs.
    async fn get(&self, url: &str, cap: u64) -> Result<Vec<u8>, Fail> {
        let mut last = String::new();
        for attempt in 0..TRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(250 << attempt)).await;
            }
            let mut resp = match self.client.get(url).send().await {
                Ok(r) => r,
                Err(e) => {
                    last = format!("{url}: {e}");
                    continue;
                }
            };
            let status = resp.status();
            if status.is_redirection() {
                return Err(Fail::Refused(format!(
                    "{url} answered with a redirect, which the fetch does not follow: a package is taken from the registry's own address only"
                )));
            }
            if matches!(status.as_u16(), 404 | 410) {
                return Err(Fail::Refused(format!("the registry has no {url}")));
            }
            if status.is_server_error() || status.as_u16() == 429 {
                last = format!("{url}: the registry answered {status}");
                continue;
            }
            if !status.is_success() {
                return Err(Fail::Refused(format!(
                    "{url}: the registry answered {status}"
                )));
            }
            let mut body = Vec::new();
            loop {
                match resp.chunk().await {
                    Ok(Some(c)) => {
                        if body.len() as u64 + c.len() as u64 > cap {
                            return Err(Fail::Refused(format!(
                                "{url} is larger than the {cap} bytes a package may be"
                            )));
                        }
                        body.extend_from_slice(&c);
                    }
                    Ok(None) => return Ok(body),
                    Err(e) => {
                        last = format!("{url}: {e}");
                        break;
                    }
                }
            }
        }
        Err(Fail::Infra(format!(
            "the registry did not answer after {TRIES} tries ({last})"
        )))
    }

    /// Every crate of `pkgs` as a directory under `vendor` (cargo's directory
    /// source), the ones already there with the checksum kept, and any other
    /// directory removed.
    pub async fn cargo(&self, vendor: &Path, pkgs: &[Pkg]) -> Result<Stats, Fail> {
        let io = |what: &str, e: std::io::Error| Fail::Infra(format!("{what}: {e}"));
        std::fs::create_dir_all(vendor)
            .map_err(|e| io("the package cache could not be made", e))?;
        let wanted: Vec<String> = pkgs
            .iter()
            .map(|p| format!("{}-{}", p.name, p.version))
            .collect();
        for e in
            std::fs::read_dir(vendor).map_err(|e| io("the package cache could not be read", e))?
        {
            let path = e
                .map_err(|e| io("the package cache could not be read", e))?
                .path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !wanted.iter().any(|w| w == name) {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| io("a stale package could not be removed", e))?;
            }
        }
        let mut stats = Stats::default();
        // Not `stream::iter(..).map(..).buffer_unordered(..)`: a closure that returns a future
        // borrowing its argument makes every future that awaits this one fail `Send`.
        let mut running = FuturesUnordered::new();
        let mut next = pkgs.iter();
        loop {
            while running.len() < PARALLEL {
                let Some(p) = next.next() else { break };
                running.push(self.one_crate(vendor, p));
            }
            let Some(done) = running.next().await else {
                return Ok(stats);
            };
            stats.add(done?);
        }
    }

    /// One crate: true when it was downloaded.
    async fn one_crate(&self, vendor: &Path, p: &Pkg) -> Result<bool, Fail> {
        let dir_name = format!("{}-{}", p.name, p.version);
        let dir = vendor.join(&dir_name);
        let marker = dir.join(".cargo-checksum.json");
        let kept = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .is_some_and(|v| v.get("package").and_then(|c| c.as_str()) == Some(p.check.as_str()));
        if kept {
            return Ok(false);
        }
        let url = format!("{}/{}/{dir_name}.crate", self.ends.crates, p.name);
        let bytes = self.get(&url, MAX_CRATE_BYTES).await?;
        let got = hex::encode(Sha256::digest(&bytes));
        if got != p.check {
            return Err(Fail::Refused(format!(
                "the registry served {} {} with a sha256 of {got}, and the lock file names {}: it is not the package the lock file asked for",
                p.name, p.version, p.check
            )));
        }
        let (vendor, check) = (vendor.to_path_buf(), p.check.clone());
        tokio::task::spawn_blocking(move || unpack_crate(&vendor, &dir_name, &bytes, &check))
            .await
            .map_err(|e| Fail::Infra(format!("an unpack did not finish: {e}")))??;
        Ok(true)
    }

    /// Every tarball of `pkgs` in npm's own cache layout under `cache`, so an
    /// `npm ci --offline` with that cache finds each by its integrity.
    pub async fn npm(&self, cache: &Path, pkgs: &[Pkg]) -> Result<Stats, Fail> {
        let mut seen = std::collections::BTreeSet::new();
        let each: Vec<&Pkg> = pkgs
            .iter()
            .filter(|p| seen.insert(p.check.as_str()))
            .collect();
        let mut stats = Stats::default();
        let mut running = FuturesUnordered::new();
        let mut next = each.into_iter();
        loop {
            while running.len() < PARALLEL {
                let Some(p) = next.next() else { break };
                running.push(self.one_tarball(cache, p));
            }
            let Some(done) = running.next().await else {
                return Ok(stats);
            };
            stats.add(done?);
        }
    }

    /// One tarball: true when it was downloaded.
    async fn one_tarball(&self, cache: &Path, p: &Pkg) -> Result<bool, Fail> {
        let at = content_path(cache, &p.check).ok_or_else(|| {
            Fail::Refused(format!(
                "{} {}: its integrity is not one that can be checked",
                p.name, p.version
            ))
        })?;
        if std::fs::read(&at).is_ok_and(|b| integrity_of(&p.check, &b).is_some_and(|ok| ok)) {
            return Ok(false);
        }
        let bytes = self.get(&p.url, MAX_TARBALL_BYTES).await?;
        if integrity_of(&p.check, &bytes) != Some(true) {
            return Err(Fail::Refused(format!(
                "the registry served {} {} with other bytes than the integrity its lock file names ({}): it is not the package the lock file asked for",
                p.name, p.version, p.check
            )));
        }
        let io = |e: std::io::Error| Fail::Infra(format!("{}: {e}", at.display()));
        std::fs::create_dir_all(at.parent().unwrap_or(cache)).map_err(io)?;
        let tmp = at.with_extension("part");
        std::fs::write(&tmp, &bytes).map_err(io)?;
        std::fs::rename(&tmp, &at).map_err(io)?;
        Ok(true)
    }
}

/// Unpack one verified crate as `<vendor>/<dir_name>` with the marker cargo's
/// directory source wants: the package's checksum, and no per-file ones (an
/// empty list is a list cargo accepts; the whole file's hash was checked).
fn unpack_crate(vendor: &Path, dir_name: &str, bytes: &[u8], check: &str) -> Result<(), Fail> {
    let io = |e: std::io::Error| Fail::Infra(format!("{dir_name}: {e}"));
    let part: PathBuf = vendor.join(format!(".part-{dir_name}"));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).map_err(io)?;
    let done = (|| {
        theseus_files::archive::unpack_tar_gz(bytes, &part, dir_name, MAX_UNPACKED_BYTES).map_err(
            |e| Fail::Refused(format!("{dir_name}'s package could not be unpacked: {e}")),
        )?;
        let unpacked = part.join(dir_name);
        let marker = serde_json::json!({"files": {}, "package": check});
        std::fs::write(unpacked.join(".cargo-checksum.json"), marker.to_string()).map_err(io)?;
        let to = vendor.join(dir_name);
        if to.exists() {
            std::fs::remove_dir_all(&to).map_err(io)?;
        }
        std::fs::rename(&unpacked, &to).map_err(io)
    })();
    let _ = std::fs::remove_dir_all(&part);
    done
}

/// Where npm's cache keeps the content whose integrity is `integrity`
/// (`<cache>/_cacache/content-v2/<algo>/<hex[0..2]>/<hex[2..4]>/<hex[4..]>`),
/// or none when the integrity is not a base64 digest of a known algorithm.
fn content_path(cache: &Path, integrity: &str) -> Option<PathBuf> {
    let (algo, b64) = integrity.split_once('-')?;
    let digest = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let want = match algo {
        "sha512" => 64,
        "sha384" => 48,
        "sha256" => 32,
        _ => return None,
    };
    (digest.len() == want).then_some(())?;
    let h = hex::encode(digest);
    Some(
        cache
            .join("_cacache/content-v2")
            .join(algo)
            .join(&h[..2])
            .join(&h[2..4])
            .join(&h[4..]),
    )
}

/// Whether `bytes` hash to `integrity`, or none when it names no known algorithm.
fn integrity_of(integrity: &str, bytes: &[u8]) -> Option<bool> {
    let (algo, b64) = integrity.split_once('-')?;
    let want = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let got: Vec<u8> = match algo {
        "sha512" => Sha512::digest(bytes).to_vec(),
        "sha384" => Sha384::digest(bytes).to_vec(),
        "sha256" => Sha256::digest(bytes).to_vec(),
        _ => return None,
    };
    Some(got == want)
}
