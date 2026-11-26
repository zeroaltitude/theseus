//! What a server's sign-in gave, kept (issue 55): one file per server,
//! `<store>/mcp-oauth/<name>.json`, mode 0600, in a directory of the daemon's
//! own at 0700, written by the daemon alone and whole (a file of its own made
//! new, synced, renamed over the old one, the directory synced), as the
//! owner's second proof is (`owner_proof/file.rs`). It is inside the store
//! directory, which the floor holds whole, as `config_change`'s directory is:
//! no tool reads or writes it and no L1 view shows it. Read at the server's
//! first start and checked for meaning before it is used; renewed before the
//! access token runs out and once on a 401, and written back whole,
//! unattended, when a refresh rotates the refresh token. Every change of it,
//! in memory and on disk, is made under one lock: a sign-in, a renewal and a
//! sign-out never interleave.
//!
//! Every value (the access and refresh tokens, a client secret, a code, a
//! verifier) is put on the secret board where it is first read, and stays
//! there for the daemon's life ([`Held`]), so the scrubber knows each one,
//! a spent one included. A refresh the server refuses for good loses the
//! sign-in: from then no token is given and nothing is renewed until the
//! owner signs in again, and the board leaves the server `needs sign-in`.

use std::collections::HashSet;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use futures_util::future::BoxFuture;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use theseus_mcp::oauth::exchange::{self, Renewal, Tokens};
use theseus_mcp::oauth::register::ClientId;
use theseus_mcp::oauth::{discover, same_origin, Reach, Rule, MAX_EXPIRES_SECS};
use tokio::sync::watch;
use zeroize::Zeroize;

use crate::secrets::{Secret, SecretBoard};

/// The directory of the kept sign-ins, inside the store directory.
pub fn dir(store_dir: &Path) -> PathBuf {
    store_dir.join("mcp-oauth")
}

/// A server's file in `dir`.
pub fn file(dir: &Path, server: &str) -> PathBuf {
    dir.join(format!("{server}.json"))
}

/// An access token this close to its end is renewed before it is sent.
pub const RENEW_BEFORE: Duration = Duration::from_secs(60);

/// The largest kept file read.
const MAX_FILE: u64 = 64 * 1024;

/// The words for a kept file that is not used, whatever is wrong with it:
/// none of its contents is shown.
const NOT_VALID: &str = "the kept sign-in is not valid, so it is not used";

/// Where a sign-in's calls may go: the web fetch's own rule (`web::net`), so
/// an address a server names is reached only at a public address, and only
/// over `https`. A daemon's tests build theirs with [`rule_for_tests`].
pub fn rule() -> Rule {
    #[cfg(test)]
    {
        rule_for_tests()
    }
    #[cfg(not(test))]
    {
        daemon_rule()
    }
}

/// The daemon's rule: the web fetch's checking resolver and its test of a
/// host, and no `http` at all.
pub fn daemon_rule() -> Rule {
    Rule {
        resolver: Arc::new(crate::web::net::Dns::checked()),
        private_host: crate::web::net::private_host,
        refused: |e| crate::web::net::refused_in(e).is_some(),
        plaintext: |_| false,
        protectable: crate::scrub::protectable,
    }
}

/// Tests' rule, which product code cannot build: a test's server on this
/// machine over `http`, and no address checked.
#[cfg(test)]
pub(crate) fn rule_for_tests() -> Rule {
    Rule {
        resolver: Arc::new(crate::web::net::Dns::default()),
        private_host: |_| None,
        refused: |_| false,
        plaintext: theseus_mcp::oauth::is_loopback,
        protectable: crate::scrub::protectable,
    }
}

/// Whether `url` may be a server that signs in: `https`, or, in a daemon's
/// tests alone, a test's server on this machine.
pub fn signs_in_at(url: &Url) -> bool {
    signs_in_at_by(url, cfg!(test))
}

/// [`signs_in_at`], with whether this is a daemon's test said.
fn signs_in_at_by(url: &Url, tests: bool) -> bool {
    url.scheme() == "https"
        || (tests && url.scheme() == "http" && theseus_mcp::oauth::is_loopback(url))
}

/// Every value of the daemon's sign-ins, on the secret board for the
/// daemon's life: each under a name of its own, never reused, so no later
/// value takes an earlier one's place and the scrubber forgets none.
pub(crate) struct Held {
    server: String,
    secrets: Arc<SecretBoard>,
    /// The digests of the values held, so a value is held once.
    seen: std::sync::Mutex<HashSet<[u8; 32]>>,
}

/// The next name's number, across every server and every sign-in.
static NEXT_HELD: AtomicU64 = AtomicU64::new(0);

impl Held {
    pub(crate) fn new(server: &str, secrets: Arc<SecretBoard>) -> Arc<Self> {
        Arc::new(Self {
            server: server.to_string(),
            secrets,
            seen: std::sync::Mutex::default(),
        })
    }

    /// Hold `value`, of kind `what`: the scrubber knows it from now on.
    pub(crate) fn hold(&self, what: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        let new = self
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(digest);
        if new {
            let n = NEXT_HELD.fetch_add(1, Ordering::SeqCst);
            self.secrets.hold(
                &format!("mcp-oauth:{}:{what}:{n}", self.server),
                Secret::new(value.to_string()),
            );
        }
    }
}

/// What one sign-in gave, as the file keeps it.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Kept {
    /// The server's address this sign-in is for: a server whose address
    /// changed signs in again.
    pub url: String,
    pub resource: String,
    pub issuer: String,
    pub token_endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,
    /// The origins of its endpoints that are not the issuer's, each shown
    /// to the owner and agreed to at the sign-in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elsewhere: Vec<String>,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_basic: bool,
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When the access token runs out, if the server said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    pub signed_in_at_ms: u64,
}

impl Kept {
    pub fn client(&self) -> ClientId {
        ClientId {
            id: self.client_id.clone(),
            secret: self.client_secret.clone().map(zeroize::Zeroizing::new),
            basic: self.client_basic,
        }
    }

    /// Take `t`, a renewal's answer: a refresh token it gives replaces the
    /// one kept, and none keeps it. Its expiry is bounded, and the sum cannot
    /// overflow.
    pub fn renewed(&mut self, t: &Tokens, now_ms: u64) {
        self.access_token = t.access.to_string();
        if let Some(r) = &t.refresh {
            self.refresh_token = Some(r.to_string());
        }
        self.expires_at_ms = t
            .expires_in
            .map(|s| now_ms.saturating_add(s.min(MAX_EXPIRES_SECS).saturating_mul(1000)));
    }

    /// Whether the access token runs out within [`RENEW_BEFORE`] of `now_ms`.
    fn due(&self, now_ms: u64) -> bool {
        self.expires_at_ms
            .is_some_and(|at| at <= now_ms.saturating_add(RENEW_BEFORE.as_millis() as u64))
    }

    /// What it shows, in the clear, of its sign-in: the address and host,
    /// the issuer, every endpoint and its host, the places agreed to, and
    /// the client id. A value it holds must equal none of them (`doors.rs`).
    pub fn public(&self) -> Vec<String> {
        let mut v = super::doors::address(&self.url);
        v.extend(super::doors::address(&self.issuer));
        v.extend(super::doors::address(&self.token_endpoint));
        if let Some(r) = &self.revocation_endpoint {
            v.extend(super::doors::address(r));
        }
        v.extend(self.elsewhere.iter().cloned());
        v.push(self.client_id.clone());
        v
    }

    /// Whether a value it holds is already shown: by the board (`shown`), or
    /// by its own sign-in (`public`).
    fn shows_a_value(&self, shown: &super::doors::Shown) -> bool {
        let values: Vec<&str> = self.values().into_iter().map(|(_, v)| v).collect();
        let mut texts = shown();
        texts.extend(self.public());
        super::doors::already_shown(&values, &texts)
    }

    /// Every value in it, by what it is.
    fn values(&self) -> Vec<(&'static str, &str)> {
        let mut v = vec![("access", self.access_token.as_str())];
        if let Some(r) = &self.refresh_token {
            v.push(("refresh", r));
        }
        if let Some(s) = &self.client_secret {
            v.push(("client", s));
        }
        v
    }

    /// Whether it may be used for the server at `url`, read by `reach`'s
    /// rule: for this address; its resource this server; its issuer and
    /// endpoints addresses the rule reaches, the endpoints on the issuer's
    /// origin or one the owner agreed to; its values ones the scrubber keeps from
    /// view; its expiry within a year of now.
    fn check(&self, url: &str, reach: &Reach, now_ms: u64) -> Result<(), &'static str> {
        if self.url != url {
            return Err("the kept sign-in is for another address, so it is not used");
        }
        let server = Url::parse(url).map_err(|_| NOT_VALID)?;
        if !discover::names_this_server(&self.resource, &server) {
            return Err(NOT_VALID);
        }
        let issuer = reach
            .endpoint(&self.issuer, "issuer")
            .map_err(|_| NOT_VALID)?;
        let on_its_own = |text: &str| {
            reach.endpoint(text, "endpoint").ok().is_some_and(|u| {
                same_origin(&u, &issuer)
                    || self.elsewhere.contains(&u.origin().ascii_serialization())
            })
        };
        if !on_its_own(&self.token_endpoint)
            || self
                .revocation_endpoint
                .as_deref()
                .is_some_and(|r| !on_its_own(r))
        {
            return Err(NOT_VALID);
        }
        let short = |v: &Option<String>| v.as_ref().is_some_and(|s| !crate::scrub::protectable(s));
        if self.client_id.is_empty()
            || self.client_id.len() > 512
            || !crate::scrub::protectable(&self.access_token)
            || short(&self.refresh_token)
            || short(&self.client_secret)
        {
            return Err(NOT_VALID);
        }
        let latest = now_ms.saturating_add(MAX_EXPIRES_SECS.saturating_mul(1000));
        if self.expires_at_ms.is_some_and(|at| at > latest) {
            return Err(NOT_VALID);
        }
        Ok(())
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(r) = &mut self.refresh_token {
            r.zeroize();
        }
        if let Some(s) = &mut self.client_secret {
            s.zeroize();
        }
    }
}

/// Never prints a value, nor what a server chose.
impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kept")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

/// The file at `path`: `Ok(None)` when there is none; an error, without its
/// contents, when it is there and cannot be trusted (it is not a file of its
/// own, another user may read it, it is too large, it does not parse).
pub fn read(path: &Path) -> Result<Option<Kept>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("the kept sign-in cannot be read".into()),
    };
    if !meta.file_type().is_file() {
        return Err("the kept sign-in is not a file of its own, so it is not used".into());
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err("the kept sign-in is readable by others, so it is not used".into());
    }
    if meta.len() > MAX_FILE {
        return Err("the kept sign-in is too large, so it is not used".into());
    }
    let mut bytes = Vec::new();
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .and_then(|f| f.take(MAX_FILE).read_to_end(&mut bytes))
        .map_err(|_| "the kept sign-in cannot be read")?;
    let kept = serde_json::from_slice::<Kept>(&bytes)
        .map_err(|_| "the kept sign-in does not parse".to_string());
    bytes.zeroize();
    kept.map(Some)
}

/// `dir`, a directory of the daemon's own: made at 0700 if it is not there;
/// refused when it is not a directory (a link), or another user's; its mode
/// set to 0700.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() || meta.uid() != crate::peer::own_uid() {
        return Err(std::io::Error::other(
            "the sign-ins' directory is not a directory of the daemon's own",
        ));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Make what was renamed or removed in `dir` last a crash.
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_DIR_SYNC.with(std::cell::Cell::get) {
        return Err(std::io::Error::other(
            "a directory sync that failed (a test's)",
        ));
    }
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(test)]
thread_local! {
    /// Tests: the next directory sync fails, after a rename.
    pub(crate) static FAIL_DIR_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A write whose new file was already renamed into place when it failed (its
/// directory was not synced): the file may hold the new sign-in, and a start
/// may read it.
#[derive(Debug)]
pub struct MayBeWritten(pub std::io::Error);

impl std::fmt::Display for MayBeWritten {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the file was written, but not made sure to last ({}): it may already hold the new \
             sign-in",
            self.0
        )
    }
}

impl std::error::Error for MayBeWritten {}

/// Whether `e` is a write that failed after its rename ([`MayBeWritten`]).
pub fn may_be_written(e: &std::io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<MayBeWritten>())
}

/// Write `kept` whole: a file made new beside `path` (never one that was
/// there, never through a link) at 0600, synced, renamed over `path`, and
/// the directory synced; a crash leaves the old file or the new one.
pub fn write(path: &Path, kept: &Kept) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a kept sign-in needs a directory"))?;
    private_dir(dir)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| std::io::Error::other("a kept sign-in needs a name"))?;
    let new = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut bytes = serde_json::to_vec(kept)?;
    let wrote = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&new)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&new, path)?;
        sync_dir(dir).map_err(|e| std::io::Error::other(MayBeWritten(e)))
    })();
    bytes.zeroize();
    if wrote.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    wrote
}

/// Take the file away, and make that last a crash.
pub fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        Err(_) => Ok(()),
        Ok(()) => path.parent().map_or(Ok(()), sync_dir),
    }
}

fn now_ms() -> u64 {
    theseus_protocol::now_unix_ms()
}

/// A sign-in's state, under its one lock.
#[derive(Default)]
struct Slot {
    kept: Option<Kept>,
    /// Why the sign-in is lost (a refresh refused for good), while it is:
    /// no token is given and nothing is renewed until a new sign-in.
    lost: Option<String>,
}

/// One server's sign-in, as the board uses it: the kept values, the file
/// they are written to, and whether the sign-in is lost.
pub struct Signed {
    server: String,
    url: String,
    path: Option<PathBuf>,
    held: Arc<Held>,
    /// None when its client could not start: the sign-in is lost.
    reach: Option<Reach>,
    /// The network rule its calls, and its server's connection, keep.
    rule: Rule,
    slot: tokio::sync::Mutex<Slot>,
    /// The loss, for the board to wake on.
    lost: watch::Sender<Option<String>>,
    /// What the board shows, read at each door a value enters by
    /// (`doors.rs`): the kept file at start, and every renewal.
    shown: super::doors::Shown,
}

/// The sign-in kept at `path` for `server` at `url`, if it may be used: its
/// values held on the board first, whatever follows; not used when it is not
/// valid for this address, or when a value it holds is already shown
/// (`doors.rs`, the kept file being a door a value enters by at start).
fn kept_at(
    server: &str,
    url: &str,
    path: Option<&Path>,
    reach: Option<&Reach>,
    held: &Held,
    shown: &super::doors::Shown,
) -> Option<Kept> {
    let (k, reach) = match (read(path?), reach) {
        (Ok(Some(k)), Some(reach)) => (k, reach),
        (Err(why), _) => {
            tracing::warn!(server, why = %why, "an MCP server's kept sign-in was not used");
            return None;
        }
        _ => return None,
    };
    for (what, v) in k.values() {
        held.hold(what, v);
    }
    let why = match k.check(url, reach, now_ms()) {
        Ok(()) if k.shows_a_value(shown) => super::doors::SHOWN_WORDS,
        Ok(()) => return Some(k),
        Err(why) => why,
    };
    tracing::warn!(server, why, "an MCP server's kept sign-in was not used");
    None
}

impl Signed {
    /// The sign-in of `server` at `url`, read from `path` (none: in memory
    /// only), its calls by `rule`. A kept sign-in that is not valid for this
    /// address, or that holds a value the board already shows (`shown`), is
    /// not used, and none of it is shown.
    pub fn load(
        server: &str,
        url: &str,
        path: Option<PathBuf>,
        secrets: Arc<SecretBoard>,
        rule: Rule,
        shown: super::doors::Shown,
    ) -> Arc<Self> {
        let held = Held::new(server, secrets);
        let h = held.clone();
        let reach = Reach::new(rule.clone(), Arc::new(move |what, v| h.hold(what, v))).ok();
        let mut slot = Slot {
            kept: kept_at(server, url, path.as_deref(), reach.as_ref(), &held, &shown),
            ..Slot::default()
        };
        if reach.is_none() {
            slot.lost = Some("its sign-in's HTTP client could not start".into());
        }
        let lost = watch::Sender::new(slot.lost.clone());
        Arc::new(Self {
            server: server.into(),
            url: url.into(),
            path,
            held,
            reach,
            rule,
            slot: tokio::sync::Mutex::new(slot),
            lost,
            shown,
        })
    }

    /// Its calls; None when its client could not start.
    pub fn reach(&self) -> Option<&Reach> {
        self.reach.as_ref()
    }

    /// The network rule its server's own connection keeps.
    pub fn rule(&self) -> Rule {
        self.rule.clone()
    }

    /// Put `value` on the secret board as `what` of this server's: the
    /// scrubber knows it from now on, for the daemon's life.
    pub fn hold_value(&self, what: &str, value: &str) {
        self.held.hold(what, value);
    }

    /// Whether a sign-in is kept, and not lost.
    pub async fn signed_in(&self) -> bool {
        let slot = self.slot.lock().await;
        slot.kept.is_some() && slot.lost.is_none()
    }

    /// Why the sign-in is lost, while it is.
    pub fn lost(&self) -> Option<String> {
        self.lost.borrow().clone()
    }

    /// A receiver that wakes when the sign-in is lost.
    pub fn watch_lost(&self) -> watch::Receiver<Option<String>> {
        self.lost.subscribe()
    }

    /// A new sign-in, checked as a kept one is, then written whole; only once
    /// it is written does it replace what was kept, and the loss is
    /// forgotten. A sign-in that cannot be written is not used: the error
    /// says why, and what was kept before stays as it was.
    pub async fn put(&self, k: Kept) -> std::io::Result<()> {
        for (what, v) in k.values() {
            self.held.hold(what, v);
        }
        let reach = self
            .reach
            .as_ref()
            .ok_or_else(|| std::io::Error::other("its sign-in's HTTP client could not start"))?;
        k.check(&self.url, reach, now_ms())
            .map_err(std::io::Error::other)?;
        let mut slot = self.slot.lock().await;
        if let Some(p) = &self.path {
            write(p, &k)?;
        }
        slot.kept = Some(k);
        slot.lost = None;
        self.lost.send_replace(None);
        Ok(())
    }

    /// Forget the sign-in: the file removed, under the lock. What was kept is
    /// returned, for a revoke, with whether the file is gone.
    pub async fn forget(&self) -> (Option<Kept>, std::io::Result<()>) {
        let mut slot = self.slot.lock().await;
        let removed = self.path.as_deref().map_or(Ok(()), remove);
        let k = slot.kept.take();
        slot.lost = None;
        self.lost.send_replace(None);
        (k, removed)
    }

    /// Renew the kept sign-in with its refresh token: written back whole when
    /// it is. A refusal for good loses the sign-in; one that may pass leaves
    /// it. Nothing is renewed once it is lost.
    async fn renew(&self, slot: &mut Slot) -> bool {
        if slot.lost.is_some() {
            return false;
        }
        let Some(k) = slot.kept.as_ref() else {
            return false;
        };
        let Some(reach) = &self.reach else {
            return false;
        };
        let Some(refresh) = k.refresh_token.clone().map(zeroize::Zeroizing::new) else {
            self.lose(
                slot,
                "its token ran out or was refused, and it gave no way to renew it",
            );
            return false;
        };
        let Ok(endpoint) = Url::parse(&k.token_endpoint) else {
            self.lose(slot, NOT_VALID);
            return false;
        };
        match exchange::refresh(reach, &endpoint, &k.client(), &refresh, &k.resource).await {
            Renewal::Renewed(t) => {
                let mut next = k.clone();
                next.renewed(&t, now_ms());
                // A door (`doors.rs`): a renewed value already shown is not
                // kept, and the sign-in is lost for good. The exchange held
                // it on the board, so the scrubber masks it where it shows.
                if next.shows_a_value(&self.shown) {
                    self.lose(slot, super::doors::RENEWAL_SHOWN);
                    return false;
                }
                if let Some(p) = &self.path {
                    if let Err(e) = write(p, &next) {
                        tracing::warn!(server = %self.server, error = %e,
                            "an MCP server's renewed sign-in was not written; it is held in memory");
                    }
                }
                slot.kept = Some(next);
                tracing::info!(server = %self.server, "an MCP server's sign-in was renewed");
                true
            }
            Renewal::ForGood(why) => {
                self.lose(
                    slot,
                    &format!("its sign-in was refused when renewed ({why})"),
                );
                false
            }
            Renewal::Passing(e) => {
                tracing::warn!(server = %self.server, error = %e,
                    "an MCP server's sign-in could not be renewed now");
                false
            }
        }
    }

    /// Lose the sign-in, under its lock: no token from now on.
    fn lose(&self, slot: &mut Slot, why: &str) {
        tracing::warn!(server = %self.server, why, "an MCP server's sign-in is lost");
        slot.lost = Some(why.to_string());
        self.lost.send_replace(Some(why.to_string()));
    }
}

impl theseus_mcp::TokenSource for Signed {
    fn token(&self) -> BoxFuture<'_, Option<String>> {
        Box::pin(async move {
            let mut slot = self.slot.lock().await;
            if slot.lost.is_some() {
                return None;
            }
            if slot.kept.as_ref().is_some_and(|k| k.due(now_ms())) {
                // A renewal that may pass sends the old token: the server says.
                self.renew(&mut slot).await;
            }
            match slot.lost {
                Some(_) => None,
                None => slot.kept.as_ref().map(|k| k.access_token.clone()),
            }
        })
    }

    fn refused<'a>(&'a self, sent: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let mut slot = self.slot.lock().await;
            if slot.lost.is_some() {
                return false;
            }
            match slot.kept.as_ref() {
                None => false,
                // Renewed since that request went: try the new one.
                Some(k) if k.access_token != sent => true,
                Some(_) => self.renew(&mut slot).await,
            }
        })
    }
}

#[cfg(test)]
#[path = "tests_oauth_kept.rs"]
mod tests;
