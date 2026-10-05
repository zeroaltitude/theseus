//! Self-extension, steps 43a and 43b (M7 §2.7; theseus-ext.5, theseus-ext.8):
//! `extend.propose` proposes a small MCP server the model wrote, and puts it
//! to the operator; the ack loads it (`load.rs`), and `extension.revoke`
//! unloads it (`revoke.rs`). "Planks, never the keel": nothing here compiles
//! or restarts Theseus.
//!
//! 1. **The tool.** `extend.propose { name, dir, command, description,
//!    tests?, network? }`, run by the harness: class `Run`, posture `notify`
//!    in the template's `[policy.tools]`, and waiting under T1's hold like any
//!    `Run` call. `dir` must be inside the workspace roots.
//! 2. **Freeze** (`freeze.rs`): `dir` is copied into
//!    `<state>/extensions/<name>/<digest>/`, read-only, the digest a SHA-256
//!    over the tree, so a later edit in the workspace changes nothing that
//!    runs.
//! 3. **Start and test**: the board starts the frozen copy as `ext-<name>`
//!    in L1 (`McpBoard::trial`, the `mcp-sandbox` role), in state `proposed`,
//!    whose tools no turn is offered; then `initialize`, `tools/list`, and
//!    each declared test as a `tools/call`; then it stops it. Its network is
//!    the proposal's `network` list, through the egress proxy; none unless
//!    asked. It is granted no secret.
//! 4. **The manifest**: a META record, `extend.manifest.<name>.<digest>`,
//!    beside 36b's `mcp.tools.<server>` (no new record kind, and no node: a
//!    node of its own would need a body and a trust field no node has). It
//!    holds the name, the digest, the command, the tools, the tests and their
//!    results, the capabilities asked for, and the proposing session and its
//!    principal; the proposing call's result shows it.
//! 5. **The ack**: a question, a planned `extend.ack` action on the proposing
//!    execution with its card, in the frame that writes the manifest, and
//!    answered as every question is (`action.confirm`: Discord's buttons,
//!    `theseus confirm`, the cockpit's card), judged by the place rule
//!    (`judge_act`): the owner, from a private place. The CLI refuses it in a
//!    job's shell (`THESEUS_SESSION`), and L1 has no route to the daemon. An
//!    ack binds the question's confirm and writes `extend.acked`; a decline,
//!    or no answer within the question's time, writes `extend.declined`.
//!    Neither wakes the execution. An ack loads it (43b, `load.rs`).
//!
//! Its rows are `extend.proposed`, `extend.tested`, `extend.acked`,
//! `extend.declined`, `extend.loaded`, and `extend.revoked` (`fact::extend`).

pub mod answer;
pub mod freeze;
pub mod list;
pub mod load;
pub mod revoke;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_load;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{Action, Proposal, RetryClass};
use theseus_protocol::ConfirmRequest;
use theseus_store::{kinds, NewRecord};
use theseus_tools::{parse, Access, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx};

use crate::config::McpServerConfig;
use crate::fact::extend::{short, ExtendProposed, ExtendTested};
use crate::mcp::McpBoard;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const PROPOSE: &str = "extend.propose";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 1] = [PROPOSE];
/// The question an ack answers: a planned action of this tool.
pub const ACK: &str = "extend.ack";
/// Each proposal's manifest: `extend.manifest.<name>.<digest>`.
pub const MANIFEST_PREFIX: &str = "extend.manifest.";
/// An extension's server is `ext-<name>` on the board.
pub const SERVER_PREFIX: &str = "ext-";
/// The longest a name may be, so `ext-<name>` is a server's name.
pub const MAX_NAME: usize = 28;
pub const MAX_TESTS: usize = 20;
pub const MAX_DESCRIPTION_CHARS: usize = 1_000;
/// The longest the trial waits for the server's handshake and list, and for
/// each test's call: the whole trial stays under the 120 s a harness call
/// may take.
pub const START_TIMEOUT_SECS: u64 = 30;
pub const CALL_TIMEOUT_SECS: u64 = 20;
pub const TRIAL_LIMIT: Duration = Duration::from_secs(100);
/// What a proposal whose trial a cancel or a stop aborted returns: the
/// runtime then answers the call as the cancel settled it.
pub const STOPPED: &str = "stopped while it was tried";
/// The most of a test's answer the manifest keeps.
const GOT_CHARS: usize = 500;

/// One declared test: a `tools/call`, and what its text must be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestSpec {
    pub tool: String,
    #[serde(default = "empty_object")]
    pub arguments: Value,
    pub expect: Expect,
}

fn empty_object() -> Value {
    json!({})
}

/// What a test's answer must be: its text contains, or equals, this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Expect {
    Contains(String),
    Equals(String),
}

/// A test, run: whether it passed, what came back, and why it failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestRun {
    #[serde(flatten)]
    pub spec: TestSpec,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub got: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// A tool the server listed, as the manifest keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

/// What a proposal asks for besides its tools.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    /// The hosts its egress proxy lets it reach; empty: no network.
    #[serde(default)]
    pub network: Vec<String>,
    /// Scratch for its writes, which the view discards: always, as a job's.
    pub scratch: bool,
}

/// Who proposed it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProposedBy {
    pub session_id: String,
    pub execution_id: String,
    pub correlation_id: String,
    /// The proposing execution's principal.
    pub principal: String,
    /// Where the session speaks, when it has a place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
}

/// One proposal's manifest: the META record `extend.manifest.<name>.<digest>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub digest: String,
    pub description: String,
    pub command: Vec<String>,
    /// The workspace directory it was frozen from.
    pub source: String,
    /// The frozen copy that runs.
    pub frozen: String,
    pub files: usize,
    pub bytes: u64,
    pub tools: Vec<ToolInfo>,
    pub tests: Vec<TestRun>,
    /// Why it did not come up in L1, when it did not: then nothing is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub capabilities: Capabilities,
    pub proposed_by: ProposedBy,
    pub proposed_at_ms: u64,
    /// The question that asks the operator; none when it did not come up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// `proposed`, `failed` (it did not come up), `acked`, or `declined`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Manifest {
    pub fn key(&self) -> String {
        manifest_key(&self.name, &self.digest)
    }

    pub fn passed(&self) -> usize {
        self.tests.iter().filter(|t| t.passed).count()
    }

    /// The card's question: "Load wordcount 3f2a1c: 1 tool, 3 of 3 tests
    /// passed, no network?"
    pub fn question_text(&self) -> String {
        let network = match self.capabilities.network.as_slice() {
            [] => "no network".to_string(),
            hosts => format!("network to {}", hosts.join(", ")),
        };
        format!(
            "Load {} {}: {}, {} of {} passed, {network}?",
            self.name,
            short(&self.digest),
            crate::narrative::count(self.tools.len() as u64, "tool", "tools"),
            self.passed(),
            crate::narrative::count(self.tests.len() as u64, "test", "tests"),
        )
    }

    pub fn record(&self) -> anyhow::Result<NewRecord> {
        NewRecord::json(kinds::META, Some(&self.key()), self)
    }
}

pub fn manifest_key(name: &str, digest: &str) -> String {
    format!("{MANIFEST_PREFIX}{name}.{digest}")
}

/// Every manifest in the store, newest proposal first.
pub fn manifests(store: &crate::store::Store) -> anyhow::Result<Vec<Manifest>> {
    use theseus_store::Store as _;
    let mut out: Vec<Manifest> = store
        .inner()
        .latest_with_prefix(kinds::META, MANIFEST_PREFIX)?
        .into_iter()
        .filter_map(|r| r.decode::<Manifest>().ok())
        .collect();
    out.sort_by_key(|m| std::cmp::Reverse(m.proposed_at_ms));
    Ok(out)
}

/// Where proposals are frozen, the roots their directories must be in, and
/// the board that tries them.
pub struct Extensions {
    /// `<state>/extensions`.
    pub dir: PathBuf,
    roots: Vec<PathBuf>,
    board: OnceLock<Weak<McpBoard>>,
    /// The ceilings loaded extensions hold their calls to (43b).
    pub floors: load::Floors,
    /// Held across each read and write of the `extensions` record: a load
    /// and a revoke never interleave.
    pub(crate) writes: std::sync::Mutex<()>,
}

impl Extensions {
    pub fn new(dir: PathBuf, roots: Vec<PathBuf>) -> Self {
        Self {
            dir,
            roots,
            board: OnceLock::new(),
            floors: Default::default(),
            writes: Default::default(),
        }
    }

    /// The board, once the core has one; held weakly, as the board holds
    /// the core.
    pub fn attach(&self, board: &Arc<McpBoard>) {
        let _ = self.board.set(Arc::downgrade(board));
    }

    fn board(&self) -> Option<Arc<McpBoard>> {
        self.board.get().and_then(Weak::upgrade)
    }

    /// `dir` resolved, if it is a directory inside a workspace root.
    fn inside(&self, ctx: &ToolCtx, dir: &str) -> Result<PathBuf, String> {
        let p = ctx.resolve(dir);
        let roots = if self.roots.is_empty() {
            &ctx.roots
        } else {
            &self.roots
        };
        if !roots.iter().any(|r| theseus_tools::paths::within(&p, r)) {
            return Err(format!(
                "{} is outside the workspace roots: propose a directory inside them",
                p.display()
            ));
        }
        Ok(p)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    name: String,
    dir: String,
    command: Vec<String>,
    description: String,
    #[serde(default)]
    tests: Vec<TestSpec>,
    #[serde(default)]
    network: Vec<String>,
}

fn input_of(input: &Value) -> Result<Input, String> {
    let i: Input = parse(input)?;
    if !crate::config::mcp::server_name_ok(&i.name) || i.name.len() > MAX_NAME {
        return Err(format!(
            "the name {:?} is letters, digits, `_` and `-` (no `__`), at most {MAX_NAME} \
             characters: its tools are mcp:{SERVER_PREFIX}<name>/<tool>",
            i.name
        ));
    }
    if i.command.first().is_none_or(|p| p.trim().is_empty()) {
        return Err(
            "the command names no program: give its argv, such as [\"python3\", \"server.py\"]"
                .into(),
        );
    }
    let n = i.description.trim().chars().count();
    if n == 0 || n > MAX_DESCRIPTION_CHARS {
        return Err(format!(
            "the description says what it is for, in 1 to {MAX_DESCRIPTION_CHARS} characters"
        ));
    }
    if i.tests.len() > MAX_TESTS {
        return Err(format!("at most {MAX_TESTS} tests"));
    }
    if let Some(t) = i.tests.iter().find(|t| !t.arguments.is_object()) {
        return Err(format!(
            "the test of {} gives arguments that are not an object",
            t.tool
        ));
    }
    if let Err(e) = crate::egress::check(&i.network) {
        return Err(format!("network has {e}"));
    }
    Ok(i)
}

/// `extend.propose`.
pub struct Propose;

impl Tool for Propose {
    fn name(&self) -> &str {
        PROPOSE
    }

    fn description(&self) -> &str {
        "Propose a small MCP server you wrote as an extension. Its directory (inside the \
         workspace) is frozen by its SHA-256, started in the sandbox (L1) with no network unless \
         `network` lists hosts, given no secret, and tested: initialize, tools/list, then each \
         test as a tools/call. The operator then decides whether to load it. Nothing loads now: \
         its tools are not yours to call until the operator acks it and it is loaded."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Its name: letters, digits, _ and -, at most 28 characters."},
                "dir": {"type": "string", "description": "The directory holding the server, inside the workspace: what is frozen and runs."},
                "command": {"type": "array", "items": {"type": "string"}, "description": "The server's argv, run in the frozen directory with no shell, speaking MCP over stdio."},
                "description": {"type": "string", "description": "What it is for, for the operator."},
                "tests": {
                    "type": "array",
                    "description": "Calls that show it works: each tool, its arguments, and what its text must contain or equal.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {"type": "string"},
                            "arguments": {"type": "object"},
                            "expect": {
                                "type": "object",
                                "properties": {"contains": {"type": "string"}, "equals": {"type": "string"}},
                                "description": "One of contains or equals."
                            }
                        },
                        "required": ["tool", "expect"]
                    }
                },
                "network": {"type": "array", "items": {"type": "string"}, "description": "Hosts it must reach (host or host:port, *.domain for every name under it). Leave it out for none."}
            },
            "required": ["name", "dir", "command", "description"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a restart, it answers with what it proposed.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let i = input_of(input)?;
        let dir = ctx.resolve(&i.dir);
        if !ctx.roots.is_empty()
            && !ctx
                .roots
                .iter()
                .any(|r| theseus_tools::paths::within(&dir, r))
        {
            return Err(format!(
                "{} is outside the workspace roots: propose a directory inside them",
                dir.display()
            ));
        }
        let network = match i.network.as_slice() {
            [] => String::new(),
            hosts => format!(", network to {}", hosts.join(", ")),
        };
        Ok(Plan {
            resources: vec![Resource {
                path: dir.clone(),
                access: Access::Read,
            }],
            summary: format!(
                "propose extension {} from {} ({}{network})",
                i.name,
                dir.display(),
                crate::narrative::count(i.tests.len() as u64, "test", "tests")
            ),
            ..Default::default()
        })
    }
}

/// What a trial found.
struct Tried {
    tools: Vec<ToolInfo>,
    tests: Vec<TestRun>,
    error: Option<String>,
    ms: u64,
}

/// The frozen copy on trial as `ext-<name>`: its tools, and each test.
async fn try_out(
    board: Arc<McpBoard>,
    name: String,
    cfg: McpServerConfig,
    tests: Vec<TestSpec>,
) -> Tried {
    let t0 = Instant::now();
    let server = format!("{SERVER_PREFIX}{name}");
    let trial = match board.trial(&server, &cfg).await {
        Ok(t) => t,
        Err(e) => {
            return Tried {
                tools: vec![],
                tests: tests
                    .into_iter()
                    .map(|spec| not_run(spec, "the server did not start"))
                    .collect(),
                error: Some(e),
                ms: t0.elapsed().as_millis() as u64,
            }
        }
    };
    let tools = trial
        .tools
        .iter()
        .map(|t| ToolInfo {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
        })
        .collect();
    let mut runs = Vec::new();
    for spec in tests {
        let call = trial.client.call_tool(&spec.tool, spec.arguments.clone());
        let run = match tokio::time::timeout(Duration::from_secs(CALL_TIMEOUT_SECS), call).await {
            Err(_) => not_run(spec, &format!("no answer within {CALL_TIMEOUT_SECS} s")),
            Ok(Err(e)) => not_run(spec, &e.to_string()),
            Ok(Ok(r)) => judged(spec, &r.text_for_model(), r.is_error),
        };
        runs.push(run);
    }
    drop(trial);
    Tried {
        tools,
        tests: runs,
        error: None,
        ms: t0.elapsed().as_millis() as u64,
    }
}

fn not_run(spec: TestSpec, why: &str) -> TestRun {
    TestRun {
        spec,
        passed: false,
        got: None,
        why: Some(why.to_string()),
    }
}

/// A test's answer against what it expects.
fn judged(spec: TestSpec, text: &str, is_error: bool) -> TestRun {
    let (ok, want) = match &spec.expect {
        Expect::Contains(w) => (
            text.contains(w.as_str()),
            format!("it does not contain {w:?}"),
        ),
        Expect::Equals(w) => (text.trim() == w.trim(), format!("it is not {w:?}")),
    };
    let why = match (is_error, ok) {
        (true, _) => Some("the server answered an error".to_string()),
        (false, false) => Some(want),
        (false, true) => None,
    };
    TestRun {
        spec,
        passed: why.is_none(),
        got: Some(text.chars().take(GOT_CHARS).collect()),
        why,
    }
}

/// The server's table for a trial: the frozen copy, in L1, with the
/// proposal's network and no secret.
fn trial_cfg(command: &[String], frozen: &Path, network: &[String]) -> McpServerConfig {
    McpServerConfig {
        command: command.to_vec(),
        env: Default::default(),
        url: None,
        auth_secret: None,
        read: vec![],
        sandbox: crate::config::mcp::McpSandbox::L1,
        egress: network.to_vec(),
        frozen: Some(frozen.to_path_buf()),
        external: true,
        enabled: true,
        start_timeout_secs: START_TIMEOUT_SECS,
        call_timeout_secs: CALL_TIMEOUT_SECS,
    }
}

/// Run `extend.propose` for the call `correlation_id` of the turn `tc`:
/// freeze, try, write the manifest and ask. An error is the result the
/// model reads.
pub async fn propose(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    let i = input_of(input)?;
    let ext = rt.extend.clone();
    let src = ext.inside(&rt.ctx, &i.dir)?;
    if !src.is_dir() {
        return Err(format!("{} is not a directory", src.display()));
    }
    let board = ext
        .board()
        .ok_or("the MCP board is not running, so nothing can be tried")?;
    let (root, name) = (ext.dir.clone(), i.name.clone());
    let from = src.clone();
    let frozen = tokio::task::spawn_blocking(move || freeze::freeze(&from, &root, &name))
        .await
        .map_err(|e| format!("the freeze failed: {e}"))?
        .map_err(|e| format!("{} could not be frozen: {e}", src.display()))?;
    tc.record(&ExtendProposed {
        name: &i.name,
        digest: &frozen.digest,
        source: &src.display().to_string(),
        files: frozen.files,
        bytes: frozen.bytes,
        command: &i.command,
        network: &i.network,
        correlation_id,
    });
    // The trial is a task of its own, so a cancel or a stop aborts it, and
    // the trial's drop stops its server.
    let cfg = trial_cfg(&i.command, &frozen.dir, &i.network);
    let mut task = tokio::spawn(try_out(board, i.name.clone(), cfg, i.tests.clone()));
    let _stoppable = rt.stops.track(correlation_id, task.abort_handle());
    let tried = match tokio::time::timeout(TRIAL_LIMIT, &mut task).await {
        Ok(Ok(t)) => t,
        Ok(Err(e)) if e.is_cancelled() => return Err(STOPPED.into()),
        Ok(Err(e)) => return Err(format!("the trial failed: {e}")),
        Err(_) => {
            task.abort();
            return Err(format!("the trial took over {} s", TRIAL_LIMIT.as_secs()));
        }
    };
    let names: Vec<String> = tried.tools.iter().map(|t| t.name.clone()).collect();
    let mut m = Manifest {
        name: i.name.clone(),
        digest: frozen.digest.clone(),
        description: i.description.trim().to_string(),
        command: i.command.clone(),
        source: src.display().to_string(),
        frozen: frozen.dir.display().to_string(),
        files: frozen.files,
        bytes: frozen.bytes,
        tools: tried.tools,
        tests: tried.tests,
        error: tried.error,
        capabilities: Capabilities {
            network: i.network.clone(),
            scratch: true,
        },
        proposed_by: ProposedBy {
            session_id: tc.session_id.into(),
            execution_id: tc.execution_id.into(),
            correlation_id: correlation_id.into(),
            principal: tc
                .kernel
                .execution(tc.execution_id)
                .ok()
                .flatten()
                .map(|e| e.authority.principal)
                .unwrap_or_default(),
            place: tc.outbox.target(tc.session_id),
        },
        proposed_at_ms: theseus_protocol::now_unix_ms(),
        question: None,
        state: "proposed".into(),
        answered_by: None,
        answered_at_ms: None,
        note: None,
    };
    tc.record(&ExtendTested {
        name: &m.name,
        digest: &m.digest,
        tools: &names,
        passed: m.passed(),
        tests: m.tests.len(),
        error: m.error.as_deref(),
        ms: tried.ms,
    });
    if m.error.is_some() {
        m.state = "failed".into();
        tc.store
            .append(&[m.record().map_err(|e| e.to_string())?])
            .map_err(|e| e.to_string())?;
    } else {
        ask(rt, tc, &mut m).map_err(|e| format!("the question was not asked: {e:#}"))?;
    }
    Ok((said(&m), json!({"manifest": m})))
}

/// The question, its card, and the manifest that names it, in one frame on
/// the proposing execution.
fn ask(rt: &ToolRuntime, tc: &TurnCtx<'_>, m: &mut Manifest) -> anyhow::Result<()> {
    let proposal = Proposal {
        tool: ACK.into(),
        args: json!({
            "name": m.name, "digest": m.digest, "short": short(&m.digest),
            "tools": m.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
            "passed": m.passed(), "tests": m.tests.len(),
            "network": m.capabilities.network, "frozen": m.frozen,
            "question": m.question_text(),
        }),
        resource: Some(format!("{SERVER_PREFIX}{}", m.name)),
        policy_context: json!({}),
    };
    let target = tc.outbox.target(tc.session_id);
    let mut card = None;
    let a =
        tc.kernel
            .plan_confirm_with(tc.guard, &proposal, RetryClass::NonRepeatable, None, |a| {
                m.question = Some(a.correlation_id.clone());
                let mut records = vec![m.record()?];
                if let Some(target) = &target {
                    let (post, more) = tc.outbox.stage(
                        tc.session_id,
                        tc.execution_id,
                        target,
                        json!({"kind": "card", "question": a.correlation_id, "node": null}),
                    )?;
                    records.extend(more);
                    card = Some(post);
                }
                Ok(records)
            })?;
    if let Some(post) = card {
        tc.outbox.posted(&post);
    }
    let ttl = tc.confirm_ttl_ms;
    rt.question_due
        .fetch_min(a.planned_at_ms + ttl, std::sync::atomic::Ordering::SeqCst);
    if let Ok(Some(session)) = tc
        .store
        .get_session::<crate::session::SessionRecord>(tc.session_id)
    {
        tc.record(&crate::fact::tool::CallAsked {
            request: &confirm_request(&a, &session, ttl),
        });
    }
    Ok(())
}

/// The proposing call's result: what was frozen, what was found, and what
/// happens next.
fn said(m: &Manifest) -> String {
    let mut out = format!(
        "Proposed extension {} {} ({}), frozen from {} at {}.\n",
        m.name,
        short(&m.digest),
        m.digest,
        m.source,
        m.frozen
    );
    if let Some(e) = &m.error {
        out.push_str(&format!(
            "It did not come up in L1: {e}. Nothing was put to the operator: fix it and propose \
             it again.\n"
        ));
        return out;
    }
    let tools: Vec<&str> = m.tools.iter().map(|t| t.name.as_str()).collect();
    out.push_str(&format!(
        "In L1 it listed {}: {}.\n",
        crate::narrative::count(tools.len() as u64, "tool", "tools"),
        tools.join(", ")
    ));
    for t in &m.tests {
        match &t.why {
            None => out.push_str(&format!("- {}: passed\n", t.spec.tool)),
            Some(why) => out.push_str(&format!("- {}: FAILED ({why})\n", t.spec.tool)),
        }
    }
    out.push_str(&format!(
        "{} of {} passed. The operator is asked: \"{}\" Nothing loads until they ack it, and \
         its tools are not offered to you meanwhile.",
        m.passed(),
        crate::narrative::count(m.tests.len() as u64, "test", "tests"),
        m.question_text()
    ));
    out
}

/// A proposal run again after a restart: what the first run proposed, from
/// its manifest; else why it must be proposed again.
pub fn proposed_by_call(tc: &TurnCtx<'_>, correlation_id: &str) -> Result<(String, Value), String> {
    let found = manifests(tc.store)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.proposed_by.correlation_id == correlation_id);
    match found {
        Some(m) => Ok((said(&m), json!({"manifest": m}))),
        None => Err(
            "the daemon restarted before this proposal was tried, so nothing was put to the \
             operator: propose it again"
                .into(),
        ),
    }
}

/// An ack's question as the operator sees it: its card, `confirm.list`.
pub fn confirm_request(
    a: &Action,
    session: &crate::session::SessionRecord,
    ttl_ms: u64,
) -> ConfirmRequest {
    let args = a
        .proposal
        .as_ref()
        .map(|p| p.args.clone())
        .unwrap_or_default();
    ConfirmRequest {
        correlation_id: a.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: a.execution_id.clone(),
        tool: ACK.into(),
        reason: args["question"].as_str().unwrap_or_default().to_string(),
        input: args,
        resource: a.resource.clone(),
        by: crate::turn::OPERATOR.into(),
        requested_at_ms: a.planned_at_ms,
        expires_at_ms: a.planned_at_ms + ttl_ms,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        external_text: None,
        change: None,
    }
}
