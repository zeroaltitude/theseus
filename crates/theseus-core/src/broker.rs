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
//!   never an alias, an extension, or `--web`'s browser; and git only for its
//!   commands that reach a remote, never an alias, `-c`, or a program that
//!   an option or a URL names. What a granted program runs on its own
//!   account still gets the variable: its repository's hooks, and the
//!   helpers its config names.
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
fn and(items: &[&str]) -> String {
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
}

impl ForJob {
    /// For the result: each thing the job was not given, and why, one
    /// bracketed line each. Never a value.
    pub fn note(&self) -> Option<String> {
        let lines: Vec<String> = self
            .withheld
            .iter()
            .map(|(g, why)| format!("[{} got no {}: {why}; it ran without it]", g.to, g.what()))
            .chain(self.indirect.iter().map(|n| format!("[{n}]")))
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

pub struct Broker {
    /// Each program's grant: variable, then secret.
    programs: BTreeMap<String, Vec<(String, String)>>,
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
                vars.iter()
                    .map(|(v, s)| Grant {
                        to: program.into(),
                        variable: Some(v.clone()),
                        secret: s.clone(),
                    })
                    .collect()
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
            let names: Vec<&str> = v.iter().map(|(n, _)| n.as_str()).collect();
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
        out
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

    // M4 (decision 15): a run-time credential request lands here too, at the requesting tool's posture; 1Password itself always waits.
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
///   (`git_launches`).
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
        _ => None,
    }
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
mod tests {
    use super::*;
    use crate::config::{ProgramGrant, SecretPosture};

    /// A directory of stand-in programs: each an executable script.
    fn bin(names: &[&str]) -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        for n in names {
            let p = d.path().join(n);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        d
    }

    fn broker(bin: &Path, board: Arc<SecretBoard>, postures: &[(&str, Posture)]) -> Broker {
        let mut cfg = BrokerConfig::default();
        cfg.programs.insert(
            "gh".into(),
            ProgramGrant {
                env: [("GH_TOKEN".to_string(), "github_token".to_string())].into(),
            },
        );
        for (n, p) in postures {
            cfg.secrets
                .insert(n.to_string(), SecretPosture { posture: *p });
        }
        Broker::new(
            &cfg,
            board,
            Some(format!("{}:/usr/bin:/bin", bin.display())),
        )
        .with_wait(Duration::from_millis(200))
    }

    fn ready(pairs: &[(&str, &str)]) -> Arc<SecretBoard> {
        let board = SecretBoard::new(pairs.iter().map(|(n, _)| n.to_string()), Instant::now());
        board.publish(
            pairs
                .iter()
                .map(|(n, v)| (n.to_string(), Ok(Secret::new(v.to_string()))))
                .collect(),
            "test",
        );
        board
    }

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// Direct argv only: `gh`, or its own path, gets the variable; a shell,
    /// `env`, or a `gh` elsewhere gets nothing, and the note says why.
    #[tokio::test]
    async fn a_program_gets_its_variable_only_by_its_own_argv() {
        let b = bin(&["gh"]);
        let other = bin(&["gh"]);
        let br = broker(b.path(), ready(&[("github_token", "tok-123456")]), &[]);
        let path = format!("{}:/usr/bin:/bin", b.path().display());
        let cwd = other.path();
        let gh = b.path().join("gh").display().to_string();
        for direct in [argv(&["gh", "api", "user"]), argv(&[&gh, "api"])] {
            let j = br
                .for_job(&direct, &[], cwd, Some(&path), Posture::Notify)
                .await;
            assert_eq!(j.env.len(), 1, "{direct:?}");
            assert_eq!(
                (j.env[0].0.as_str(), j.env[0].1.expose()),
                ("GH_TOKEN", "tok-123456")
            );
            assert_eq!(got(&j.granted).as_deref(), Some("gh got GH_TOKEN"));
            assert!(j.note().is_none());
        }
        for (indirect, says) in [
            (
                argv(&["sh", "-c", "gh api user"]),
                "it is run by `sh`, not by its own argv",
            ),
            (argv(&["env", "gh", "api"]), "it is run by `env`"),
            (
                argv(&["bash", "-lc", "cd x && /usr/bin/gh pr list"]),
                "it is run by `bash`",
            ),
            (
                argv(&["./gh", "api"]),
                "`./gh` is not the gh on the daemon's PATH",
            ),
        ] {
            let j = br
                .for_job(&indirect, &[], cwd, Some(&path), Posture::Notify)
                .await;
            assert!(j.env.is_empty(), "{indirect:?}");
            let note = j.note().unwrap();
            assert!(note.starts_with("[gh got no GH_TOKEN: "), "{note}");
            assert!(note.contains(says), "{note}");
        }
        // A PATH the model set, with its own gh first, is not the daemon's.
        let evil = format!("{}:{path}", other.path().display());
        let j = br
            .for_job(
                &argv(&["gh", "api"]),
                &[],
                cwd,
                Some(&evil),
                Posture::Notify,
            )
            .await;
        assert!(j.env.is_empty());
        // A program the broker does not know, naming none, gets no note.
        let j = br
            .for_job(
                &argv(&["ls", "-la"]),
                &[],
                cwd,
                Some(&path),
                Posture::Notify,
            )
            .await;
        assert!(j.env.is_empty() && j.note().is_none());
    }

    /// A secret that has not settled is waited for, bounded, and then
    /// withheld, never replaced; one that settles within the wait is given.
    /// A failed one is withheld at once, with its reason.
    #[tokio::test]
    async fn an_unresolved_secret_is_waited_for_then_withheld() {
        let b = bin(&["gh"]);
        let path = format!("{}:/usr/bin:/bin", b.path().display());
        let board = SecretBoard::new(["github_token".to_string()], Instant::now());
        let br = broker(b.path(), board.clone(), &[]);
        let t0 = Instant::now();
        let j = br
            .for_job(
                &argv(&["gh", "api"]),
                &[],
                b.path(),
                Some(&path),
                Posture::Notify,
            )
            .await;
        assert!(t0.elapsed() >= Duration::from_millis(200), "it waited");
        assert!(j.env.is_empty());
        let note = j.note().unwrap();
        assert!(
            note.contains(
                "gh got no GH_TOKEN: github_token was still resolving after 200 ms; it ran \
                 without it"
            ),
            "{note}"
        );
        let later = board.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            later.publish(
                [("github_token".to_string(), Ok(Secret::new("tok".into())))].into(),
                "test",
            );
        });
        let j = br
            .for_job(
                &argv(&["gh", "api"]),
                &[],
                b.path(),
                Some(&path),
                Posture::Notify,
            )
            .await;
        assert_eq!(j.env.len(), 1, "given once it resolved");
        let failed = SecretBoard::new(["github_token".to_string()], Instant::now());
        failed.publish(
            [("github_token".to_string(), Err("vault said no".to_string()))].into(),
            "test",
        );
        let br = broker(b.path(), failed, &[]);
        let j = br
            .for_job(
                &argv(&["gh", "api"]),
                &[],
                b.path(),
                Some(&path),
                Posture::Notify,
            )
            .await;
        assert!(j
            .note()
            .unwrap()
            .contains("github_token did not resolve (vault said no)"));
    }

    /// A secret's posture holds the call to it: the gate gets the strictest
    /// posture and its setting, and a spawn at a looser posture than the
    /// secret's withholds it.
    #[tokio::test]
    async fn a_secrets_posture_is_the_least_a_call_given_it_runs_at() {
        let b = bin(&["gh"]);
        let path = format!("{}:/usr/bin:/bin", b.path().display());
        let br = broker(
            b.path(),
            ready(&[("github_token", "tok")]),
            &[("github_token", Posture::Approve)],
        );
        let grants = br.at_gate(
            "proc.run",
            Some(&argv(&["gh", "api"])),
            &[],
            b.path(),
            Some(&path),
        );
        assert_eq!(gets(&grants).as_deref(), Some("gh gets GH_TOKEN"));
        assert_eq!(
            br.need(&grants),
            Some((
                Posture::Approve,
                "[broker.secrets.github_token] posture = approve".into()
            ))
        );
        let j = br
            .for_job(
                &argv(&["gh", "api"]),
                &[],
                b.path(),
                Some(&path),
                Posture::Notify,
            )
            .await;
        assert!(j.env.is_empty());
        assert!(j
            .note()
            .unwrap()
            .contains("github_token's posture is approve, and this call ran at notify"));
        let j = br
            .for_job(
                &argv(&["gh", "api"]),
                &[],
                b.path(),
                Some(&path),
                Posture::Approve,
            )
            .await;
        assert_eq!(j.env.len(), 1);
        let (p, setting) = broker(b.path(), ready(&[]), &[]).posture_of("github_token");
        assert_eq!(p, Posture::Notify, "{setting}");
    }

    /// A toollet gets only what the wiring granted it, and each use counts;
    /// health lists every grant by name, never a value.
    #[tokio::test]
    async fn a_toollet_gets_only_its_grant_and_health_counts_uses() {
        let b = bin(&["gh"]);
        let br = broker(
            b.path(),
            ready(&[("github_token", "tok"), ("search_key", "key-9")]),
            &[],
        );
        br.grant_tool("test.keyed", "search_key");
        br.settle_for_tool("test.keyed").await;
        let v = br
            .secret_for_tool("test.keyed", "search_key", Posture::Notify)
            .unwrap();
        assert_eq!(v.expose(), "key-9");
        assert!(br
            .secret_for_tool("test.keyed", "github_token", Posture::Notify)
            .unwrap_err()
            .contains("was not granted github_token"));
        assert!(br
            .secret_for_tool("fs.read", "search_key", Posture::Open)
            .is_err());
        let s = br.status();
        let show = serde_json::to_string(&s).unwrap();
        assert!(
            !show.contains("key-9") && !show.contains("\"tok\""),
            "{show}"
        );
        let row = |to: &str| s.iter().find(|g| g.to == to).unwrap().clone();
        assert_eq!(
            (row("test.keyed").kind.as_str(), row("test.keyed").uses),
            ("tool", 1)
        );
        assert_eq!(
            (
                row("gh").variable.as_deref(),
                row("gh").secret.as_str(),
                row("gh").posture.as_str()
            ),
            (Some("GH_TOKEN"), "github_token", "notify")
        );
    }

    /// Review 2's H7: a launcher's grant reaches only the program the call
    /// names. gh gets it for its own commands, never an alias, an extension,
    /// `--web`'s browser, or a program in git's flags after `--`; git for its
    /// commands that reach a remote, never an alias, `-c`, or a program that
    /// an option or a URL names; and no program gets it from a call that
    /// sets its own environment. The gate's view agrees with the spawn's.
    #[tokio::test]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    async fn a_launchers_grant_reaches_only_the_program_the_call_names() {
        let b = bin(&["gh", "git"]);
        let mut cfg = BrokerConfig::default();
        for program in ["gh", "git"] {
            cfg.programs.insert(
                program.into(),
                ProgramGrant {
                    env: [("GH_TOKEN".to_string(), "github_token".to_string())].into(),
                },
            );
        }
        let path = format!("{}:/usr/bin:/bin", b.path().display());
        let br = Broker::new(
            &cfg,
            ready(&[("github_token", "tok-123456")]),
            Some(path.clone()),
        );
        let given: [&[&str]; 9] = [
            &["gh", "pr", "list"],
            &["gh", "api", "user", "--jq", ".login"],
            &["gh", "repo", "clone", "invented/x", "--", "--depth=1"],
            &["git", "push", "-u", "origin", "main"],
            &["git", "-C", "sub", "--no-pager", "fetch", "--prune"],
            &["git", "--git-dir=x/.git", "pull", "--rebase"],
            &["git", "clone", "--depth", "1", "ssh://git.invented/x.git"],
            &["git", "clone", "git@git.invented:x.git"],
            &["git", "ls-remote", "origin"],
        ];
        for a in given {
            let j = br
                .for_job(&argv(a), &[], b.path(), Some(&path), Posture::Notify)
                .await;
            assert_eq!((j.env.len(), j.note()), (1, None), "{a:?}");
            let at_gate = br.at_gate("proc.run", Some(&argv(a)), &[], b.path(), Some(&path));
            assert_eq!(at_gate.len(), 1, "{a:?}");
        }
        let withheld: [(&[&str], &[&str], &str); 25] = [
            (
                &["gh", "leak"],
                &[],
                "`gh leak` is not one of gh's own commands",
            ),
            (
                &["gh", "extension", "exec", "leak"],
                &[],
                "`gh extension` is not",
            ),
            (&["gh", "ext", "exec", "leak"], &[], "`gh ext` is not"),
            (&["gh", "help", "leak"], &[], "`gh help` is not"),
            (&["gh", "browse"], &[], "`gh browse` is not"),
            (&["gh", "codespace", "ssh"], &[], "`gh codespace` is not"),
            (&["gh"], &[], "the call runs none of gh's commands"),
            (
                &["gh", "pr", "view", "--web"],
                &[],
                "`--web` opens a browser",
            ),
            (
                &["gh", "issue", "list", "-w"],
                &[],
                "`--web` opens a browser",
            ),
            (&["gh", "pr", "view", "-cw"], &[], "`--web` opens a browser"),
            (
                &["gh", "repo", "clone", "invented/x", "--", "-u", "leak"],
                &[],
                "`-u` names a program for git to run",
            ),
            (
                &["gh", "api", "user"],
                &["GH_REPO"],
                "the call sets GH_REPO, and a variable",
            ),
            (
                &["git", "push"],
                &["PATH", "GIT_EXEC_PATH"],
                "the call sets PATH and GIT_EXEC_PATH",
            ),
            (
                &["git", "leak"],
                &[],
                "`git leak` is not one of git's commands that reach",
            ),
            (&["git", "status"], &[], "`git status` is not one of"),
            (
                &["git", "-c", "alias.x=!leak", "x"],
                &[],
                "`-c` before git's command",
            ),
            (
                &["git", "--exec-path=/tmp", "push"],
                &[],
                "`--exec-path=/tmp` before",
            ),
            (
                &["git", "fetch", "--upload-pack=leak", "."],
                &[],
                "names a program",
            ),
            (
                &["git", "fetch", "--upload=leak", "."],
                &[],
                "names a program",
            ),
            (
                &["git", "push", "--receive-pack", "leak", "origin"],
                &[],
                "names a program",
            ),
            (
                &["git", "clone", "-u", "leak", "src"],
                &[],
                "names a program",
            ),
            (
                &["git", "clone", "-c", "core.fsmonitor=leak", "src"],
                &[],
                "names a program",
            ),
            (
                &["git", "clone", "--template=t", "src"],
                &[],
                "names a program",
            ),
            (
                &["git", "fetch", "leak::x"],
                &[],
                "names the transport `leak`",
            ),
            (
                &["git", "push", "--repo=leak://x"],
                &[],
                "names the transport `leak`",
            ),
        ];
        for (a, env, says) in withheld {
            let j = br
                .for_job(&argv(a), env, b.path(), Some(&path), Posture::Notify)
                .await;
            assert!(j.env.is_empty(), "{a:?}");
            let note = j.note().unwrap_or_default();
            assert!(
                note.starts_with(&format!("[{} got no GH_TOKEN: ", a[0])),
                "{note}"
            );
            assert!(note.contains(says), "{a:?}: {note}");
            let at_gate = br.at_gate("proc.run", Some(&argv(a)), env, b.path(), Some(&path));
            assert!(at_gate.is_empty(), "{a:?}");
        }
    }
}
