//! Tool policy (spec §3.9): the three-band gate — allow, confirm, deny — over
//! what a call will touch, not over strings. Evaluated after the toollet has
//! validated its input and named its resources (`Plan`), before anything runs.
//! Most restrictive wins. Deny reasons are written for the model and the
//! operator to read ("path /etc/passwd is outside the workspace roots").

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use theseus_kernel::{Authority, Policy, PolicyDecision, Proposal};
use theseus_tools::{paths, Access, Plan, Tool, ToolClass, ToolCtx};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Allow,
    Confirm,
    Deny,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Allow => "allow",
            Mode::Confirm => "confirm",
            Mode::Deny => "deny",
        }
    }
}

/// `[policy].approve_policy`: what a call that needs approval does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovePolicy {
    /// Wait for the operator's Approve (the confirm card).
    #[default]
    Approve,
    /// Run at once; post a notice in the session's channel.
    Notify,
}

/// `[policy].off_policy`: what a call the policy denies does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OffPolicy {
    /// Refuse it, with the reason.
    #[default]
    Deny,
    /// Run it anyway; post a notice in the session's channel.
    Notify,
}

/// Why a call ran that the policy alone would have stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// `approval_skipped` or `off_policy`.
    pub kind: String,
    /// The setting that let it run, e.g. `approve_policy = notify`.
    pub setting: String,
    /// What the policy said (the confirm or deny reason).
    pub rule: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub mode: Mode,
    pub reason: String,
    /// Set when a notify setting let the call run instead of asking or refusing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<Notice>,
    /// A floor denial: no setting lifts it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
}

impl Decision {
    fn new(mode: Mode, reason: String) -> Self {
        Self {
            mode,
            reason,
            notify: None,
            floor: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolPolicy {
    /// Canonical workspace roots.
    pub roots: Vec<PathBuf>,
    /// Canonical protected paths, denied even under a root.
    pub deny_paths: Vec<PathBuf>,
    pub read: Mode,
    pub write: Mode,
    pub run: Mode,
    /// `proc.run` argv prefixes that run without confirmation.
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that never run.
    pub deny_argv: Vec<Vec<String>>,
    /// Per-tool mode overrides by canonical name.
    pub overrides: BTreeMap<String, Mode>,
    /// Who confirms (the execution's principal).
    pub confirmer: String,
    pub approve_policy: ApprovePolicy,
    pub off_policy: OffPolicy,
    /// Denied whatever the settings say: Theseus's store, spool, and bindings
    /// file, and the 1Password CLI's credentials (canonical paths).
    pub floor_paths: Vec<PathBuf>,
    /// Programs denied whatever the settings say (`theseusd`, `op`).
    pub floor_argv: Vec<Vec<String>>,
}

/// Programs no setting can make Theseus run: its own binary (spec §3.21, the
/// kernel is off limits to the agent) and the 1Password CLI (every secret).
pub fn floor_argv() -> Vec<Vec<String>> {
    vec![vec!["theseusd".into()], vec!["op".into()]]
}

/// Flags that never run unconfirmed, whatever the allow list says: they make
/// an innocent-looking command write a file (`--output`), read any file
/// (`--no-index`), or run a program configured elsewhere (`--ext-diff`,
/// `--textconv`, `--exec`, `--upload-pack`, `-c`).
const NEVER_UNCONFIRMED: &[&str] = &[
    "--output",
    "--no-index",
    "--ext-diff",
    "--textconv",
    "--exec",
    "--upload-pack",
    "--receive-pack",
    "--config-env",
    "-c",
];

/// The command's arguments that name paths, resolved against `cwd`: absolute,
/// `~`, `.`-relative, or containing a separator; `--opt=value` is judged by
/// its value. Plain words (`status`, `-la`) are not paths.
fn path_args(argv: &[String], cwd: &std::path::Path) -> Vec<(String, PathBuf)> {
    argv.iter()
        .skip(1)
        .filter_map(|a| {
            let v = match a.split_once('=') {
                Some((k, v)) if k.starts_with('-') => v,
                _ if a.starts_with('-') => return None,
                _ => a.as_str(),
            };
            let looks =
                v.starts_with('/') || v.starts_with('~') || v.starts_with('.') || v.contains('/');
            if v.is_empty() || !looks {
                return None;
            }
            let expanded = PathBuf::from(shellexpand::tilde(v).into_owned());
            let full = if expanded.is_absolute() {
                expanded
            } else {
                cwd.join(expanded)
            };
            Some((a.clone(), paths::canonical_best_effort(&full)))
        })
        .collect()
}

fn prefix_match(argv: &[String], prefix: &[String]) -> bool {
    !prefix.is_empty() && argv.len() >= prefix.len() && argv.iter().zip(prefix).all(|(a, p)| a == p)
}

/// The program name without its directory: `/usr/bin/sudo` matches `sudo`.
fn normalized_argv(argv: &[String]) -> Vec<String> {
    let mut v = argv.to_vec();
    if let Some(first) = v.first_mut() {
        if let Some(base) = std::path::Path::new(first.as_str()).file_name() {
            *first = base.to_string_lossy().into_owned();
        }
    }
    v
}

impl ToolPolicy {
    pub fn class_mode(&self, tool: &dyn Tool) -> Mode {
        if let Some(m) = self.overrides.get(tool.name()) {
            return *m;
        }
        match tool.class() {
            ToolClass::Read => self.read,
            ToolClass::Write => self.write,
            ToolClass::Run => self.run,
        }
    }

    /// The gate's answer after the operator's settings: a confirm becomes a
    /// notified allow under `approve_policy = notify`, and a deny (other than
    /// the floor) becomes a notified allow under `off_policy = notify`.
    pub fn decide(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        let d = self.decide_by_policy(tool, plan);
        match d.mode {
            Mode::Confirm if self.approve_policy == ApprovePolicy::Notify => Decision {
                mode: Mode::Allow,
                reason: format!("ran without approval: {}", d.reason),
                notify: Some(Notice {
                    kind: "approval_skipped".into(),
                    setting: "approve_policy = notify".into(),
                    rule: d.reason,
                }),
                floor: false,
            },
            Mode::Deny if self.off_policy == OffPolicy::Notify && !d.floor => Decision {
                mode: Mode::Allow,
                reason: format!("ran against policy: {}", d.reason),
                notify: Some(Notice {
                    kind: "off_policy".into(),
                    setting: "off_policy = notify".into(),
                    rule: d.reason,
                }),
                floor: false,
            },
            _ => d,
        }
    }

    /// The policy alone: floor, protected paths, roots, class, argv lists.
    fn decide_by_policy(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        let floor = |reason: String| Decision {
            mode: Mode::Deny,
            reason,
            notify: None,
            floor: true,
        };
        for r in &plan.resources {
            let p = paths::canonical_best_effort(&r.path);
            if let Some(f) = self.floor_paths.iter().find(|f| paths::within(&p, f)) {
                return floor(format!(
                    "{} is Theseus's own or holds its secrets ({}); no setting allows it",
                    p.display(),
                    f.display()
                ));
            }
        }
        if let Some(argv) = &plan.argv {
            let nargv = normalized_argv(argv);
            if let Some(f) = self.floor_argv.iter().find(|f| prefix_match(&nargv, f)) {
                return floor(format!(
                    "`{}` is never run (`{}` is on the floor: Theseus's own binary or the 1Password CLI)",
                    argv.join(" "),
                    f.join(" ")
                ));
            }
        }
        for r in &plan.resources {
            let p = paths::canonical_best_effort(&r.path);
            if let Some(d) = self.deny_paths.iter().find(|d| paths::within(&p, d)) {
                return Decision::new(
                    Mode::Deny,
                    format!(
                        "{} is protected ({} is on the deny list)",
                        p.display(),
                        d.display()
                    ),
                );
            }
            if !self.roots.iter().any(|root| paths::within(&p, root)) {
                let roots: Vec<String> =
                    self.roots.iter().map(|r| r.display().to_string()).collect();
                return Decision::new(
                    Mode::Deny,
                    format!(
                        "{} is outside the workspace roots ({})",
                        p.display(),
                        if roots.is_empty() {
                            "none configured: set [tools].projects_dir".into()
                        } else {
                            roots.join(", ")
                        }
                    ),
                );
            }
        }
        let mut mode = self.class_mode(tool);
        let mut reason = format!(
            "{} is `{}` for {} tools",
            tool.name(),
            mode.as_str(),
            tool.class().as_str()
        );
        if let Some(argv) = &plan.argv {
            let nargv = normalized_argv(argv);
            if let Some(p) = self.deny_argv.iter().find(|p| prefix_match(&nargv, p)) {
                return Decision::new(
                    Mode::Deny,
                    format!(
                        "`{}` matches the deny list entry `{}`",
                        argv.join(" "),
                        p.join(" ")
                    ),
                );
            }
            // The resources above are only the working directory; a command's
            // arguments can name any path, so they are judged too.
            let cwd = plan
                .resources
                .iter()
                .find(|r| r.access == Access::Exec)
                .map(|r| paths::canonical_best_effort(&r.path))
                .or_else(|| self.roots.first().cloned())
                .unwrap_or_default();
            let args = path_args(argv, &cwd);
            for (a, p) in &args {
                if let Some(d) = self.deny_paths.iter().find(|d| paths::within(p, d)) {
                    return Decision::new(
                        Mode::Deny,
                        format!(
                            "argument `{a}` is protected ({} is on the deny list)",
                            d.display()
                        ),
                    );
                }
            }
            if mode != Mode::Deny {
                if let Some(p) = self.allow_argv.iter().find(|p| prefix_match(&nargv, p)) {
                    let entry = format!(
                        "`{}` matches the allow list entry `{}`",
                        argv.join(" "),
                        p.join(" ")
                    );
                    let flag = argv.iter().skip(1).find(|a| {
                        NEVER_UNCONFIRMED
                            .iter()
                            .any(|f| a.as_str() == *f || a.starts_with(&format!("{f}=")))
                    });
                    let outside = args
                        .iter()
                        .find(|(_, p)| !self.roots.iter().any(|root| paths::within(p, root)));
                    match (flag, outside) {
                        (Some(f), _) => {
                            reason = format!("{entry}, but `{f}` never runs unconfirmed")
                        }
                        (None, Some((a, _))) => {
                            reason = format!(
                                "{entry}, but its argument `{a}` is outside the workspace roots"
                            )
                        }
                        (None, None) => {
                            mode = Mode::Allow;
                            reason = entry;
                        }
                    }
                }
            }
        }
        if mode == Mode::Confirm {
            reason = format!("{}: {}", plan.summary, reason);
        }
        Decision::new(mode, reason)
    }

    /// Does any resource of this plan write?
    pub fn writes(plan: &Plan) -> bool {
        plan.resources.iter().any(|r| r.access == Access::Write)
    }
}

/// The kernel's gate ordering (transform → validate → policy) over a toollet:
/// `validate` is the toollet's own typed parse, `decide` is `ToolPolicy`.
pub struct GatePolicy<'a> {
    pub tool: &'a dyn Tool,
    pub ctx: &'a ToolCtx,
    pub policy: &'a ToolPolicy,
    pub plan: std::sync::Mutex<Option<Plan>>,
    pub decision: std::sync::Mutex<Option<Decision>>,
}

impl<'a> GatePolicy<'a> {
    pub fn new(tool: &'a dyn Tool, ctx: &'a ToolCtx, policy: &'a ToolPolicy) -> Self {
        Self {
            tool,
            ctx,
            policy,
            plan: Default::default(),
            decision: Default::default(),
        }
    }
}

impl Policy for GatePolicy<'_> {
    fn validate(&self, p: &Proposal) -> Result<(), String> {
        let plan = self.tool.plan(&p.args, self.ctx)?;
        *self.plan.lock().unwrap() = Some(plan);
        Ok(())
    }
    fn decide(&self, _p: &Proposal, _auth: &Authority) -> PolicyDecision {
        let plan = self.plan.lock().unwrap().clone().unwrap_or_default();
        let d = self.policy.decide(self.tool, &plan);
        let out = match d.mode {
            Mode::Allow => PolicyDecision::Allow,
            Mode::Confirm => PolicyDecision::Confirm {
                by: self.policy.confirmer.clone(),
            },
            Mode::Deny => PolicyDecision::Deny {
                reason: d.reason.clone(),
            },
        };
        *self.decision.lock().unwrap() = Some(d);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use theseus_tools::{Resource, Retry};

    struct T(ToolClass);
    impl Tool for T {
        fn name(&self) -> &'static str {
            "fs.test"
        }
        fn description(&self) -> &'static str {
            ""
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn class(&self) -> ToolClass {
            self.0
        }
        fn retry(&self) -> Retry {
            Retry::SafeToRepeat
        }
        fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
            Ok(Plan::default())
        }
    }

    fn policy(root: &std::path::Path) -> ToolPolicy {
        ToolPolicy {
            roots: vec![root.to_path_buf()],
            deny_paths: vec![root.join("secret")],
            read: Mode::Allow,
            write: Mode::Confirm,
            run: Mode::Confirm,
            allow_argv: vec![vec!["git".into(), "status".into()]],
            deny_argv: vec![vec!["sudo".into()]],
            overrides: BTreeMap::new(),
            confirmer: "operator".into(),
            approve_policy: ApprovePolicy::Approve,
            off_policy: OffPolicy::Deny,
            floor_paths: vec![root.join("state")],
            floor_argv: floor_argv(),
        }
    }

    fn plan(path: std::path::PathBuf, access: Access, argv: Option<Vec<&str>>) -> Plan {
        Plan {
            resources: vec![Resource { path, access }],
            argv: argv.map(|v| v.into_iter().map(String::from).collect()),
            summary: "s".into(),
        }
    }

    #[test]
    fn roots_deny_list_argv_lists_and_class_modes() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let p = policy(&root);
        let r = T(ToolClass::Read);
        let w = T(ToolClass::Write);
        let x = T(ToolClass::Run);
        assert_eq!(
            p.decide(&r, &plan(root.join("a.rs"), Access::Read, None))
                .mode,
            Mode::Allow
        );
        assert_eq!(
            p.decide(&w, &plan(root.join("a.rs"), Access::Write, None))
                .mode,
            Mode::Confirm
        );
        let out = p.decide(&r, &plan("/etc/passwd".into(), Access::Read, None));
        assert_eq!(out.mode, Mode::Deny);
        assert!(
            out.reason.contains("outside the workspace roots"),
            "{}",
            out.reason
        );
        let out = p.decide(&r, &plan(root.join("secret/key"), Access::Read, None));
        assert_eq!(out.mode, Mode::Deny);
        assert!(out.reason.contains("protected"));
        assert_eq!(
            p.decide(
                &x,
                &plan(
                    root.clone(),
                    Access::Exec,
                    Some(vec!["git", "status", "-s"])
                )
            )
            .mode,
            Mode::Allow
        );
        assert_eq!(
            p.decide(
                &x,
                &plan(root.clone(), Access::Exec, Some(vec!["cargo", "test"]))
            )
            .mode,
            Mode::Confirm
        );
        let out = p.decide(
            &x,
            &plan(
                root.clone(),
                Access::Exec,
                Some(vec!["/usr/bin/sudo", "ls"]),
            ),
        );
        assert_eq!(out.mode, Mode::Deny, "{}", out.reason);
        // Arguments are judged too: a protected one is denied for any command;
        // an allow-listed command runs unconfirmed only with every path inside
        // the roots and no write- or exec-capable flag.
        let run = |argv: Vec<&str>| p.decide(&x, &plan(root.clone(), Access::Exec, Some(argv)));
        let secret = format!("{}/secret/key", root.display());
        let out = run(vec!["cat", &secret]);
        assert_eq!(out.mode, Mode::Deny, "{}", out.reason);
        assert!(out.reason.contains("is protected"), "{}", out.reason);
        let out = run(vec!["git", "status", "--output=/tmp/x"]);
        assert_eq!(out.mode, Mode::Confirm, "{}", out.reason);
        assert!(
            out.reason.contains("never runs unconfirmed"),
            "{}",
            out.reason
        );
        let out = run(vec!["git", "status", "/etc"]);
        assert_eq!(out.mode, Mode::Confirm, "{}", out.reason);
        assert!(
            out.reason.contains("outside the workspace roots"),
            "{}",
            out.reason
        );
        assert_eq!(
            run(vec!["git", "status", ".."]).mode,
            Mode::Confirm,
            "the parent of the root"
        );
        assert_eq!(
            run(vec!["git", "status", "src/lib.rs"]).mode,
            Mode::Allow,
            "inside the root"
        );
        assert_eq!(run(vec!["git", "status", "-s"]).mode, Mode::Allow);
        // The operator's settings: approvals become notices, denials become
        // notices, and the floor stays denied either way.
        let mut n = p.clone();
        n.approve_policy = ApprovePolicy::Notify;
        let out = n.decide(&w, &plan(root.join("a.rs"), Access::Write, None));
        assert_eq!(out.mode, Mode::Allow);
        assert_eq!(out.notify.as_ref().unwrap().kind, "approval_skipped");
        let out = n.decide(&r, &plan("/etc/passwd".into(), Access::Read, None));
        assert_eq!(
            out.mode,
            Mode::Deny,
            "approve_policy alone never lifts a deny"
        );
        n.off_policy = OffPolicy::Notify;
        let out = n.decide(&r, &plan("/etc/passwd".into(), Access::Read, None));
        assert_eq!(out.mode, Mode::Allow);
        let nt = out.notify.unwrap();
        assert_eq!(nt.kind, "off_policy");
        assert!(
            nt.rule.contains("outside the workspace roots"),
            "{}",
            nt.rule
        );
        let out = n.decide(
            &x,
            &plan(root.clone(), Access::Exec, Some(vec!["sudo", "ls"])),
        );
        assert_eq!(
            out.mode,
            Mode::Allow,
            "off_policy = notify runs the deny list too"
        );
        for (what, pl) in [
            (
                "state dir",
                plan(root.join("state/store"), Access::Read, None),
            ),
            (
                "theseusd",
                plan(root.clone(), Access::Exec, Some(vec!["theseusd", "config"])),
            ),
            (
                "op",
                plan(
                    root.clone(),
                    Access::Exec,
                    Some(vec!["/usr/bin/op", "read", "x"]),
                ),
            ),
        ] {
            let out = n.decide(&x, &pl);
            assert_eq!(out.mode, Mode::Deny, "the floor holds: {what}");
            assert!(out.floor && out.notify.is_none(), "{what}: {}", out.reason);
        }
        let mut p2 = p.clone();
        p2.overrides.insert("fs.test".into(), Mode::Allow);
        assert_eq!(
            p2.decide(&w, &plan(root.join("a.rs"), Access::Write, None))
                .mode,
            Mode::Allow
        );
    }
}
