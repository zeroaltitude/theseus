//! AWS (AWS design §3; rows 29 and 30, C1 = 14a and C2 = 14b): the accounts
//! Theseus owns, and the tools that read and change them.
//!
//! - **The accounts.** One [`Account`] per `[aws.accounts.<id>]` table. Its
//!   key, the root of trust, comes from the secrets board after serving, and
//!   is checked once with `sts:GetCallerIdentity`, which must name this
//!   account. Until the check passes no call of the account signs: a call
//!   waits for it, at most [`BIND_WAIT`], then fails as unbound, and says
//!   why. Health shows each account's state (fail closed, §3.5).
//! - **Who signs** ([`session`]). Until the config names the owner role the
//!   bootstrap made, the key signs. Then the key signs only STS, and each
//!   call signs in a role session: its execution's (the guards on), a job's,
//!   a floor session for one call the operator approved, or a tender's.
//! - **The tools** ([`tools`], [`stack`], [`cost`]): `aws.call` (any
//!   operation of any service: the guardrails at the floor, IaC-only as
//!   invalid input, deletions of what holds state waiting), `aws.describe`
//!   (the catalog, local), `aws.whoami`, `aws.s3.list`, `aws.stack.plan`,
//!   `.apply`, `.status`, and `.delete`, and `aws.cost`. A call that returns
//!   a secret holds it on the secrets board under a handle ([`secret`]).
//! - **The bootstrap** ([`bootstrap`]) and **the tenders** ([`tend`],
//!   [`durable`]): the account's first stacks, on the operator's yes; after
//!   serving, the budget's reconcile and reads, GuardDuty's weekly usage,
//!   and the store shipped to the foundation's bucket and table.
//! - **FAST** (§3.10). Building this does nothing: the catalog decodes on the
//!   first call, the HTTP client builds on the first send, and the check runs
//!   after serving (`theseusd`'s `aws.check` phase) or at the first call.
//! - **Visibility** (§3.8). Each request a call makes is recorded on the
//!   call's binding ([`AwsBinding`]): the runtime ledgers it as an
//!   `aws.called` row with the call's result, and the turn's trace shows it
//!   as a span under the call's own. The account's check is its startup
//!   phase, and health's line.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_aws::{Attribution, Call, CallError, Client, ClientConfig, Credentials, Output};
use theseus_protocol::{AwsAccountStatus, AwsStatus, Span};
use theseus_tools::{AwsBinding, AwsRequest, Tool};
use tokio::sync::watch;

use crate::config::{AwsAccountConfig, AwsConfig};
use crate::secrets::{Secret, SecretBoard, SecretState};

pub mod alerts;
pub mod bootstrap;
pub mod cost;
pub mod crosscheck;
pub mod durable;
pub mod external;
pub mod hands;
pub mod inventory;
pub mod logs;
pub mod s3;
pub mod secret;
pub mod session;
pub mod stack;
pub mod tend;
pub mod tools;
pub mod trail;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_c2;
#[cfg(test)]
mod tests_c3;
#[cfg(test)]
mod tests_durable;
#[cfg(test)]
mod tests_handles;
#[cfg(test)]
mod tests_inventory;
#[cfg(test)]
mod tests_l1;
#[cfg(test)]
mod tests_network;
#[cfg(test)]
mod tests_outside;
#[cfg(test)]
mod tests_restore;

/// The AWS tools' names, for the config's check of `[policy.tools]`.
pub const NAMES: [&str; 16] = [
    "aws.call",
    "aws.cost",
    "aws.describe",
    "aws.hands.run",
    "aws.inventory",
    "aws.logs.query",
    "aws.logs.tail",
    "aws.s3.get",
    "aws.s3.list",
    "aws.s3.put",
    "aws.stack.apply",
    "aws.stack.delete",
    "aws.stack.plan",
    "aws.stack.status",
    "aws.trail",
    "aws.whoami",
];

/// The allow policy of a work, job, or floor session (§3.5), which the
/// foundation stack makes.
pub const ALLOW_ALL: &str = "theseus-allow-all";

/// An AWS tool's own `[policy.aws]` class, as the tool list and the system
/// note show its posture: a stack's apply and delete and an S3 put write, and the rest
/// read (an `aws.call` that writes or runs takes its own class's line at the
/// gate). None for `aws.describe`, which calls nothing.
pub fn tool_class(name: &str) -> Option<&'static str> {
    match name {
        "aws.describe" => None,
        "aws.stack.apply" | "aws.stack.delete" | "aws.s3.put" => Some("write"),
        hands::RUN => Some("run"),
        n if NAMES.contains(&n) => Some("read"),
        _ => None,
    }
}

/// How long a call waits for its account's check (§3.10), as a turn waits
/// for a secret.
pub const BIND_WAIT: Duration = Duration::from_secs(30);

/// A check that failed for a reason that may pass (the network, a throttle,
/// AWS's own error) runs again at a call this long after.
const RECHECK_AFTER: Duration = Duration::from_secs(30);

/// The calls whose requests wait for their turn's trace, at most.
const TRACED: usize = 256;

/// Every account the config binds. Built with the tool runtime, and inert
/// until a call or the daemon's check after serving.
pub struct Aws {
    accounts: BTreeMap<String, Arc<Account>>,
    /// Each AWS call's binding, by its `tool_use` id, until its turn's trace
    /// takes its requests (`spans`). The oldest go first past `TRACED`.
    traced: Mutex<VecDeque<(String, Arc<AwsBinding>)>>,
    /// The change sets plans showed, which `aws.stack.apply` names.
    shows: stack::Shows,
    /// Where hands run, and the completion poller's wake (§3.3).
    pub hands: hands::Hands,
}

impl Aws {
    /// The accounts `cfg` binds, or None when it binds none. Nothing runs
    /// yet: no secret is read, no catalog decoded, no connection opened.
    pub fn from_config(cfg: &AwsConfig, board: Arc<SecretBoard>) -> Option<Arc<Self>> {
        if cfg.is_empty() {
            return None;
        }
        let accounts = cfg
            .accounts
            .iter()
            .map(|(id, a)| (id.clone(), Arc::new(Account::new(id, a, board.clone()))))
            .collect();
        Some(Arc::new(Self {
            accounts,
            traced: Mutex::default(),
            shows: stack::Shows::default(),
            hands: hands::Hands::default(),
        }))
    }

    /// Every AWS tool ([`NAMES`]).
    pub fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        let mut all = tools::all(self);
        all.extend(s3::all(self));
        all.extend(logs::all(self));
        all.extend(trail::all(self));
        all.extend(inventory::all(self));
        all.extend(stack::all(self));
        all.push(Arc::new(cost::Cost::new(self.clone())));
        all.push(Arc::new(hands::tool::HandsRun(self.clone())));
        all
    }

    /// Every bound account.
    pub fn accounts(&self) -> impl Iterator<Item = &Arc<Account>> {
        self.accounts.values()
    }

    /// The account a call names, or the only one.
    pub fn account(&self, named: Option<&str>) -> Result<&Arc<Account>, String> {
        let ids = || self.accounts.keys().cloned().collect::<Vec<_>>().join(", ");
        match named {
            Some(id) => self.accounts.get(id).ok_or_else(|| {
                format!(
                    "no AWS account {id:?} is bound; the bound accounts: {}",
                    ids()
                )
            }),
            None if self.accounts.len() == 1 => Ok(self.accounts.values().next().expect("one")),
            None => Err(format!(
                "several AWS accounts are bound, so the call names one with \"account\": {}",
                ids()
            )),
        }
    }

    /// Every account's check, started, and waited for until each is bound
    /// or has failed: the daemon's `aws.check` phase after serving (§3.10).
    /// What each found, for the phase's detail.
    pub async fn check_all(&self) -> Value {
        let mut out = Vec::new();
        for a in self.accounts.values() {
            a.start_check();
            let mut rx = a.check.subscribe();
            let _ = rx.wait_for(|c| c.settled()).await;
            let s = a.status();
            out.push(json!({"account": s.account, "state": s.state, "error": s.error}));
        }
        json!({"accounts": out})
    }

    /// Each account as health shows it.
    pub fn status(&self) -> AwsStatus {
        AwsStatus {
            accounts: self.accounts.values().map(|a| a.status()).collect(),
        }
    }

    /// An AWS call's binding, made by the runtime as the call starts, and
    /// kept until its turn's trace takes its requests.
    pub fn bind(
        &self,
        execution_id: &str,
        correlation_id: &str,
        tool_use_id: &str,
    ) -> Arc<AwsBinding> {
        let b = Arc::new(AwsBinding::new(execution_id, correlation_id));
        let mut t = self.traced.lock().unwrap();
        if t.len() >= TRACED {
            t.pop_front();
        }
        t.push_back((tool_use_id.to_string(), b.clone()));
        b
    }

    /// The spans of the AWS requests one call made, for its tool's span in
    /// the turn's trace, each placed on the trace's clock by `at`. A call's
    /// are taken once.
    pub fn spans(&self, tool_use_id: &str, at: impl Fn(Instant) -> u64) -> Vec<Span> {
        let b = {
            let mut t = self.traced.lock().unwrap();
            match t.iter().position(|(id, _)| id == tool_use_id) {
                Some(i) => t.remove(i).map(|(_, b)| b),
                None => None,
            }
        };
        b.map(|b| b.requests())
            .unwrap_or_default()
            .iter()
            .map(|r| span(r, &at))
            .collect()
    }
}

/// A request's span (§3.8): OpenTelemetry's AWS names (`rpc.system`,
/// `rpc.service`, `rpc.method`, `aws.request_id`, `cloud.account.id`,
/// `cloud.region`), with the call's correlation id, class, and outcome.
fn span(r: &AwsRequest, at: &impl Fn(Instant) -> u64) -> Span {
    let row = &r.row;
    let s = |k: &str| row.get(k).cloned().unwrap_or(Value::Null);
    let mut attrs = json!({
        "rpc.system": "aws-api",
        "rpc.service": s("service"),
        "rpc.method": s("operation"),
        "aws.request_id": s("request_id"),
        "cloud.account.id": s("account"),
        "cloud.region": s("region"),
        "correlation_id": s("correlation_id"),
        "class": s("class"),
        "status": s("status"),
        "http_status": s("http_status"),
        "attempts": s("attempts"),
        "pages": s("pages"),
        "result_bytes": s("result_bytes"),
    });
    if let Some(e) = row.get("error").filter(|e| !e.is_null()) {
        attrs["error"] = s("error_code");
        attrs["message"] = e.clone();
    }
    if let Value::Object(m) = &mut attrs {
        m.retain(|_, v| !v.is_null());
    }
    Span {
        name: format!(
            "aws {}:{}",
            row["service"].as_str().unwrap_or("?"),
            row["operation"].as_str().unwrap_or("?")
        ),
        kind: "aws".into(),
        start_us: at(r.started),
        end_us: Some(at(r.ended)),
        attrs,
        children: Vec::new(),
    }
}

/// Where an account's check stands.
#[derive(Clone)]
enum Check {
    /// Nothing has asked yet.
    Unchecked,
    /// Waiting for its key's secrets; why, when one has failed (the board
    /// asks the vault again).
    Waiting(Option<String>),
    Checking,
    Bound(Bound),
    Failed {
        why: String,
        at_ms: u64,
        /// When a call may run the check again: never for a key that names
        /// another account, or that AWS refused.
        recheck: Option<Instant>,
    },
}

impl Check {
    fn settled(&self) -> bool {
        matches!(self, Check::Bound(_) | Check::Failed { .. })
    }

    fn word(&self) -> &'static str {
        match self {
            Check::Unchecked => "unchecked",
            Check::Waiting(_) => "waiting",
            Check::Checking => "checking",
            Check::Bound(_) => "bound",
            Check::Failed { .. } => "failed",
        }
    }
}

#[derive(Clone)]
struct Bound {
    creds: Credentials,
    arn: String,
    user_id: String,
    at_ms: u64,
}

/// What a request failed with.
#[derive(Debug)]
pub enum Failure {
    /// The account's key is not bound, so nothing was sent.
    Unbound(String),
    /// The client's error: AWS's answer, or why nothing was sent.
    Call(CallError),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unbound(why) => f.write_str(why),
            Failure::Call(e) => write!(f, "{e}"),
        }
    }
}

/// One AWS request, as a tool asks for it.
pub struct Request<'a> {
    pub service: &'a str,
    pub operation: &'a str,
    pub input: &'a Value,
    pub region: &'a str,
    pub pages: u32,
    /// `read`, `write`, or `run`.
    pub class: &'a str,
    /// What signs it.
    pub signer: Signer<'a>,
}

/// What signs a request (§3.5).
#[derive(Clone, Copy)]
pub enum Signer<'a> {
    /// A role session of this kind once the config names the owner role;
    /// until then the key, but never for a job.
    As(session::Kind),
    /// The key itself: the bootstrap's creation of the foundation, before
    /// any role exists.
    Key,
    /// Credentials the caller holds: the bootstrap's floor session.
    With(&'a Credentials),
}

/// What the account's tenders last found, for health (§3.7).
#[derive(Default)]
pub struct Tended {
    pub budget: Option<theseus_protocol::AwsBudgetStatus>,
    pub guardduty: Option<theseus_protocol::AwsGuardDutyStatus>,
    pub reconcile: Option<String>,
    /// The durability tender's line (step 15), when it ships here.
    pub durability: Option<theseus_protocol::AwsDurabilityStatus>,
    /// Its hands and the hour's meter (step 40 part 2), as their last event
    /// left them (`hands::watch`).
    pub hands: Option<theseus_protocol::AwsHandsStatus>,
}

/// One bound account: its key's check, its client, its sessions, and its
/// counts.
pub struct Account {
    pub id: String,
    pub cfg: AwsAccountConfig,
    client: Client,
    board: Arc<SecretBoard>,
    check: watch::Sender<Check>,
    /// The first check began.
    started: AtomicBool,
    calls: AtomicU64,
    failed: AtomicU64,
    sessions: session::Sessions,
    pub tended: Mutex<Tended>,
}

impl Account {
    fn new(id: &str, cfg: &AwsAccountConfig, board: Arc<SecretBoard>) -> Self {
        let mut c = ClientConfig::new(cfg.region.clone());
        c.product = format!("theseus/{}", crate::VERSION);
        c.endpoint_override = cfg.endpoint.clone();
        Self {
            id: id.into(),
            cfg: cfg.clone(),
            client: Client::new(c),
            board,
            check: watch::Sender::new(Check::Unchecked),
            started: AtomicBool::new(false),
            calls: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            sessions: session::Sessions::default(),
            tended: Mutex::default(),
        }
    }

    /// What signs this account's calls, in health's words.
    pub fn signer(&self) -> String {
        match &self.cfg.owner_role {
            Some(r) => format!("role sessions ({r})"),
            None => "its key (no owner_role yet: theseus aws bootstrap makes it)".into(),
        }
    }

    /// A role session of `kind` named `name`, minted with the key (which
    /// waits for its check), or from the cache; its `aws.session.minted` row
    /// goes on `binding`. Into the config's owner role, or `role`.
    pub async fn session(
        self: &Arc<Self>,
        want: &session::Want<'_>,
        role: Option<&str>,
        binding: Option<&AwsBinding>,
    ) -> Result<Credentials, String> {
        let Some(role) = role.or(self.cfg.owner_role.as_deref()) else {
            return Err(format!(
                "AWS account {} has no role sessions yet: the config names no owner_role (theseus \
                 aws bootstrap makes theseus-owner)",
                self.id
            ));
        };
        let key = self.credentials().await?;
        let minted = self
            .sessions
            .get(
                &self.client,
                &key,
                &self.id,
                role,
                self.cfg.deployment(),
                want,
            )
            .await?;
        if let (Some(row), Some(b)) = (minted.row, binding) {
            b.record_session(row);
        }
        Ok(minted.creds)
    }

    /// A job's session (§3.5), for the broker's grant at its launch: named
    /// by its correlation id, under the guards and the stack path's, for its
    /// deadline. None before the owner role exists: never the key.
    pub async fn job_session(
        self: &Arc<Self>,
        correlation_id: &str,
        lasts: Duration,
    ) -> Result<Credentials, String> {
        if self.cfg.owner_role.is_none() {
            return Err(format!(
                "AWS account {} gives a job no session before its owner role exists \
                 (owner_role; theseus aws bootstrap), and never its key",
                self.id
            ));
        }
        let want = session::Want {
            kind: session::Kind::Job,
            name: session::session_name(correlation_id),
            execution: None,
            lasts,
            inline: None,
        };
        self.session(&want, None, None).await
    }

    /// The credentials a request signs with.
    async fn signing(
        self: &Arc<Self>,
        signer: Signer<'_>,
        binding: Option<&AwsBinding>,
    ) -> Result<Credentials, String> {
        let kind = match signer {
            Signer::With(c) => return Ok(c.clone()),
            Signer::Key => return self.credentials().await,
            Signer::As(kind) => kind,
        };
        if self.cfg.owner_role.is_none() {
            return match kind {
                session::Kind::Job => Err(format!(
                    "AWS account {} gives a job no session before its owner role exists \
                     (owner_role; theseus aws bootstrap), and never its key",
                    self.id
                )),
                _ => self.credentials().await,
            };
        }
        let execution = binding.map(|b| b.execution_id.as_str());
        let (name, lasts) = match kind {
            session::Kind::Work => (
                execution.unwrap_or("theseus-core").to_string(),
                session::LONGEST,
            ),
            session::Kind::Floor => (
                format!("{}.floor", execution.unwrap_or("theseus-core")),
                session::SHORTEST,
            ),
            session::Kind::Tender(t) => (format!("theseus-{t}"), session::LONGEST),
            session::Kind::Job => (
                binding
                    .map_or("theseus-job", |b| b.correlation_id.as_str())
                    .to_string(),
                session::LONGEST,
            ),
        };
        let inline = match kind {
            session::Kind::Tender(t) => {
                tend::policy(t, &self.id).or_else(|| durable::policy(t, &self.id, &self.cfg))
            }
            _ => None,
        };
        let want = session::Want {
            kind,
            name: session::session_name(&name),
            execution,
            lasts,
            inline: inline.as_ref(),
        };
        self.session(&want, None, binding).await
    }

    /// The client: its catalog, and its checks before a call.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The region a call goes to: the one it names, if this account allows
    /// it, else the account's own.
    pub fn region(&self, named: Option<&str>) -> Result<String, String> {
        let allowed = self.cfg.allowed_regions();
        match named {
            None => Ok(self.cfg.region.clone()),
            Some(r) if allowed.iter().any(|a| a == r) => Ok(r.to_string()),
            Some(r) => Err(format!(
                "{r} is not one of account {}'s regions ({}): the operator adds a region to \
                 [aws.accounts.{}] regions",
                self.id,
                allowed.join(", "),
                self.id
            )),
        }
    }

    /// Start the check, once: the daemon after serving, or the first call.
    pub fn start_check(self: &Arc<Self>) {
        if !self.started.swap(true, Ordering::SeqCst) {
            tokio::spawn(self.clone().run_check());
        }
    }

    /// The check: the key, as its secrets resolve, then
    /// `sts:GetCallerIdentity`, which must name this account.
    async fn run_check(self: Arc<Self>) {
        let (key_id, secret) = match self.key().await {
            Ok(k) => k,
            Err(why) => return self.fail(why, None),
        };
        self.check.send_replace(Check::Checking);
        let creds = Credentials::new(key_id.expose(), secret.expose(), None, None);
        let attribution = Attribution {
            execution: None,
            call: Some(format!("check-{}", self.id)),
        };
        let input = json!({});
        let call = Call {
            service: "sts",
            operation: "GetCallerIdentity",
            input: &input,
            region: None,
            pages: 1,
            attribution: &attribution,
        };
        let t0 = Instant::now();
        let r = self.client.call(&call, &creds).await;
        self.count(r.is_err());
        let ms = t0.elapsed().as_millis() as u64;
        match r {
            Ok(out) => {
                let text = |k: &str| out.body[k].as_str().unwrap_or_default().to_string();
                let account = text("Account");
                if account != self.id {
                    return self.fail(
                        format!(
                            "its key is account {}'s, not {}: no call of {} signs until the \
                             operator gives it this account's key",
                            if account.is_empty() { "?" } else { &account },
                            self.id,
                            self.id
                        ),
                        None,
                    );
                }
                let arn = text("Arn");
                tracing::info!(account = %self.id, arn = %arn, ms, "aws: account bound");
                self.check.send_replace(Check::Bound(Bound {
                    creds,
                    arn,
                    user_id: text("UserId"),
                    at_ms: theseus_protocol::now_unix_ms(),
                }));
            }
            Err(e) => {
                // A refusal of the key (a wrong or disabled one) stays until
                // the key changes; the network, a throttle, or AWS's own
                // failure may pass.
                let recheck = match &e {
                    CallError::Aws(a) => (a.retry != theseus_aws::ErrorRetry::No)
                        .then(|| Instant::now() + RECHECK_AFTER),
                    CallError::NotSent { .. }
                    | CallError::OutcomeUnknown { .. }
                    | CallError::Unreadable { .. } => Some(Instant::now() + RECHECK_AFTER),
                    _ => None,
                };
                self.fail(format!("sts:GetCallerIdentity failed: {e}"), recheck);
            }
        }
    }

    fn fail(&self, why: String, recheck: Option<Instant>) {
        tracing::warn!(account = %self.id, error = %why, "aws: account not bound; its calls fail closed");
        self.check.send_replace(Check::Failed {
            why,
            at_ms: theseus_protocol::now_unix_ms(),
            recheck,
        });
    }

    /// The key's two secrets, as they resolve: the check waits through the
    /// board's retries, and says why while one has failed.
    async fn key(&self) -> Result<(Secret, Secret), String> {
        let names = &self.cfg.credentials;
        let mut rx = self.board.subscribe();
        loop {
            {
                let states = rx.borrow_and_update();
                let (a, b) = (
                    states.get(&names.access_key_id),
                    states.get(&names.secret_access_key),
                );
                match (a, b) {
                    (Some(SecretState::Ready(a)), Some(SecretState::Ready(b))) => {
                        return Ok((a.clone(), b.clone()));
                    }
                    (None, _) | (_, None) => {
                        return Err(format!(
                            "its key's [secrets] entries ({} and {}) are not on the board",
                            names.access_key_id, names.secret_access_key
                        ));
                    }
                    _ => {
                        let why = [(&names.access_key_id, a), (&names.secret_access_key, b)]
                            .into_iter()
                            .find_map(|(n, s)| match s {
                                Some(SecretState::Failed(e)) => Some(format!(
                                    "its secret {n} did not resolve ({e}); the vault is asked again"
                                )),
                                _ => None,
                            });
                        self.check.send_replace(Check::Waiting(why));
                    }
                }
            }
            if rx.changed().await.is_err() {
                return Err("the secrets board closed".into());
            }
        }
    }

    /// The key, for a call, once the check has passed: a call starts the
    /// check if nothing has (a test, or a daemon before its phase), runs it
    /// again when it failed for a reason that may pass, and waits for it at
    /// most `BIND_WAIT`.
    async fn credentials(self: &Arc<Self>) -> Result<Credentials, String> {
        self.start_check();
        let again = self.check.send_if_modified(|c| match c {
            Check::Failed {
                recheck: Some(at), ..
            } if Instant::now() >= *at => {
                *c = Check::Checking;
                true
            }
            _ => false,
        });
        if again {
            tokio::spawn(self.clone().run_check());
        }
        let mut rx = self.check.subscribe();
        // Settled, or out of time: what it says either way is the state now.
        let _ = tokio::time::timeout(BIND_WAIT, async {
            let _ = rx.wait_for(Check::settled).await;
        })
        .await;
        let c = self.check.borrow().clone();
        match c {
            Check::Bound(b) => Ok(b.creds),
            Check::Failed { why, .. } => {
                Err(format!("AWS account {} is not bound: {why}", self.id))
            }
            other => Err(format!(
                "AWS account {} is not bound yet: its check is {} after {} s{}",
                self.id,
                other.word(),
                BIND_WAIT.as_secs(),
                match other {
                    Check::Waiting(Some(why)) => format!(" ({why})"),
                    Check::Waiting(None) => " (for its key's secrets from the vault)".into(),
                    _ => String::new(),
                }
            )),
        }
    }

    fn count(&self, failed: bool) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if failed {
            self.failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// One request for a call: signed as `r.signer` says once the key's
    /// check has passed (the key, or a role session it mints), attributed to
    /// the call's execution and correlation id, and recorded on its binding
    /// (an `aws.called` row, and a span). An unbound account sends nothing,
    /// and that is recorded too.
    pub async fn request(
        self: &Arc<Self>,
        binding: Option<&AwsBinding>,
        r: &Request<'_>,
    ) -> Result<Output, Failure> {
        let started = Instant::now();
        let attribution = Attribution {
            execution: binding.map(|b| b.execution_id.clone()),
            call: binding.map(|b| b.correlation_id.clone()),
        };
        let result = match self.signing(r.signer, binding).await {
            Err(why) => Err(Failure::Unbound(why)),
            Ok(creds) => {
                let call = Call {
                    service: r.service,
                    operation: r.operation,
                    input: r.input,
                    region: Some(r.region),
                    pages: r.pages,
                    attribution: &attribution,
                };
                let out = self.client.call(&call, &creds).await;
                self.count(out.is_err());
                out.map_err(Failure::Call)
            }
        };
        if let Some(b) = binding {
            b.record(AwsRequest {
                started,
                ended: Instant::now(),
                row: self.row(b, r, &result, started),
            });
        }
        result
    }

    /// A request's `aws.called` row (§3.8): the account, region, operation,
    /// class, the call's ids, AWS's request id (CloudTrail's `requestID`),
    /// and how it went. Never a credential, and never the result.
    fn row(
        &self,
        b: &AwsBinding,
        r: &Request<'_>,
        result: &Result<Output, Failure>,
        started: Instant,
    ) -> Value {
        let mut row = json!({
            "account": self.id,
            "region": r.region,
            "service": r.service,
            "operation": r.operation,
            "class": r.class,
            "execution_id": b.execution_id,
            "correlation_id": b.correlation_id,
            "duration_ms": started.elapsed().as_millis() as u64,
        });
        if self.cfg.owner_role.is_some() {
            if let Signer::As(kind) = r.signer {
                row["session"] = json!(kind.as_str());
            }
        }
        match result {
            Ok(out) => {
                row["status"] = json!("ok");
                row["http_status"] = json!(out.status);
                row["request_id"] = json!(out.request_id);
                if out.request_ids.len() > 1 {
                    row["request_ids"] = json!(out.request_ids);
                }
                row["attempts"] = json!(out.attempts);
                row["pages"] = json!(out.pages);
                row["more"] = json!(out.next.is_some());
                row["result_bytes"] = json!(out.body.to_string().len());
            }
            Err(Failure::Unbound(why)) => {
                row["status"] = json!("unbound");
                row["sent"] = json!(false);
                row["error"] = json!(why);
            }
            Err(Failure::Call(e)) => {
                row["status"] = json!("error");
                row["sent"] = json!(!matches!(
                    e,
                    CallError::InvalidInput(_)
                        | CallError::Unsupported(_)
                        | CallError::NotSent { .. }
                ));
                row["may_have_run"] = json!(e.may_have_run());
                row["error"] = json!(e.to_string());
                if let Some(a) = e.aws() {
                    row["http_status"] = json!(a.status);
                    row["error_code"] = json!(a.code);
                    row["request_id"] = json!(a.request_id);
                    if let Some(d) = &a.denial {
                        row["enforcer"] = json!(d.enforcer.as_str());
                    }
                }
            }
        }
        row
    }

    /// The account as health shows it.
    pub fn status(&self) -> AwsAccountStatus {
        let c = self.check.borrow();
        let (arn, at, error) = match &*c {
            Check::Bound(b) => (Some(b.arn.clone()), Some(b.at_ms), None),
            Check::Failed { why, at_ms, .. } => (None, Some(*at_ms), Some(why.clone())),
            Check::Waiting(why) => (None, None, why.clone()),
            Check::Unchecked | Check::Checking => (None, None, None),
        };
        let t = self.tended.lock().unwrap();
        AwsAccountStatus {
            account: self.id.clone(),
            region: self.cfg.region.clone(),
            regions: self.cfg.allowed_regions(),
            state: c.word().into(),
            arn,
            checked_at_unix_ms: at,
            error,
            calls: self.calls.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            signer: Some(self.signer()),
            budget: t.budget.clone(),
            guardduty: t.guardduty.clone(),
            reconcile: t.reconcile.clone(),
            durability: t.durability.clone().map(durable::with_lag),
            hands: t.hands.clone(),
        }
    }

    /// The key's IAM user, from the check's ARN (`arn:aws:iam::…:user/x`):
    /// the bootstrap's `OwnerUserName`.
    pub fn user_name(&self) -> Option<String> {
        let (arn, _) = self.identity()?;
        arn.split_once(":user/")
            .map(|(_, path)| path.rsplit('/').next().unwrap_or(path).to_string())
    }

    /// The key, once its check has passed: for the bootstrap's foundation.
    pub async fn root_key(self: &Arc<Self>) -> Result<Credentials, String> {
        self.credentials().await
    }

    /// Start the check if nothing has, and wait for it to settle, however
    /// long the vault takes: whether the key is bound. The tenders' wait.
    pub async fn settled(self: &Arc<Self>) -> bool {
        self.start_check();
        let mut rx = self.check.subscribe();
        let _ = rx.wait_for(Check::settled).await;
        let bound = matches!(&*self.check.borrow(), Check::Bound(_));
        bound
    }

    /// Who the key is, as the check found it: its ARN and user id.
    pub fn identity(&self) -> Option<(String, String)> {
        match &*self.check.borrow() {
            Check::Bound(b) => Some((b.arn.clone(), b.user_id.clone())),
            _ => None,
        }
    }
}
