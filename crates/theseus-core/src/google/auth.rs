//! `theseus google auth`: the one-time consent (the Google design §2.2). An
//! OAuth 2.0 Desktop client with a loopback redirect and PKCE S256. The daemon
//! draws the verifier and the state, listens once on `127.0.0.1`, and hands the
//! CLI the consent URL (which holds no secret). The redirect, or the address
//! the owner pastes, gives a code; the daemon redeems it with the verifier, and
//! only then checks what came back: the account (the `id_token`'s `email`, a
//! mismatch stores **nothing**), and the scopes (a partial grant turns off only
//! the tools missing a scope). Then the refresh token goes where `[secrets]`
//! points (`secrets::write`), is held on the live board at once, and the state
//! is `ok`. A refused store is not a failed consent: the token is held in
//! memory, and the result says so.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_google::oauth::{self, AuthError, Code, Loopback, Pkce};
use theseus_google::{scopes, Service};
use theseus_protocol::google::{
    GoogleAuthBeginResult, GoogleAuthFinishParams, GoogleAuthFinishResult, GoogleRevokeResult,
};
use tokio::sync::oneshot;

use super::{Google, Phase};
use crate::secrets::{write, OpReader, Secret, Waited};

/// A consent in flight: its verifier, and the listener's answer to come.
pub struct Pending {
    id: String,
    pkce: Pkce,
    redirect: String,
    rx: Option<oneshot::Receiver<Result<Code, AuthError>>>,
    began: Instant,
}

/// How long a consent stays open: the listener's own bound.
const OPEN_FOR: Duration = oauth::LISTEN_FOR;

impl Google {
    /// The `op` the consent mints through: a test's fake, else the daemon's
    /// own reader (its one token, `--op-token-file`), made when needed.
    fn op_reader(&self, token_file: Option<&std::path::Path>) -> Result<Arc<OpReader>, String> {
        if let Some(op) = self.op.lock().unwrap().clone() {
            return Ok(op);
        }
        OpReader::from_env(token_file.and_then(|p| p.to_str()))
            .map(Arc::new)
            .map_err(|e| format!("no 1Password access: {e:#}"))
    }

    /// The reader for a store of `reference`'s kind: `op` only for a vault's.
    fn writer_for(
        &self,
        reference: &str,
        token_file: Option<&std::path::Path>,
    ) -> Result<Arc<OpReader>, String> {
        if write::kind_of(reference) == "op" {
            self.op_reader(token_file)
        } else {
            Ok(Arc::new(OpReader::absent(String::new())))
        }
    }

    fn reference(&self) -> &str {
        self.refresh_ref.get().map_or("", String::as_str)
    }

    /// Start a consent: the URL, and the listener behind it. On WSL the
    /// owner's browser runs on Windows and cannot reach the loopback redirect
    /// (theseus-i4yt6), so the consent goes straight to the paste, as with
    /// `--manual`, instead of waiting out the paste delay first.
    pub async fn auth_begin(&self, manual: bool) -> Result<GoogleAuthBeginResult, String> {
        self.auth_begin_on(manual, on_wsl()).await
    }

    /// `auth_begin`, told whether this is WSL.
    pub(super) async fn auth_begin_on(
        &self,
        manual: bool,
        wsl: bool,
    ) -> Result<GoogleAuthBeginResult, String> {
        let manual = manual || wsl;
        let name = &self.cfg.credentials.client;
        match self.board.wait(name, Duration::from_secs(10)).await {
            Waited::Ready(_) => {}
            Waited::Failed(why) => {
                return Err(format!("the client secret `{name}` did not resolve: {why}"))
            }
            Waited::Resolving => {
                return Err(format!(
                    "the client secret `{name}` is still resolving: try again"
                ))
            }
            Waited::Absent => return Err(format!("`{name}` is not a [secrets] entry")),
        }
        let client = self.client_secret()?;
        let loopback = Loopback::bind()
            .await
            .map_err(|e| format!("the loopback listener: {e}"))?;
        let redirect = loopback.redirect();
        let pkce = Pkce::new();
        let scopes = self.requested();
        let url = oauth::consent_url(
            self.client.endpoints(),
            &client.id,
            &redirect,
            &scopes,
            &pkce,
            &self.cfg.account,
        );
        let (tx, rx) = oneshot::channel();
        let state = pkce.state.clone();
        tokio::spawn(async move {
            let _ = tx.send(loopback.wait(&state, OPEN_FOR).await);
        });
        let id = uuid::Uuid::now_v7().simple().to_string();
        *self.pending.lock().await = Some(Pending {
            id: id.clone(),
            pkce,
            redirect: redirect.clone(),
            rx: Some(rx),
            began: Instant::now(),
        });
        Ok(GoogleAuthBeginResult {
            auth_id: id,
            url,
            redirect: if manual { String::new() } else { redirect },
            account: self.cfg.account.clone(),
            asks: self
                .services()
                .iter()
                .map(|s| s.display().to_string())
                .collect(),
            expires_secs: OPEN_FOR.as_secs(),
            paste_after_secs: if manual {
                0
            } else {
                oauth::PASTE_AFTER.as_secs()
            },
        })
    }

    /// Wait for the redirect, or take the owner's paste, and finish the
    /// consent. `waiting` when no redirect came in `wait_secs` and nothing was
    /// pasted: the consent stays open for the paste.
    pub async fn auth_finish(
        &self,
        p: &GoogleAuthFinishParams,
        token_file: Option<&std::path::Path>,
    ) -> Result<GoogleAuthFinishResult, String> {
        let mut slot = self.pending.lock().await;
        let Some(pending) = slot
            .as_mut()
            .filter(|c| c.id == p.auth_id && c.began.elapsed() < OPEN_FOR)
        else {
            *slot = None;
            return Err("no sign-in is open: run `theseus google auth` again".into());
        };
        let state = pending.pkce.state.clone();
        let code = match p.pasted.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(pasted) => oauth::parse_redirect(pasted, &state).map_err(|e| e.to_string())?,
            None => {
                let Some(rx) = pending.rx.as_mut() else {
                    return Err("the redirect was taken: paste the address, or run `theseus google auth` again".into());
                };
                match tokio::time::timeout(Duration::from_secs(p.wait_secs), rx).await {
                    Err(_) => return Ok(waiting(&self.cfg.account)),
                    Ok(Err(_)) => {
                        return Err("the listener stopped: run `theseus google auth` again".into())
                    }
                    Ok(Ok(Err(e))) => {
                        *slot = None;
                        return Err(e.to_string());
                    }
                    Ok(Ok(Ok(code))) => code,
                }
            }
        };
        let (verifier, redirect) = (
            pending.pkce.verifier().to_string(),
            pending.redirect.clone(),
        );
        // The code is single-use: whatever follows, this consent is over.
        *slot = None;
        drop(slot);
        self.redeem_and_keep(&code, &verifier, &redirect, token_file)
            .await
    }

    async fn redeem_and_keep(
        &self,
        code: &Code,
        verifier: &str,
        redirect: &str,
        token_file: Option<&std::path::Path>,
    ) -> Result<GoogleAuthFinishResult, String> {
        let client = self.client_secret()?;
        let grant = oauth::redeem(
            self.client.http(),
            self.client.endpoints(),
            &client,
            code,
            verifier,
            redirect,
        )
        .await
        .map_err(|e| e.words())?;
        // The account first: a token for another address is stored nowhere.
        match grant.email.as_deref() {
            Some(e) if e.eq_ignore_ascii_case(&self.cfg.account) => {}
            Some(_) => {
                return Err("that sign-in was for a different account than `[google] account`: nothing was stored. \
                            Sign in as the account named there"
                    .into())
            }
            None => return Err("Google named no account for that sign-in: nothing was stored".into()),
        }
        let granted: std::collections::BTreeSet<String> = grant.scopes.iter().cloned().collect();
        let asked = self.services();
        let missing = scopes::missing(&asked, &granted);
        let on = scopes::services_on(&granted);
        if on.is_empty() {
            return Err(
                "Google granted none of the services asked for: nothing was stored. Run \
                        `theseus google auth` again and leave the boxes ticked"
                    .into(),
            );
        }
        let not_granted: Vec<String> = missing
            .iter()
            .map(|s| {
                format!(
                    "{} was not granted: the {} tools stay off",
                    s.display(),
                    s.name()
                )
            })
            .collect();
        let secret = Secret::new(grant.refresh_token.to_string());
        let reference = self.reference().to_string();
        let stored = match self.writer_for(&reference, token_file) {
            Ok(op) => write::store(&op, &reference, &secret).await,
            Err(e) => Err(e),
        };
        let (store, note) = match stored {
            Ok(kept) => (kept.as_str().to_string(), String::new()),
            Err(why) => (
                "memory".to_string(),
                format!(
                    "the refresh token could not be stored ({why}): Google works until the next restart, \
                     and `theseus google auth` mints it again"
                ),
            ),
        };
        self.memory_only
            .store(store == "memory", std::sync::atomic::Ordering::SeqCst);
        // Held at once: no restart.
        self.board.hold(self.refresh_name(), secret);
        self.tokens.forget();
        self.tokens.set_granted(grant.scopes.clone());
        self.lapse_told_reset();
        self.set(|c| {
            c.phase = Phase::Ok;
            c.granted = granted.clone();
            c.last_refresh_ms = Some(theseus_protocol::now_unix_ms());
        });
        Ok(GoogleAuthFinishResult {
            state: "done".into(),
            account: self.cfg.account.clone(),
            granted: on.iter().map(|s| s.display().to_string()).collect(),
            not_granted,
            tools_on: scopes::tools_on(&granted)
                .into_iter()
                .map(String::from)
                .collect(),
            store,
            note,
        })
    }

    /// `theseus google revoke`: revoked at Google, cleared from the store,
    /// dropped from the board; the state is `needs_auth`.
    pub async fn revoke(&self, token_file: Option<&std::path::Path>) -> GoogleRevokeResult {
        let mut notes = Vec::new();
        let refresh = self
            .board
            .get(self.refresh_name())
            .filter(|s| !s.is_empty());
        let revoked_at_google = match refresh {
            Some(t) => {
                match oauth::revoke(self.client.http(), self.client.endpoints(), t.expose()).await {
                    Ok(()) => true,
                    Err(e) => {
                        notes.push(format!("Google did not answer the revoke ({}): revoke it at Google's permissions page too", e.words()));
                        false
                    }
                }
            }
            None => {
                notes.push("there was no refresh token to revoke".into());
                false
            }
        };
        let reference = self.reference().to_string();
        let cleared = match self.writer_for(&reference, token_file) {
            Ok(op) => write::clear(&op, &reference).await,
            Err(e) => Err(e),
        };
        if let Err(why) = &cleared {
            notes.push(format!("the store was not cleared ({why})"));
        }
        self.board.withdraw(self.refresh_name(), "revoked");
        self.tokens.forget();
        self.tokens.set_granted(Vec::new());
        self.memory_only
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.lapse_told_reset();
        self.set(|c| {
            c.phase =
                Phase::NeedsAuth("Google access was revoked: run `theseus google auth`".into());
            c.granted.clear();
        });
        GoogleRevokeResult {
            revoked_at_google,
            cleared: cleared.is_ok(),
            note: notes.join("; "),
        }
    }

    /// The services the config turns on, as words.
    pub fn service_words(&self) -> Vec<String> {
        self.services()
            .iter()
            .map(|s: &Service| s.display().to_string())
            .collect()
    }
}

/// Whether this kernel is WSL's: `/proc/version` names Microsoft.
fn on_wsl() -> bool {
    wsl_at(std::path::Path::new("/proc/version"))
}

/// Whether the `/proc/version` at `path` is WSL's; unreadable is not.
pub(super) fn wsl_at(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|v| wsl_of(&v))
}

/// Whether a `/proc/version` line is WSL's (`...-microsoft-standard-WSL2`).
pub(super) fn wsl_of(proc_version: &str) -> bool {
    proc_version.to_ascii_lowercase().contains("microsoft")
}

fn waiting(account: &str) -> GoogleAuthFinishResult {
    GoogleAuthFinishResult {
        state: "waiting".into(),
        account: account.into(),
        ..Default::default()
    }
}
