//! `policy.explain` (step 42a, theseus-ext.7): why a call waits, on one
//! screen. For each tool, every layer of the gate in its order, what it
//! says, the setting that says it, and the posture after it; and the layers
//! that depend on the call (the floor's paths and programs, the approve and
//! allow lists, the roots, a private address, AWS operations, L1, a granted
//! program, a language server's start) as conditions with their entries.
//!
//! It is built on the gate's own functions, never a copy of its order: the
//! place's refusal and the order (`toolrun::order`) run here on a call inside
//! the roots, with what the gate would read of the place (`TurnRunner::view_of`,
//! the session's hold, an MCP client's floor), and each layer's row is the
//! decision the order handed its watcher there. FAST (§9): it reads records
//! (a session's, its place's, its execution's) and the config, never the
//! history.

use std::path::PathBuf;
use std::sync::Arc;

use theseus_protocol::{
    error_code, ExplainCondition, ExplainLayer, ExternalText, PlaceClass, PlaceExplain,
    PolicyExplainParams, PolicyExplainResult, ToolExplain,
};
use theseus_tools::{Access, Backend, Plan, Resource, Tool, ToolClass};

use super::server::RpcFailure;
use super::Core;
use crate::ceiling::PlaceView;
use crate::policy::{Decision, Posture};
use crate::toolrun::order::{At, Layer};

/// One place as the gate reads it, with its session's hold when a session
/// is named or the place runs on one.
struct Scene {
    place: String,
    name: String,
    view: PlaceView,
    session_id: Option<String>,
    hold: Result<Option<ExternalText>, String>,
    mcp: Option<Posture>,
}

impl Core {
    /// `policy.explain`.
    pub fn policy_explain(
        &self,
        p: PolicyExplainParams,
    ) -> Result<PolicyExplainResult, RpcFailure> {
        let tools = self.explained_tools(p.tool.as_deref())?;
        let scenes = match &p.session_id {
            Some(s) => vec![self.session_scene(s)?],
            None => self.every_scene(),
        };
        Ok(PolicyExplainResult {
            places: scenes
                .into_iter()
                .map(|s| PlaceExplain {
                    tools: tools
                        .iter()
                        .map(|t| self.explain_tool(&s, t.as_ref()))
                        .collect(),
                    place: s.place,
                    name: s.name,
                    class: s.view.class,
                    ceiling: s.view.ceiling.map(|c| c.wire.clone()),
                    session_id: s.session_id,
                    hold: s.hold.ok().flatten(),
                })
                .collect(),
            roots: self
                .tools
                .policy
                .roots
                .iter()
                .map(|r| r.display().to_string())
                .collect(),
        })
    }

    /// The tools explained: the one named, or every built-in and MCP tool,
    /// in `tool.list`'s order.
    fn explained_tools(&self, name: Option<&str>) -> Result<Vec<Arc<dyn Tool>>, RpcFailure> {
        let mut all: Vec<Arc<dyn Tool>> = self.tools.registry.all().cloned().collect();
        all.extend(
            self.tools
                .mcp
                .all()
                .iter()
                .map(|t| t.clone() as Arc<dyn Tool>),
        );
        let Some(name) = name else {
            return Ok(all);
        };
        match all.into_iter().find(|t| t.name() == name) {
            Some(t) => Ok(vec![t]),
            None => Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no tool is named {name:?}: `theseus tools` lists them"),
            )),
        }
    }

    /// A session's place, as its turn's gate reads it.
    fn session_scene(&self, session_id: &str) -> Result<Scene, RpcFailure> {
        let rec = self
            .store
            .get_session::<crate::session::SessionRecord>(session_id)?
            .ok_or_else(|| {
                RpcFailure::new(error_code::NOT_FOUND, format!("no session {session_id}"))
            })?;
        let view = self.runner.view_of(session_id);
        let target = self
            .outbox
            .try_target(session_id)
            .ok()
            .flatten()
            .or_else(|| self.outbox.wake_target(session_id));
        let (place, name) = match &target {
            None => ("cli".to_string(), "CLI".to_string()),
            Some(t) => {
                let name = self
                    .runner
                    .place_rule
                    .find(t)
                    .map_or_else(|| t.clone(), |b| b.name);
                (t.clone(), name)
            }
        };
        let mcp = rec
            .execution_id
            .as_deref()
            .and_then(|x| crate::toolrun::mcp_floor_of(&self.kernel, x));
        Ok(Scene {
            place,
            name,
            view,
            session_id: Some(session_id.to_string()),
            hold: crate::external::held(&self.store, session_id),
            mcp,
        })
    }

    /// The CLI, then every place the binding bound, each with the hold of
    /// the session it runs on, if one does.
    fn every_scene(&self) -> Vec<Scene> {
        let rule = &self.runner.place_rule;
        let mut scenes = vec![Scene {
            place: "cli".into(),
            name: "CLI".into(),
            view: rule.place(&self.cfg, None),
            session_id: None,
            hold: Ok(None),
            mcp: None,
        }];
        for p in rule.health(&self.cfg).places {
            if !p.place.starts_with("discord:") {
                continue;
            }
            let session = p
                .place
                .strip_prefix("discord:")
                .and_then(|k| self.outbox.place_session(k).ok().flatten());
            let hold = match &session {
                Some(s) => crate::external::held(&self.store, s),
                None => Ok(None),
            };
            scenes.push(Scene {
                view: rule.place(&self.cfg, Some(&p.place)),
                place: p.place,
                name: p.name,
                session_id: session,
                hold,
                mcp: None,
            });
        }
        scenes
    }

    /// One tool in one place: the place's refusal, then the order on a call
    /// inside the roots, layer by layer, and the conditions.
    fn explain_tool(&self, s: &Scene, tool: &dyn Tool) -> ToolExplain {
        let rt = &self.tools;
        let name = tool.name();
        let plan = edit_probe(self, s.view.class, tool, probe(rt, s.view, tool));
        let mut layers = Vec::new();
        let rule_offers = crate::places::offered(s.view.class, name, &rt.public_roots);
        layers.push(ExplainLayer {
            layer: "place".into(),
            says: match (s.view.class, rule_offers) {
                (PlaceClass::Private, _) => "a private place: every tool is offered".into(),
                (PlaceClass::Shared, true) => {
                    "a shared place: offered, one of the public tools, a glide, or a file tool under the public paths".into()
                }
                (PlaceClass::Shared, false) => {
                    "a shared place: only the public tools, and the file tools under the public paths".into()
                }
            },
            setting: (s.view.class == PlaceClass::Shared)
                .then(|| "the bindings file's class, and [places] public_paths".into()),
            result: if rule_offers { "offered" } else { "refused" }.into(),
            raised: false,
        });
        if let Some(c) = s.view.ceiling {
            layers.push(ExplainLayer {
                layer: "ceiling".into(),
                says: match (&c.tools, c.offers(name)) {
                    (None, _) => format!("{}'s ceiling narrows no tools", c.place),
                    (Some(t), true) => format!("{}'s ceiling offers {}", c.place, t.join(", ")),
                    (Some(_), false) => c.refusal(name).unwrap_or_default(),
                },
                setting: c.tools.as_ref().map(|_| format!("{}'s tools", c.place)),
                result: if c.offers(name) { "offered" } else { "refused" }.into(),
                raised: false,
            });
        }
        let mut conditions = conditions(self, s, tool);
        if let Some(why) = rt.refusal(s.view, name, &plan) {
            return ToolExplain {
                tool: name.into(),
                class: tool.class().as_str().into(),
                offered: false,
                refused: Some(why.clone()),
                layers,
                conditions,
                result: "refused".into(),
                reason: format!("{}: {why}", crate::toolrun::PLACE_REFUSAL),
            };
        }
        let hold = s.hold.clone();
        let held = move || hold.clone();
        let mcp = s.mcp;
        let mcp = move || mcp;
        // A glide's other place is its call's (38b): `glide`'s condition
        // says what the rule does there.
        let at = At {
            place: s.view,
            held: &held,
            mcp: &mcp,
            glide: None,
            session: None,
        };
        let mut seen: Vec<(Layer, Decision)> = Vec::new();
        let (d, _) = rt.order(&at, tool, &plan, &serde_json::Value::Null, &mut |l, d| {
            seen.push((l, d.clone()))
        });
        let mut before: Option<Posture> = None;
        for (l, after) in &seen {
            if let Some(row) = self.layer_row(s, tool, *l, after, before) {
                layers.extend(row);
            }
            before = Some(after.posture);
        }
        if tool.family() == "lsp" {
            conditions.push(lsp_condition(self));
        }
        let starting = starts_on_edit(self, s.view.class, tool);
        if !starting.is_empty() {
            conditions.push(edit_start_condition(self, &starting));
        }
        ToolExplain {
            tool: name.into(),
            class: tool.class().as_str().into(),
            offered: true,
            refused: None,
            layers,
            conditions,
            result: d.posture.as_str().into(),
            reason: d.reason,
        }
    }

    /// The rows one layer of the order adds, from the decision after it.
    fn layer_row(
        &self,
        s: &Scene,
        tool: &dyn Tool,
        layer: Layer,
        after: &Decision,
        before: Option<Posture>,
    ) -> Option<Vec<ExplainLayer>> {
        let rt = &self.tools;
        let name = tool.name();
        let row = |layer: &str, says: String, setting: Option<String>| ExplainLayer {
            layer: layer.into(),
            says,
            setting,
            result: after.posture.as_str().into(),
            raised: before.is_some_and(|b| after.posture > b),
        };
        Some(match layer {
            Layer::Policy => self.policy_rows(tool, after, row),
            Layer::Grant => vec![row(
                "grant",
                match &after.granted {
                    Some(g) => format!("the broker grants it a secret: {g}, so it runs at no looser a posture than the secret's"),
                    None => "no secret is granted to a call like this".into(),
                },
                after.granted.as_ref().and_then(|_| {
                    rt.broker
                        .status()
                        .into_iter()
                        .find(|g| g.kind == "tool" && g.to == name)
                        .map(|g| format!("[broker] {} → {} ({})", g.secret, g.to, g.posture))
                }),
            )],
            Layer::Lsp if tool.family() == "lsp" => vec![row(
                "lsp",
                "a call on a root whose language server has started takes its own posture".into(),
                None,
            )],
            // L3: an edit that starts its file's server, judged as proc.run
            // (theseus-t2xr); a row only where it raised the posture.
            Layer::Lsp
                if !starts_on_edit(self, s.view.class, tool).is_empty()
                    && before.is_some_and(|b| after.posture > b) =>
            {
                vec![row(
                    "lsp",
                    "an edit that starts its file's language server is judged as proc.run for the server's argv, and takes the stricter posture (L3)".into(),
                    Some("[lsp.servers.<name>] start_on_edit, and proc.run's posture".into()),
                )]
            }
            Layer::Lsp | Layer::SharedFetch | Layer::Glide => return None,
            Layer::Floor => vec![row(
                "floor",
                match s.view.ceiling.and_then(|c| c.floor.map(|f| (c, f))) {
                    Some((c, f)) => format!("{}'s ceiling sets a floor of {}", c.place, f.as_str()),
                    None => "no floor: the place's ceiling sets none".into(),
                },
                s.view
                    .ceiling
                    .filter(|c| c.floor.is_some())
                    .map(|c| format!("{}'s posture_floor", c.place)),
            )],
            Layer::Hold => vec![row(
                "hold",
                hold_says(s, tool, rt.external_text),
                Some(format!("[policy] external_text = {}", rt.external_text.as_str())),
            )],
            Layer::McpClient => vec![row(
                "mcp_client",
                format!(
                    "an MCP client opened this session: its calls that act run at no looser than {}",
                    s.mcp?.as_str()
                ),
                Some(crate::mcp_server::SETTING.into()),
            )],
        })
    }

    /// The call's own policy, as rows: its class (a job's), the config's
    /// posture, and a tightening. The tightening's row has the policy's
    /// decision.
    fn policy_rows(
        &self,
        tool: &dyn Tool,
        after: &Decision,
        row: impl Fn(&str, String, Option<String>) -> ExplainLayer,
    ) -> Vec<ExplainLayer> {
        let rt = &self.tools;
        let name = tool.name();
        let mut rows = Vec::new();
        let l1 = (tool.backend() == Backend::Job)
            .then(|| rt.sandbox.l1_for(&[], &serde_json::Value::Null))
            .flatten();
        if tool.backend() == Backend::Job {
            rows.push(ExplainLayer {
                result: match &l1 {
                    Some(_) => "l1".into(),
                    None => "l0".into(),
                },
                raised: false,
                ..row(
                    "class",
                    match &l1 {
                        Some(why) => format!(
                            "L1, a sandbox ({why}): it runs at notify whatever the floor and lists say, unless the operator's own line or a tightening asks"
                        ),
                        None => "L0: it runs on the host, under the floor, the lists, and its posture".into(),
                    },
                    Some("[sandbox] default and l1_argv".into()),
                )
            });
        }
        let config = match (&l1, rt.policy.tools.get(name)) {
            (Some(why), None) => (Posture::Notify, format!("L1: {why}")),
            (Some(_), Some(p)) | (None, Some(p)) => {
                (*p, format!("[policy.tools] \"{name}\" = {}", p.as_str()))
            }
            (None, None) => rt.policy.posture(name),
        };
        rows.push(ExplainLayer {
            result: config.0.as_str().into(),
            raised: false,
            ..row(
                "posture",
                "the config's posture for the tool".into(),
                Some(config.1),
            )
        });
        let t = rt.tightened.get(name);
        let config_posture = config.0;
        rows.push(ExplainLayer {
            raised: after.posture > config_posture,
            ..row(
                "tightening",
                match &t {
                    Some(t) => format!(
                        "tightened by {} (\"should have asked\"): it asks first until undone",
                        t.by
                    ),
                    None => "not tightened".into(),
                },
                t.as_ref().map(|t| format!("tightened by {}", t.by)),
            )
        });
        rows
    }
}

/// A call of `tool` inside the roots that no condition matches: its
/// resource the first workspace root (in a shared place, a file tool's the
/// first public path), as its class reaches it.
fn probe(rt: &crate::toolrun::ToolRuntime, view: PlaceView, tool: &dyn Tool) -> Plan {
    let shared_files = view.class == PlaceClass::Shared && crate::places::files_tool(tool.name());
    let at: PathBuf = match shared_files {
        true => rt.public_roots.first().cloned(),
        false => rt.policy.roots.first().cloned(),
    }
    .unwrap_or_else(|| rt.ctx.cwd.clone());
    let access = match tool.class() {
        ToolClass::Read => Access::Read,
        ToolClass::Write => Access::Write,
        ToolClass::Run => Access::Exec,
    };
    Plan {
        resources: vec![Resource { path: at, access }],
        summary: format!("a call of {}", tool.name()),
        ..Default::default()
    }
}

/// What T1's hold says of a call of `tool` in the scene's session.
fn hold_says(s: &Scene, tool: &dyn Tool, mode: crate::external::Mode) -> String {
    if crate::external::exempt(tool.class(), tool.name(), &serde_json::Value::Null) {
        return "a read (or a one-shot wake) keeps its posture after external text".into();
    }
    match (&s.hold, &s.session_id) {
        (Ok(Some(h)), _) => crate::external::why(h, mode),
        (Err(e), _) => format!(
            "its session's record could not be read to check for external text, so a call that acts waits: {e}"
        ),
        (Ok(None), Some(_)) => "this session holds no external text".into(),
        (Ok(None), None) => "no session here holds external text".into(),
    }
}

/// Whether a tool's input names a property.
fn takes(tool: &dyn Tool, property: &str) -> bool {
    tool.input_schema()
        .pointer(&format!("/properties/{property}"))
        .is_some()
}

fn shown(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

fn argvs(list: &[Vec<String>]) -> Vec<String> {
    list.iter().map(|a| a.join(" ")).collect()
}

/// The layers that depend on the call, with their entries.
fn conditions(core: &Core, s: &Scene, tool: &dyn Tool) -> Vec<ExplainCondition> {
    let rt = &core.tools;
    let p = &rt.policy;
    let name = tool.name();
    let mut out = Vec::new();
    if s.view.class == PlaceClass::Shared && crate::places::files_tool(name) {
        out.push(cond(
            "public_paths",
            "a path outside the public paths, in a shared place",
            shown(&rt.public_roots),
            "refused",
        ));
    }
    if crate::glide::is_glide(name) {
        out.push(cond(
            "glide",
            "out of a private place into a shared one, or between two shared places, by the place \
             the call names (38b); into a private place it keeps its posture",
            Vec::new(),
            "approve",
        ));
    }
    if tool.backend() == Backend::Harness {
        return out;
    }
    let argv = takes(tool, "argv");
    if takes(tool, "steps") {
        out.push(cond(
            "steps",
            "a batch: each step is judged as the call it would be alone, through every layer here, \
             and the batch takes the strictest (a refusal of any step refuses it), its reason naming \
             the step; one approval binds every step, and a batch whose steps differ in class \
             (L1 and L0) is invalid input",
            Vec::new(),
            "the strictest step's",
        ));
    }
    let aws = name.starts_with("aws.");
    let mut floor = shown(&p.floor_paths);
    if argv {
        floor.extend(argvs(&p.floor_argv));
    }
    if aws {
        floor.push("an AWS guardrail (AWS design §3.6)".into());
    }
    out.push(cond(
        "floor",
        "it touches Theseus's own binary or state, or the 1Password CLI or its token: at every posture",
        floor,
        "approve",
    ));
    if !p.approve_paths.is_empty() {
        out.push(cond(
            "approve_paths",
            "a resource or a path argument on the approve list",
            shown(&p.approve_paths),
            "approve",
        ));
    }
    if tool.class() != ToolClass::Write || argv {
        out.push(cond(
            "outside_roots",
            "a read or a working directory outside the workspace roots (a write outside them takes the posture)",
            shown(&p.roots),
            "approve",
        ));
    }
    // `[policy] private_addresses = "open"` judges it as any other address.
    if takes(tool, "url") && p.private_addresses == crate::web::net::PrivateAddresses::Ask {
        out.push(cond(
            "private_address",
            "a URL whose host is a private address (DD5)",
            Vec::new(),
            "approve",
        ));
    }
    if argv {
        out.extend(argv_conditions(rt));
    }
    if aws {
        out.extend(aws_conditions(p));
    }
    if tool.backend() == Backend::Job && rt.sandbox.l1_for(&[], &serde_json::Value::Null).is_none()
    {
        out.push(cond(
            "l1",
            "its argv starts with an [sandbox] l1_argv entry, or the call asks for sandbox: true",
            argvs(&rt.sandbox.cfg.l1_argv),
            "notify (L1), unless its [policy.tools] line or a tightening asks",
        ));
    }
    out
}

/// The allow and approve lists' entries, and the programs granted secrets:
/// the layers a command's argv decides.
fn argv_conditions(rt: &crate::toolrun::ToolRuntime) -> Vec<ExplainCondition> {
    let p = &rt.policy;
    let mut out = Vec::new();
    if !p.approve_argv.is_empty() {
        out.push(cond(
            "approve_argv",
            "its argv starts with an entry of the approve list",
            argvs(&p.approve_argv),
            "approve",
        ));
    }
    if !p.allow_argv.is_empty() {
        out.push(cond(
            "allow_argv",
            "its argv starts with an entry of the allow list, and every path argument is inside the roots",
            argvs(&p.allow_argv),
            "open",
        ));
    }
    let programs: Vec<String> = rt
        .broker
        .status()
        .into_iter()
        .filter(|g| g.kind == "program")
        .map(|g| {
            format!(
                "{} gets {} ({})",
                g.to,
                g.variable.unwrap_or(g.secret),
                g.posture
            )
        })
        .collect();
    if !programs.is_empty() {
        out.push(cond(
            "grant",
            "its program is granted a secret: it runs at no looser a posture than the secret's",
            programs,
            "at least the secret's posture",
        ));
    }
    out
}

/// An AWS call's own lines: its deletions, and `[policy.aws]`.
fn aws_conditions(p: &crate::policy::ToolPolicy) -> Vec<ExplainCondition> {
    let mut out = Vec::new();
    out.push(cond(
        "session_mint",
        "an AWS session mint (STS AssumeRole*, AssumeRoot, GetSessionToken, GetFederationToken, \
         GetDelegatedAccessToken): at every posture, its keys held as handles",
        Vec::new(),
        "approve",
    ));
    out.push(cond(
        "destructive",
        "an AWS call that deletes, replaces, or removes something that holds state",
        Vec::new(),
        "approve",
    ));
    if !p.aws.is_empty() {
        out.push(cond(
            "aws",
            "[policy.aws] by the call's service:Operation, then its service, then its class",
            p.aws
                .iter()
                .map(|(k, v)| format!("{k} = {}", v.as_str()))
                .collect(),
            "that line's posture",
        ));
    }
    out
}

fn cond(layer: &str, when: &str, entries: Vec<String>, then: &str) -> ExplainCondition {
    ExplainCondition {
        layer: layer.into(),
        when: when.into(),
        entries,
        then: then.into(),
    }
}

/// A language server's first start on a root (L2).
fn lsp_condition(core: &Core) -> ExplainCondition {
    let now = core.tools.posture_now("proc.run");
    ExplainCondition {
        layer: "lsp_start".into(),
        when:
            "the call starts its language server on a root for the first time in this daemon's life"
                .into(),
        entries: Vec::new(),
        then: format!(
            "at least proc.run's posture for the server's argv ({}, {})",
            now.posture.as_str(),
            now.setting
        ),
    }
}

/// The configured language servers an edit by `tool` in a place of `class`
/// can start (L3): an edit tool (`lsp::edits::EDITS`, not `lsp.rename`,
/// which is L2's) in a private place, with `[lsp] edit_diagnostics` on and a
/// server whose `start_on_edit` is.
fn starts_on_edit(core: &Core, class: PlaceClass, tool: &dyn Tool) -> Vec<crate::lsp::Spec> {
    let edit = crate::lsp::edits::EDITS.contains(&tool.name()) && tool.family() != "lsp";
    if !edit
        || class != PlaceClass::Private
        || core.tools.lsp.is_none()
        || !core.cfg.lsp.edit_diagnostics
    {
        return Vec::new();
    }
    crate::lsp::Spec::all(&core.cfg.lsp)
        .into_iter()
        .filter(|s| s.start_on_edit && !s.extensions.is_empty())
        .collect()
}

/// `plan` for an edit, its file one of a type the first such server serves
/// (a directory has no server), so the order's L3 step judges a start as it
/// does an edit's real file.
fn edit_probe(core: &Core, class: PlaceClass, tool: &dyn Tool, mut plan: Plan) -> Plan {
    let Some(ext) = starts_on_edit(core, class, tool)
        .first()
        .and_then(|s| s.extensions.first().cloned())
    else {
        return plan;
    };
    for r in plan
        .resources
        .iter_mut()
        .filter(|r| r.access == Access::Write)
    {
        r.path = r
            .path
            .join(format!("explain-probe.{}", ext.trim_start_matches('.')));
    }
    plan
}

/// An edit that starts its file's language server (L3).
fn edit_start_condition(core: &Core, servers: &[crate::lsp::Spec]) -> ExplainCondition {
    let now = core.tools.posture_now("proc.run");
    ExplainCondition {
        layer: "lsp_start".into(),
        when: "the edit is of a file whose language server has not started on its root in this \
               daemon's life, and its server starts on an edit"
            .into(),
        entries: servers.iter().map(|s| s.name.clone()).collect(),
        then: format!(
            "at least proc.run's posture for the server's argv ({}, {})",
            now.posture.as_str(),
            now.setting
        ),
    }
}
