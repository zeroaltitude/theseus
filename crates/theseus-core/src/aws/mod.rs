//! AWS (AWS design §3; row 29, C1 = 14a): the accounts Theseus owns, and the
//! tools that read them.
//!
//! - **The accounts.** One [`Account`] per `[aws.accounts.<id>]` table. Its
//!   key, the root of trust, comes from the secrets board after serving, and
//!   is checked once with `sts:GetCallerIdentity`, which must name this
//!   account. Until the check passes no call of the account signs: a call
//!   waits for it, at most [`BIND_WAIT`], then fails as unbound, and says
//!   why. Health shows each account's state (fail closed, §3.5). Until the
//!   foundation stack exists (14b), the key signs these reads directly.
//! - **The tools** ([`tools`]): `aws.call` (any read of any service),
//!   `aws.describe` (the catalog, local), `aws.whoami`, and `aws.s3.list`.
//!   Until 14b brings the guards, a call that writes, runs code, or returns a
//!   secret is invalid input that names the step that brings it.
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

pub mod tools;

#[cfg(test)]
mod tests;

/// The AWS tools' names, for the config's check of `[policy.tools]`.
pub const NAMES: [&str; 4] = ["aws.call", "aws.describe", "aws.s3.list", "aws.whoami"];

/// The tools whose calls are AWS requests (`aws.describe` reads the local
/// catalog). Until 14b every such call is a read, so `[policy.aws] read` is
/// their posture short of a line for the call's service or operation.
pub const CALLS: [&str; 3] = ["aws.call", "aws.s3.list", "aws.whoami"];

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
        }))
    }

    /// `aws.call`, `aws.describe`, `aws.s3.list`, and `aws.whoami`.
    pub fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        tools::all(self)
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
    /// `read` (until 14b, every call's).
    pub class: &'a str,
}

/// One bound account: its key's check, its client, and its counts.
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
        }
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

    /// One request for a call: signed with the account's key once its check
    /// has passed, attributed to the call's execution and correlation id,
    /// and recorded on its binding (an `aws.called` row, and a span). An
    /// unbound account sends nothing, and that is recorded too.
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
        let result = match self.credentials().await {
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
        }
    }

    /// Who the key is, as the check found it: its ARN and user id.
    pub fn identity(&self) -> Option<(String, String)> {
        match &*self.check.borrow() {
            Check::Bound(b) => Some((b.arn.clone(), b.user_id.clone())),
            _ => None,
        }
    }
}
