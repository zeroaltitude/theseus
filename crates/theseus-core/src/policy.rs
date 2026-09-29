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
//! 3. the operator's explicit allow (`allow_argv`) runs;
//! 4. otherwise the tool's posture: `[policy.tools]`, then for an MCP tool
//!    `[policy.mcp]` "server/tool" and "server", then `[policy].enforcement`.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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
}

impl Decision {
    fn new(posture: Posture, reason: String) -> Self {
        Self {
            posture,
            reason,
            notify: None,
            floor: false,
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

/// Flags the allow list never covers, whatever it says: they make an
/// innocent-looking command write a file (`--output`), read any file
/// (`--no-index`), or run a program configured elsewhere (`--ext-diff`,
/// `--textconv`, `--exec`, `--upload-pack`, `-c`). A command carrying one
/// takes its tool's posture instead.
const NOT_ALLOW_LISTED: &[&str] = &[
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

    /// The gate's answer for one call; the order is in the module doc.
    pub fn decide(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
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
        // The allow list runs a command outright; a near miss says why not.
        let mut near_miss = String::new();
        if let Some(p) = self.allow_argv.iter().find(|p| prefix_match(&nargv, p)) {
            let entry = format!(
                "`{}` matches the allow list entry `{}`",
                argv.join(" "),
                p.join(" ")
            );
            let flag = argv.iter().skip(1).find(|a| {
                NOT_ALLOW_LISTED
                    .iter()
                    .any(|f| a.as_str() == *f || a.starts_with(&format!("{f}=")))
            });
            let outside = args.iter().find(|(_, p)| !self.within_roots(p));
            match (flag, outside) {
                (Some(f), _) => {
                    near_miss = format!("; {entry}, but the allow list never covers `{f}`")
                }
                (None, Some((a, _))) => {
                    near_miss =
                        format!("; {entry}, but its argument `{a}` is outside the workspace roots")
                }
                (None, None) => {
                    return Decision::new(Posture::Open, format!("{name} — open ({entry})"))
                }
            }
        }
        let (posture, setting) = self.posture(name);
        let reason = format!("{name} — {} ({setting}{near_miss})", posture.as_str());
        match posture {
            Posture::Approve => Decision::new(posture, format!("{}: {reason}", plan.summary)),
            Posture::Notify => Decision {
                notify: Some(Notice {
                    kind: "notify".into(),
                    setting,
                    rule: reason.clone(),
                }),
                ..Decision::new(posture, reason)
            },
            Posture::Open => Decision::new(posture, reason),
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
            // A near miss takes the posture and says why the list did not apply.
            for (argv, why) in [
                (
                    vec!["git", "status", "--output=/tmp/x"],
                    "the allow list never covers `--output=/tmp/x`",
                ),
                (
                    vec!["git", "status", "/etc"],
                    "its argument `/etc` is outside the workspace roots",
                ),
                (
                    vec!["git", "status", ".."],
                    "its argument `..` is outside the workspace roots",
                ),
            ] {
                let out = run(argv);
                assert_eq!(out.posture, e, "{}", out.reason);
                assert!(out.reason.contains(why), "{}", out.reason);
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
