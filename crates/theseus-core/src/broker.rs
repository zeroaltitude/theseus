//! The secret broker (theseus-dcy, spec §3.19). Theseus resolves every secret
//! once, from 1Password, into the secrets board (F1). The broker is the one
//! place that hands those values to anything beyond the daemon's own
//! consumers (the providers, Discord, the GitHub check, telemetry), and it
//! never calls `op`.
//!
//! - **A program** gets a secret in an environment variable
//!   (`[broker.programs.<name>] env = { VAR = "<secret>" }`) only when a job
//!   runs it by direct argv: `argv[0]`, resolved as `exec` resolves it, is the
//!   file the program's name resolves to on the daemon's PATH. A shell or an
//!   interpreter that runs it gets nothing, since it would hand the variable
//!   to every program it runs, and the tool result says so.
//! - **Nor does a program the call could make run another** (review 2's H7,
//!   `launches`): a call that sets its own environment (`PATH`, `GIT_*`,
//!   `LD_AUDIT`, …) gets no grant; gh gets one only for its own commands,
//!   never an alias, an extension, or `--web`'s browser; git only for its
//!   commands that reach a remote, never an alias, `-c`, or a program that
//!   an option or a URL names; cargo only for its registry commands that
//!   build nothing; and npm only for its registry commands that run no
//!   package's scripts (theseus-txvt).
//! - **No grant to a launcher** (theseus-txvt, `launcher`): a shell, an
//!   interpreter, a wrapper that runs the command it is given (`env`,
//!   `sudo`, `xargs`, …), or a runner of a project's scripts (`make`, `npx`,
//!   …) runs whatever the call names, so the config refuses a grant to one,
//!   and a granted name that resolves to one gets nothing.
//! - **A granted git runs no hooks** (theseus-ur1t, `GIT_PINS`): a job given
//!   a secret by a grant to git or gh has `core.hooksPath` and
//!   `core.fsmonitor` pinned off in its environment, over every config
//!   file, so neither a cloned project's hooks nor its fsmonitor program
//!   gets the variable. What else a granted program runs on its own account
//!   still gets it: a credential helper or `core.sshCommand` its config
//!   names, ssh's own config, and gh's.
//! - **A native toollet** gets a secret through `secret_for_tool`, once the
//!   wiring has granted it one (`grant_tool`): DD5's `web.search`, its key.
//! - **Each secret has a posture** (`[broker.secrets.<name>] posture`, notify
//!   by default). A call that is given a secret runs at no looser a posture.
//! - A secret with no grant is never handed out, so the AWS keys stay behind
//!   the op floor (decision 15).
//! - A value is never written: not to the spool, the WAL, a node, the ledger,
//!   or a log. The ledger's `secret.granted` names the program, the variable,
//!   and the secret, and health counts each grant's uses.
//!
//! At L0 a job runs as the operator's user, so this keeps a value out of every
//! record and every other program's environment. It is not a boundary against
//! a hostile job of the same user (§3.9, "What the gate is not"); L1 is (M4).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::BrokerConfig;
use crate::policy::Posture;
use crate::secrets::{Secret, SecretBoard, SecretState, Waited};

/// A program's grant: each variable, and the secret whose value it holds.
type Vars = [(String, String)];

/// One secret handed out: to a program in a variable, or to a toollet.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Grant {
    /// The program (`gh`) or the toollet (`web.search`).
    pub to: String,
    /// The environment variable a program gets it in; none for a toollet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    /// The `[secrets]` name.
    pub secret: String,
}

impl Grant {
    fn what(&self) -> &str {
        self.variable.as_deref().unwrap_or(&self.secret)
    }
}

/// `a`, `a and b`, `a, b and c`.
pub(crate) fn and(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn phrase(grants: &[Grant], verb: &str) -> Option<String> {
    let to = &grants.first()?.to;
    let what: Vec<&str> = grants.iter().map(Grant::what).collect();
    Some(format!("{to} {verb} {}", and(&what)))
}

/// What a call was given, by name, as its tool line and its notice say it:
/// `gh got GH_TOKEN`. None when it was given nothing.
pub fn got(grants: &[Grant]) -> Option<String> {
    phrase(grants, "got")
}

/// What a call will be given, as the gate's reason says it: `gh gets GH_TOKEN`.
pub fn gets(grants: &[Grant]) -> Option<String> {
    phrase(grants, "gets")
}

/// What a job gets at its spawn, and what it does not.
#[derive(Debug, Default)]
pub struct ForJob {
    /// Each granted variable, and its value.
    pub env: Vec<(String, Secret)>,
    /// What it was given, by name.
    pub granted: Vec<Grant>,
    /// What its program was granted and not given, and why.
    pub withheld: Vec<(Grant, String)>,
    /// Why a granted program that the call names got nothing: it is not run
    /// by its own argv.
    pub indirect: Option<String>,
    /// The program given a secret runs git, or is git: the job's git has its
    /// hooks and its fsmonitor pinned off (`GIT_PINS`, theseus-ur1t).
    pub git_pinned: bool,
}

impl ForJob {
    /// For the result: each thing the job was not given, and why, and a
    /// granted git's pins, one bracketed line each. Never a value. A gh's
    /// pins go unsaid: most of its commands run no git.
    pub fn note(&self) -> Option<String> {
        let git = self.granted.first().is_some_and(|g| g.to == "git");
        let pinned = (self.git_pinned && git).then(|| {
            format!(
                "[git ran with its hooks and core.fsmonitor off, since it got {}]",
                and(&self.granted.iter().map(Grant::what).collect::<Vec<_>>())
            )
        });
        let lines: Vec<String> = self
            .withheld
            .iter()
            .map(|(g, why)| format!("[{} got no {}: {why}; it ran without it]", g.to, g.what()))
            .chain(self.indirect.iter().map(|n| format!("[{n}]")))
            .chain(pinned)
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// The job's environment with `GIT_PINS`, when its program was given a
    /// secret and runs git (theseus-ur1t): set after any `GIT_CONFIG_*`
    /// pairs it already has, so they keep theirs and the pins win.
    pub fn pin(&self, env: &mut Vec<(String, String)>) {
        if self.git_pinned {
            pin_git(env);
        }
    }
}

/// The settings a granted git, and every git a granted gh runs, takes from
/// its environment over every config file (theseus-ur1t): no hooks, the
/// repository's or the operator's, and no fsmonitor program. They are what
/// runs third-party code on a remote command without a job's help: a cloned
/// project's hook manager (`core.hooksPath` into its tree, or hooks it
/// installs), and a `fsmonitor-watchman` hook script. git 2.31 or later
/// reads them (`GIT_CONFIG_COUNT`). An empty `core.fsmonitor` is off for
/// every git: before 2.36 the setting is a program's path, and `false` would
/// run a program named `false`, found on the job's PATH.
pub const GIT_PINS: &[(&str, &str)] = &[("core.hooksPath", "/dev/null"), ("core.fsmonitor", "")];

/// Add `GIT_PINS` to a job's environment as git's `GIT_CONFIG_KEY_<n>` and
/// `GIT_CONFIG_VALUE_<n>` pairs, after the pairs it already has.
pub fn pin_git(env: &mut Vec<(String, String)>) {
    let get = |env: &Vec<(String, String)>, k: &str| {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    };
    let start: usize = get(env, "GIT_CONFIG_COUNT")
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    for (i, (key, value)) in GIT_PINS.iter().enumerate() {
        let n = start + i;
        for (var, val) in [
            (format!("GIT_CONFIG_KEY_{n}"), key),
            (format!("GIT_CONFIG_VALUE_{n}"), value),
        ] {
            env.retain(|(k, _)| *k != var);
            env.push((var, (*val).to_string()));
        }
    }
    env.retain(|(k, _)| k != "GIT_CONFIG_COUNT");
    env.push((
        "GIT_CONFIG_COUNT".into(),
        (start + GIT_PINS.len()).to_string(),
    ));
}

pub struct Broker {
    /// Each program's grant: variable, then secret.
    programs: BTreeMap<String, Vec<(String, String)>>,
    /// Each program granted an AWS job session, and its account (AWS design
    /// §3.5): short-lived credentials under the guards, never the key.
    aws_programs: BTreeMap<String, String>,
    /// The accounts those sessions come from, once the runtime is built.
    aws: std::sync::OnceLock<Arc<crate::aws::Aws>>,
    /// Each toollet's secrets, as the wiring granted them.
    tools: RwLock<BTreeMap<String, BTreeSet<String>>>,
    /// `[broker.secrets]`: a secret's posture, notify when it has none.
    postures: BTreeMap<String, Posture>,
    board: Arc<SecretBoard>,
    /// The daemon's PATH, where a program's name is resolved.
    path: Option<String>,
    /// How long a call waits for a secret that has not settled: a turn's
    /// wait (F1).
    wait: Duration,
    /// Each grant's uses since the daemon started.
    uses: Mutex<BTreeMap<Grant, u64>>,
}

/// The job an AWS session is for: its correlation id names the session, and
/// its deadline bounds it.
#[derive(Clone, Copy)]
pub struct JobAws<'a> {
    pub correlation_id: &'a str,
    pub lasts: Duration,
}

/// What the gate and the ledger call an AWS job session's grant.
pub fn aws_session_label(account: &str) -> String {
    format!("an AWS job session ({account})")
}

impl fmt::Debug for Broker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Broker")
            .field("programs", &self.programs)
            .field("tools", &self.tools.read().map(|t| t.clone()).ok())
            .finish()
    }
}

impl Broker {
    /// The broker the config describes, over the daemon's board. `path` is
    /// the daemon's PATH.
    pub fn new(cfg: &BrokerConfig, board: Arc<SecretBoard>, path: Option<String>) -> Self {
        Self {
            programs: cfg
                .programs
                .iter()
                .map(|(p, g)| {
                    let vars = g.env.iter().map(|(v, s)| (v.clone(), s.clone())).collect();
                    (p.clone(), vars)
                })
                .collect(),
            aws_programs: cfg
                .programs
                .iter()
                .filter_map(|(p, g)| Some((p.clone(), g.aws_account.clone()?)))
                .collect(),
            aws: std::sync::OnceLock::new(),
            tools: RwLock::default(),
            postures: cfg
                .secrets
                .iter()
                .map(|(n, s)| (n.clone(), s.posture))
                .collect(),
            board,
            path,
            wait: crate::turn::SECRET_WAIT,
            uses: Mutex::default(),
        }
    }

    /// A broker with no grants.
    pub fn empty() -> Self {
        Self::new(&BrokerConfig::default(), SecretBoard::empty(), None)
    }

    /// The same broker, waiting at most `wait` for a secret (tests).
    pub fn with_wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    /// The accounts a program's AWS job session comes from: set once, as the
    /// tool runtime is built.
    pub fn set_aws(&self, aws: Arc<crate::aws::Aws>) {
        let _ = self.aws.set(aws);
    }

    /// The programs granted an AWS job session, for the harness-only line.
    pub fn aws_programs(&self) -> Vec<String> {
        self.aws_programs.keys().cloned().collect()
    }

    /// Grant `tool` the secret `secret`: the wiring's grant for a native
    /// toollet, such as DD5's `web.search` and its key. Each call of the tool
    /// then runs at no looser a posture than the secret's.
    pub fn grant_tool(&self, tool: &str, secret: &str) {
        self.tools
            .write()
            .unwrap()
            .entry(tool.into())
            .or_default()
            .insert(secret.into());
    }

    fn tool_secrets(&self, tool: &str) -> Vec<String> {
        self.tools
            .read()
            .unwrap()
            .get(tool)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// A secret's posture, and the setting that chose it.
    pub fn posture_of(&self, secret: &str) -> (Posture, String) {
        match self.postures.get(secret) {
            Some(p) => (
                *p,
                format!("[broker.secrets.{secret}] posture = {}", p.as_str()),
            ),
            None => (
                Posture::Notify,
                format!("the broker's posture for {secret}, notify by default"),
            ),
        }
    }

    /// Whether the operator's `[broker]` names `secret`, in a program's grant
    /// or in its own `[broker.secrets.<name>]` entry: what `harness_only`
    /// counts as a secret a job may be handed. A toollet's grant is the
    /// wiring's, not the operator's word, so it is not enough (web.search's
    /// key stays the toollet's), and any other name is never handed out: the
    /// AWS keys and the providers' keys stay behind the floor (decision 15).
    pub fn may_hand_out(&self, secret: &str) -> bool {
        self.postures.contains_key(secret)
            || self
                .programs
                .values()
                .any(|vars| vars.iter().any(|(_, s)| s == secret))
    }

    /// The strictest posture of `grants`, and its setting: the posture a call
    /// given them runs at, at least.
    pub fn need(&self, grants: &[Grant]) -> Option<(Posture, String)> {
        grants
            .iter()
            .map(|g| self.posture_of(&g.secret))
            .reduce(|a, b| if b.0 > a.0 { b } else { a })
    }

    /// What a call would be given, for the gate: a job's program by its argv
    /// (`argv`, with the names of the variables the call sets, `env`, run in
    /// `cwd` with the job's `PATH`), and the tool's own grants.
    pub fn at_gate(
        &self,
        tool: &str,
        argv: Option<&[String]>,
        env: &[&str],
        cwd: &Path,
        job_path: Option<&str>,
    ) -> Vec<Grant> {
        let mut grants: Vec<Grant> = argv
            .and_then(|a| self.program_for(a, env, cwd, job_path).ok())
            .map(|(program, vars)| {
                let mut g: Vec<Grant> = vars
                    .iter()
                    .map(|(v, s)| Grant {
                        to: program.into(),
                        variable: Some(v.clone()),
                        secret: s.clone(),
                    })
                    .collect();
                if let Some(account) = self.aws_programs.get(program) {
                    g.push(Grant {
                        to: program.into(),
                        variable: None,
                        secret: aws_session_label(account),
                    });
                }
                g
            })
            .unwrap_or_default();
        grants.extend(self.tool_secrets(tool).into_iter().map(|s| Grant {
            to: tool.into(),
            variable: None,
            secret: s,
        }));
        grants
    }

    /// The program `argv` runs by its own argv, and its grant: `argv[0]` is
    /// the file its name resolves to on the daemon's PATH, and the call,
    /// setting the variables `env` names, cannot make it run another program
    /// (`launches`). Otherwise, the note for a granted program the call names
    /// without running it so, or None when it names none.
    fn program_for(
        &self,
        argv: &[String],
        env: &[&str],
        cwd: &Path,
        job_path: Option<&str>,
    ) -> Result<(&str, &Vars), Option<String>> {
        let first = argv.first().ok_or(None)?;
        let name = Path::new(first)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(first);
        let vars = |v: &Vars| {
            let mut names: Vec<&str> = v.iter().map(|(n, _)| n.as_str()).collect();
            if self.aws_programs.contains_key(name) {
                names.push("an AWS job session");
            }
            and(&names)
        };
        if let Some((program, grant)) = self.programs.get_key_value(name) {
            let ran = resolve(first, job_path, cwd, false);
            let named = resolve(program, self.path.as_deref(), cwd, true);
            if ran.is_none() || ran != named {
                return Err(Some(format!(
                    "{program} got no {}: `{first}` is not the {program} on the daemon's PATH{}",
                    vars(grant),
                    named.map_or_else(String::new, |n| format!(" ({})", n.display()))
                )));
            }
            // A granted name that is a launcher's file (a link `py` to
            // python3), which the config's check of the name cannot see.
            let file = ran
                .as_deref()
                .and_then(Path::file_name)
                .and_then(|f| f.to_str())
                .unwrap_or(program);
            if let Some(what) = launcher(file) {
                return Err(Some(format!(
                    "{program} got no {}: it is {} ({what}), which runs whatever the call names, \
                     so the variable would reach that too",
                    vars(grant),
                    ran.as_deref()
                        .map_or(file.into(), |p| p.display().to_string())
                )));
            }
            if let Some(why) = launches(program, &argv[1..], env) {
                return Err(Some(format!("{program} got no {}: {why}", vars(grant))));
            }
            return Ok((program.as_str(), grant.as_slice()));
        }
        // A granted program named in another's arguments: a shell's script,
        // an interpreter's, or `env`, `xargs`, and `timeout` running it.
        let words = argv[1..]
            .iter()
            .flat_map(|a| a.split(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c))));
        for w in words {
            if let Some((program, grant)) = self.programs.get_key_value(w) {
                return Err(Some(format!(
                    "{program} got no {}: it is run by `{name}`, not by its own argv, and a \
                     program that runs others would hand the variable to each of them. Run \
                     {program} directly to get it, as in [\"{program}\", …]",
                    vars(grant)
                )));
            }
        }
        Err(None)
    }

    /// What a job gets at its spawn: each granted variable and its value, from
    /// the board. A secret that has not settled is waited for, bounded as a
    /// turn waits (F1); one that still has not, or that failed, is withheld,
    /// never replaced by a placeholder. `ran_at` is the posture the call ran
    /// at: a secret whose posture is stricter is withheld too, so no call is
    /// given a secret at a looser posture than its own.
    pub async fn for_job(
        &self,
        argv: &[String],
        env: &[&str],
        cwd: &Path,
        job_path: Option<&str>,
        ran_at: Posture,
    ) -> ForJob {
        self.for_job_of(argv, env, cwd, job_path, ran_at, None)
            .await
    }

    /// `for_job`, for the job `job` names: a program granted an AWS job
    /// session gets one, named by the job's correlation id and bounded by its
    /// deadline (AWS design §3.5), and never the key.
    pub async fn for_job_of(
        &self,
        argv: &[String],
        env: &[&str],
        cwd: &Path,
        job_path: Option<&str>,
        ran_at: Posture,
        job: Option<JobAws<'_>>,
    ) -> ForJob {
        let mut out = ForJob::default();
        let (program, vars) = match self.program_for(argv, env, cwd, job_path) {
            Ok(p) => p,
            Err(note) => {
                out.indirect = note;
                return out;
            }
        };
        let until = Instant::now() + self.wait;
        for (var, secret) in vars {
            let grant = Grant {
                to: program.into(),
                variable: Some(var.clone()),
                secret: secret.clone(),
            };
            let (posture, _) = self.posture_of(secret);
            let withheld = if posture > ran_at {
                Some(format!(
                    "{secret}'s posture is {}, and this call ran at {}",
                    posture.as_str(),
                    ran_at.as_str()
                ))
            } else {
                let left = until.saturating_duration_since(Instant::now());
                match self.board.wait(secret, left).await {
                    Waited::Ready(v) => {
                        out.env.push((var.clone(), v));
                        None
                    }
                    Waited::Failed(e) => Some(format!("{secret} did not resolve ({e})")),
                    Waited::Resolving => Some(format!(
                        "{secret} was still resolving after {}",
                        crate::narrative::duration(self.wait.as_millis() as u64)
                    )),
                    Waited::Absent => Some(format!("{secret} is not a configured secret")),
                }
            };
            match withheld {
                Some(why) => out.withheld.push((grant, why)),
                None => {
                    self.count(&grant);
                    out.granted.push(grant);
                }
            }
        }
        if let Some(account) = self.aws_programs.get(program) {
            self.aws_session(program, account, job, ran_at, &mut out)
                .await;
        }
        // A git given a secret, or the git a gh given one runs, takes no
        // hooks and no fsmonitor program (theseus-ur1t).
        out.git_pinned = !out.env.is_empty() && matches!(program, "git" | "gh");
        out
    }

    /// An AWS job session for `program` (§3.5): its three variables, the
    /// region, and no `~/.aws` profile; or why it gets none.
    async fn aws_session(
        &self,
        program: &str,
        account: &str,
        job: Option<JobAws<'_>>,
        ran_at: Posture,
        out: &mut ForJob,
    ) {
        let label = aws_session_label(account);
        let grant = |var: &str| Grant {
            to: program.into(),
            variable: Some(var.into()),
            secret: label.clone(),
        };
        let (posture, _) = self.posture_of(&label);
        let minted = if posture > ran_at {
            Err(format!(
                "{label}'s posture is {}, and this call ran at {}",
                posture.as_str(),
                ran_at.as_str()
            ))
        } else {
            match (self.aws.get(), job) {
                (Some(aws), Some(job)) => match aws.account(Some(account)) {
                    Ok(a) => a
                        .job_session(job.correlation_id, job.lasts)
                        .await
                        .map(|c| (a.cfg.region.clone(), c)),
                    Err(e) => Err(e),
                },
                (None, _) => Err("no AWS account is bound".into()),
                (_, None) => Err("the job has no correlation id to name a session by".into()),
            }
        };
        let (region, creds) = match minted {
            Ok(m) => m,
            Err(why) => {
                out.withheld.push((grant("AWS_SESSION_TOKEN"), why));
                return;
            }
        };
        for (var, value) in [
            ("AWS_ACCESS_KEY_ID", creds.access_key_id().to_string()),
            ("AWS_SECRET_ACCESS_KEY", creds.expose_secret().to_string()),
            (
                "AWS_SESSION_TOKEN",
                creds.expose_token().unwrap_or_default().to_string(),
            ),
        ] {
            out.env.push((var.into(), Secret::new(value)));
            let g = grant(var);
            self.count(&g);
            out.granted.push(g);
        }
        for (var, value) in [
            ("AWS_REGION", region.clone()),
            ("AWS_DEFAULT_REGION", region),
            ("AWS_CONFIG_FILE", "/dev/null".into()),
            ("AWS_SHARED_CREDENTIALS_FILE", "/dev/null".into()),
        ] {
            out.env.push((var.into(), Secret::new(value)));
        }
    }

    /// Wait, bounded as a turn waits, for each secret granted to `tool` to
    /// settle, so the toollet, which runs on a core and cannot wait, reads
    /// each at once.
    pub async fn settle_for_tool(&self, tool: &str) {
        let until = Instant::now() + self.wait;
        for name in self.tool_secrets(tool) {
            let left = until.saturating_duration_since(Instant::now());
            let _ = self.board.wait(&name, left).await;
        }
    }

    /// Whether the wiring granted `tool` any secret.
    pub fn has_tool(&self, tool: &str) -> bool {
        self.tools.read().unwrap().contains_key(tool)
    }

    /// A toollet's secret: its value, when the wiring granted `name` to `tool`,
    /// its posture is no stricter than `ran_at`, and it has resolved. Else
    /// why not, never a value.
    pub fn secret_for_tool(
        &self,
        tool: &str,
        name: &str,
        ran_at: Posture,
    ) -> Result<Secret, String> {
        if !self.tool_secrets(tool).iter().any(|s| s == name) {
            return Err(format!("{tool} was not granted {name}"));
        }
        let (posture, _) = self.posture_of(name);
        if posture > ran_at {
            return Err(format!(
                "{name}'s posture is {}, and this call ran at {}",
                posture.as_str(),
                ran_at.as_str()
            ));
        }
        match self.board.states().get(name) {
            Some(SecretState::Ready(v)) => {
                self.count(&Grant {
                    to: tool.into(),
                    variable: None,
                    secret: name.into(),
                });
                Ok(v.clone())
            }
            Some(SecretState::Failed(e)) => Err(format!("{name} did not resolve ({e})")),
            Some(SecretState::Resolving) => Err(format!("{name} is still resolving")),
            None => Err(format!("{name} is not a configured secret")),
        }
    }

    fn count(&self, g: &Grant) {
        *self.uses.lock().unwrap().entry(g.clone()).or_default() += 1;
    }

    /// Every grant, with its posture and its uses, for health and the
    /// Observatory. Never a value.
    pub fn status(&self) -> Vec<theseus_protocol::GrantStatus> {
        let uses = self.uses.lock().unwrap();
        let row = |g: Grant, kind: &str| theseus_protocol::GrantStatus {
            kind: kind.into(),
            posture: self.posture_of(&g.secret).0.as_str().into(),
            uses: uses.get(&g).copied().unwrap_or(0),
            to: g.to,
            variable: g.variable,
            secret: g.secret,
        };
        let programs = self.programs.iter().flat_map(|(p, vars)| {
            vars.iter().map(|(v, s)| Grant {
                to: p.clone(),
                variable: Some(v.clone()),
                secret: s.clone(),
            })
        });
        let tools: Vec<Grant> = self
            .tools
            .read()
            .unwrap()
            .iter()
            .flat_map(|(t, secrets)| {
                secrets.iter().map(|s| Grant {
                    to: t.clone(),
                    variable: None,
                    secret: s.clone(),
                })
            })
            .collect();
        programs
            .map(|g| row(g, "program"))
            .chain(tools.into_iter().map(|g| row(g, "tool")))
            .collect()
    }

    /// Every secret `[broker]` names: each one a job may be handed, by a
    /// program's grant at its start, at L0 and in L1 alike.
    pub fn handed(&self) -> BTreeSet<String> {
        self.postures
            .keys()
            .cloned()
            .chain(
                self.programs
                    .values()
                    .flat_map(|vars| vars.iter().map(|(_, s)| s.clone())),
            )
            .collect()
    }
}

/// What a job may be handed, and what stays the harness's own (theseus-gh7),
/// for `theseusd check` and health. The AWS keys (each bound account's, and
/// the template's two names when `[secrets]` keeps them) and the providers'
/// keys, the speech provider's among them (`[voice]`), are read only by
/// Theseus's own tools (`aws`, the providers, voice); a job is never handed
/// one unless `[broker]` names it, which `exposed` then says.
pub fn harness_only(cfg: &crate::Config, broker: &Broker) -> theseus_protocol::cred::HarnessOnly {
    let template = crate::config::AwsCredentialNames::default();
    let aws: BTreeSet<String> = cfg
        .aws
        .accounts
        .values()
        .flat_map(|a| {
            [
                a.credentials.access_key_id.clone(),
                a.credentials.secret_access_key.clone(),
            ]
        })
        .chain(
            [template.access_key_id, template.secret_access_key]
                .into_iter()
                .filter(|n| cfg.secrets.contains_key(n)),
        )
        .collect();
    // The speech provider's key (rows 77 and 78) is a provider's key too.
    let providers: BTreeSet<String> = std::iter::once(cfg.model.api_key_secret.clone())
        .chain(cfg.providers.values().map(|p| p.api_key_secret.clone()))
        .chain(cfg.voice_key_secret().map(str::to_string))
        .collect();
    let exposed = aws
        .iter()
        .chain(&providers)
        .filter(|n| broker.may_hand_out(n))
        .cloned()
        .collect::<BTreeSet<_>>();
    theseus_protocol::cred::HarnessOnly {
        handed: broker.handed().into_iter().collect(),
        aws: aws.into_iter().collect(),
        providers: providers.into_iter().collect(),
        exposed: exposed.into_iter().collect(),
        aws_sessions: broker.aws_programs(),
        google: crate::google::secret_names(&cfg.google),
    }
}

/// The template's harness-only keys, un-commented (theseus-gh7): no AWS
/// key and no provider's key is one a job may be handed, and the line says
/// so by name. Driven from the config, so a grant that named one fails here.
#[cfg(test)]
pub(crate) fn the_templates_harness_only_keys(cfg: &crate::Config) {
    let broker = Broker::new(&cfg.broker, SecretBoard::empty(), None);
    broker.grant_tool("web.search", &cfg.tools.web.search_key_secret);
    let h = harness_only(cfg, &broker);
    assert_eq!(h.aws, ["aws_access_key_id", "aws_secret_access_key"]);
    // The template's [voice], un-commented, names Deepgram's key: a provider's.
    assert_eq!(
        h.providers,
        [
            "anthropic_api_key",
            "deepgram_api_key",
            "deepseek_api_key",
            "moonshot_api_key",
            // The second wire protocol's key is a provider's key by the same
            // rule and is harness-only for the same reason (theseus-bq73s).
            "openai_api_key",
            "zai_api_key"
        ]
    );
    for name in h.aws.iter().chain(&h.providers) {
        assert!(!broker.may_hand_out(name), "a job may be handed {name}");
    }
    assert!(h.exposed.is_empty(), "{:?}", h.exposed);
    assert_eq!(h.handed, ["github_token"]);
    assert_eq!(
        h.line(),
        "broker: a job may be handed 1 secret (github_token); harness-only: the AWS keys \
         (aws_access_key_id, aws_secret_access_key) and the providers' keys (anthropic_api_key, \
         deepgram_api_key, deepseek_api_key, moonshot_api_key, openai_api_key, zai_api_key) and \
         Google's secrets (google_client, \
         google_refresh_token); jobs get short-lived AWS sessions, never the key (aws)"
    );
}

/// The template's broker section, un-commented (design §4's item 8): a job
/// may be handed `github_token` alone, gh's grant, at notify, and no
/// provider's key, AWS key, bot token, or web.search's key, though the
/// wiring grants that one to its toollet.
#[cfg(test)]
pub(crate) fn the_templates_broker_section(cfg: &crate::Config) {
    let broker = Broker::new(&cfg.broker, SecretBoard::empty(), None);
    broker.grant_tool("web.search", &cfg.tools.web.search_key_secret);
    let handed: Vec<&str> = cfg
        .secrets
        .keys()
        .map(String::as_str)
        .filter(|n| broker.may_hand_out(n))
        .collect();
    assert_eq!(handed, ["github_token"]);
    assert_eq!(broker.posture_of("github_token").0, Posture::Notify);
}

/// The broker bound to one call of one toollet: what `ToolCtx::secret` asks.
/// It keeps the names it handed out, for the call's `secret.granted` rows.
pub struct Bound {
    pub broker: Arc<Broker>,
    pub tool: String,
    pub ran_at: Posture,
    pub handed: Mutex<Vec<String>>,
}

impl fmt::Debug for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bound").field("tool", &self.tool).finish()
    }
}

impl theseus_tools::Secrets for Bound {
    fn secret(&self, name: &str) -> Result<zeroize::Zeroizing<String>, String> {
        let v = self.broker.secret_for_tool(&self.tool, name, self.ran_at)?;
        self.handed.lock().unwrap().push(name.into());
        Ok(zeroize::Zeroizing::new(v.expose().to_string()))
    }
}

/// Why `program`, run by its own argv with `args` and the variables `env`
/// names, could hand its grant to another program that the call chose, or
/// None (review 2's H7). A grant reaches the program a call names, and what
/// that program runs on its own account (its repository's hooks, the helpers
/// its config names), but never a program the call itself names to it:
/// - by the environment: a variable can make a program run another (`PATH`
///   for the programs gh and git run, `GIT_CONFIG_*`, `GIT_EXEC_PATH`,
///   `HOME`, `LD_AUDIT`, …), so a call that sets any gets no grant;
/// - by gh: an alias, an extension, or `--web`'s browser (`gh_launches`);
/// - by git: an alias, `-c`, or a program that an option or a URL names
///   (`git_launches`);
/// - by cargo: a build, which runs the package's build scripts and its
///   dependencies', an alias, a `cargo-<word>` program, or `--config`
///   (`cargo_launches`, theseus-txvt);
/// - by npm: a package's scripts, `npm exec`, or an option that names a
///   program (`npm_launches`, theseus-txvt).
fn launches(program: &str, args: &[String], env: &[&str]) -> Option<String> {
    if !env.is_empty() {
        return Some(format!(
            "the call sets {}, and a variable can make {program} run another program, which \
             would get it too. Run {program} without `env` to get it",
            and(env)
        ));
    }
    match program {
        "gh" => gh_launches(args),
        "git" => git_launches(args),
        "cargo" => cargo_launches(args),
        "npm" => npm_launches(args),
        _ => None,
    }
}

/// Programs whose job is to run another program that the call names, by
/// name (theseus-txvt): a grant to one would reach whatever it runs. The
/// list names the common ones and cannot be finished (`find -exec`, `tar
/// --to-command`, an editor), so the template says to grant only to a
/// program whose commands run nothing the call chooses.
const SHELLS: &[&str] = &[
    "sh", "ash", "bash", "dash", "zsh", "ksh", "mksh", "oksh", "rbash", "csh", "tcsh", "fish",
    "nu", "elvish", "xonsh", "pwsh", "busybox", "toybox",
];
const INTERPRETERS: &[&str] = &[
    "python",
    "pypy",
    "node",
    "nodejs",
    "deno",
    "bun",
    "perl",
    "ruby",
    "irb",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "Rscript",
    "julia",
    "guile",
    "racket",
    "groovy",
    "java",
    "jshell",
    "dotnet",
    "osascript",
    "awk",
    "gawk",
    "mawk",
    "nawk",
];
const WRAPPERS: &[&str] = &[
    "env",
    "xargs",
    "parallel",
    "timeout",
    "nohup",
    "nice",
    "ionice",
    "chrt",
    "taskset",
    "setsid",
    "setpriv",
    "stdbuf",
    "unbuffer",
    "flock",
    "time",
    "watch",
    "sudo",
    "doas",
    "su",
    "runuser",
    "pkexec",
    "chroot",
    "unshare",
    "nsenter",
    "firejail",
    "bwrap",
    "systemd-run",
    "strace",
    "ltrace",
    "gdb",
    "valgrind",
    "script",
    "expect",
    "tmux",
    "screen",
    "xdg-open",
    "docker",
    "podman",
];
const RUNNERS: &[&str] = &[
    "make", "gmake", "bmake", "just", "ninja", "rake", "gradle", "mvn", "ant", "bazel", "cmake",
    "scons", "tox", "nox", "npx", "pnpx", "bunx", "pnpm", "yarn", "pip", "pipx", "uv", "uvx",
    "poetry", "pipenv", "conda", "mamba", "go",
];

/// What `name`, a program's file name, is when its job is to run another
/// program the call names: a shell, an interpreter (a version suffix is
/// ignored: `python3.12` is `python`), a wrapper that runs a command, or a
/// runner of a project's own scripts. None for any other program.
pub fn launcher(name: &str) -> Option<&'static str> {
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let stem = if stem.is_empty() { name } else { stem };
    let is = |list: &[&str]| list.contains(&name) || list.contains(&stem);
    if is(SHELLS) {
        Some("a shell")
    } else if is(INTERPRETERS) {
        Some("an interpreter")
    } else if is(WRAPPERS) {
        Some("a program that runs the command it is given")
    } else if is(RUNNERS) {
        Some("a runner of a project's own scripts")
    } else {
        None
    }
}

/// cargo's commands that use a registry token and build nothing (`publish`
/// only with `--no-verify`, since it otherwise builds the package). Its
/// aliases cannot shadow these, and any other word may be an alias, a
/// build, or a `cargo-<word>` program.
const CARGO_COMMANDS: &[&str] = &["login", "logout", "owner", "publish", "search", "yank"];

fn cargo_launches(args: &[String]) -> Option<String> {
    // Before the command, only the flags that name no toolchain, no
    // setting, and no program.
    let mut i = 0;
    let command = loop {
        let Some(a) = args.get(i) else {
            return Some("the call runs none of cargo's registry commands".into());
        };
        match a.as_str() {
            "-v" | "-vv" | "--verbose" | "-q" | "--quiet" | "--locked" | "--offline"
            | "--frozen" => i += 1,
            "--color" => i += 2,
            a if a.starts_with("--color=") => i += 1,
            a if a.starts_with('-') || a.starts_with('+') => {
                return Some(format!(
                    "`{a}` before cargo's command can name a toolchain, a setting, or a program \
                     for cargo to run (`+<toolchain>`, `--config`, `-Z`), which would get it too"
                ));
            }
            a => break a,
        }
    };
    if !CARGO_COMMANDS.contains(&command) {
        return Some(format!(
            "`cargo {command}` is not one of cargo's registry commands that build nothing ({}), \
             and cargo runs a build's scripts, an alias, or a `cargo-{command}` program by such \
             a word, which would get it too",
            CARGO_COMMANDS.join(", ")
        ));
    }
    let rest = &args[i + 1..];
    // `--config` and `-Z` are taken after the command too.
    if let Some(a) = rest
        .iter()
        .find(|a| *a == "--config" || a.starts_with("--config=") || a.starts_with("-Z"))
    {
        return Some(format!(
            "`{a}` can name a setting or a program for cargo to run, which would get it too"
        ));
    }
    if command == "publish" && !rest.iter().any(|a| a == "--no-verify") {
        return Some(
            "`cargo publish` builds the package unless `--no-verify`, and the build runs its \
             build scripts and its dependencies', which would get it too"
                .into(),
        );
    }
    None
}

/// npm's registry commands that run no package's scripts (`publish` only
/// with `--ignore-scripts`). npm runs scripts for `install`, `ci`, `pack`,
/// `run`, `test`, and more; `exec` and `init` run packages; `login` opens a
/// browser.
const NPM_COMMANDS: &[&str] = &[
    "access",
    "deprecate",
    "dist-tag",
    "info",
    "org",
    "owner",
    "ping",
    "profile",
    "publish",
    "search",
    "show",
    "star",
    "stars",
    "team",
    "token",
    "unpublish",
    "unstar",
    "v",
    "view",
    "whoami",
];

/// npm's options that name a program to run, or a config file that can.
const NPM_PROGRAM_OPTIONS: &[&str] = &[
    "--browser",
    "--call",
    "--editor",
    "--git",
    "--globalconfig",
    "--node-options",
    "--script-shell",
    "--shell",
    "--userconfig",
];

fn npm_launches(args: &[String]) -> Option<String> {
    let Some(at) = args.iter().position(|a| !a.starts_with('-')) else {
        return Some("the call runs none of npm's registry commands".into());
    };
    let command = args[at].as_str();
    if !NPM_COMMANDS.contains(&command) {
        return Some(format!(
            "`npm {command}` is not one of npm's registry commands that run no package's \
             scripts, and npm runs a package's scripts, or a package, for such a word, which \
             would get it too"
        ));
    }
    if let Some(a) = args.iter().find(|a| {
        let name = a.split_once('=').map_or(a.as_str(), |(n, _)| n);
        NPM_PROGRAM_OPTIONS.contains(&name) || name == "-c"
    }) {
        return Some(format!(
            "`{a}` names a program for npm to run, or a config file that can, which would get \
             it too"
        ));
    }
    if command == "publish" {
        // The last word on scripts wins, as npm reads its options, and a
        // `false` after the flag is its value.
        let mut ignore = false;
        for (k, a) in args.iter().enumerate() {
            match a.as_str() {
                "--ignore-scripts" | "--ignore-scripts=true" => {
                    ignore = args.get(k + 1).is_none_or(|v| v != "false");
                }
                "--no-ignore-scripts" => ignore = false,
                a if a.starts_with("--ignore-scripts=") => ignore = false,
                _ => {}
            }
        }
        if !ignore {
            return Some(
                "`npm publish` runs the package's lifecycle scripts unless `--ignore-scripts`, \
                 which would get it too"
                    .into(),
            );
        }
        // A spec npm fetches (a git or tarball URL, `github:…`) is prepared
        // by running its scripts; a local folder or tarball is not. A word
        // after an option that takes a value (`--registry <url>`) is that
        // value.
        let flag = |a: &str| {
            a.contains('=')
                || matches!(
                    a,
                    "--ignore-scripts"
                        | "--no-ignore-scripts"
                        | "--dry-run"
                        | "--provenance"
                        | "--json"
                        | "--force"
                        | "-f"
                        | "--workspaces"
                        | "--include-workspace-root"
                )
        };
        let mut value_next = false;
        for a in &args[at + 1..] {
            let a_value = std::mem::take(&mut value_next);
            if a.starts_with('-') {
                value_next = !flag(a);
            } else if !a_value && a.contains(':') {
                return Some(format!(
                    "`{a}` is a package npm fetches and prepares, by running its scripts, which \
                     would get it too"
                ));
            }
        }
    }
    None
}

/// gh's own commands that can use its token and run no program the call
/// chooses. gh runs a word as an alias, or else as an extension, only when it
/// is none of its own commands. Not here: `extension` (`ext`) runs
/// extensions, `codespace` (`cs`) ssh and editors, `browse` a browser, and
/// `help <word>` an extension's help; `alias`, `config`, and `completion` need
/// no token.
const GH_COMMANDS: &[&str] = &[
    "api",
    "attestation",
    "auth",
    "cache",
    "gist",
    "gpg-key",
    "issue",
    "label",
    "org",
    "pr",
    "project",
    "release",
    "repo",
    "rs",
    "ruleset",
    "run",
    "search",
    "secret",
    "ssh-key",
    "status",
    "variable",
    "workflow",
];

fn gh_launches(args: &[String]) -> Option<String> {
    let Some(command) = args.first() else {
        return Some("the call runs none of gh's commands".into());
    };
    if !GH_COMMANDS.contains(&command.as_str()) {
        return Some(format!(
            "`gh {command}` is not one of gh's own commands that use a token, and gh runs an \
             alias or an extension by such a word, which would get it too"
        ));
    }
    // `--web`, or `-w` alone or among other one-letter flags.
    let web = |a: &String| {
        let cluster = a.len() > 1
            && a.starts_with('-')
            && !a.starts_with("--")
            && a[1..].bytes().all(|c| c.is_ascii_alphabetic());
        a == "--web" || a.starts_with("--web=") || (cluster && a.contains('w'))
    };
    if args.iter().any(web) {
        return Some("`--web` opens a browser, which would get it too".into());
    }
    // What follows `--` goes to `git clone` (`gh repo clone`, `gh repo fork`).
    let rest = args
        .iter()
        .position(|a| a == "--")
        .map_or(&[][..], |k| &args[k + 1..]);
    git_args_launch("clone", rest)
}

/// git's commands that reach a remote, and so may need its credentials. The
/// rest need none, and several run a program they are given (`bisect run`,
/// `rebase --exec`, `submodule foreach`, `difftool`).
const GIT_COMMANDS: &[&str] = &["clone", "fetch", "ls-remote", "pull", "push", "remote"];

/// git's long options that name a program for it to run, or settings for the
/// repository it makes. git takes any unambiguous prefix of a long option, so
/// a prefix counts too.
const GIT_PROGRAM_OPTIONS: &[&str] = &[
    "--upload-pack",
    "--receive-pack",
    "--exec",
    "--template",
    "--config",
];

fn git_launches(args: &[String]) -> Option<String> {
    // Before the command, only the options that name no program and no
    // setting.
    let mut i = 0;
    let command = loop {
        let Some(a) = args.get(i) else {
            return Some("the call runs none of git's commands that reach a remote".into());
        };
        match a.as_str() {
            "-C" | "--git-dir" | "--work-tree" => i += 2,
            "-P"
            | "--no-pager"
            | "--bare"
            | "--no-replace-objects"
            | "--literal-pathspecs"
            | "--no-optional-locks" => i += 1,
            a if a.starts_with("--git-dir=") || a.starts_with("--work-tree=") => i += 1,
            a if a.starts_with('-') => {
                return Some(format!(
                    "`{a}` before git's command can name a program or a setting for git to run \
                     (as `-c alias.x=!…` does), which would get it too"
                ));
            }
            a => break a,
        }
    };
    if !GIT_COMMANDS.contains(&command) {
        return Some(format!(
            "`git {command}` is not one of git's commands that reach a remote ({}), and git runs \
             an alias or a `git-{command}` program by such a word, which would get it too",
            GIT_COMMANDS.join(", ")
        ));
    }
    git_args_launch(command, &args[i + 1..])
}

/// Why the arguments of git's `command` could make it run a program they
/// name, or None: an option that names one (`--upload-pack`, `clone -c`, …),
/// or a URL that git hands to a helper program.
fn git_args_launch(command: &str, args: &[String]) -> Option<String> {
    // `-u` is the upload-pack program for clone and ls-remote (push's is
    // --set-upstream), and `-c` a setting for the repository clone makes.
    let letters = match command {
        "clone" => "uc",
        "ls-remote" => "u",
        _ => "",
    };
    for a in args {
        let (name, value) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v)),
            _ => (a.as_str(), None),
        };
        let long = name.len() > 2
            && name.starts_with("--")
            && GIT_PROGRAM_OPTIONS.iter().any(|o| o.starts_with(name));
        let short = a.len() > 1
            && a.starts_with('-')
            && !a.starts_with("--")
            && a[1..].contains(|c: char| letters.contains(c));
        if long || short {
            return Some(format!(
                "`{a}` names a program for git to run, or a setting for the repository it makes, \
                 which would get it too"
            ));
        }
        if let Some(t) = helper_transport(value.unwrap_or(a)) {
            return Some(format!(
                "`{a}` names the transport `{t}`, which git hands to a program \
                 (`git-remote-{t}`), which would get it too"
            ));
        }
    }
    None
}

/// The transport that git hands a URL to a helper program for: `<name>::…`,
/// or a `<scheme>://…` it does not speak itself.
fn helper_transport(a: &str) -> Option<&str> {
    let word = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_alphanumeric())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    };
    if let Some((t, _)) = a.split_once("::") {
        if word(t) {
            return Some(t);
        }
    }
    const NATIVE: &[&str] = &[
        "file", "ftp", "ftps", "git", "git+ssh", "http", "https", "ssh", "ssh+git",
    ];
    let (scheme, _) = a.split_once("://")?;
    (word(scheme) && !NATIVE.contains(&scheme.to_ascii_lowercase().as_str())).then_some(scheme)
}

/// Where `exec` finds `program` for a job run in `cwd` with `path`: a name
/// with a `/` is the file it names, relative to `cwd`; a bare name is the
/// first executable file of that name on `path`, where an empty or relative
/// entry is relative to `cwd`. With `absolute_only`, only `path`'s absolute
/// entries count. Canonical, so a link is its target.
fn resolve(program: &str, path: Option<&str>, cwd: &Path, absolute_only: bool) -> Option<PathBuf> {
    fn exe(p: PathBuf) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(&p).ok()?;
        if m.is_file() && m.permissions().mode() & 0o111 != 0 {
            p.canonicalize().ok()
        } else {
            None
        }
    }
    if program.contains('/') {
        return exe(cwd.join(program));
    }
    path?.split(':').find_map(|dir| {
        let d = Path::new(dir);
        if d.is_absolute() {
            exe(d.join(program))
        } else if absolute_only {
            None
        } else {
            exe(cwd.join(d).join(program))
        }
    })
}

#[cfg(test)]
mod tests;
