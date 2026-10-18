//! The tender: keep, retire or graduate a live minted tool by its use
//! (theseus-pw1q.21.8; RSI loop 1 §3.2). Counted from the ledger's `action.*`
//! rows as `extend/usage.rs` counts extensions (a flow's call is
//! `harness:flow:<name>`, an extension's `async:mcp:ext-<name>/<tool>`); no
//! per-call write exists anywhere. In the order the rules are asked ([`decide`]):
//!
//! | Rule | Action |
//! |---|---|
//! | a red health signal attributed to its call | retire now |
//! | more than 20% of at least 5 calls failed | retire now |
//! | fewer than 3 calls in `[self.mint] retire_after_days`, once it is that old | retire |
//! | its motif's by-hand runs are not falling (the miner keeps counting), once it is that old | retire, the evidence filed as a cause |
//! | `[self] graduate_min_calls` calls in `graduate_window_days` with 0 failures | graduate (the night's backlog files the port: `backlog/graduate.rs`) |
//! | otherwise | keep |
//!
//! Retiring unloads the tool (a flow leaves its record, an extension is
//! revoked by `self`) and writes `self.mint.retired`; **the frozen copy is
//! never deleted**, and the row says where it is. A write outside the declared
//! scope is not a rule here: a flow has no authored write (its allow-list) and an
//! extension's writes are discarded with L1's scratch, so nothing in the ledger
//! could show one. A saving under 10% is not a rule either: the saving is
//! measured by the A/B before the tool deploys and passes at 15%, and no
//! live measurement of it exists (named in the report). The tender asks `rsi::gate`
//! first and with the mode off, or the switch halted, reads and writes nothing.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::json;
use theseus_protocol::rsi::SelfMode;

use super::deploy::Deployed;
use super::kind::Kind;
use crate::extend::usage::Call;
use crate::fact::rsi_mint::MintRetired;
use crate::rpc::Core;
use crate::rsi::pr::PRINCIPAL;
use crate::rsi::revert::signals::{Red, Signals};
use crate::rsi::{gate, Gate};

const DAY_MS: u64 = 86_400_000;
/// A red is a tool's when one of its calls ended this long before it.
pub const ATTRIBUTE_MS: u64 = 60 * 60_000;
/// More than this percent of the calls failing retires (given [`FAIL_MIN_CALLS`]).
pub const FAIL_PERCENT: u64 = 20;
pub const FAIL_MIN_CALLS: u64 = 5;
/// Fewer calls than this in the retire window is clutter.
pub const UNUSED_BELOW: u64 = 3;

/// What the tender knows of one live tool.
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    /// Calls and failures since it loaded, within the retire window.
    pub calls: u64,
    pub failures: u64,
    /// Days since it loaded.
    pub age_days: u32,
    /// A red health signal's detail, attributed to its call.
    pub red: Option<String>,
    /// The motif's by-hand runs when it was minted, and now; none when the
    /// miner was not asked (the tool is younger than the window).
    pub by_hand: Option<(u32, u32)>,
}

/// The rules' one number: `[self.mint] retire_after_days`.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    pub retire_after_days: u32,
}

/// What the rules say.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Keep,
    /// Unload it: the rule's name and the numbers it fired on, in words.
    Retire {
        rule: &'static str,
        why: String,
    },
}

/// The rules, in order.
pub fn decide(e: &Evidence, r: &Rules) -> Verdict {
    if let Some(red) = &e.red {
        return Verdict::Retire {
            rule: "health",
            why: format!("a red signal followed its call: {red}"),
        };
    }
    if e.calls >= FAIL_MIN_CALLS && e.failures * 100 > FAIL_PERCENT * e.calls {
        return Verdict::Retire {
            rule: "failures",
            why: format!(
                "{} of {} calls failed ({}%)",
                e.failures,
                e.calls,
                e.failures * 100 / e.calls
            ),
        };
    }
    let old = e.age_days >= r.retire_after_days;
    if old && e.calls < UNUSED_BELOW {
        return Verdict::Retire {
            rule: "unused",
            why: format!("{} calls in {} days", e.calls, r.retire_after_days),
        };
    }
    if let (true, Some((before, now))) = (old, e.by_hand) {
        if now >= before {
            return Verdict::Retire {
                rule: "not-falling",
                why: format!("by-hand runs of its motif did not fall: {before} then, {now} now"),
            };
        }
    }
    Verdict::Keep
}

/// A minted tool's key in a call count: `flow:<name>` or `ext:<name>`.
pub(crate) fn minted_name_of(tool: &str) -> Option<String> {
    if let Some(n) = crate::extend::usage::name_of(tool) {
        return Some(format!("ext:{n}"));
    }
    let t = tool.strip_prefix("harness:").unwrap_or(tool);
    t.strip_prefix(crate::flow::tool::PREFIX)
        .map(|n| format!("flow:{n}"))
}

/// The one tool a red is attributed to: the only one with a call that ended
/// within [`ATTRIBUTE_MS`] before it (two suspects, or none, and it is no
/// tool's: a red is another change's more often than a tool's).
pub fn attributed<'a>(
    red: &Red,
    calls: &'a BTreeMap<String, Vec<Call>>,
    loaded_at: &BTreeMap<String, u64>,
) -> Option<&'a str> {
    let mut hit = calls.iter().filter(|(k, cs)| {
        loaded_at.get(*k).is_some_and(|at| *at <= red.at_ms)
            && cs
                .iter()
                .any(|c| c.at_ms <= red.at_ms && red.at_ms - c.at_ms <= ATTRIBUTE_MS)
    });
    match (hit.next(), hit.next()) {
        (Some((k, _)), None) => Some(k.as_str()),
        _ => None,
    }
}

/// What one tender pass did.
#[derive(Debug, Default, PartialEq)]
pub struct Tended {
    /// `(tool, rule)` for each tool retired.
    pub retired: Vec<(String, &'static str)>,
    /// Validated flows that found room now.
    pub deployed: Vec<Deployed>,
}

impl Core {
    /// One pass: the module's doc. `signals` is the revert watch's reader.
    pub async fn mint_tend(self: &Arc<Self>, signals: &dyn Signals, now_ms: u64) -> Result<Tended> {
        let mut out = Tended::default();
        if gate(&self.self_ctx()) != Gate::Allowed {
            return Ok(out);
        }
        let live = self.mint_live()?;
        if !live.is_empty() {
            self.mint_tend_live(&live, signals, now_ms, &mut out)
                .await?;
        }
        out.deployed = self.mint_deploy_waiting()?;
        Ok(out)
    }

    async fn mint_tend_live(
        self: &Arc<Self>,
        live: &[(Kind, String, String)],
        signals: &dyn Signals,
        now_ms: u64,
        out: &mut Tended,
    ) -> Result<()> {
        let rules = Rules {
            retire_after_days: self.cfg.self_improve.mint.retire_after_days,
        };
        let loaded_at = self.mint_loaded_at()?;
        let window = rules.retire_after_days.max(1);
        let calls = self.tool_calls(now_ms, window, minted_name_of)?;
        let since = loaded_at.values().copied().min().unwrap_or(now_ms);
        let reds = signals.reds(self, since, now_ms)?;
        let old = |at: &u64| age_days(Some(at), now_ms) >= rules.retire_after_days;
        let by_hand = match loaded_at.values().any(old) {
            true => {
                let core = self.clone();
                Some(crate::rsi::join::blocking(move || core.mint_by_hand_now(now_ms)).await?)
            }
            false => None,
        };
        for (kind, name, digest) in live {
            let key = match kind {
                Kind::Flow => format!("flow:{name}"),
                _ => format!("ext:{name}"),
            };
            let at = loaded_at.get(&key).copied().unwrap_or(now_ms);
            let from = at.max(now_ms.saturating_sub(u64::from(window) * DAY_MS));
            let u = calls.usage(&key, from, window);
            let red = reds
                .iter()
                .find(|r| attributed(r, &calls.by_name, &loaded_at) == Some(key.as_str()))
                .map(|r| r.detail.clone());
            let hand = match (&by_hand, self.mint_pending_for(name)) {
                (Some(now), Some(p)) => Some((
                    p.motif.support.occurrences,
                    now.get(&p.motif.digest).copied().unwrap_or(0),
                )),
                _ => None,
            };
            let e = Evidence {
                calls: u.calls,
                failures: u.failures,
                age_days: age_days(Some(&at), now_ms),
                red,
                by_hand: hand,
            };
            if let Verdict::Retire { rule, why } = decide(&e, &rules) {
                self.mint_retire(
                    (*kind, name, digest),
                    (rule, &why),
                    json!({"calls": e.calls, "failures": e.failures, "days": e.age_days}),
                )?;
                out.retired.push((key, rule));
            }
        }
        Ok(())
    }

    /// When each live minted tool was loaded, by `flow:<name>` / `ext:<name>`.
    fn mint_loaded_at(&self) -> Result<BTreeMap<String, u64>> {
        let mut at = BTreeMap::new();
        for s in crate::flow::load::LoadedSet::read(&self.store)?
            .loaded
            .values()
        {
            if s.by == PRINCIPAL {
                at.insert(format!("flow:{}", s.name), s.at_ms);
            }
        }
        for l in crate::extend::load::LoadedSet::read(&self.store)?
            .loaded
            .values()
        {
            if l.acked_by == PRINCIPAL {
                at.insert(format!("ext:{}", l.name), l.acked_at_ms);
            }
        }
        Ok(at)
    }

    /// The pending record of the mint whose report names `name`: how the motif
    /// is found again from the tool.
    fn mint_pending_for(&self, name: &str) -> Option<super::run::Pending> {
        let dirs = std::fs::read_dir(self.tools.flows.root()).ok()?;
        dirs.flatten().find_map(|e| {
            let placeholder = e.file_name().to_string_lossy().into_owned();
            let report = self.mint_report_of(&placeholder).ok()?;
            (report.name == name).then(|| self.mint_pending(&placeholder).ok())?
        })
    }

    /// The miner's current by-hand runs of each motif, by digest: asked once a
    /// pass (a read in memory; it writes nothing).
    fn mint_by_hand_now(&self, now_ms: u64) -> Result<BTreeMap<String, u32>> {
        let n = self.mine_run(now_ms, false)?;
        Ok(n.mined
            .motifs
            .iter()
            .map(|m| (m.digest.clone(), m.support.occurrences))
            .collect())
    }

    /// Unload `tool` by `rule` and write the row. The frozen copy stays.
    pub(crate) fn mint_retire(
        &self,
        (kind, name, digest): (Kind, &str, &str),
        (rule, why): (&str, &str),
        numbers: serde_json::Value,
    ) -> Result<()> {
        let frozen = match kind {
            Kind::Flow => {
                let info = self.flow_retire(name, PRINCIPAL, "mint")?;
                info.frozen
            }
            _ => {
                let r = self.revoke_loaded(
                    name,
                    Some(digest),
                    (PRINCIPAL, "mint"),
                    Some(rule),
                    || Ok(()),
                )?;
                r.frozen
            }
        };
        let fact = MintRetired {
            name: name.into(),
            kind: kind.name().into(),
            // As the reports and the deploy rows write it, so the rows agree.
            digest: format!("sha256:{}", digest.trim_start_matches("sha256:")),
            tool: match kind {
                Kind::Flow => format!("flow:{name}"),
                _ => format!("ext-{name}"),
            },
            bench_born: self
                .mint_pending_for(name)
                .is_some_and(|p| p.motif.bench_born),
            rule: rule.into(),
            why: why.into(),
            numbers,
            undo: format!("nothing was deleted: its frozen copy is kept at {frozen}"),
        };
        self.store.append(&[crate::fact::row(&fact, None, None)?])?;
        self.rec(None).announce(&fact);
        Ok(())
    }

    /// The minted tools' tender, after serving: a pass [`FIRST`] after the
    /// start and every [`EVERY`] after; nothing with the mode off.
    pub fn mint_tender_after_serving(self: &Arc<Self>) {
        if self.cfg.self_improve.mode != SelfMode::Act {
            return;
        }
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(FIRST).await;
            let signals = crate::rsi::revert::signals::Files::default();
            loop {
                let Some(c) = core.upgrade() else { return };
                match c.mint_tend(&signals, theseus_protocol::now_unix_ms()).await {
                    Ok(t) if t != Tended::default() => {
                        tracing::info!(retired = ?t.retired, deployed = t.deployed.len(),
                            "self mint: a tender pass ran");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "self mint: a tender pass failed")
                    }
                }
                drop(c);
                tokio::time::sleep(EVERY).await;
            }
        });
    }
}

/// The first pass, after the start, and between passes.
pub const FIRST: Duration = Duration::from_secs(30 * 60);
pub const EVERY: Duration = Duration::from_secs(6 * 3600);

/// Whole days since `at` (a missing time is now).
fn age_days(at: Option<&u64>, now_ms: u64) -> u32 {
    at.map_or(0, |a| (now_ms.saturating_sub(*a) / DAY_MS) as u32)
}
