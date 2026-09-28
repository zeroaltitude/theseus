//! Tool policy (spec §3.9): the three-band gate — allow, confirm, deny — over
//! what a call will touch, not over strings. Evaluated after the toollet has
//! validated its input and named its resources (`Plan`), before anything runs.
//! Most restrictive wins. Deny reasons are written for the model and the
//! operator to read ("path /etc/passwd is outside the workspace roots").

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use theseus_kernel::{Authority, Policy, PolicyDecision, Proposal};
use theseus_protocol::ConsequenceTag;
use theseus_tools::consequence::{self, Consequence, OwnerRule};
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

/// `[policy].enforcement`: the operator's posture toward the gate's stops, as
/// one setting so they can never combine incoherently: a call against the
/// policy never meets less friction than a call that only needs approval, and
/// an irreversible call never meets less friction than either (spec §3.9).
///
/// | enforcement | needs approval     | irreversible      | against policy            |
/// |-------------|--------------------|-------------------|---------------------------|
/// | `strict`    | waits for Approve  | waits for Approve | refused                   |
/// | `ask`       | waits for Approve  | waits for Approve | waits for Approve, marked |
/// | `notify`    | runs, amber notice | waits for Approve | refused                   |
/// | `open`      | runs, amber notice | waits for Approve | runs, red notice          |
///
/// A call both irreversible and against the policy takes the stricter
/// treatment: refused under `strict` and `notify`, a marked wait under `ask`
/// and `open`. The floor is refused at every level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    #[default]
    Strict,
    Ask,
    Notify,
    Open,
}

impl Enforcement {
    pub fn as_str(self) -> &'static str {
        match self {
            Enforcement::Strict => "strict",
            Enforcement::Ask => "ask",
            Enforcement::Notify => "notify",
            Enforcement::Open => "open",
        }
    }
}

/// Why a call ran that the policy alone would have stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// `approval_skipped` or `off_policy`.
    pub kind: String,
    /// The setting that let it run, e.g. `enforcement = notify`.
    pub setting: String,
    /// What the policy said (the confirm or deny reason).
    pub rule: String,
    /// The consequences it named (never an irreversible one: those wait).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consequences: Vec<ConsequenceTag>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub mode: Mode,
    pub reason: String,
    /// Set when the enforcement level let the call run instead of asking or refusing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<Notice>,
    /// A floor denial: no setting lifts it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    /// A confirm that stands in for a refusal (`enforcement = ask`, or an
    /// irreversible call against the policy under `open`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub against_policy: bool,
    /// What the call would do to the world, graded (spec §3.9).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub consequences: Vec<ConsequenceTag>,
    /// A consequence is irreversible: the call waits at every level.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub irreversible: bool,
    /// The rule table that read the argv (`proc.run`), for replay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<&'static str>,
}

impl Decision {
    fn new(mode: Mode, reason: String) -> Self {
        Self {
            mode,
            reason,
            notify: None,
            floor: false,
            against_policy: false,
            consequences: vec![],
            irreversible: false,
            rules: None,
        }
    }
}

/// The consequence kinds the gate grades: the built-in seeds
/// (`theseus_tools::consequence::SEED_KINDS`) with the owner's
/// `[consequences.kinds]` merged over them.
#[derive(Debug, Clone)]
pub struct Kinds {
    /// kind → irreversible.
    pub grades: BTreeMap<String, bool>,
    pub descriptions: BTreeMap<String, String>,
    /// The owner's argv prefixes (`[consequences.kinds.<k>].argv`).
    pub owner_rules: Vec<OwnerRule>,
}

impl Default for Kinds {
    fn default() -> Self {
        Self::built_in()
    }
}

impl Kinds {
    pub fn built_in() -> Self {
        Self {
            grades: consequence::SEED_KINDS
                .iter()
                .map(|k| (k.name.to_string(), k.irreversible))
                .collect(),
            descriptions: consequence::SEED_KINDS
                .iter()
                .map(|k| (k.name.to_string(), k.description.to_string()))
                .collect(),
            owner_rules: vec![],
        }
    }

    pub fn from_config(cfg: &crate::config::ConsequencesConfig) -> Self {
        let mut k = Self::built_in();
        for (name, c) in &cfg.kinds {
            let grade = c
                .irreversible
                .unwrap_or_else(|| k.grades.get(name).copied().unwrap_or(false));
            k.grades.insert(name.clone(), grade);
            match &c.description {
                Some(d) => {
                    k.descriptions.insert(name.clone(), d.clone());
                }
                None => {
                    k.descriptions.entry(name.clone()).or_default();
                }
            }
            for p in &c.argv {
                k.owner_rules.push(OwnerRule {
                    kind: name.clone(),
                    prefix: p.clone(),
                });
            }
        }
        k
    }

    /// A kind nobody graded needs approval; only a graded kind is irreversible.
    pub fn irreversible(&self, kind: &str) -> bool {
        self.grades.get(kind).copied().unwrap_or(false)
    }

    pub fn tag(&self, c: &Consequence) -> ConsequenceTag {
        ConsequenceTag {
            kind: c.kind.clone(),
            irreversible: self.irreversible(&c.kind),
            rule: c.rule.clone(),
            detail: c.detail.clone(),
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
    pub enforcement: Enforcement,
    /// Denied whatever the settings say: Theseus's store, spool, and bindings
    /// file, and the 1Password CLI's credentials (canonical paths).
    pub floor_paths: Vec<PathBuf>,
    /// Programs denied whatever the settings say (`theseusd`, `op`).
    pub floor_argv: Vec<Vec<String>>,
    /// Consequence kinds and their grades (`[consequences]`).
    pub kinds: Kinds,
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

    /// The gate's answer at the operator's enforcement level (the table on
    /// `Enforcement`): the policy's band, then the consequences' column. The
    /// floor is decided first and never lifted.
    pub fn decide(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        use Enforcement::*;
        let d = self.decide_by_policy(tool, plan);
        if d.floor {
            return d;
        }
        let (tags, rules) = self.consequences(plan);
        let irreversible = tags.iter().any(|t| t.irreversible);
        let named = ConsequenceTag::summary(&tags);
        let setting = format!("enforcement = {}", self.enforcement.as_str());
        let what = if d.mode == Mode::Confirm {
            d.reason.clone()
        } else {
            format!("{}: {}", plan.summary, d.reason)
        };
        let notice = |kind: &str, rule: String| Notice {
            kind: kind.into(),
            setting: setting.clone(),
            rule,
            consequences: tags.clone(),
        };
        let mut out = match (d.mode, irreversible, self.enforcement) {
            // Against the policy and irreversible: the stricter treatment.
            (Mode::Deny, true, Strict | Notify) => {
                Decision::new(Mode::Deny, format!("{} (and it is {named})", d.reason))
            }
            (Mode::Deny, true, Ask | Open) => Decision {
                against_policy: true,
                ..Decision::new(
                    Mode::Confirm,
                    format!("against policy and {named}: {}", d.reason),
                )
            },
            (Mode::Deny, false, Ask) => Decision {
                against_policy: true,
                ..Decision::new(Mode::Confirm, format!("against policy: {}", d.reason))
            },
            (Mode::Deny, false, Open) => Decision {
                notify: Some(notice("off_policy", d.reason.clone())),
                against_policy: true,
                ..Decision::new(Mode::Allow, format!("ran against policy: {}", d.reason))
            },
            (Mode::Deny, false, Strict | Notify) => d,
            // Irreversible: waits for approval at every level.
            (Mode::Allow | Mode::Confirm, true, _) => Decision::new(
                Mode::Confirm,
                format!("{named}: an irreversible call waits for approval at every enforcement level ({setting}). {what}"),
            ),
            // Needs approval, by the tool's class or by a consequence.
            (Mode::Confirm, false, Notify | Open) => Decision {
                notify: Some(notice("approval_skipped", d.reason.clone())),
                ..Decision::new(Mode::Allow, format!("ran without approval: {}", d.reason))
            },
            (Mode::Allow, false, Notify | Open) if !tags.is_empty() => Decision {
                notify: Some(notice("approval_skipped", format!("{named}: {what}"))),
                ..Decision::new(Mode::Allow, format!("ran without approval: {named}: {what}"))
            },
            (Mode::Allow, false, Strict | Ask) if !tags.is_empty() => {
                Decision::new(Mode::Confirm, format!("{named}: {what}"))
            }
            (Mode::Allow | Mode::Confirm, false, _) => d,
        };
        out.consequences = tags;
        out.irreversible = irreversible;
        out.rules = rules;
        out
    }

    /// Every consequence of the plan, graded: the toollet's own (layer 1)
    /// and, for an argv, the rule table's and the owner's (layer 2, which also
    /// names what it cannot see through as `opaque`).
    pub fn consequences(&self, plan: &Plan) -> (Vec<ConsequenceTag>, Option<&'static str>) {
        let mut found: Vec<Consequence> = plan.consequences.clone();
        let mut rules = None;
        if let Some(argv) = &plan.argv {
            for c in consequence::detect(argv, &self.exec_cwd(plan), &self.kinds.owner_rules) {
                if !found.contains(&c) {
                    found.push(c);
                }
            }
            rules = Some(consequence::RULES_VERSION);
        }
        (found.iter().map(|c| self.kinds.tag(c)).collect(), rules)
    }

    /// Where a program runs: its `Exec` resource, else the first root.
    fn exec_cwd(&self, plan: &Plan) -> PathBuf {
        plan.resources
            .iter()
            .find(|r| r.access == Access::Exec)
            .map(|r| paths::canonical_best_effort(&r.path))
            .or_else(|| self.roots.first().cloned())
            .unwrap_or_default()
    }

    /// The policy alone: floor, protected paths, roots, class, argv lists.
    fn decide_by_policy(&self, tool: &dyn Tool, plan: &Plan) -> Decision {
        let floor = |reason: String| Decision {
            floor: true,
            ..Decision::new(Mode::Deny, reason)
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
            let args = path_args(argv, &self.exec_cwd(plan));
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

/// How a decision treated a call, in the words `policy.replay` prints.
pub fn treatment(mode: &str, notice: Option<&str>) -> &'static str {
    match (mode, notice) {
        ("allow", Some("off_policy")) => "ran against policy",
        ("allow", Some(_)) => "ran without approval",
        ("allow", None) => "ran",
        ("confirm", _) => "waited",
        _ => "refused",
    }
}

impl ToolPolicy {
    /// `policy.rules`: the kinds as graded and every detection rule.
    pub fn rules_info(&self) -> theseus_protocol::PolicyRulesResult {
        use theseus_protocol::{KindInfo, RuleInfo};
        let seed = |n: &str| consequence::SEED_KINDS.iter().find(|k| k.name == n);
        let kinds = self
            .kinds
            .grades
            .iter()
            .map(|(name, irreversible)| KindInfo {
                name: name.clone(),
                irreversible: *irreversible,
                description: self
                    .kinds
                    .descriptions
                    .get(name)
                    .cloned()
                    .unwrap_or_default(),
                source: match seed(name) {
                    Some(k)
                        if k.irreversible == *irreversible
                            && self.kinds.descriptions.get(name).map(String::as_str)
                                == Some(k.description) =>
                    {
                        "built_in"
                    }
                    Some(_) => "regraded",
                    None => "config",
                }
                .into(),
            })
            .collect();
        let mut rules: Vec<RuleInfo> = consequence::RULES
            .iter()
            .map(|r| RuleInfo {
                id: r.id.into(),
                kind: r.kind.into(),
                why: r.why.into(),
                must: r.must.iter().map(|x| x.to_string()).collect(),
                must_not: r.must_not.iter().map(|x| x.to_string()).collect(),
                source: "built_in".into(),
            })
            .collect();
        rules.extend(self.kinds.owner_rules.iter().map(|o| RuleInfo {
            id: format!("config:{}", o.kind),
            kind: o.kind.clone(),
            why: format!("argv starts with `{}`", o.prefix.join(" ")),
            must: vec![],
            must_not: vec![],
            source: "config".into(),
        }));
        theseus_protocol::PolicyRulesResult {
            rules_version: consequence::RULES_VERSION.into(),
            enforcement: self.enforcement.as_str().into(),
            kinds,
            rules,
        }
    }

    /// Judge one past call again: what the stored gate record says happened,
    /// and what the current gate (rules, kinds, and settings) would decide for
    /// the same plan. Read-only; `None` for a call that never passed
    /// validation or whose tool is gone.
    pub fn replay_call(
        &self,
        tool: &dyn Tool,
        ctx: &ToolCtx,
        input: &serde_json::Value,
        gate: &serde_json::Value,
    ) -> Option<(
        theseus_protocol::ReplayVerdict,
        theseus_protocol::ReplayVerdict,
        String,
    )> {
        use theseus_protocol::ReplayVerdict;
        if gate["validated"] != serde_json::Value::Bool(true) {
            return None;
        }
        let d = &gate["decision"];
        let then = ReplayVerdict {
            treatment: treatment(
                d["mode"].as_str().unwrap_or("deny"),
                d["notify"]["kind"].as_str(),
            )
            .into(),
            consequences: serde_json::from_value(d["consequences"].clone()).unwrap_or_default(),
            rules: d["rules"].as_str().map(String::from),
        };
        // The plan as judged then (its resources and argv), with the
        // toollet's current declarations.
        let replanned = tool.plan(input, ctx).ok();
        let mut plan: Plan = match serde_json::from_value(gate["plan"].clone()) {
            Ok(p) => p,
            Err(_) => replanned.clone()?,
        };
        if let Some(p) = replanned {
            plan.consequences = p.consequences;
        }
        let now_d = self.decide(tool, &plan);
        let now = ReplayVerdict {
            treatment: treatment(
                now_d.mode.as_str(),
                now_d.notify.as_ref().map(|n| n.kind.as_str()),
            )
            .into(),
            consequences: now_d.consequences,
            rules: now_d.rules.map(String::from),
        };
        Some((then, now, plan.summary))
    }
}

/// Did the verdict change: another treatment, or other kinds or grades?
pub fn verdict_changed(
    then: &theseus_protocol::ReplayVerdict,
    now: &theseus_protocol::ReplayVerdict,
) -> bool {
    let kinds = |v: &theseus_protocol::ReplayVerdict| {
        let mut k: Vec<(String, bool)> = v
            .consequences
            .iter()
            .map(|t| (t.kind.clone(), t.irreversible))
            .collect();
        k.sort();
        k.dedup();
        k
    };
    then.treatment != now.treatment || kinds(then) != kinds(now)
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
            enforcement: Enforcement::Strict,
            floor_paths: vec![root.join("state")],
            floor_argv: floor_argv(),
            kinds: Kinds::built_in(),
        }
    }

    fn plan(path: std::path::PathBuf, access: Access, argv: Option<Vec<&str>>) -> Plan {
        Plan {
            resources: vec![Resource { path, access }],
            argv: argv.map(|v| v.into_iter().map(String::from).collect()),
            summary: "s".into(),
            consequences: vec![],
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
        // The enforcement ladder: every level is coherent, and the floor holds.
        let at = |e: Enforcement| {
            let mut q = p.clone();
            q.enforcement = e;
            q
        };
        let write = plan(root.join("a.rs"), Access::Write, None);
        let outside = plan("/etc/passwd".into(), Access::Read, None);
        let sudo = plan(root.clone(), Access::Exec, Some(vec!["sudo", "ls"]));
        let s = at(Enforcement::Strict);
        assert_eq!(s.decide(&w, &write).mode, Mode::Confirm);
        assert_eq!(s.decide(&r, &outside).mode, Mode::Deny);
        let a = at(Enforcement::Ask);
        assert_eq!(a.decide(&w, &write).mode, Mode::Confirm);
        let out = a.decide(&r, &outside);
        assert_eq!(
            out.mode,
            Mode::Confirm,
            "ask: a refusal becomes a marked confirm"
        );
        assert!(
            out.against_policy && out.reason.starts_with("against policy"),
            "{}",
            out.reason
        );
        let n = at(Enforcement::Notify);
        let out = n.decide(&w, &write);
        assert_eq!(out.mode, Mode::Allow);
        assert_eq!(out.notify.as_ref().unwrap().kind, "approval_skipped");
        assert_eq!(out.notify.unwrap().setting, "enforcement = notify");
        assert_eq!(
            n.decide(&r, &outside).mode,
            Mode::Deny,
            "notify never lifts a refusal"
        );
        let o = at(Enforcement::Open);
        let out = o.decide(&r, &outside);
        assert_eq!(out.mode, Mode::Allow);
        let nt = out.notify.unwrap();
        assert_eq!(nt.kind, "off_policy");
        assert!(
            nt.rule.contains("outside the workspace roots"),
            "{}",
            nt.rule
        );
        assert_eq!(
            o.decide(&x, &sudo).mode,
            Mode::Allow,
            "open runs the deny list too"
        );
        for e in [
            Enforcement::Strict,
            Enforcement::Ask,
            Enforcement::Notify,
            Enforcement::Open,
        ] {
            let q = at(e);
            for (what, pl) in [
                ("state", plan(root.join("state/store"), Access::Read, None)),
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
                let out = q.decide(&x, &pl);
                assert_eq!(
                    out.mode,
                    Mode::Deny,
                    "the floor holds at {}: {what}",
                    e.as_str()
                );
                assert!(out.floor && out.notify.is_none() && !out.against_policy);
            }
        }
        let mut p2 = p.clone();
        p2.overrides.insert("fs.test".into(), Mode::Allow);
        assert_eq!(
            p2.decide(&w, &plan(root.join("a.rs"), Access::Write, None))
                .mode,
            Mode::Allow
        );
    }

    /// A native toollet that declares a consequence from its arguments (layer 1).
    struct Publishes;
    impl Tool for Publishes {
        fn name(&self) -> &'static str {
            "pkg.publish"
        }
        fn description(&self) -> &'static str {
            ""
        }
        fn input_schema(&self) -> Value {
            json!({})
        }
        fn class(&self) -> ToolClass {
            ToolClass::Read
        }
        fn retry(&self) -> Retry {
            Retry::NonRepeatable
        }
        fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
            Ok(Plan::default())
        }
    }

    #[test]
    fn irreversible_waits_at_every_level_and_the_stricter_treatment_wins() {
        use Enforcement::*;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        let x = T(ToolClass::Run);
        let at = |e: Enforcement| {
            let mut q = policy(&root);
            q.enforcement = e;
            q
        };
        let run = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
        let force = run(vec!["git", "push", "--force", "origin", "main"]);
        let shell = run(vec!["bash", "-c", "git push -f origin main"]);
        for e in [Strict, Ask, Notify, Open] {
            for pl in [&force, &shell] {
                let out = at(e).decide(&x, pl);
                assert_eq!(
                    out.mode,
                    Mode::Confirm,
                    "irreversible waits at {}",
                    e.as_str()
                );
                assert!(out.irreversible && !out.against_policy && out.notify.is_none());
                assert_eq!(out.consequences[0].kind, "history_rewrite");
                assert_eq!(out.consequences[0].rule, "git.push.force");
                assert!(
                    out.reason.starts_with("irreversible: history_rewrite"),
                    "{}",
                    out.reason
                );
                assert_eq!(out.rules, Some(consequence::RULES_VERSION));
            }
        }
        // A plain push is no consequence: at notify it runs with the usual notice.
        let out = at(Notify).decide(&x, &run(vec!["git", "push", "origin", "main"]));
        assert_eq!(out.mode, Mode::Allow);
        assert!(out.consequences.is_empty() && !out.irreversible);
        assert_eq!(out.notify.unwrap().kind, "approval_skipped");
        // Irreversible and against the policy (`sudo` is on the deny list):
        // refused under strict and notify, a marked wait under ask and open.
        let both = run(vec!["sudo", "git", "push", "-f"]);
        for (e, mode) in [
            (Strict, Mode::Deny),
            (Ask, Mode::Confirm),
            (Notify, Mode::Deny),
            (Open, Mode::Confirm),
        ] {
            let out = at(e).decide(&x, &both);
            assert_eq!(out.mode, mode, "both at {}: {}", e.as_str(), out.reason);
            assert!(
                out.irreversible && out.notify.is_none(),
                "never runs unasked at {}",
                e.as_str()
            );
            assert_eq!(out.against_policy, mode == Mode::Confirm);
            assert!(
                out.reason.contains("irreversible: history_rewrite"),
                "{}",
                out.reason
            );
        }
        // The floor is decided first, whatever the call would also do.
        let mut floor = run(vec!["theseusd", "config"]);
        floor.argv = Some(vec!["op".into(), "item".into(), "get".into()]);
        for e in [Strict, Ask, Notify, Open] {
            let out = at(e).decide(&x, &floor);
            assert!(out.floor && out.mode == Mode::Deny, "{}", e.as_str());
        }
        // A needs-approval kind raises an allowed call, and notices name it.
        let mut p = policy(&root);
        p.allow_argv.push(vec!["gh".into()]);
        let merge = run(vec!["gh", "pr", "merge", "12"]);
        let out = p.decide(&x, &merge);
        assert_eq!(out.mode, Mode::Confirm, "strict: {}", out.reason);
        assert!(
            out.reason.starts_with("needs approval: merge"),
            "{}",
            out.reason
        );
        p.enforcement = Notify;
        let out = p.decide(&x, &merge);
        assert_eq!(out.mode, Mode::Allow);
        let n = out.notify.unwrap();
        assert_eq!(n.consequences[0].kind, "merge");
        assert!(!n.consequences[0].irreversible);
        // `opaque` keeps today's treatment (needs approval) and is named.
        let out = at(Notify).decide(&x, &run(vec!["python3", "-c", "import os"]));
        assert_eq!(out.mode, Mode::Allow);
        assert_eq!(
            ConsequenceTag::summary(&out.notify.unwrap().consequences),
            "needs approval: opaque"
        );
        assert_eq!(
            at(Strict).decide(&x, &run(vec!["make", "deploy"])).mode,
            Mode::Confirm
        );
        // The owner regrades a kind: merges now wait under notify too.
        let mut q = at(Notify);
        let mut cfg = crate::config::ConsequencesConfig::default();
        cfg.kinds.insert(
            "merge".into(),
            crate::config::KindConfig {
                irreversible: Some(true),
                ..Default::default()
            },
        );
        cfg.kinds.insert(
            "deploy".into(),
            crate::config::KindConfig {
                irreversible: Some(true),
                argv: vec![vec!["./deploy.sh".into()]],
                ..Default::default()
            },
        );
        q.kinds = Kinds::from_config(&cfg);
        assert_eq!(
            q.decide(&x, &run(vec!["gh", "pr", "merge", "1"])).mode,
            Mode::Confirm
        );
        let out = q.decide(&x, &run(vec!["bash", "-c", "./deploy.sh --now"]));
        assert_eq!(out.mode, Mode::Confirm);
        assert_eq!(
            ConsequenceTag::summary(&out.consequences),
            "irreversible: deploy · needs approval: opaque"
        );
        // Layer 1: a toollet's own declaration is graded the same way, even
        // for a class the policy allows.
        let mut pl = Plan::default();
        pl.consequences.push(Consequence {
            kind: "publish".into(),
            rule: "toollet:pkg.publish".into(),
            detail: "publish pkg 1.0".into(),
        });
        let out = at(Open).decide(&Publishes, &pl);
        assert_eq!(out.mode, Mode::Confirm);
        assert!(out.irreversible && out.rules.is_none());
    }
}
