//! What a server's sign-in gave, kept (issue 55): one file per server,
//! `<store>/mcp-oauth/<name>.json`, mode 0600, written by the daemon alone and
//! whole (a file of its own, synced, renamed over the old one), as the owner's
//! second proof is (`owner_proof/file.rs`). It is inside the store directory,
//! which the floor holds whole, as `config_change`'s directory is: no tool
//! reads or writes it and no L1 view shows it. Read at the server's first
//! start, renewed before the access token runs out and once on a 401, and
//! written back whole, unattended, when a refresh rotates the refresh token.
//!
//! Every value (the access and refresh tokens, a client secret) is put on the
//! secret board the moment it exists (`SecretBoard::hold`), so the scrubber
//! knows it, as the vault's values are. A refresh the server refuses for good
//! leaves the sign-in lost: [`Signed::lost`] says why, and the board leaves
//! the server `needs sign-in` until the owner signs in again.

use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use theseus_mcp::oauth::exchange::{self, Renewal, Tokens};
use theseus_mcp::oauth::register::ClientId;
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

/// How many of each kind of value stay held: a renewal holds its new token
/// beside the last ones, so a token just spent is still known to the
/// scrubber.
pub const HELD_EACH: u32 = 4;

/// The name a value is held under on the secret board: its kind and its slot.
pub fn held_name(server: &str, what: &str, slot: u32) -> String {
    format!("mcp-oauth:{server}:{what}:{}", slot % HELD_EACH)
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
    /// one kept, and none keeps it.
    pub fn renewed(&mut self, t: &Tokens, now_ms: u64) {
        self.access_token = t.access.to_string();
        if let Some(r) = &t.refresh {
            self.refresh_token = Some(r.to_string());
        }
        self.expires_at_ms = t.expires_in.map(|s| now_ms + s * 1000);
    }

    /// Whether the access token runs out within [`RENEW_BEFORE`] of `now_ms`.
    fn due(&self, now_ms: u64) -> bool {
        self.expires_at_ms
            .is_some_and(|at| at <= now_ms + RENEW_BEFORE.as_millis() as u64)
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

/// Never prints a value.
impl std::fmt::Debug for Kept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kept")
            .field("url", &self.url)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

/// The file at `path`: `Ok(None)` when there is none; an error, without its
/// contents, when it is there and cannot be trusted (it does not parse, or
/// another user may read it).
pub fn read(path: &Path) -> Result<Option<Kept>, String> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("the kept sign-in cannot be read".into()),
    };
    if meta.permissions().mode() & 0o077 != 0 {
        return Err("the kept sign-in is readable by others, so it is not used".into());
    }
    let mut bytes = std::fs::read(path).map_err(|_| "the kept sign-in cannot be read")?;
    let kept = serde_json::from_slice::<Kept>(&bytes)
        .map_err(|_| "the kept sign-in does not parse".to_string());
    bytes.zeroize();
    kept.map(Some)
}

/// Write `kept` whole: a file of its own at 0600, synced, then renamed over
/// `path`, so a crash leaves the old file or the new one, never half.
pub fn write(path: &Path, kept: &Kept) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    let new = path.with_extension("json.new");
    let mut bytes = serde_json::to_vec(kept)?;
    let wrote = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&new)?;
        // A stray file of that name keeps its own mode: set it whatever was there.
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::rename(&new, path)
    })();
    bytes.zeroize();
    if wrote.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    wrote
}

/// Take the file away.
pub fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn now_ms() -> u64 {
    theseus_protocol::now_unix_ms()
}

/// One server's sign-in, as the board uses it: the kept values, the file
/// they are written to, and whether the sign-in is lost.
pub struct Signed {
    server: String,
    path: Option<PathBuf>,
    secrets: Arc<SecretBoard>,
    http: reqwest::Client,
    kept: tokio::sync::Mutex<Option<Kept>>,
    /// Why the sign-in is lost (a refresh refused for good), while it is.
    lost: watch::Sender<Option<String>>,
    /// The next slot of each kind of value held ([`held_name`]).
    slots: std::sync::Mutex<std::collections::BTreeMap<String, u32>>,
}

impl Signed {
    /// The sign-in of `server` at `url`, read from `path` (none: in memory
    /// only). A kept sign-in for another address is not used.
    pub fn load(
        server: &str,
        url: &str,
        path: Option<PathBuf>,
        secrets: Arc<SecretBoard>,
    ) -> Arc<Self> {
        let loopback = reqwest::Url::parse(url).is_ok_and(|u| theseus_mcp::oauth::is_loopback(&u));
        let kept = match path.as_deref().map(read) {
            Some(Ok(Some(k))) if k.url == url => Some(k),
            Some(Err(why)) => {
                tracing::warn!(server, why = %why, "an MCP server's kept sign-in was not used");
                None
            }
            _ => None,
        };
        let me = Arc::new(Self {
            server: server.into(),
            path,
            secrets,
            http: theseus_mcp::oauth::http(loopback),
            kept: tokio::sync::Mutex::new(None),
            lost: watch::Sender::new(None),
            slots: std::sync::Mutex::default(),
        });
        if let Some(k) = kept {
            me.hold(&k);
            *me.kept.try_lock().expect("a new lock") = Some(k);
        }
        me
    }

    /// Put `value` on the secret board as `what` of this server's: the
    /// scrubber knows it from now on.
    pub fn hold_value(&self, what: &str, value: &str) {
        let slot = {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            let n = slots.entry(what.to_string()).or_insert(0);
            *n += 1;
            *n - 1
        };
        self.secrets.hold(
            &held_name(&self.server, what, slot),
            Secret::new(value.to_string()),
        );
    }

    /// Hold `k`'s values: each one not already held in its kind's last slot.
    fn hold(&self, k: &Kept) {
        for (what, v) in k.values() {
            let last = {
                let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
                slots
                    .get(what)
                    .map(|n| held_name(&self.server, what, n.wrapping_sub(1)))
            };
            let same = last
                .and_then(|name| self.secrets.get(&name))
                .is_some_and(|held| held.expose() == v);
            if !same {
                self.hold_value(what, v);
            }
        }
    }

    /// Whether a sign-in is kept.
    pub async fn signed_in(&self) -> bool {
        self.kept.lock().await.is_some()
    }

    /// Why the sign-in is lost, while it is.
    pub fn lost(&self) -> Option<String> {
        self.lost.borrow().clone()
    }

    /// A receiver that wakes when the sign-in is lost.
    pub fn watch_lost(&self) -> watch::Receiver<Option<String>> {
        self.lost.subscribe()
    }

    /// A new sign-in: held, written whole, and the loss forgotten.
    pub async fn put(&self, k: Kept) -> std::io::Result<()> {
        self.hold(&k);
        if let Some(p) = &self.path {
            write(p, &k)?;
        }
        *self.kept.lock().await = Some(k);
        self.lost.send_replace(None);
        Ok(())
    }

    /// Forget the sign-in: the file is removed. What was kept is returned,
    /// for a revoke.
    pub async fn forget(&self) -> (Option<Kept>, std::io::Result<()>) {
        let k = self.kept.lock().await.take();
        let removed = self.path.as_deref().map_or(Ok(()), remove);
        (k, removed)
    }

    /// Renew `k` with its refresh token: written back whole when it is. A
    /// refusal for good loses the sign-in; one that may pass leaves it.
    async fn renew(&self, slot: &mut Option<Kept>) -> bool {
        let Some(k) = slot.as_mut() else { return false };
        let Some(refresh) = k.refresh_token.clone() else {
            self.lose("its token ran out or was refused, and it gave no way to renew it");
            return false;
        };
        let Ok(endpoint) = reqwest::Url::parse(&k.token_endpoint) else {
            self.lose("the kept token endpoint is not an address");
            return false;
        };
        match exchange::refresh(&self.http, &endpoint, &k.client(), &refresh, &k.resource).await {
            Renewal::Renewed(t) => {
                k.renewed(&t, now_ms());
                self.hold(k);
                if let Some(p) = &self.path {
                    if let Err(e) = write(p, k) {
                        tracing::warn!(server = %self.server, error = %e,
                            "an MCP server's renewed sign-in was not written; it is held in memory");
                    }
                }
                tracing::info!(server = %self.server, "an MCP server's sign-in was renewed");
                true
            }
            Renewal::ForGood(why) => {
                self.lose(&format!("its sign-in was refused when renewed ({why})"));
                false
            }
            Renewal::Passing(e) => {
                tracing::warn!(server = %self.server, error = %e,
                    "an MCP server's sign-in could not be renewed now");
                false
            }
        }
    }

    fn lose(&self, why: &str) {
        tracing::warn!(server = %self.server, why, "an MCP server's sign-in is lost");
        self.lost.send_replace(Some(why.to_string()));
    }
}

impl theseus_mcp::TokenSource for Signed {
    fn token(&self) -> BoxFuture<'_, Option<String>> {
        Box::pin(async move {
            let mut slot = self.kept.lock().await;
            if slot.as_ref().is_some_and(|k| k.due(now_ms())) {
                // A renewal that may pass sends the old token: the server says.
                self.renew(&mut slot).await;
            }
            slot.as_ref().map(|k| k.access_token.clone())
        })
    }

    fn refused<'a>(&'a self, sent: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let mut slot = self.kept.lock().await;
            match slot.as_ref() {
                None => false,
                // Renewed since that request went: try the new one.
                Some(k) if k.access_token != sent => true,
                Some(_) => self.renew(&mut slot).await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept() -> Kept {
        Kept {
            url: "https://docs.example.invalid/mcp".into(),
            resource: "https://docs.example.invalid/mcp".into(),
            issuer: "https://as.example.invalid".into(),
            token_endpoint: "https://as.example.invalid/token".into(),
            client_id: "c-1".into(),
            access_token: "access-value-0001".into(),
            refresh_token: Some("refresh-value-0001".into()),
            expires_at_ms: Some(1_000),
            signed_in_at_ms: 1,
            revocation_endpoint: None,
            client_secret: None,
            client_basic: false,
        }
    }

    /// Written whole at 0600 in a 0700 directory; read back; a file others
    /// may read is not used; a half-written file never replaces the old one.
    #[test]
    fn the_file_is_private_and_written_whole() {
        let d = tempfile::tempdir().unwrap();
        let p = file(&dir(d.path()), "docs");
        write(&p, &kept()).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = std::fs::metadata(p.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dmode, 0o700);
        let back = read(&p).unwrap().unwrap();
        assert_eq!(back.access_token, "access-value-0001");
        assert!(!format!("{back:?}").contains("access-value"));
        // A stray temporary file is not the kept one.
        std::fs::write(p.with_extension("json.new"), b"{half").unwrap();
        assert_eq!(read(&p).unwrap().unwrap().client_id, "c-1");
        write(&p, &kept()).unwrap();
        assert!(!p.with_extension("json.new").exists());
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let e = read(&p).unwrap_err();
        assert!(e.contains("readable by others"), "{e}");
        assert!(!e.contains("access-value"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&p, b"not json").unwrap();
        assert_eq!(read(&p).unwrap_err(), "the kept sign-in does not parse");
        remove(&p).unwrap();
        remove(&p).unwrap();
        assert_eq!(read(&p).unwrap().map(|k| k.url.clone()), None);
    }

    /// A renewal's refresh token replaces the kept one; a renewal with none
    /// keeps it.
    #[test]
    fn a_rotated_refresh_token_replaces_the_kept_one() {
        let mut k = kept();
        let t = Tokens {
            access: "access-value-0002".to_string().into(),
            refresh: Some("refresh-value-0002".to_string().into()),
            expires_in: Some(30),
            scope: None,
        };
        k.renewed(&t, 10_000);
        assert_eq!(k.refresh_token.as_deref(), Some("refresh-value-0002"));
        assert_eq!(k.expires_at_ms, Some(40_000));
        assert!(k.due(0));
        let t = Tokens {
            access: "access-value-0003".to_string().into(),
            refresh: None,
            expires_in: None,
            scope: None,
        };
        k.renewed(&t, 0);
        assert_eq!(k.refresh_token.as_deref(), Some("refresh-value-0002"));
        assert!(!k.due(u64::MAX / 2));
    }

    /// A kept sign-in's values are on the secret board at once, and a sign-in
    /// for another address is not used.
    #[tokio::test]
    async fn a_kept_sign_in_is_read_held_and_bound_to_its_address() {
        let d = tempfile::tempdir().unwrap();
        let p = file(&dir(d.path()), "docs");
        write(&p, &kept()).unwrap();
        let board = SecretBoard::empty();
        let s = Signed::load(
            "docs",
            "https://docs.example.invalid/mcp",
            Some(p.clone()),
            board.clone(),
        );
        assert!(s.signed_in().await);
        assert_eq!(
            board
                .get(&held_name("docs", "refresh", 0))
                .unwrap()
                .expose(),
            "refresh-value-0001"
        );
        let other = Signed::load(
            "docs",
            "https://moved.example.invalid/mcp",
            Some(p),
            SecretBoard::empty(),
        );
        assert!(!other.signed_in().await);
    }
}
