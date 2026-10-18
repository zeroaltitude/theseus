//! Deploying a checked minted tool (theseus-pw1q.21.8; RSI loop 1 §2.7,
//! §5.2): what a pass of [`super::validate`] turns into a live tool, "act and
//! tell after". Everything here is a self step: it asks `rsi::gate` first, and
//! with the mode off or the switch halted it reads nothing and writes nothing.
//!
//! - **A flow** is loaded through R6's `Core::flow_load` from its frozen copy,
//!   in the scope its spec names (one repository's flow never appears in
//!   another's sessions): one `flow.loaded` row, then `self.mint.deployed`.
//! - **An MCP extension** is *self-loaded*: the extension's own proposal card
//!   is answered by the `"self"` principal, `extend.acked {by: "self", mint}`,
//!   but only for a manifest with no network, no hands (a credential) and every
//!   declared test passed ([`self_load_refusal`]); anything else stays the
//!   owner's card. It loads under the minted ceiling (`extend::load::minted_ceiling`),
//!   whose floor holds every call. The principal answers nothing else:
//!   [`Core::mint_self_load`] takes a manifest, never a question to choose, and
//!   a card of any other kind is refused (its tool is not `extend.ack`); the
//!   place rule refuses `"self"` as an answerer of any card.
//! - **A Rust built-in** is no deploy here: its item carries `mint:<id>` and
//!   goes through self-pr, the self-join's gate, planted revert, review and
//!   keel, and the nightly build and install; nothing is rebuilt on the fly.
//!
//! The rates (§5.2 item 7), which only lower spend: at most `[self.mint]
//! per_night` deploys a night (the last 24 hours' rows) and `max_live` live
//! minted tools; a tool that finds no room *waits*, validated, and is tried
//! again by the tender ([`Core::mint_deploy_waiting`]). The money (`usd` a
//! mint) was held by the validation's replays on `[self.budget]`.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use super::kind::Kind;
use super::rows;
use super::run::Pending;
use super::validate::ValidateReport;
use crate::extend::answer::Answer;
use crate::extend::{manifest_key, Manifest};
use crate::fact::rsi_mint::MintDeployed;
use crate::rpc::Core;
use crate::rsi::pr::PRINCIPAL;
use crate::rsi::{gate, Gate};

const DAY_MS: u64 = 86_400_000;
/// What the notice and the log show of a deploy's undo, by kind.
pub fn undo_of(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Flow => format!("theseus self revert flow:{name}"),
        Kind::Mcp => format!("theseus self revert ext:{name}"),
        Kind::Rust => "theseus self revert <the merge of its join>".into(),
    }
}

/// A report file beside the pending record: what a later deploy reads.
pub(super) const REPORT: &str = "report.json";

/// Why a manifest may not be self-loaded; none when it may: it came up in L1,
/// asks for no network and no hands (the broker's credential), and every one
/// of its declared tests passed (at least one is declared).
pub fn self_load_refusal(m: &Manifest) -> Option<String> {
    if m.state != "proposed" {
        return Some(format!("it is {}, not waiting to be loaded", m.state));
    }
    if let Some(e) = &m.error {
        return Some(format!("it did not come up in L1: {e}"));
    }
    if !m.capabilities.network.is_empty() {
        return Some(format!(
            "it asks for the network ({}): that is the owner's card",
            m.capabilities.network.join(", ")
        ));
    }
    if m.capabilities.hands.is_some() {
        return Some(
            "it asks for AWS hands, which hold a credential: that is the owner's card".into(),
        );
    }
    match (m.passed(), m.tests.len()) {
        (_, 0) => Some("it declares no test".into()),
        (p, n) if p < n => Some(format!("{p} of {n} declared tests passed")),
        _ => None,
    }
}

/// What a deploy made live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployed {
    pub name: String,
    pub kind: Kind,
    pub digest: String,
    pub tool: String,
}

/// A description as a sentence with its full stop, once.
fn sentence(t: &str) -> String {
    format!("{}.", t.trim().trim_end_matches('.'))
}

/// The numbers the notice quotes, from the motif's support and the report.
fn numbers_of(p: &Pending, r: Option<&ValidateReport>) -> Value {
    let s = &p.motif.support;
    let mut n = json!({"runs": s.occurrences, "sessions": s.sessions, "days": s.days});
    if let Some(ab) = r.and_then(|r| r.ab.as_ref()) {
        n["check_same"] = json!(ab.b_matched);
        n["check_total"] = json!(ab.turns);
        n["tokens_pct"] = json!((-ab.token_saving * 100.0).round());
        n["time_pct"] = json!((-ab.time_saving * 100.0).round());
        n["decisions_removed"] = json!((ab.decisions_removed * 10.0).round() / 10.0);
    }
    n
}

impl Core {
    /// Ask `rsi::gate`: a deploy is a self step.
    fn mint_allowed(&self) -> Result<()> {
        match gate(&self.self_ctx()) {
            Gate::Allowed => Ok(()),
            g => bail!("self-improvement is {}: nothing deploys", g.name()),
        }
    }

    /// The live minted tools: flows and extensions the `"self"` principal
    /// loaded.
    pub(crate) fn mint_live(&self) -> Result<Vec<(Kind, String, String)>> {
        let mut out = Vec::new();
        for s in crate::flow::load::LoadedSet::read(&self.store)?
            .loaded
            .values()
        {
            if s.by == PRINCIPAL {
                out.push((Kind::Flow, s.name.clone(), s.digest.clone()));
            }
        }
        for l in crate::extend::load::LoadedSet::read(&self.store)?
            .loaded
            .values()
        {
            if l.acked_by == PRINCIPAL {
                out.push((Kind::Mcp, l.name.clone(), l.digest.clone()));
            }
        }
        Ok(out)
    }

    /// Room for one more: under `max_live` live tools and `per_night` deploys
    /// in the last 24 hours. The `Err` says which line holds it.
    pub(crate) fn mint_room(&self, now_ms: u64) -> Result<()> {
        let cfg = &self.cfg.self_improve.mint;
        let live = self.mint_live()?.len();
        if live >= cfg.max_live as usize {
            bail!("{live} minted tools are live, the most `[self.mint] max_live` allows: it waits");
        }
        let lately = rows::between(
            &self.store,
            &["self.mint.deployed"],
            now_ms.saturating_sub(DAY_MS),
            now_ms,
        )?;
        if lately.len() >= cfg.per_night as usize {
            bail!(
                "{} minted tools went live in the last day, the most `[self.mint] per_night` \
                 allows: it waits",
                lately.len()
            );
        }
        Ok(())
    }

    pub(super) fn mint_report_of(&self, placeholder: &str) -> Result<ValidateReport> {
        let bytes = std::fs::read(self.tools.flows.root().join(placeholder).join(REPORT))
            .map_err(|e| anyhow!("no validation report for {placeholder}: {e}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Write the validation's report beside its pending record.
    pub(crate) fn mint_keep_report(&self, placeholder: &str, r: &ValidateReport) -> Result<()> {
        let dir = self.tools.flows.root().join(placeholder);
        std::fs::write(dir.join(REPORT), serde_json::to_vec_pretty(r)?)?;
        Ok(())
    }

    /// Load the flow `placeholder`'s validation passed, in its scope, and say
    /// so in a `self.mint.deployed` row. Nothing is loaded for a report that
    /// did not pass, a kind that is not a flow, or while no room is left.
    pub(crate) fn mint_deploy_flow(&self, placeholder: &str) -> Result<Deployed> {
        self.mint_allowed()?;
        let pending = self.mint_pending(placeholder)?;
        let report = self.mint_report_of(placeholder)?;
        if pending.kind != Kind::Flow || report.kind != Kind::Flow || !report.pass {
            bail!("{placeholder} is not a flow that passed its checks: nothing is loaded");
        }
        self.mint_room(theseus_protocol::now_unix_ms())?;
        let hex = report.digest.trim_start_matches("sha256:");
        let spec = self
            .tools
            .flows
            .frozen_dir(&report.name, hex)
            .join("spec.json");
        let info = self.flow_load(&spec, PRINCIPAL, "mint")?;
        let described = self
            .tools
            .flows
            .get(&info.name)
            .map(|e| e.spec.description.clone())
            .unwrap_or_default();
        let scope = info.repos.join(", ");
        self.mint_deployed_row(
            Some(&pending),
            (Kind::Flow, &info.name, &report.digest),
            format!(
                "flow {}: {} Live in {scope}.",
                info.name,
                sentence(&described)
            ),
            numbers_of(&pending, Some(&report)),
        )?;
        Ok(Deployed {
            tool: format!("flow:{}", info.name),
            name: info.name,
            kind: Kind::Flow,
            digest: report.digest,
        })
    }

    /// Self-load the proposed extension `name` at `digest`, for the mint
    /// `placeholder`: the module's doc says when it may and what it loads
    /// under. A refusal is the reason, and the card stays the owner's.
    pub(crate) fn mint_self_load(
        &self,
        placeholder: &str,
        name: &str,
        digest: &str,
    ) -> Result<Deployed> {
        self.mint_allowed()?;
        // Its motif's support is quoted when the mint's record is there.
        let pending = self.mint_pending(placeholder).ok();
        let m: Manifest = self
            .store
            .get_meta(&manifest_key(name, digest))?
            .ok_or_else(|| anyhow!("no proposed extension {name} at {digest}"))?;
        if let Some(why) = self_load_refusal(&m) {
            bail!("{name} is not self-loaded: {why}");
        }
        self.mint_room(theseus_protocol::now_unix_ms())?;
        let question = m
            .question
            .as_deref()
            .ok_or_else(|| anyhow!("{name} has no question to answer"))?;
        let a = self
            .kernel
            .action(question)?
            .ok_or_else(|| anyhow!("{name}'s question is gone"))?;
        if a.tool != crate::extend::ACK || a.state != theseus_kernel::ActionState::Planned {
            bail!(
                "{question} is not a waiting extension card: the self principal answers none else"
            );
        }
        self.answer_extension(&a, Answer::Ack, PRINCIPAL, "mint", Some(placeholder))?;
        let mut numbers = pending
            .as_ref()
            .map_or_else(|| json!({}), |p| numbers_of(p, None));
        numbers["tests_passed"] = json!(m.passed());
        numbers["tests"] = json!(m.tests.len());
        self.mint_deployed_row(
            pending.as_ref(),
            (Kind::Mcp, name, digest),
            format!(
                "extension ext-{name}: {} Offered in private places, read-only, no network.",
                sentence(&m.description)
            ),
            numbers,
        )?;
        Ok(Deployed {
            tool: format!("ext-{name}"),
            name: name.into(),
            kind: Kind::Mcp,
            digest: digest.into(),
        })
    }

    fn mint_deployed_row(
        &self,
        p: Option<&Pending>,
        (kind, name, digest): (Kind, &str, &str),
        what: String,
        numbers: Value,
    ) -> Result<()> {
        let fact = MintDeployed {
            name: name.into(),
            kind: kind.name().into(),
            digest: digest.into(),
            tool: match kind {
                Kind::Flow => format!("flow:{name}"),
                _ => format!("ext-{name}"),
            },
            bench_born: p.is_some_and(|p| p.motif.bench_born),
            what,
            numbers,
            undo: undo_of(kind, name),
        };
        self.store.append(&[crate::fact::row(&fact, None, None)?])?;
        self.rec(None).announce(&fact);
        Ok(())
    }

    /// Validated flows that found no room: tried again, oldest first, until
    /// the room is gone. Under the tender's gate; nothing with the mode off.
    pub(crate) fn mint_deploy_waiting(&self) -> Result<Vec<Deployed>> {
        self.mint_allowed()?;
        let root = self.tools.flows.root().to_path_buf();
        let Ok(dirs) = std::fs::read_dir(&root) else {
            return Ok(vec![]);
        };
        let now = theseus_protocol::now_unix_ms();
        let seen = rows::between(
            &self.store,
            &["self.mint.deployed", "self.mint.retired"],
            now.saturating_sub(90 * DAY_MS),
            now,
        )?;
        let has = |d: &str| seen.iter().any(|r| r.data["digest"] == d);
        let mut waiting: Vec<(std::time::SystemTime, String)> = Vec::new();
        for e in dirs.flatten() {
            let report = e.path().join(REPORT);
            let Ok(md) = std::fs::metadata(&report) else {
                continue;
            };
            let placeholder = e.file_name().to_string_lossy().into_owned();
            if let Ok(r) = self.mint_report_of(&placeholder) {
                if r.kind == Kind::Flow && r.pass && !has(&r.digest) {
                    waiting.push((md.modified().unwrap_or(std::time::UNIX_EPOCH), placeholder));
                }
            }
        }
        waiting.sort();
        let mut out = Vec::new();
        for (_, placeholder) in waiting {
            match self.mint_deploy_flow(&placeholder) {
                Ok(d) => out.push(d),
                Err(e) => {
                    tracing::info!(tool = %placeholder, why = %format!("{e:#}"), "self mint: waits");
                    break;
                }
            }
        }
        Ok(out)
    }
}
