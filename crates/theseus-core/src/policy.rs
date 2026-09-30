//! Tool policy (spec §3.9). The finite, named list of tools (and, when MCP
//! lands, of MCP servers) is the surface the operator controls, so the gate
//! decides per tool and never infers what a command does. Every tool and every
//! MCP inherits one posture, with per-tool and per-MCP overrides:
//!
//! | posture   | what happens                       |
//! |-----------|------------------------------------|
//! | `open`    | run                                |
//! | `notify`  | run, and post a structured notice  |
//! | `approve` | wait for the operator's approval   |
//!
//! Nothing the gate decides refuses a call (theseus-8az): the strongest answer
//! is to wait for the operator, who may approve or decline.
//!
//! Evaluated after the toollet has validated its input and named its
//! resources (`Plan`), before anything runs. The first match wins:
//!
//! 1. the floor (Theseus's own binary and state, the 1Password CLI and its
//!    token) waits for approval at every posture, and is marked as the floor;
//! 2. the operator's approve lists (`approve_argv`, `approve_paths`) and any
//!    path outside the roots wait for approval;
//! 3. the operator's explicit allow (`allow_argv`) runs, when every path
//!    argument is inside the roots. An entry is a prefix (`["ls"]` also runs
//!    `ls -la src`), so entries should be narrow;
//! 4. otherwise the tool's posture: `[policy.tools]`, then for an MCP tool
//!    `[policy.mcp]` "server/tool" and "server", then `[policy].enforcement`.
//!    A runtime tightening ("should have asked", theseus-sgh) applies here,
//!    after the config's posture, and the stricter of the two wins. So a
//!    tightening never loosens anything, and a config that already asks is
//!    unchanged. Like a config `approve`, it leaves the allow list alone.
//!
//! Reasons are written for the model and the operator to read
//! ("proc.run — approve (enforcement = approve)").

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use theseus_tools::{paths, Access, Plan, Tool};

/// What the gate does with a call: `[policy].enforcement` (the posture every
/// tool and MCP inherits) and each `[policy.tools]` / `[policy.mcp]` override.
/// A config naming any other value fails to load, and the error lists these.
/// They are ordered from the least strict to the strictest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    /// Run.
    #[default]
    Open,
    /// Run, and post a structured notice.
    Notify,
    /// Wait for the operator's approval.
    Approve,
}

impl Posture {
    pub const ALL: [Posture; 3] = [Posture::Open, Posture::Notify, Posture::Approve];

    pub fn as_str(self) -> &'static str {
        match self {
            Posture::Open => "open",
            Posture::Notify => "notify",
            Posture::Approve => "approve",
        }
    }

    /// A posture by its name.
    pub fn parse(s: &str) -> Option<Posture> {
        Posture::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// A runtime tightening as the gate reads it (theseus-sgh): the posture a
/// "should have asked" press recorded for one tool, and who pressed.
#[derive(Debug, Clone, Copy)]
pub struct Tightened<'a> {
    pub posture: Posture,
    pub by: &'a str,
}

/// A tool's posture now: the config's, or a tightening's when that is
/// stricter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostureNow {
    pub posture: Posture,
    /// What chose it: the config's setting, or `tightened by <who>`.
    pub setting: String,
    /// What the config alone says.
    pub config: Posture,
    pub config_setting: String,
}

impl PostureNow {
    /// A tightening chose the posture.
    pub fn tightened(&self) -> bool {
        self.posture != self.config
    }

    /// Why, as a reason's parenthesis says it: the setting, and under a
    /// tightening what the config says as well.
    fn why(&self) -> String {
        if self.tightened() {
            format!("{}; the config says {}", self.setting, self.config_setting)
        } else {
            self.setting.clone()
        }
    }
}

/// The structured notice a `notify` posture posts: to the session's channel
/// (Discord, web UI, CLI) and to the ledger (`tool.notified`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// `notify`.
    pub kind: String,
    /// The setting that chose the posture, e.g. `enforcement = notify`.
    pub setting: String,
    /// The gate's reason, e.g. `proc.run — notify (enforcement = notify)`.
    pub rule: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    /// `Approve` waits for the operator; `Open` and `Notify` run.
    pub posture: Posture,
    pub reason: String,
    /// The notice a `notify` posture posts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<Notice>,
    /// The floor asked: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. No setting makes it run unasked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    /// What the secret broker gives the call, by name: `gh got GH_TOKEN`
    /// (theseus-dcy). Its tool line and its notice say so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted: Option<String>,
}

impl Decision {
    fn new(posture: Posture, reason: String) -> Self {
        Self {
            posture,
            reason,
            notify: None,
            floor: false,
            granted: None,
        }
    }

    /// This decision at no looser a posture than `need` (theseus-dcy): a call
    /// given a secret runs at least at the secret's posture, whatever chose
    /// its own. `why` is the parenthesis of the new reason, and `setting` the
    /// notice's setting. A decision as strict already is kept.
    pub fn at_least(
        self,
        need: Posture,
        why: &str,
        setting: &str,
        tool: &str,
        summary: &str,
    ) -> Self {
        if need <= self.posture {
            return self;
        }
        let reason = format!("{tool} — {} ({why})", need.as_str());
        let granted = self.granted;
        match need {
            Posture::Approve => Decision {
                granted,
                ..Decision::new(need, format!("{summary}: {reason}"))
            },
            Posture::Notify => Decision {
                notify: Some(Notice {
                    kind: "notify".into(),
                    setting: setting.into(),
                    rule: reason.clone(),
                }),
                granted,
                ..Decision::new(need, reason)
            },
            Posture::Open => Decision {
                granted,
                ..Decision::new(need, reason)
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolPolicy {
    /// Canonical workspace roots; a path outside them waits for approval.
    pub roots: Vec<PathBuf>,
    /// Canonical paths that wait for approval, even under a root.
    pub approve_paths: Vec<PathBuf>,
    /// `proc.run` argv prefixes that run (open) when every path argument is
    /// inside the roots.
    pub allow_argv: Vec<Vec<String>>,
    /// `proc.run` argv prefixes that wait for approval.
    pub approve_argv: Vec<Vec<String>>,
    /// The posture every tool and MCP inherits (`[policy].enforcement`).
    pub enforcement: Posture,
    /// Per-tool postures by canonical name (`[policy.tools]`).
    pub tools: BTreeMap<String, Posture>,
    /// Per-MCP postures by `server/tool` or `server` (`[policy.mcp]`).
    pub mcp: BTreeMap<String, Posture>,
    /// Who confirms (the execution's principal).
    pub confirmer: String,
    /// Paths that always wait for approval: Theseus's store, spool, and
    /// bindings file, and the 1Password CLI's credentials and token (canonical).
    pub floor_paths: Vec<PathBuf>,
    /// Programs that always wait for approval (`theseusd`, `op`).
    pub floor_argv: Vec<Vec<String>>,
}

/// An MCP tool's canonical name is `mcp:<server>/<tool>`.
pub const MCP_PREFIX: &str = "mcp:";

/// Programs Theseus never runs unasked, whatever the posture: its own binary
/// (spec §3.21, the kernel is off limits to the agent) and the 1Password CLI
/// (every secret).
pub fn floor_argv() -> Vec<Vec<String>> {
    vec![vec!["theseusd".into()], vec!["op".into()]]
}

/// What the floor's paths hold, in the operator's words.
const FLOOR_STATE: &str = "Theseus's own state or the 1Password token";

/// The command's arguments that name paths, resolved against `cwd`: absolute,
/// `~`, `.`-relative, or containing a separator; `--opt=value` is judged by
/// its value. Plain words (`status`, `-la`) are not paths.
fn path_args(argv: &[String], cwd: &Path) -> Vec<(String, PathBuf)> {
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
        if let Some(base) = Path::new(first.as_str()).file_name() {
            *first = base.to_string_lossy().into_owned();
        }
    }
    v
}

impl ToolPolicy {
    /// A tool's posture and the setting that chose it: `[policy.tools]`, then
    /// for an MCP tool (`mcp:<server>/<tool>`) `[policy.mcp]` "server/tool"
    /// and "server", then `[policy].enforcement`.
    pub fn posture(&self, name: &str) -> (Posture, String) {
        if let Some(p) = self.tools.get(name) {
            return (*p, format!("[policy.tools] \"{name}\" = {}", p.as_str()));
        }
        if let Some(full) = name.strip_prefix(MCP_PREFIX) {
            let server = full.split_once('/').map_or(full, |(s, _)| s);
            for key in [full, server] {
                if let Some(p) = self.mcp.get(key) {
                    return (*p, format!("[policy.mcp] \"{key}\" = {}", p.as_str()));
                }
            }
        }
        (
            self.enforcement,
            format!("enforcement = {}", self.enforcement.as_str()),
        )
    }

    /// A tool's posture now: the config's posture, then a tightening, and
    /// the stricter one wins (theseus-sgh).
    pub fn posture_now(&self, name: &str, tightened: Option<Tightened<'_>>) -> PostureNow {
        let (config, config_setting) = self.posture(name);
        match tightened {
            Some(t) if t.posture > config => PostureNow {
                posture: t.posture,
                setting: format!("tightened by {}", t.by),
                config,
                config_setting,
            },
            _ => PostureNow {
                posture: config,
                setting: config_setting.clone(),
                config,
                config_setting,
            },
        }
    }

    /// The gate's answer for one call from the config alone.
    pub fn decide(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        self.decide_with(tool, plan, None)
    }

    /// The gate's answer for one call, with the tool's tightening if it has
    /// one; the order is in the module doc.
    pub fn decide_with(
        &self,
        tool: &dyn Tool,
        plan: &Plan,
        tightened: Option<Tightened<'_>>,
    ) -> Decision {
        let name = tool.name();
        let resources: Vec<PathBuf> = plan
            .resources
            .iter()
            .map(|r| paths::canonical_best_effort(&r.path))
            .collect();
        let argv: &[String] = plan.argv.as_deref().unwrap_or(&[]);
        let nargv = normalized_argv(argv);
        // A command's resources are only its working directory; its arguments
        // can name any path, so they are judged too.
        let args = if argv.is_empty() {
            vec![]
        } else {
            let cwd = plan
                .resources
                .iter()
                .zip(&resources)
                .find(|(r, _)| r.access == Access::Exec)
                .map(|(_, p)| p.clone())
                .or_else(|| self.roots.first().cloned())
                .unwrap_or_default();
            path_args(argv, &cwd)
        };

        if let Some(what) = self.floor(&resources, &nargv, &args) {
            return Decision {
                floor: true,
                ..Decision::new(
                    Posture::Approve,
                    format!("{}: {name} — approve (floor: {what})", plan.summary),
                )
            };
        }
        if let Some(why) = self.listed(&resources, argv, &nargv, &args) {
            return Decision::new(
                Posture::Approve,
                format!("{}: {name} — approve ({why})", plan.summary),
            );
        }
        // The allow list runs a command outright when its path arguments stay
        // inside the roots; otherwise the tool's posture decides.
        if let Some(p) = self.allow_argv.iter().find(|p| prefix_match(&nargv, p)) {
            if args.iter().all(|(_, p)| self.within_roots(p)) {
                return Decision::new(
                    Posture::Open,
                    format!(
                        "{name} — open (`{}` matches the allow list entry `{}`)",
                        argv.join(" "),
                        p.join(" ")
                    ),
                );
            }
        }
        let now = self.posture_now(name, tightened);
        let reason = format!("{name} — {} ({})", now.posture.as_str(), now.why());
        match now.posture {
            Posture::Approve => Decision::new(now.posture, format!("{}: {reason}", plan.summary)),
            Posture::Notify => Decision {
                notify: Some(Notice {
                    kind: "notify".into(),
                    setting: now.setting,
                    rule: reason.clone(),
                }),
                ..Decision::new(now.posture, reason)
            },
            Posture::Open => Decision::new(now.posture, reason),
        }
    }

    fn within_roots(&self, p: &Path) -> bool {
        self.roots.iter().any(|root| paths::within(p, root))
    }

    /// What of the call is on the floor: a resource, the program, or a path
    /// argument.
    fn floor(
        &self,
        resources: &[PathBuf],
        nargv: &[String],
        args: &[(String, PathBuf)],
    ) -> Option<String> {
        let on = |p: &Path| self.floor_paths.iter().any(|f| paths::within(p, f));
        if let Some(p) = resources.iter().find(|p| on(p)) {
            return Some(format!("{} is {FLOOR_STATE}", p.display()));
        }
        if let Some(f) = self.floor_argv.iter().find(|f| prefix_match(nargv, f)) {
            return Some(format!(
                "`{}` is Theseus's own binary or the 1Password CLI",
                f.join(" ")
            ));
        }
        args.iter()
            .find(|(_, p)| on(p))
            .map(|(a, _)| format!("argument `{a}` is {FLOOR_STATE}"))
    }

    /// What of the call the operator listed for approval: a path on the
    /// approve list, a path outside the roots, an approve-listed program, or
    /// a path argument on the approve list.
    fn listed(
        &self,
        resources: &[PathBuf],
        argv: &[String],
        nargv: &[String],
        args: &[(String, PathBuf)],
    ) -> Option<String> {
        for p in resources {
            if let Some(d) = self.approve_paths.iter().find(|d| paths::within(p, d)) {
                return Some(format!(
                    "{} is protected: {} is on the approve list",
                    p.display(),
                    d.display()
                ));
            }
            if !self.within_roots(p) {
                let roots: Vec<String> =
                    self.roots.iter().map(|r| r.display().to_string()).collect();
                return Some(format!(
                    "{} is outside the workspace roots: {}",
                    p.display(),
                    if roots.is_empty() {
                        "none configured, set [tools].projects_dir".into()
                    } else {
                        roots.join(", ")
                    }
                ));
            }
        }
        if let Some(p) = self.approve_argv.iter().find(|p| prefix_match(nargv, p)) {
            return Some(format!(
                "`{}` matches the approve list entry `{}`",
                argv.join(" "),
                p.join(" ")
            ));
        }
        args.iter().find_map(|(a, p)| {
            self.approve_paths
                .iter()
                .find(|d| paths::within(p, d))
                .map(|d| {
                    format!(
                        "argument `{a}` is protected: {} is on the approve list",
                        d.display()
                    )
                })
        })
    }

    /// Does any resource of this plan write?
    pub fn writes(plan: &Plan) -> bool {
        plan.resources.iter().any(|r| r.access == Access::Write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use theseus_tools::{Resource, Retry, ToolClass, ToolCtx};

    /// A tool by name only: the gate looks at nothing else about it.
    struct T(&'static str);
    impl Tool for T {
        fn name(&self) -> &'static str {
            self.0
        }
        fn description(&self) -> &'static str {
            ""
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn class(&self) -> ToolClass {
            ToolClass::Run
        }
        fn retry(&self) -> Retry {
            Retry::SafeToRepeat
        }
        fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
            Ok(Plan::default())
        }
    }

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        (d, root)
    }

    fn policy(root: &Path, enforcement: Posture) -> ToolPolicy {
        ToolPolicy {
            roots: vec![root.to_path_buf()],
            approve_paths: vec![root.join("secret")],
            allow_argv: vec![vec!["git".into(), "status".into()]],
            approve_argv: vec![vec!["sudo".into()]],
            enforcement,
            tools: BTreeMap::new(),
            mcp: BTreeMap::new(),
            confirmer: "operator".into(),
            floor_paths: vec![root.join("state")],
            floor_argv: floor_argv(),
        }
    }

    fn plan(path: PathBuf, access: Access, argv: Option<Vec<&str>>) -> Plan {
        Plan {
            resources: vec![Resource { path, access }],
            argv: argv.map(|v| v.into_iter().map(String::from).collect()),
            summary: "the call".into(),
        }
    }

    #[test]
    fn each_posture_runs_notifies_or_waits_on_a_plain_call() {
        let (_d, root) = workspace();
        assert_eq!(
            Posture::ALL.map(Posture::as_str),
            ["open", "notify", "approve"],
            "three postures, and none refuses"
        );
        // No class axis: a read, a write, and a command all take the posture.
        for (tool, pl) in [
            (
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["cargo", "test"])),
            ),
            ("fs.write", plan(root.join("a.rs"), Access::Write, None)),
            ("fs.read", plan(root.join("a.rs"), Access::Read, None)),
        ] {
            let t = T(tool);
            let out = policy(&root, Posture::Open).decide(&t, &pl);
            assert_eq!(out.posture, Posture::Open);
            assert!(out.notify.is_none() && !out.floor);
            assert_eq!(out.reason, format!("{tool} — open (enforcement = open)"));

            let out = policy(&root, Posture::Notify).decide(&t, &pl);
            assert_eq!(out.posture, Posture::Notify);
            assert_eq!(
                out.notify,
                Some(Notice {
                    kind: "notify".into(),
                    setting: "enforcement = notify".into(),
                    rule: format!("{tool} — notify (enforcement = notify)"),
                })
            );

            let out = policy(&root, Posture::Approve).decide(&t, &pl);
            assert_eq!(out.posture, Posture::Approve);
            assert!(out.notify.is_none() && !out.floor);
            assert_eq!(
                out.reason,
                format!("the call: {tool} — approve (enforcement = approve)")
            );
        }
    }

    #[test]
    fn a_per_tool_override_beats_the_global_posture() {
        let (_d, root) = workspace();
        let mut p = policy(&root, Posture::Notify);
        p.tools.insert("proc.run".into(), Posture::Approve);
        p.tools.insert("fs.read".into(), Posture::Open);
        let run = plan(root.clone(), Access::Exec, Some(vec!["cargo", "test"]));
        let out = p.decide(&T("proc.run"), &run);
        assert_eq!(out.posture, Posture::Approve);
        assert_eq!(
            out.reason,
            "the call: proc.run — approve ([policy.tools] \"proc.run\" = approve)"
        );
        let read = plan(root.join("a.rs"), Access::Read, None);
        let out = p.decide(&T("fs.read"), &read);
        assert_eq!((out.posture, out.notify), (Posture::Open, None));
        assert_eq!(
            p.decide(&T("fs.grep"), &read).posture,
            Posture::Notify,
            "a tool without an override inherits"
        );
        // An override lowers as well as raises.
        p.enforcement = Posture::Approve;
        assert_eq!(p.decide(&T("fs.read"), &read).posture, Posture::Open);
        let write = plan(root.join("a.rs"), Access::Write, None);
        assert_eq!(p.decide(&T("fs.write"), &write).posture, Posture::Approve);
    }

    /// "Should have asked" (theseus-sgh): a tightening applies after the
    /// config's posture, and the stricter one wins. Under a config that
    /// already asks, the decision and its reason are the config's. The floor
    /// and the approve lists ask with their own reasons, and the allow list
    /// still runs its commands, as it does under a config `approve`.
    #[test]
    fn a_tightening_is_the_stricter_posture_and_never_loosens() {
        let (_d, root) = workspace();
        let t = Tightened {
            posture: Posture::Approve,
            by: "discord:eddie",
        };
        let cmd = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
        let run = cmd(vec!["cargo", "test"]);
        for e in Posture::ALL {
            let p = policy(&root, e);
            let out = p.decide_with(&T("proc.run"), &run, Some(t));
            assert_eq!(out.posture, Posture::Approve, "under {}", e.as_str());
            assert!(out.notify.is_none() && !out.floor);
            let want = if e == Posture::Approve {
                "the call: proc.run — approve (enforcement = approve)".to_string()
            } else {
                format!(
                    "the call: proc.run — approve (tightened by discord:eddie; the config says \
                     enforcement = {})",
                    e.as_str()
                )
            };
            assert_eq!(out.reason, want);
            assert_eq!(p.decide(&T("proc.run"), &run).posture, e, "untightened");
            let write = plan(root.join("a.rs"), Access::Write, None);
            assert_eq!(
                p.decide_with(&T("fs.write"), &write, None).posture,
                e,
                "another tool keeps its posture"
            );
            let allowed = p.decide_with(&T("proc.run"), &cmd(vec!["git", "status"]), Some(t));
            assert_eq!(allowed.posture, Posture::Open, "{}", allowed.reason);
            let floor = p.decide_with(&T("proc.run"), &cmd(vec!["op", "read", "x"]), Some(t));
            assert!(floor.floor, "{}", floor.reason);
            let listed = p.decide_with(&T("proc.run"), &cmd(vec!["sudo", "ls"]), Some(t));
            assert!(
                listed
                    .reason
                    .ends_with("(`sudo ls` matches the approve list entry `sudo`)"),
                "{}",
                listed.reason
            );
        }
        // A per-tool override is the config's posture too.
        let mut p = policy(&root, Posture::Notify);
        p.tools.insert("proc.run".into(), Posture::Open);
        let now = p.posture_now("proc.run", Some(t));
        assert_eq!(
            (now.posture, now.config, now.tightened()),
            (Posture::Approve, Posture::Open, true)
        );
        assert_eq!(now.setting, "tightened by discord:eddie");
        assert_eq!(now.config_setting, "[policy.tools] \"proc.run\" = open");
        // Stricter wins whatever the tightening says: one to notify raises an
        // open tool to a notice, and leaves a tool that asks asking.
        let n = Tightened {
            posture: Posture::Notify,
            by: "sock#1",
        };
        let out = policy(&root, Posture::Open).decide_with(&T("proc.run"), &run, Some(n));
        assert_eq!(out.posture, Posture::Notify);
        assert_eq!(
            out.notify.map(|x| x.setting).as_deref(),
            Some("tightened by sock#1")
        );
        let out = policy(&root, Posture::Approve).decide_with(&T("proc.run"), &run, Some(n));
        assert_eq!(
            out.reason,
            "the call: proc.run — approve (enforcement = approve)"
        );
        assert!(Posture::Open < Posture::Notify && Posture::Notify < Posture::Approve);
        assert_eq!(Posture::parse("notify"), Some(Posture::Notify));
        assert_eq!(Posture::parse("deny"), None);
    }

    #[test]
    fn an_mcp_tool_takes_server_tool_then_server_then_the_global_posture() {
        let (_d, root) = workspace();
        let call = plan(root.clone(), Access::Read, None);
        let at = |p: &ToolPolicy, name: &'static str| {
            let d = p.decide(&T(name), &call);
            (d.posture, d.reason)
        };
        let mut p = policy(&root, Posture::Notify);
        assert_eq!(
            at(&p, "mcp:x/y"),
            (
                Posture::Notify,
                "mcp:x/y — notify (enforcement = notify)".into()
            )
        );
        p.mcp.insert("x".into(), Posture::Approve);
        assert_eq!(
            at(&p, "mcp:x/y"),
            (
                Posture::Approve,
                "the call: mcp:x/y — approve ([policy.mcp] \"x\" = approve)".into()
            )
        );
        p.mcp.insert("x/y".into(), Posture::Open);
        assert_eq!(
            at(&p, "mcp:x/y"),
            (
                Posture::Open,
                "mcp:x/y — open ([policy.mcp] \"x/y\" = open)".into()
            )
        );
        assert_eq!(
            at(&p, "mcp:x/z").0,
            Posture::Approve,
            "the server's other tools take the server's"
        );
        assert_eq!(
            at(&p, "mcp:w/y").0,
            Posture::Notify,
            "another server inherits"
        );
        assert_eq!(
            at(&p, "x/y").0,
            Posture::Notify,
            "only an mcp: name consults [policy.mcp]"
        );
        p.tools.insert("mcp:x/y".into(), Posture::Approve);
        assert_eq!(
            at(&p, "mcp:x/y").0,
            Posture::Approve,
            "[policy.tools] first"
        );
    }

    #[test]
    fn the_floor_asks_at_every_posture_and_is_never_refused_or_silent() {
        let (_d, root) = workspace();
        let store = root.join("state/store/000.wal");
        let store_arg = store.to_string_lossy().into_owned();
        // The token file lives outside the workspace, like the real one in $HOME.
        let token = tempfile::NamedTempFile::new().unwrap();
        let token = token.path().canonicalize().unwrap();
        for e in Posture::ALL {
            let mut p = policy(&root, e);
            p.floor_paths.push(token.clone());
            for (what, tool, pl) in [
                (
                    "the store",
                    "fs.read",
                    plan(store.clone(), Access::Read, None),
                ),
                (
                    "the token file",
                    "fs.read",
                    plan(token.clone(), Access::Read, None),
                ),
                (
                    "theseusd",
                    "proc.run",
                    plan(root.clone(), Access::Exec, Some(vec!["theseusd", "config"])),
                ),
                (
                    "op by path",
                    "proc.run",
                    plan(
                        root.clone(),
                        Access::Exec,
                        Some(vec!["/usr/bin/op", "read", "x"]),
                    ),
                ),
                (
                    "a path argument in the store",
                    "proc.run",
                    plan(root.clone(), Access::Exec, Some(vec!["cat", &store_arg])),
                ),
            ] {
                let out = p.decide(&T(tool), &pl);
                assert_eq!(
                    out.posture,
                    Posture::Approve,
                    "the floor asks at {}: {what}",
                    e.as_str()
                );
                assert!(out.floor && out.notify.is_none(), "{what}");
                assert!(
                    out.reason
                        .starts_with(&format!("the call: {tool} — approve (floor: ")),
                    "{}",
                    out.reason
                );
            }
        }
        // The floor comes before the operator's approve lists and overrides:
        // an overlap is still marked as the floor.
        let mut p = policy(&root, Posture::Open);
        p.approve_argv.push(vec!["op".into()]);
        p.approve_paths.push(root.join("state"));
        p.tools.insert("proc.run".into(), Posture::Open);
        let out = p.decide(
            &T("proc.run"),
            &plan(root.clone(), Access::Exec, Some(vec!["op", "read", "x"])),
        );
        assert!(out.floor, "{}", out.reason);
        assert_eq!(
            out.reason,
            "the call: proc.run — approve (floor: `op` is Theseus's own binary or the 1Password CLI)"
        );
        let out = p.decide(
            &T("proc.run"),
            &plan(root, Access::Exec, Some(vec!["cat", &store_arg])),
        );
        assert!(out.floor, "{}", out.reason);
        assert_eq!(
            out.reason,
            format!("the call: proc.run — approve (floor: argument `{store_arg}` is Theseus's own state or the 1Password token)")
        );
    }

    #[test]
    fn the_approve_lists_wait_and_the_allow_list_runs_at_every_posture() {
        let (_d, root) = workspace();
        let secret = format!("{}/secret/key", root.display());
        for e in Posture::ALL {
            let mut p = policy(&root, e);
            // An override does not lower an approve list: the list comes first.
            p.tools.insert("fs.read".into(), Posture::Open);
            let run = |argv: Vec<&str>| {
                p.decide(
                    &T("proc.run"),
                    &plan(root.clone(), Access::Exec, Some(argv)),
                )
            };
            let read = |path: PathBuf| p.decide(&T("fs.read"), &plan(path, Access::Read, None));
            for (out, why) in [
                (
                    read("/etc/passwd".into()),
                    "the call: fs.read — approve (/etc/passwd is outside the workspace roots: ",
                ),
                (
                    read(root.join("secret/key")),
                    "the call: fs.read — approve (",
                ),
                (
                    run(vec!["/usr/bin/sudo", "ls"]),
                    "the call: proc.run — approve (`/usr/bin/sudo ls` matches the approve list entry `sudo`)",
                ),
                (
                    run(vec!["cat", &secret]),
                    "the call: proc.run — approve (argument `",
                ),
            ] {
                assert_eq!(
                    out.posture,
                    Posture::Approve,
                    "waits at {}: {}",
                    e.as_str(),
                    out.reason
                );
                assert!(!out.floor && out.notify.is_none(), "{}", out.reason);
                assert!(out.reason.starts_with(why), "{}", out.reason);
            }
            let r = root.display();
            assert_eq!(
                read(root.join("secret/key")).reason,
                format!("the call: fs.read — approve ({r}/secret/key is protected: {r}/secret is on the approve list)")
            );
            assert_eq!(
                run(vec!["cat", &secret]).reason,
                format!("the call: proc.run — approve (argument `{secret}` is protected: {r}/secret is on the approve list)")
            );
            // The allow list runs a command outright, even under approve.
            for argv in [
                vec!["git", "status", "-s"],
                vec!["git", "status", "src/lib.rs"],
            ] {
                let out = run(argv);
                assert_eq!(out.posture, Posture::Open);
                assert!(out.notify.is_none());
                assert!(
                    out.reason
                        .contains("matches the allow list entry `git status`"),
                    "{}",
                    out.reason
                );
            }
            // A path argument outside the roots, `--opt=value` included, leaves
            // the command to its tool's posture.
            for argv in [
                vec!["git", "status", "--output=/tmp/x"],
                vec!["git", "status", "/etc"],
                vec!["git", "status", ".."],
            ] {
                let out = run(argv);
                assert_eq!(out.posture, e, "{}", out.reason);
                assert!(
                    out.reason
                        .ends_with(&format!("proc.run — {0} (enforcement = {0})", e.as_str())),
                    "{}",
                    out.reason
                );
            }
        }
    }

    /// No reason the gate writes speaks of refusing: every answer runs,
    /// notifies, or waits (theseus-8az).
    #[test]
    fn no_reason_says_deny_or_refuse() {
        let (_d, root) = workspace();
        let secret = format!("{}/secret/key", root.display());
        let store_arg = root.join("state/x").to_string_lossy().into_owned();
        for e in Posture::ALL {
            let p = policy(&root, e);
            for (tool, pl) in [
                ("fs.read", plan("/etc/passwd".into(), Access::Read, None)),
                ("fs.read", plan(root.join("secret/k"), Access::Read, None)),
                ("fs.write", plan(root.join("a.rs"), Access::Write, None)),
                (
                    "proc.run",
                    plan(root.clone(), Access::Exec, Some(vec!["sudo", "ls"])),
                ),
                (
                    "proc.run",
                    plan(root.clone(), Access::Exec, Some(vec!["cat", &secret])),
                ),
                (
                    "proc.run",
                    plan(root.clone(), Access::Exec, Some(vec!["cat", &store_arg])),
                ),
                (
                    "proc.run",
                    plan(
                        root.clone(),
                        Access::Exec,
                        Some(vec!["git", "status", "/etc"]),
                    ),
                ),
                (
                    "proc.run",
                    plan(root.clone(), Access::Exec, Some(vec!["op", "read", "x"])),
                ),
            ] {
                let r = p.decide(&T(tool), &pl).reason.to_lowercase();
                for word in ["deny", "denied", "refuse"] {
                    assert!(!r.contains(word), "{word} in {r}");
                }
            }
        }
    }

    /// FAST: the gate is a few map lookups and path comparisons per call.
    #[test]
    fn deciding_stays_well_under_a_millisecond() {
        let (_d, root) = workspace();
        let mut p = policy(&root, Posture::Notify);
        p.tools.insert("fs.read".into(), Posture::Open);
        p.mcp.insert("x".into(), Posture::Approve);
        let calls = [
            (
                T("proc.run"),
                plan(
                    root.clone(),
                    Access::Exec,
                    Some(vec!["cargo", "test", "--manifest-path", "./Cargo.toml"]),
                ),
            ),
            (
                T("fs.read"),
                plan(root.join("src/lib.rs"), Access::Read, None),
            ),
            (T("mcp:x/y"), plan(root.clone(), Access::Read, None)),
        ];
        let n = 2_000u32;
        let t0 = std::time::Instant::now();
        for _ in 0..n {
            for (t, pl) in &calls {
                std::hint::black_box(p.decide(t, pl));
            }
        }
        let per = t0.elapsed() / (n * calls.len() as u32);
        eprintln!(
            "FAST decide: {per:?} per call over {} calls (debug build)",
            n as usize * calls.len()
        );
        assert!(per < std::time::Duration::from_millis(1), "{per:?}");
    }
}
