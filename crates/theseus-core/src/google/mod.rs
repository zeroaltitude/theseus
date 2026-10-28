//! Google Workspace (the Google design; `theseus-google` is its client): the
//! one configured account, and the tools that read and change it.
//!
//! - **The account.** `[google] account` and its `[secrets]` names. After
//!   serving the daemon's `google.check` phase refreshes the access token from
//!   the board's refresh token and confirms the account with the cheapest call
//!   among the granted services; until it passes, every tool fails closed. A
//!   lapsed grant (`invalid_grant`) is `needs_auth`: each tool says so, with
//!   one notice to the owner and one `google.auth_lapsed` row.
//! - **The tools** ([`call`], and each service's row after it): every one is
//!   deferred, goes through [`Google::call`], and ends in [`finish`]: the
//!   `private` marker on its result and the External rule ([`external`]).
//! - **The consent and the token** ([`auth`]): `theseus google auth` mints the
//!   refresh token into the store `[secrets]` names; the access token lives in
//!   memory only.
//! - **FAST.** Building this reads nothing: no secret, no file, no socket. With
//!   no `[google]`, [`Google::from_config`] is `None` and nothing Google exists.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_google::error::LAPSED;
use theseus_google::{
    scopes, AccessToken, Client, ClientSecret, Endpoints, Kind, Policy, Request, Response, Service,
    Tokens,
};
use theseus_protocol::google::GoogleStatus;
use theseus_tools::Tool;
use tokio::sync::{watch, Notify};

use crate::config::GoogleConfig;
use crate::secrets::{Secret, SecretBoard, Waited};

pub mod auth;
pub mod calendar;
pub mod calendar_cache;
pub mod call;
pub mod drive;
pub mod external;
pub mod mail;
pub mod mime;
pub mod private;
pub mod send_cap;
pub mod sheets;
pub mod slides;
pub mod tend;

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_auth;
#[cfg(test)]
mod tests_calendar;
#[cfg(test)]
pub(crate) mod tests_calendar_cache;
#[cfg(test)]
mod tests_config;
#[cfg(test)]
mod tests_drive;
#[cfg(test)]
mod tests_gate;
#[cfg(test)]
mod tests_label;
#[cfg(test)]
mod tests_mail;
#[cfg(test)]
mod tests_morning;
#[cfg(test)]
mod tests_sensitive;
#[cfg(test)]
mod tests_sheets_slides;
#[cfg(test)]
mod tests_write_hold;

pub use external::{finish, Finished, Source};

/// The Google tools' names, for the config's check of `[policy.tools]`.
pub const NAMES: [&str; 15] = [
    call::NAME,
    mail::SEARCH,
    mail::READ,
    mail::DRAFT,
    mail::SEND,
    mail::LABEL,
    drive::FIND,
    drive::READ,
    drive::WRITE,
    drive::SHARE,
    sheets::READ,
    sheets::WRITE,
    slides::WRITE,
    calendar::EVENTS,
    calendar::PUT,
];

/// How long a call waits for the account's check, as a turn waits for a secret.
pub const BIND_WAIT: Duration = Duration::from_secs(30);

/// A check that failed for a reason that may pass runs again at a call this
/// long after.
const RECHECK_AFTER: Duration = Duration::from_secs(30);

/// Where the account's check stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not yet asked: the daemon's phase, or the first call, asks.
    Unchecked,
    Checking,
    Ok,
    /// The grant is gone or was never given: `theseus google auth`.
    NeedsAuth(String),
    /// Something else is wrong, and the words say what.
    Failed(String),
}

impl Phase {
    fn settled(&self) -> bool {
        !matches!(self, Phase::Unchecked | Phase::Checking)
    }

    fn word(&self) -> &'static str {
        match self {
            Phase::Unchecked | Phase::Checking => "checking",
            Phase::Ok => "ok",
            Phase::NeedsAuth(_) => "needs_auth",
            Phase::Failed(_) => "needs_auth",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub phase: Phase,
    /// The scopes Google says the grant holds.
    pub granted: BTreeSet<String>,
    pub last_refresh_ms: Option<u64>,
    at: Option<Instant>,
}

/// What a tool says of its call, for the `google.called` row.
#[derive(Debug, Clone)]
pub struct Meta {
    pub tool: &'static str,
    /// The method's id (`gmail.users.messages.send`), or a tool's own label.
    pub method: String,
    /// The thread, message, file or event id the call is about.
    pub resource: Option<String>,
    /// `read`, `write`, or `authority`.
    pub class: &'static str,
    /// A send's recipients, counted, never named.
    pub recipients: Option<u32>,
}

/// A call that went through: Google's answer and the row that records it.
pub struct Called {
    pub response: Response,
    pub row: Value,
}

/// A call that did not: what to tell the model, and the row.
pub struct CallFailed {
    pub message: String,
    pub row: Value,
    /// A call that must not repeat may have run (`outcome_unknown`).
    pub unknown: bool,
}

/// The configured account. Built with the tool runtime, and inert until a
/// call, the consent, or the daemon's check after serving.
pub struct Google {
    pub(crate) cfg: GoogleConfig,
    pub(crate) board: Arc<SecretBoard>,
    pub(crate) client: Client,
    pub(crate) tokens: Tokens,
    check: watch::Sender<Check>,
    check_lock: tokio::sync::Mutex<()>,
    /// The notice for a lapse is told once, until the next consent.
    lapse_told: AtomicBool,
    /// Woken when a lapse needs its row and its notice (`tend`).
    pub(crate) lapsed: Notify,
    pub sends: send_cap::Sends,
    pub(crate) pending: tokio::sync::Mutex<Option<auth::Pending>>,
    /// An `op` reader a test points at a fake; the daemon's is made on demand.
    pub(crate) op: Mutex<Option<Arc<crate::secrets::OpReader>>>,
    /// The refresh token's `[secrets]` reference, which the config holds and
    /// this does not: set by [`build`].
    pub(crate) refresh_ref: OnceLock<String>,
    /// The store refused the token: it is held in memory only until a restart.
    pub(crate) memory_only: AtomicBool,
    /// The configured calendars' events, kept in memory ([`calendar_cache`]).
    pub(crate) calendars: calendar_cache::Cache,
}

impl Google {
    /// The account `cfg` binds, or None when it binds none. Nothing runs yet:
    /// no secret is read, no connection opened.
    pub fn from_config(cfg: &GoogleConfig, board: Arc<SecretBoard>) -> Option<Arc<Self>> {
        Self::with_policy(cfg, board, Policy::default())
    }

    /// `from_config`, with the retry's own backoff (a test's, a few ms).
    pub fn with_policy(
        cfg: &GoogleConfig,
        board: Arc<SecretBoard>,
        policy: Policy,
    ) -> Option<Arc<Self>> {
        if cfg.is_empty() {
            return None;
        }
        let endpoints = match &cfg.endpoint {
            Some(e) => Endpoints::loopback(e),
            None => Endpoints::production(),
        };
        let client = Client::new(endpoints.clone()).with_policy(policy);
        let tokens = Tokens::new(client.http().clone(), endpoints);
        let (check, _) = watch::channel(Check {
            phase: Phase::Unchecked,
            granted: BTreeSet::new(),
            last_refresh_ms: None,
            at: None,
        });
        Some(Arc::new(Self {
            sends: send_cap::Sends::new(cfg.max_sends_per_day),
            cfg: cfg.clone(),
            board,
            client,
            tokens,
            check,
            check_lock: tokio::sync::Mutex::new(()),
            lapse_told: AtomicBool::new(false),
            lapsed: Notify::new(),
            pending: tokio::sync::Mutex::new(None),
            op: Mutex::new(None),
            refresh_ref: OnceLock::new(),
            memory_only: AtomicBool::new(false),
            calendars: calendar_cache::Cache::new(cfg),
        }))
    }

    /// Every Google tool ([`NAMES`] and each service's), by the list below:
    /// one line for each row's module, in the order mail, drive, sheets,
    /// slides, calendar, each marker with a line between, so two rows' lines
    /// never touch.
    pub fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        let mut all: Vec<Arc<dyn Tool>> = Vec::new();
        // gw-mail:
        all.extend(mail::tools(self));
        //
        all.extend(drive::tools(self));
        //
        // gw-sheets-slides:
        all.extend(sheets::tools(self));
        all.extend(slides::tools(self));
        //
        all.extend(calendar::tools(self));
        //
        all.extend([call::tool(self)]);
        all
    }

    pub fn account(&self) -> &str {
        &self.cfg.account
    }

    /// The service list the config turns on.
    pub fn services(&self) -> Vec<Service> {
        self.cfg.service_list()
    }

    pub fn check_now(&self) -> Check {
        self.check.borrow().clone()
    }

    pub(crate) fn set(&self, f: impl FnOnce(&mut Check)) {
        self.check.send_modify(f);
    }

    /// The scopes the consent asks for.
    pub fn requested(&self) -> Vec<String> {
        scopes::requested(&self.services())
    }

    /// The `[secrets]` name and the reference of the refresh token.
    pub(crate) fn refresh_name(&self) -> &str {
        &self.cfg.credentials.refresh_token
    }

    /// The client, read from the board each use, so a rotated secret works at
    /// once. An error is the words `google.check` fails with.
    pub(crate) fn client_secret(&self) -> Result<ClientSecret, String> {
        let name = &self.cfg.credentials.client;
        let value = self
            .board
            .get(name)
            .ok_or_else(|| format!("the client secret `{name}` has not resolved"))?;
        theseus_google::secret::parse_client(name, value.expose())
    }

    fn refresh_token(&self) -> Option<Secret> {
        self.board.get(self.refresh_name())
    }

    /// The grant lapsed: `needs_auth`, and one row and one notice until the
    /// next consent. Never an error of its own: the caller returns the words.
    pub(crate) fn lapse(&self) {
        self.tokens.forget();
        self.set(|c| c.phase = Phase::NeedsAuth(LAPSED.into()));
        if !self.lapse_told.swap(true, Ordering::SeqCst) {
            self.lapsed.notify_one();
        }
    }

    /// A consent finished: the next lapse tells again.
    pub(crate) fn lapse_told_reset(&self) {
        self.lapse_told.store(false, Ordering::SeqCst);
    }

    /// The account's check, once at a time (a second caller waits for the
    /// first and takes its answer): the client parsed, the token refreshed,
    /// the granted scopes read, the account confirmed. What it found, for the
    /// startup phase's detail.
    pub async fn check(&self) -> Value {
        let before = self.check.borrow().phase.clone();
        let _one = self.check_lock.lock().await;
        let now = self.check.borrow().phase.clone();
        if now != before && now.settled() {
            return self.detail();
        }
        self.set(|c| c.phase = Phase::Checking);
        let phase = self.run_check().await;
        self.set(|c| {
            c.phase = phase;
            c.at = Some(Instant::now());
        });
        self.detail()
    }

    fn detail(&self) -> Value {
        let c = self.check_now();
        let (state, error) = match &c.phase {
            Phase::NeedsAuth(e) | Phase::Failed(e) => (c.phase.word(), e.clone()),
            p => (p.word(), String::new()),
        };
        json!({"account": self.cfg.account, "state": state, "error": error,
            "tools": scopes::tools_on(&c.granted)})
    }

    async fn run_check(&self) -> Phase {
        let name = &self.cfg.credentials.client;
        match self.board.wait(name, BIND_WAIT).await {
            Waited::Ready(_) => {}
            Waited::Failed(why) => {
                return Phase::Failed(format!("the client secret `{name}` did not resolve: {why}"))
            }
            Waited::Resolving => {
                return Phase::Failed(format!(
                    "the client secret `{name}` did not resolve in time"
                ))
            }
            Waited::Absent => return Phase::Failed(format!("`{name}` is not a [secrets] entry")),
        }
        let client = match self.client_secret() {
            Ok(c) => c,
            Err(e) => return Phase::Failed(e),
        };
        let refresh = match self.board.wait(self.refresh_name(), BIND_WAIT).await {
            Waited::Ready(r) => r,
            Waited::Failed(_) | Waited::Absent => {
                return Phase::NeedsAuth("no refresh token yet: run `theseus google auth`".into())
            }
            Waited::Resolving => {
                return Phase::Failed("the refresh token did not resolve in time".into())
            }
        };
        let token = match self.tokens.access(&client, refresh.expose()).await {
            Ok(t) => t,
            Err(e) if e.kind == Kind::NeedsAuth => {
                self.lapse();
                return Phase::NeedsAuth(LAPSED.into());
            }
            Err(e) => return Phase::Failed(e.words()),
        };
        let mut granted: BTreeSet<String> = self.tokens.granted().into_iter().collect();
        if granted.is_empty() {
            // A refresh that names no scope: the consent's own request stands.
            granted = self.requested().into_iter().collect();
        }
        if let Err(p) = self.confirm_account(&token, &granted).await {
            return p;
        }
        self.set(|c| {
            c.granted = granted;
            c.last_refresh_ms = Some(theseus_protocol::now_unix_ms());
        });
        Phase::Ok
    }

    /// The cheapest call among the granted services that names the account:
    /// the mail profile, else Drive's about, else the primary calendar. A
    /// grant of Sheets, Slides or Docs alone has none: the consent's
    /// `id_token` was the check.
    async fn confirm_account(
        &self,
        token: &AccessToken,
        granted: &BTreeSet<String>,
    ) -> Result<(), Phase> {
        let on = scopes::services_on(granted);
        let (req, field): (Request, &[&str]) = if on.contains(&Service::Gmail) {
            (
                Request::get(Service::Gmail, "/gmail/v1/users/me/profile"),
                &["emailAddress"],
            )
        } else if on.contains(&Service::Drive) {
            let mut r = Request::get(Service::Drive, "/drive/v3/about");
            r.query = vec![("fields".into(), "user(emailAddress)".into())];
            (r, &["user", "emailAddress"])
        } else if on.contains(&Service::Calendar) {
            (
                Request::get(Service::Calendar, "/calendar/v3/calendars/primary"),
                &["id"],
            )
        } else {
            return Ok(());
        };
        let resp = match self.client.send(token, &req).await {
            Ok(r) => r,
            Err(f) if f.error.kind == Kind::NeedsAuth => {
                self.lapse();
                return Err(Phase::NeedsAuth(LAPSED.into()));
            }
            Err(f) => return Err(Phase::Failed(f.error.words())),
        };
        let mut v = resp.json();
        for k in field {
            v = v.get(*k).cloned().unwrap_or(Value::Null);
        }
        match v.as_str() {
            Some(email) if email.eq_ignore_ascii_case(&self.cfg.account) => Ok(()),
            Some(_) => Err(Phase::Failed(
                "the token is for a different account than `[google] account`: run `theseus google auth` as the account named there".into(),
            )),
            None => Err(Phase::Failed("Google did not name the account the token is for".into())),
        }
    }

    /// The access token for `tool`, once the check has passed and the grant
    /// holds the tool's scope. Fail closed: the words say why not.
    pub(crate) async fn ready(&self, tool: &str) -> Result<AccessToken, String> {
        for _ in 0..2 {
            let c = self.check_now();
            match &c.phase {
                Phase::Ok => break,
                Phase::NeedsAuth(e) => return Err(e.clone()),
                Phase::Failed(e) if c.at.is_some_and(|t| t.elapsed() < RECHECK_AFTER) => {
                    return Err(format!("Google is not usable: {e}"))
                }
                _ => {
                    if tokio::time::timeout(BIND_WAIT, self.check()).await.is_err() {
                        return Err("Google's check did not finish in time".into());
                    }
                }
            }
        }
        let c = self.check_now();
        match c.phase {
            Phase::Ok => {}
            Phase::NeedsAuth(e) => return Err(e),
            Phase::Failed(e) => return Err(format!("Google is not usable: {e}")),
            _ => return Err("Google's check has not passed".into()),
        }
        if !scopes::tool_on(&c.granted, tool) {
            return Err(format!(
                "{tool} is off: its scope was not granted. `theseus google auth` asks again, and `theseus google status` shows what is on"
            ));
        }
        let (client, refresh) = match (self.client_secret(), self.refresh_token()) {
            (Ok(c), Some(r)) => (c, r),
            (Err(e), _) => return Err(e),
            (_, None) => return Err(LAPSED.into()),
        };
        match self.tokens.access(&client, refresh.expose()).await {
            Ok(t) => Ok(t),
            Err(e) if e.kind == Kind::NeedsAuth => {
                self.lapse();
                Err(LAPSED.into())
            }
            Err(e) => Err(e.words()),
        }
    }

    /// One call of `meta.tool`: the token, the request retried by the table
    /// in `theseus-google`, and the `google.called` row, which rides in the
    /// result's meta. Every Google tool calls through here.
    pub async fn call(&self, meta: Meta, req: Request) -> Result<Called, CallFailed> {
        let began = Instant::now();
        let row = |status: u16, attempts: u32, outcome: &str, reason: Option<&str>| {
            Self::row(
                &meta,
                &req,
                (status, attempts, outcome, reason),
                began.elapsed().as_millis() as u64,
            )
        };
        let token = match self.ready(meta.tool).await {
            Ok(t) => t,
            Err(message) => {
                return Err(CallFailed {
                    message,
                    row: row(0, 0, "not_sent", None),
                    unknown: false,
                })
            }
        };
        // A partial grant turns off a service, not the account: a call of a
        // service whose scope was not granted is refused before it is sent.
        if !scopes::services_on(&self.check_now().granted).contains(&req.service) {
            let message = format!(
                "{} is off: its scope was not granted. `theseus google auth` asks again, and \
                 `theseus google status` shows what is on",
                req.service.display()
            );
            return Err(CallFailed {
                message,
                row: row(0, 0, "not_sent", None),
                unknown: false,
            });
        }
        match self.client.send(&token, &req).await {
            Ok(response) => {
                let r = row(response.status, response.attempts, "ok", None);
                Ok(Called { response, row: r })
            }
            Err(f) => {
                if f.error.kind == Kind::NeedsAuth {
                    self.lapse();
                }
                let outcome = if f.unknown { "unknown" } else { "failed" };
                let r = row(f.error.status, f.attempts, outcome, Some(&f.error.reason));
                let mut message = format!("{}: {}", meta.method, f.error.words());
                if f.unknown {
                    message.push_str(
                        ". It may have gone out (outcome unknown): look before trying again, it is never sent twice by itself",
                    );
                }
                Err(CallFailed {
                    message,
                    row: r,
                    unknown: f.unknown,
                })
            }
        }
    }

    /// The `google.called` row: no content, no subject, no address, no token.
    fn row(
        meta: &Meta,
        req: &Request,
        (status, attempts, outcome, reason): (u16, u32, &str, Option<&str>),
        ms: u64,
    ) -> Value {
        let mut row = json!({
            "service": req.service.name(), "tool": meta.tool, "method": meta.method,
            "resource": meta.resource, "class": meta.class, "status": status,
            "attempts": attempts, "quota_units": quota_units(&meta.method), "outcome": outcome, "ms": ms,
        });
        if let Some(n) = meta.recipients {
            row["recipients"] = n.into();
        }
        if let Some(r) = reason.filter(|r| !r.is_empty()) {
            row["error"] = r.into();
        }
        row
    }

    /// Where the refresh token is kept now: `op`, `file`, `env`, or `memory`
    /// (the store refused it: it works until the next restart).
    pub fn kept(&self) -> String {
        if self.memory_only.load(Ordering::SeqCst) {
            return "memory".into();
        }
        crate::secrets::write::kind_of(self.refresh_ref.get().map_or("", String::as_str)).into()
    }

    /// The status `theseus google status` and health show.
    pub fn status(&self) -> GoogleStatus {
        let c = self.check_now();
        let error = match &c.phase {
            Phase::NeedsAuth(e) | Phase::Failed(e) => e.clone(),
            _ => String::new(),
        };
        GoogleStatus {
            account: self.cfg.account.clone(),
            state: c.phase.word().into(),
            services: self
                .services()
                .iter()
                .map(|s| s.name().to_string())
                .collect(),
            granted: c.granted.iter().cloned().collect(),
            tools_on: scopes::tools_on(&c.granted)
                .into_iter()
                .map(String::from)
                .collect(),
            last_refresh_ms: c.last_refresh_ms,
            error,
            sends_today: self.sends.used(),
            max_sends_per_day: self.cfg.max_sends_per_day,
            store: self.kept(),
        }
    }
}

/// The `[secrets]` names no job is ever handed: the client and the refresh
/// token, when `[google]` binds an account (`broker::harness_only` names them).
pub fn secret_names(cfg: &GoogleConfig) -> Vec<String> {
    if cfg.is_empty() {
        return Vec::new();
    }
    vec![
        cfg.credentials.client.clone(),
        cfg.credentials.refresh_token.clone(),
    ]
}

/// The account `cfg` binds, with its refresh token's reference: what the tool
/// runtime builds (`toolrun::build_runtime`). None with no `[google]`.
pub fn build(cfg: &crate::Config, board: &Arc<SecretBoard>, tools_on: bool) -> Option<Arc<Google>> {
    let g = Google::from_config(&cfg.google, board.clone()).filter(|_| tools_on)?;
    let name = &cfg.google.credentials.refresh_token;
    let _ = g
        .refresh_ref
        .set(cfg.secrets.get(name).cloned().unwrap_or_default());
    Some(g)
}

/// `theseusd check`'s lines for Google's calendars: one per configured
/// calendar, by label (never by an event), with how its read went and, when it
/// did not go, the one fix. Empty with no `[google]` or no calendar service.
/// Reads Google (a refresh and one list a calendar); a calendar that fails is
/// said, and fails nothing.
pub async fn calendar_check(cfg: &crate::Config, board: &Arc<SecretBoard>) -> Vec<String> {
    let Some(g) = build(cfg, board, true).filter(|g| g.services().contains(&Service::Calendar))
    else {
        return Vec::new();
    };
    let (lines, _) = g.calendar_reach().await;
    lines
        .into_iter()
        .map(|(label, status, detail)| {
            let word = serde_json::to_value(status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            match detail.is_empty() {
                true => format!("google calendar {label}: {word}"),
                false => format!("google calendar {label}: {word}: {detail}"),
            }
        })
        .collect()
}

/// Gmail's quota units for a method (the design's §4.3); other services have
/// none the design names.
pub fn quota_units(method: &str) -> Option<u32> {
    if !method.starts_with("gmail.") {
        return None;
    }
    let last = method.rsplit('.').next().unwrap_or("");
    Some(match (method, last) {
        (_, "send") => 100,
        (m, "get") if m.contains(".threads.") => 10,
        (m, "list") if m.contains(".threads.") => 10,
        (_, "create") if method.contains(".drafts.") => 10,
        _ => 5,
    })
}
