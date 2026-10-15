//! The hands broker (theseus-pw1q.14; rsi-plan §3, wave 2): an extension the
//! owner loaded with `hands` may ask the daemon for hands, through one
//! narrow channel, and never holds what launches them.
//!
//! **The channel** is the extension's own MCP connection: the daemon is its
//! client, and answers three requests of the server's own (`hands.run`,
//! `hands.status`, `hands.cancel`, `theseus_mcp::client::Serve`) on the
//! stdio pipe it spawned the process with. No path, port or socket is made:
//! nothing else on the machine can reach it, nothing inside L1's view names
//! it, and its identity is the pipe, which only that process holds. The
//! board gives it to a server loaded with `hands` alone (`Core::board_load`);
//! any other server's request of these methods is "method not found".
//! Revoking the extension closes the pipe with the process.
//!
//! **What it never gets**: AWS credentials, the hands' root key, a hand's
//! per-dispatch key, or the daemon's socket. Its hands run the hand image
//! under the `basic` profile on the default account and region: `image`,
//! `profile`, `account` and `region` are refused.
//!
//! **Each request** is checked against the loaded record (the frozen
//! manifest the owner acked): its backend among `backends`, its hands with
//! those running now at most `max_group`; then each hand's worst case
//! reserves against **both** the extension's own account and the account's
//! global lines. The extension's account is its hands session for the local
//! day, a harness session of its own whose execution's limit is pinned at
//! `usd_cap`: the kernel refuses a reservation past it in the group's own
//! frame, so two requests cannot both take the last dollar. The global
//! lines are runaway mode's (`aws::hands::runaway`), as for any launch.
//! Then the group starts through `aws.hands.run`'s own path
//! (`aws::hands::start`), and its hands' envelopes are checked by the
//! poller as every hand's are.
//!
//! **Depth one.** A hand's spec and environment carry nothing that reaches
//! the broker: no socket, no key but its own dispatch's, no name of the
//! extension's connection. A request that says it comes from a hand
//! (`hand`) is refused and ledgered. Nothing on the channel grants a
//! capability: only the owner's ack of a proposal (`extend.propose`, a
//! model's tool) does, and the self loop's `"self"` principal acks nothing.

use std::sync::{Arc, Weak};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{Authority, Completion, Outcome, RetryClass, TurnEnd, Wake};
use theseus_protocol::SessionOpenParams;

use super::hands::HandsCap;
use super::load::{Loaded, LoadedSet};
use crate::aws::hands::group::{self, GroupRecord};
use crate::aws::hands::launch::{self, RUN};
use crate::aws::hands::start;
use crate::fact::extend_hands::{HandsLaunched, HandsRefused};
use crate::rpc::Core;

/// The requests the channel answers.
pub const METHODS: &[&str] = &["hands.run", "hands.status", "hands.cancel"];

/// A refusal's JSON-RPC code: a server-defined one.
pub const REFUSED: i64 = -32001;

/// The most refusals of one extension's the ledger takes a minute: a loop of
/// refused requests is answered every time, but past this it writes no row
/// (and no frame) until the next minute, whose first row counts what it left.
pub const REFUSALS_A_MINUTE: u32 = 20;

/// Each extension's refusals this minute: the minute, the rows written, and
/// those left unwritten.
#[derive(Default)]
pub struct Refusals(std::sync::Mutex<std::collections::BTreeMap<String, (u64, u32, u32)>>);

impl Refusals {
    /// Whether `name`'s refusal at `now_ms` is ledgered: `Some` with the
    /// count of refusals the minutes before it left unwritten, or `None`.
    pub fn admit(&self, name: &str, now_ms: u64) -> Option<u32> {
        let minute = now_ms / 60_000;
        let mut m = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let e = m.entry(name.to_string()).or_insert((minute, 0, 0));
        if e.0 != minute {
            (e.0, e.1) = (minute, 0);
        }
        if e.1 >= REFUSALS_A_MINUTE {
            e.2 += 1;
            return None;
        }
        e.1 += 1;
        Some(std::mem::take(&mut e.2))
    }
}

/// The principal of an extension's hands sessions: never an owner's handle.
pub fn principal(name: &str) -> String {
    format!("ext:{name}")
}

/// The META key of an extension's hands book.
pub fn book_key(name: &str) -> String {
    format!("extend.hands.{name}")
}

/// One local day's hands session of an extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaySession {
    /// The local day's start, in ms.
    pub day_ms: u64,
    pub session_id: String,
    pub execution_id: String,
}

/// An extension's hands sessions, a META record: each day it ran hands.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub sessions: Vec<DaySession>,
}

impl Book {
    pub fn read(store: &crate::store::Store, name: &str) -> anyhow::Result<Self> {
        Ok(store.get_meta(&book_key(name))?.unwrap_or_default())
    }

    fn owns(&self, rec: &GroupRecord) -> bool {
        self.sessions.iter().any(|s| s.session_id == rec.session_id)
    }
}

/// One extension's channel: its connection's answers, bound to its name.
pub struct Channel {
    core: Weak<Core>,
    name: String,
}

impl Channel {
    pub fn served(core: Weak<Core>, name: &str) -> theseus_mcp::client::Served {
        theseus_mcp::client::Served(Arc::new(Self {
            core,
            name: name.into(),
        }))
    }
}

impl theseus_mcp::client::Serve for Channel {
    fn methods(&self) -> &'static [&'static str] {
        METHODS
    }

    fn answer(
        &self,
        method: String,
        params: Value,
    ) -> futures_util::future::BoxFuture<'static, Result<Value, (i64, String)>> {
        let (core, name) = (self.core.clone(), self.name.clone());
        Box::pin(async move {
            let Some(core) = core.upgrade() else {
                return Err((REFUSED, "the daemon is stopping".into()));
            };
            core.extension_hands(&name, &method, params).await
        })
    }
}

/// A refusal: its reason's word and its words.
struct Refusal {
    reason: &'static str,
    why: String,
}

fn refuse(reason: &'static str, why: impl Into<String>) -> Refusal {
    Refusal {
        reason,
        why: why.into(),
    }
}

/// `hands.run`'s request: `aws.hands.run`'s fields an extension may name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunSpec {
    argv: Vec<String>,
    #[serde(default)]
    count: Option<u64>,
    #[serde(default)]
    inputs: Option<Vec<Value>>,
    #[serde(default)]
    vcpu: Option<f64>,
    #[serde(default)]
    memory_mb: Option<u64>,
    #[serde(default)]
    ttl_secs: Option<u64>,
    #[serde(default)]
    max_usd: Option<f64>,
    #[serde(default)]
    until: Value,
    #[serde(default)]
    concurrency: Option<u32>,
    #[serde(default)]
    backend: Option<String>,
    /// The hand a relayed request comes from: always refused (depth one).
    #[serde(default)]
    hand: Option<String>,
}

/// `hands.status` and `hands.cancel`'s request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ById {
    id: String,
}

impl Core {
    /// One request of extension `name`'s channel: its answer, or a refusal
    /// (ledgered, with its reason).
    pub async fn extension_hands(
        self: &Arc<Self>,
        name: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, (i64, String)> {
        let out = match method {
            "hands.run" => self.hands_run(name, params).await,
            "hands.status" => self.hands_status(name, params),
            "hands.cancel" => self.hands_cancel(name, params).await,
            other => Err(refuse("invalid", format!("no method {other}"))),
        };
        out.map_err(|r| {
            let now = theseus_protocol::now_unix_ms();
            if let Some(unledgered) = self.tools.extend.refusals.admit(name, now) {
                self.rec(None).record(&HandsRefused {
                    name,
                    method,
                    reason: r.reason,
                    why: &r.why,
                    unledgered,
                });
            }
            (REFUSED, r.why)
        })
    }

    /// The loaded extension `name` and its grant, or why it has none.
    fn granted(&self, name: &str) -> Result<(Loaded, HandsCap), Refusal> {
        let set = LoadedSet::read(&self.store).map_err(|e| refuse("unavailable", e.to_string()))?;
        let Some(l) = set.loaded.get(name) else {
            return Err(refuse(
                "revoked",
                format!("extension {name} is not loaded: it was revoked, or never acked"),
            ));
        };
        let Some(cap) = l.capabilities.hands.clone() else {
            return Err(refuse(
                "not_granted",
                format!("extension {name} was loaded without hands"),
            ));
        };
        Ok((l.clone(), cap))
    }

    async fn hands_run(self: &Arc<Self>, name: &str, params: Value) -> Result<Value, Refusal> {
        let spec: RunSpec = serde_json::from_value(params).map_err(|e| {
            refuse(
                "invalid",
                format!(
                    "hands.run takes argv, count or inputs, vcpu, memory_mb, ttl_secs, max_usd, \
                     until, concurrency and backend ({e}); its hands run the hand image under the \
                     basic profile"
                ),
            )
        })?;
        if let Some(h) = &spec.hand {
            return Err(refuse(
                "from_hand",
                format!("a hand cannot launch a hand (depth one): the request came from {h}"),
            ));
        }
        let (_, cap) = self.granted(name)?;
        let input = json!({
            "argv": spec.argv, "count": spec.count, "inputs": spec.inputs, "vcpu": spec.vcpu,
            "memory_mb": spec.memory_mb, "ttl_secs": spec.ttl_secs, "max_usd": spec.max_usd,
            "until": spec.until, "concurrency": spec.concurrency, "backend": spec.backend,
        });
        let input = strip_nulls(input);
        let r = launch::parse(&input).map_err(|e| refuse("invalid", e))?;
        let backend = launch::choose(&r).map_err(|e| refuse("invalid", e))?;
        if !cap.allows(backend) {
            return Err(refuse(
                "backend",
                format!(
                    "extension {name} may run hands on {} only, not {}",
                    cap.backends
                        .iter()
                        .map(|b| b.as_str())
                        .collect::<Vec<_>>()
                        .join(" or "),
                    backend.as_str()
                ),
            ));
        }
        let aws = self
            .tools
            .aws
            .clone()
            .ok_or_else(|| refuse("unavailable", "no AWS account is bound ([aws.accounts])"))?;
        // One launch at a time: the day's session holds one turn, and a
        // revoke's sweep waits for a launch under way.
        let _one = self.tools.extend.launches.lock().await;
        let (_, cap) = self.granted(name)?;
        let n = r.inputs.len() as u32;
        let running = self
            .hands_running(name)
            .map_err(|e| refuse("unavailable", e))?;
        if running + n > cap.max_group {
            return Err(refuse(
                "group_size",
                format!(
                    "extension {name} runs at most {} at a time: {running} run now, and this \
                     group has {n}",
                    crate::narrative::count(u64::from(cap.max_group), "hand", "hands")
                ),
            ));
        }
        let day = self
            .hands_day(name, &cap)
            .map_err(|e| refuse("unavailable", format!("{e:#}")))?;
        self.launch_for(name, &aws, &day, r, backend).await
    }

    /// The hands of `name`'s open groups that run or wait now.
    fn hands_running(&self, name: &str) -> Result<u32, String> {
        let book = Book::read(&self.store, name).map_err(|e| e.to_string())?;
        let now = theseus_protocol::now_unix_ms();
        let mut n = 0;
        for g in
            crate::aws::hands::watch::groups_from(&self.store, now).map_err(|e| e.to_string())?
        {
            if g.settled.is_some() || !book.owns(&g) {
                continue;
            }
            let actions = group::hands(&self.kernel, &g).map_err(|e| e.to_string())?;
            let t = group::tally(&self.store, &g, &actions);
            n += t.running + t.waiting.len() as u32;
        }
        Ok(n)
    }

    /// Today's hands session of `name`: its execution's limit pinned at the
    /// day's cap, opened at the day's first launch.
    fn hands_day(&self, name: &str, cap: &HandsCap) -> anyhow::Result<DaySession> {
        let day_ms = crate::aws::hands::runaway::day_of(theseus_protocol::now_unix_ms());
        let mut book = Book::read(&self.store, name)?;
        if let Some(d) = book.sessions.iter().find(|s| s.day_ms == day_ms) {
            return Ok(d.clone());
        }
        let authority = Authority {
            principal: principal(name),
            ..Default::default()
        };
        let rec = self.open_session_as(
            SessionOpenParams {
                dir: None,
                kind: None,
                label: Some(format!("extension {name}'s hands, one local day")),
                opened_from: None,
                persona: None,
            },
            authority,
            Some(cap.cap_micros()),
        )?;
        let d = DaySession {
            day_ms,
            session_id: rec.session_id.clone(),
            execution_id: rec.execution_id.unwrap_or_default(),
        };
        book.sessions.push(d.clone());
        self.store.put_meta(&book_key(name), &book)?;
        Ok(d)
    }

    /// Plan and launch `r` on the day's session: one short turn holds it
    /// while the group's call and its hands are planned, then ends.
    async fn launch_for(
        self: &Arc<Self>,
        name: &str,
        aws: &Arc<crate::aws::Aws>,
        day: &DaySession,
        r: launch::HandsRequest,
        backend: launch::Backend,
    ) -> Result<Value, Refusal> {
        let unavailable = |e: anyhow::Error| refuse("unavailable", format!("{e:#}"));
        let guard = self
            .kernel
            .admit_input(&day.execution_id)
            .map_err(unavailable)?;
        let n = r.inputs.len();
        let proposal = theseus_kernel::Proposal {
            tool: RUN.into(),
            args: json!({"extension": name, "request": r}),
            resource: Some(super::load::server_name(name)),
            policy_context: Value::Null,
        };
        let call = self.kernel.plan_and_dispatch(
            &guard,
            &proposal,
            RetryClass::NonRepeatable,
            None,
            0,
            |_| Ok(vec![]),
        );
        let over_cap = std::sync::atomic::AtomicBool::new(false);
        let planned = match &call {
            Err(e) => Err(format!("{e:#}")),
            Ok(call) => {
                let over = |need: u64| self.over_cap(name, &day.execution_id, need, &over_cap);
                let starter = start::Starter {
                    kernel: &self.kernel,
                    store: &self.store,
                    outbox: &self.outbox,
                    rec: self.rec(Some(&day.session_id)),
                    guard: &guard,
                    execution_id: &day.execution_id,
                    session_id: &day.session_id,
                    extension: Some(name),
                };
                let id = call.correlation_id.as_str();
                match start::plan(&starter, aws, id, id, r, backend, &over).await {
                    Ok(p) => p,
                    Err(e) => Err(format!("{e:#}")),
                }
            }
        };
        let ended = self
            .kernel
            .end_turn(guard, TurnEnd::Wait { wake: Wake::Input });
        let call = call.map_err(unavailable)?;
        ended.map_err(unavailable)?;
        let planned = match planned {
            Ok(p) => p,
            Err(why) => {
                self.settle_refused(&call.correlation_id, &why);
                let reason = match () {
                    () if over_cap.load(std::sync::atomic::Ordering::Relaxed) => "cap",
                    () if why.contains("runaway mode") => "runaway",
                    () => "unavailable",
                };
                return Err(refuse(reason, why));
            }
        };
        let worst = planned.reserve.saturating_mul(n as u64);
        let group = planned.rec.group.clone();
        let ctx = group::Ctx {
            kernel: &self.kernel,
            store: &self.store,
            aws,
        };
        start::launch_first(&ctx, planned).await;
        let left = self
            .kernel
            .execution(&day.execution_id)
            .ok()
            .flatten()
            .map_or(0, |e| e.budget.available());
        self.rec(Some(&day.session_id)).record(&HandsLaunched {
            name,
            group: &group,
            backend: backend.as_str(),
            hands: n,
            worst_micros: worst,
            left_micros: left,
        });
        Ok(json!({"id": group, "hands": n, "backend": backend.as_str(),
            "worst_usd": worst as f64 / 1e6, "left_today_usd": left as f64 / 1e6}))
    }

    /// Why a group whose worst case is `need` does not fit the extension's
    /// day: its cap's words, with `hit` set; or `None` when it fits.
    fn over_cap(
        &self,
        name: &str,
        execution_id: &str,
        need: u64,
        hit: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<Option<String>> {
        let Some(e) = self.kernel.execution(execution_id)? else {
            return Ok(None);
        };
        let b = &e.budget;
        if need <= b.available() {
            return Ok(None);
        }
        hit.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(Some(format!(
            "over extension {name}'s day cap: the group's worst case is {}, and {} of its {} is \
             left today",
            crate::narrative::dollars(need),
            crate::narrative::dollars(b.available()),
            crate::narrative::dollars(b.limit_micros),
        )))
    }

    /// The group's call, refused before any hand: settled failed, at nothing.
    fn settle_refused(&self, call: &str, why: &str) {
        let now = theseus_protocol::now_unix_ms();
        let c = Completion {
            correlation_id: call.into(),
            outcome: Outcome::Failed,
            result_ref: None,
            external_op_id: None,
            started_at_ms: now,
            finished_at_ms: now,
            producer: "extend:broker".into(),
            signature: None,
            cost_micros: Some(0),
            detail: Some(json!({"error": why})),
        };
        if let Err(e) = self.kernel.accept_completion(&c) {
            tracing::warn!(call, error = %format!("{e:#}"), "extend: a refused group's call");
        }
    }

    /// Its own group `p.id`, or why not.
    fn own_group(&self, name: &str, params: Value) -> Result<GroupRecord, Refusal> {
        let p: ById = serde_json::from_value(params)
            .map_err(|e| refuse("invalid", format!("give the group's id ({e})")))?;
        let book =
            Book::read(&self.store, name).map_err(|e| refuse("unavailable", e.to_string()))?;
        match GroupRecord::load(&self.store, &p.id) {
            Ok(Some(g)) if book.owns(&g) => Ok(g),
            Ok(_) => Err(refuse(
                "not_its_group",
                format!("{} is no hands group of extension {name}'s", p.id),
            )),
            Err(e) => Err(refuse("unavailable", e.to_string())),
        }
    }

    /// `hands.status {id}`: the group as it is, with each hand's envelope
    /// as the poller checked it, once settled. A revoked extension still
    /// reads its own groups while its connection lasts.
    fn hands_status(&self, name: &str, params: Value) -> Result<Value, Refusal> {
        let g = self.own_group(name, params)?;
        let actions =
            group::hands(&self.kernel, &g).map_err(|e| refuse("unavailable", e.to_string()))?;
        let t = group::tally(&self.store, &g, &actions);
        let detail = group::completion(&self.store, &g.group).and_then(|c| c.detail);
        Ok(json!({
            "id": g.group, "backend": g.backend.as_str(), "settled": g.settled,
            "succeeded": t.succeeded, "failed": t.failed, "cancelled": t.cancelled,
            "running": t.running, "waiting": t.waiting.len(), "not_launched": t.not_launched,
            "spent_usd": t.spent_usd,
            "result": detail.as_ref().map(group::result_text),
            "hands": detail.as_ref().and_then(|d| d.get("hands").cloned()),
        }))
    }

    /// `hands.cancel {id}`: its group stopped, by each backend's means.
    async fn hands_cancel(self: &Arc<Self>, name: &str, params: Value) -> Result<Value, Refusal> {
        let g = self.own_group(name, params)?;
        let aws = self
            .tools
            .aws
            .clone()
            .ok_or_else(|| refuse("unavailable", "no AWS account is bound"))?;
        let ctx = group::Ctx {
            kernel: &self.kernel,
            store: &self.store,
            aws: &aws,
        };
        let stopped = crate::aws::hands::cancel::stop_group(&ctx, &g.group)
            .await
            .map_err(|e| refuse("unavailable", format!("{e:#}")))?;
        let rows: Vec<_> = stopped
            .iter()
            .filter_map(|(a, _)| crate::cancel::verdict_row(a))
            .collect();
        if !rows.is_empty() {
            if let Err(e) = self.store.append(&rows) {
                tracing::warn!(error = %format!("{e:#}"), "extend: a cancel's verdicts");
            }
        }
        let call = stopped.last().map(|(a, _)| a.clone());
        Ok(json!({"id": g.group, "cancel": call.as_ref().map(crate::cancel::words)}))
    }

    /// `extend.list`'s hands of a loaded extension: its grant, today's spend
    /// and hold against the cap, every day's spend, and its hands now.
    pub(crate) fn hands_info(
        &self,
        l: &Loaded,
    ) -> Option<theseus_protocol::extend::ExtendHandsInfo> {
        let cap = l.capabilities.hands.as_ref()?;
        let book = Book::read(&self.store, &l.name).unwrap_or_default();
        let today = crate::aws::hands::runaway::day_of(theseus_protocol::now_unix_ms());
        let (mut today_micros, mut spent_micros) = (0u64, 0u64);
        for d in &book.sessions {
            let Ok(Some(e)) = self.kernel.execution(&d.execution_id) else {
                continue;
            };
            let b = &e.budget;
            spent_micros += b.spent_micros;
            if d.day_ms == today {
                today_micros = b.spent_micros + b.reserved_micros + b.held_unknown_micros;
            }
        }
        Some(theseus_protocol::extend::ExtendHandsInfo {
            usd_cap: cap.usd_cap,
            backends: cap
                .backends
                .iter()
                .map(|b| b.as_str().to_string())
                .collect(),
            max_group: cap.max_group,
            today_usd: today_micros as f64 / 1e6,
            spent_usd: spent_micros as f64 / 1e6,
            running: self.hands_running(&l.name).unwrap_or_default(),
        })
    }

    /// A revoke's sweep: every open group of `name`'s stopped, after any
    /// launch under way. Spawned by the revoke, which is not async.
    pub(crate) async fn hands_revoked(self: Arc<Self>, name: String) {
        let _one = self.tools.extend.launches.lock().await;
        let Some(aws) = self.tools.aws.clone() else {
            return;
        };
        let Ok(book) = Book::read(&self.store, &name) else {
            return;
        };
        let now = theseus_protocol::now_unix_ms();
        let Ok(groups) = crate::aws::hands::watch::groups_from(&self.store, now) else {
            return;
        };
        let ctx = group::Ctx {
            kernel: &self.kernel,
            store: &self.store,
            aws: &aws,
        };
        for g in groups
            .iter()
            .filter(|g| g.settled.is_none() && book.owns(g))
        {
            match crate::aws::hands::cancel::stop_group(&ctx, &g.group).await {
                Ok(stopped) => {
                    let rows: Vec<_> = stopped
                        .iter()
                        .filter_map(|(a, _)| crate::cancel::verdict_row(a))
                        .collect();
                    if !rows.is_empty() {
                        let _ = self.store.append(&rows);
                    }
                }
                Err(e) => {
                    tracing::warn!(group = g.group, error = %format!("{e:#}"), "extend: a revoke's cancel")
                }
            }
        }
    }
}

/// `v` without its null fields, as `launch::parse` reads absent ones.
fn strip_nulls(mut v: Value) -> Value {
    if let Value::Object(m) = &mut v {
        m.retain(|_, x| !x.is_null());
    }
    v
}
