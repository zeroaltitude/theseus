//! Self-narrow's driver (theseus-pw1q.15): Theseus proposes a narrower
//! policy for its hand roles from what CloudTrail showed them calling, and
//! the owner answers. The pure parts are `crate::aws::narrow`; this is the
//! core's side: the weekly pass under `rsi::gate`, the proposal's record and
//! its owner's card, the owner's answers, and the week's watch after an apply.
//!
//! - **Off by default.** `[self] mode = "off"`, or the kill switch thrown:
//!   nothing is read, written or posted (`gate` first, and before each role).
//! - **At most once a week per role** (`[self] narrow_every_days`), and never
//!   a second proposal for a role while one waits for the owner.
//! - **Reads, not model spend.** The pass makes AWS reads (CloudTrail's
//!   history up to `narrow_read_pages` pages, IAM, Access Analyzer's
//!   validation, the policy simulator, the stack's template) and no model
//!   call, so it touches no model budget.
//! - **An IAM change is the owner's.** `apply`, `decline` and `revert` are
//!   `judge_act(Act::SelfNarrow)`: the owner, from a private place, never a
//!   job's shell. Apply plans a change set of the hands stack from its
//!   deployed template, drops it unrun unless it touches only the role and
//!   its new policy, and runs it. The template as it was is kept so the
//!   revert is one more change set.
//! - **The watch.** For a week after an apply, the nightly pass reads the
//!   role's AccessDenied events since the apply; a call not told of before is
//!   a `self.narrow.red` row and a card with the revert.
//!
//! State is one META record, `self.narrow` (a new key, not a new record
//! kind): the newest proposals and when each role was last looked at.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::rsi::{SelfNarrowItem, SelfNarrowParams, SelfNarrowResult};
use theseus_store::{kinds, NewRecord};

use super::{gate, Gate};
use crate::approval::Answerer;
use crate::aws::narrow::apply::{self, Profile, PROFILES};
use crate::aws::narrow::live::{Live, Stack};
use crate::aws::narrow::proposal::{Proposal, State};
use crate::aws::narrow::run::{propose, Env, Made, Settings};
use crate::aws::narrow::usage::observed_since;
use crate::fact::rsi_narrow::{
    NarrowApplied, NarrowDeclined, NarrowProposed, NarrowRed, NarrowReverted,
};
use crate::rpc::{Act, Core};

/// The record's key.
pub const KEY: &str = "self.narrow";
/// The newest proposals kept.
const KEPT: usize = 12;
/// How long the watch lasts after an apply.
pub const WATCH_MS: u64 = 7 * 86_400_000;
const DAY_MS: u64 = 86_400_000;
/// The profiles with a wide policy to narrow: the basic one holds only the
/// hand basics.
fn profiles() -> impl Iterator<Item = Profile> {
    PROFILES.iter().copied().filter(|p| p.name != "basic")
}

/// A word from the owner that cannot be done as typed (no such proposal, a
/// proposal in the wrong state): the caller's mistake.
fn bad(s: impl Into<String>) -> anyhow::Error {
    super::backlog::BadItem(s.into()).into()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub(crate) proposals: Vec<Proposal>,
    /// When each role was last looked at (unix ms), by role.
    pub(crate) last: BTreeMap<String, u64>,
}

/// One pass at a time, and one answer at a time.
fn one() -> &'static tokio::sync::Mutex<()> {
    static ONE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    ONE.get_or_init(Default::default)
}

fn item(p: &Proposal) -> SelfNarrowItem {
    SelfNarrowItem {
        id: p.id.clone(),
        role: p.role.clone(),
        state: match p.state {
            State::Open => "open",
            State::Applied => "applied",
            State::Declined => "declined",
            State::Reverted => "reverted",
        }
        .into(),
        at_ms: p.at_ms,
        actions_before: p.reach_before,
        actions_after: p.reach_after,
        evidence_days: p.days,
        applied_ms: p.applied_ms,
    }
}

impl Core {
    pub(crate) fn narrow_record(&self) -> Result<Record> {
        Ok(self.store.get_meta::<Record>(KEY)?.unwrap_or_default())
    }

    /// The record and `rows`, in one frame.
    fn narrow_save(&self, r: &Record, rows: Vec<NewRecord>) -> Result<()> {
        let mut recs = vec![NewRecord::json(kinds::META, Some(KEY), r)?];
        recs.extend(rows);
        self.store.append(&recs)?;
        Ok(())
    }

    fn narrow_dir(&self) -> std::path::PathBuf {
        self.cfg.state_dir().join("self").join("narrow")
    }

    /// The account the pass reads and the stack it changes.
    fn narrow_live(&self) -> Result<Live> {
        let Some(aws) = self.tools.aws.as_ref() else {
            bail!("no AWS account is bound: the config has no [aws.accounts.<id>] table");
        };
        let account = aws.account(None).map_err(|e| anyhow::anyhow!(e))?.clone();
        let region = account.cfg.region.clone();
        Ok(Live {
            account,
            region,
            binding: None,
        })
    }

    fn narrow_card(&self, kind: &str, text: String) {
        let sent = (|| -> Result<()> {
            let (post, frame) = self
                .outbox
                .stage_to_operator(None, json!({"kind": kind, "text": text}))?;
            self.store.append(&frame)?;
            self.outbox.posted(&post);
            Ok(())
        })();
        if let Err(e) = sent {
            tracing::warn!(error = %format!("{e:#}"), "self-narrow: the card was not posted");
        }
    }

    fn narrow_allowed(&self) -> bool {
        gate(&self.self_ctx()) == Gate::Allowed
    }

    /// The weekly pass, from the nightly tender: the watch, then each role
    /// that is due. Nothing with the mode off or the switch thrown.
    pub async fn self_narrow_nightly(&self) {
        if !self.narrow_allowed() {
            return;
        }
        let Ok(live) = self.narrow_live() else { return };
        let now = theseus_protocol::now_unix_ms();
        let universe = crate::aws::narrow::live::universe();
        let _one = one().lock().await;
        match self.narrow_watch(&live, now).await {
            Ok(lines) => lines
                .iter()
                .for_each(|l| tracing::info!("self-narrow: {l}")),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self-narrow: the watch did not run")
            }
        }
        match self
            .narrow_pass(&live, &universe, now, None, false, false)
            .await
        {
            Ok(lines) => lines
                .iter()
                .for_each(|l| tracing::info!("self-narrow: {l}")),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self-narrow: the pass did not run")
            }
        }
    }

    /// Look at each role in `only` (else each that has a wide policy) that is
    /// due, or all of them with `force`. What happened, a line a role.
    pub(crate) async fn narrow_pass(
        &self,
        env: &impl Env,
        universe: &[(String, u32)],
        now_ms: u64,
        only: Option<Profile>,
        force: bool,
        dry: bool,
    ) -> Result<Vec<String>> {
        let cfg = &self.cfg.self_improve;
        let mut lines = Vec::new();
        for profile in profiles().filter(|p| only.is_none_or(|o| o == *p)) {
            if !dry && !self.narrow_allowed() {
                lines.push("stopped: self-improvement is halted or off".into());
                break;
            }
            let mut rec = self.narrow_record()?;
            if !force && !dry {
                let every = u64::from(cfg.narrow_every_days) * DAY_MS;
                if rec
                    .last
                    .get(profile.role)
                    .is_some_and(|t| now_ms.saturating_sub(*t) < every)
                {
                    lines.push(format!(
                        "{}: looked at in the last {} days",
                        profile.role, cfg.narrow_every_days
                    ));
                    continue;
                }
                if rec
                    .proposals
                    .iter()
                    .any(|p| p.role == profile.role && p.state == State::Open)
                {
                    lines.push(format!("{}: a proposal waits for the owner", profile.role));
                    continue;
                }
            }
            let settings = Settings {
                days: cfg.narrow_days,
                max_pages: cfg.narrow_read_pages,
                now_ms,
                account: self.narrow_account(),
                region: self.narrow_region(),
                universe,
            };
            let made = propose(env, profile, &settings).await;
            if dry {
                // A look: the reads were made, and nothing is kept or posted.
                lines.push(match made {
                    Err(e) => format!("{}: the pass failed: {e}", profile.role),
                    Ok(Made::Not(why)) => format!("{}: no proposal: {why}", profile.role),
                    Ok(Made::Proposal(p)) => {
                        format!("{}: would propose:\n{}", profile.role, p.words())
                    }
                });
                continue;
            }
            if !self.narrow_allowed() {
                lines.push("stopped before writing: self-improvement is halted or off".into());
                break;
            }
            rec.last.insert(profile.role.into(), now_ms);
            match made {
                Err(e) => {
                    self.narrow_save(&rec, vec![])?;
                    lines.push(format!("{}: the pass failed: {e}", profile.role));
                }
                Ok(Made::Not(why)) => {
                    self.narrow_save(&rec, vec![])?;
                    lines.push(format!("{}: no proposal: {why}", profile.role));
                }
                Ok(Made::Proposal(p)) => {
                    let fact = NarrowProposed {
                        role: p.role.clone(),
                        proposal: p.id.clone(),
                        actions_before: p.reach_before,
                        actions_after: p.reach_after,
                        evidence_days: p.days,
                        analyzer_findings: p.analyzer.clone(),
                        events: p.events,
                    };
                    let row = crate::fact::row(&fact, None, None)?;
                    lines.push(format!(
                        "{}: proposed {} ({} to {})",
                        p.role, p.id, p.reach_before, p.reach_after
                    ));
                    let text = p.words();
                    rec.proposals.insert(0, *p);
                    rec.proposals.truncate(KEPT);
                    self.narrow_save(&rec, vec![row])?;
                    self.rec(None).announce(&fact);
                    self.narrow_card("self_narrow", text);
                }
            }
        }
        Ok(lines)
    }

    fn narrow_account(&self) -> &str {
        self.tools
            .aws
            .as_ref()
            .and_then(|a| a.account(None).ok())
            .map_or("", |a| a.id.as_str())
    }

    fn narrow_region(&self) -> &str {
        self.tools
            .aws
            .as_ref()
            .and_then(|a| a.account(None).ok())
            .map_or("", |a| a.cfg.region.as_str())
    }

    /// The week after an apply: the role's AccessDenied events since it, each
    /// action not told of before one `self.narrow.red` row and a card.
    pub(crate) async fn narrow_watch(&self, env: &impl Env, now_ms: u64) -> Result<Vec<String>> {
        let mut rec = self.narrow_record()?;
        let mut lines = Vec::new();
        let mut rows = Vec::new();
        let mut cards = Vec::new();
        for p in rec.proposals.iter_mut() {
            let Some(at) = p.applied_ms.filter(|_| p.state == State::Applied) else {
                continue;
            };
            if now_ms.saturating_sub(at) > WATCH_MS {
                continue;
            }
            if !self.narrow_allowed() {
                break;
            }
            let days = (now_ms.saturating_sub(at) / DAY_MS + 1).clamp(1, 8) as u32;
            let o = observed_since(
                env,
                &p.role,
                days,
                now_ms / 1000,
                self.cfg.self_improve.narrow_read_pages,
                at / 1000,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
            let new: Vec<(&String, &u64)> = o
                .denied
                .iter()
                .filter(|(a, _)| !p.red.contains(a))
                .collect();
            if new.is_empty() {
                lines.push(format!("{}: no AccessDenied since the apply", p.role));
                continue;
            }
            let first = new[0].0.clone();
            let count: u64 = new.iter().map(|(_, n)| **n).sum();
            let fact = NarrowRed {
                role: p.role.clone(),
                proposal: p.id.clone(),
                denied: first.clone(),
                count,
            };
            rows.push(crate::fact::row(&fact, None, None)?);
            cards.push((
                format!(
                    "{} was denied {} ({} AccessDenied since the owner applied its narrower policy {}). \
                     If a legitimate call was refused, revert with `theseus self narrow revert {}`; \
                     the policy before it comes back through a change set.",
                    p.role, new.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>().join(", "), count, p.id, p.id
                ),
                fact,
            ));
            lines.push(format!("{}: denied {first}", p.role));
            p.red.extend(new.iter().map(|(a, _)| (*a).clone()));
        }
        if !rows.is_empty() {
            self.narrow_save(&rec, rows)?;
            for (text, fact) in cards {
                self.rec(None).announce(&fact);
                self.narrow_card("self_narrow_red", text);
            }
        }
        Ok(lines)
    }

    /// The proposal `name` (an id or the end of one) names, by position.
    fn narrow_find(rec: &Record, name: &str) -> Result<usize> {
        let name = name.trim();
        if name.len() < 4 {
            return Err(bad(
                "name a proposal by its id, or the last four characters of it or more",
            ));
        }
        let hits: Vec<usize> = rec
            .proposals
            .iter()
            .enumerate()
            .filter(|(_, p)| p.id == name || p.id.ends_with(name))
            .map(|(i, _)| i)
            .collect();
        match hits[..] {
            [i] => Ok(i),
            [] => Err(bad(format!(
                "no narrowing proposal {name}: `theseus self narrow` lists them"
            ))),
            _ => Err(bad(format!(
                "{name} names more than one proposal: write more of its id"
            ))),
        }
    }

    /// The owner's "not now": the proposal is closed and the role is looked
    /// at again in `narrow_every_days`.
    pub(crate) fn narrow_decline(&self, name: &str, by: &str, now_ms: u64) -> Result<String> {
        let mut rec = self.narrow_record()?;
        let i = Self::narrow_find(&rec, name)?;
        let p = rec.proposals[i].clone();
        if p.state != State::Open {
            return Err(bad(format!(
                "{} is {:?}, not waiting for an answer",
                p.id, p.state
            )));
        }
        rec.proposals[i].state = State::Declined;
        rec.last.insert(p.role.clone(), now_ms);
        let fact = NarrowDeclined {
            role: p.role.clone(),
            proposal: p.id.clone(),
            by: by.into(),
        };
        self.narrow_save(&rec, vec![crate::fact::row(&fact, None, None)?])?;
        self.rec(None).announce(&fact);
        Ok(format!(
            "Not now: {} stays as it is. It is looked at again in {} days.",
            p.role, self.cfg.self_improve.narrow_every_days
        ))
    }

    /// The owner's apply or revert of a proposal.
    pub(crate) async fn narrow_answer(
        &self,
        env: &(impl Env + Stack),
        action: &str,
        name: &str,
        by: &str,
        now_ms: u64,
    ) -> Result<String> {
        let mut rec = self.narrow_record()?;
        let i = Self::narrow_find(&rec, name)?;
        let p = rec.proposals[i].clone();
        let profile = apply::profile(&p.profile)
            .ok_or_else(|| bad(format!("{} is no hand profile", p.profile)))?;
        let (policy_id, _) = profile.policy();
        let expect = vec![profile.logical.to_string(), policy_id];
        let dir = self.narrow_dir();
        let before = dir.join(format!("{}.before.yaml", p.id));
        match action {
            "apply" => {
                if p.state != State::Open {
                    return Err(bad(format!(
                        "{} is {:?}, not waiting for an answer",
                        p.id, p.state
                    )));
                }
                if !self.narrow_allowed() {
                    bail!("self-improvement is halted or off: the apply waits for it");
                }
                let deployed = env.template().await.map_err(|e| anyhow::anyhow!(e))?;
                let patched = apply::patch(&deployed, profile, &p.candidate).map_err(|e| {
                    anyhow::anyhow!("the deployed template cannot take the proposal any more: {e}")
                })?;
                std::fs::create_dir_all(&dir)?;
                write_private(&before, &deployed)?;
                let done = env
                    .apply(&patched, &expect, &format!("narrow {} ({})", p.role, p.id))
                    .await
                    .map_err(|e| anyhow::anyhow!("not applied: {e}"))?;
                rec.proposals[i].state = State::Applied;
                rec.proposals[i].applied_ms = Some(now_ms);
                rec.proposals[i].change_set = Some(done.change_set.clone());
                let fact = NarrowApplied {
                    role: p.role.clone(),
                    proposal: p.id.clone(),
                    stack: apply::STACK.into(),
                    change_set: done.change_set.clone(),
                    by: by.into(),
                };
                self.narrow_save(&rec, vec![crate::fact::row(&fact, None, None)?])?;
                self.rec(None).announce(&fact);
                Ok(format!(
                    "Applied: {} now holds the narrower policy ({}, change set {}). A week's watch for AccessDenied begins; `theseus self narrow revert {}` brings the old policy back.",
                    p.role, done.status, done.change_set, p.id
                ))
            }
            "revert" => {
                if p.state != State::Applied {
                    return Err(bad(format!(
                        "{} is {:?}: only an applied narrowing is reverted",
                        p.id, p.state
                    )));
                }
                let body = std::fs::read_to_string(&before).map_err(|e| {
                    bad(format!(
                        "the template from before the apply is not at {}: {e}",
                        before.display()
                    ))
                })?;
                let done = env
                    .apply(
                        &body,
                        &expect,
                        &format!("revert the narrowing of {} ({})", p.role, p.id),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("not reverted: {e}"))?;
                rec.proposals[i].state = State::Reverted;
                let fact = NarrowReverted {
                    role: p.role.clone(),
                    proposal: p.id.clone(),
                    stack: apply::STACK.into(),
                    change_set: done.change_set.clone(),
                    by: by.into(),
                };
                self.narrow_save(&rec, vec![crate::fact::row(&fact, None, None)?])?;
                self.rec(None).announce(&fact);
                Ok(format!(
                    "Reverted: {} holds the policy it had before ({}, change set {}).",
                    p.role, done.status, done.change_set
                ))
            }
            other => Err(bad(format!("`{other}` is no answer: apply or revert"))),
        }
    }

    /// `list` and `show`: reads, anyone's.
    fn narrow_read(&self, action: &str, name: &str) -> Result<SelfNarrowResult> {
        let rec = self.narrow_record()?;
        let mut text = String::new();
        if action == "show" {
            let pr = &rec.proposals[Self::narrow_find(&rec, name)?];
            text = format!(
                "{}\nThe change as JSON:\n{}\n",
                pr.words(),
                serde_json::to_string_pretty(&pr.diff_json())?
            );
        } else {
            for p in &rec.proposals {
                text.push_str(&format!(
                    "{}  {:<9} {}  about {} actions to {} ({} days of evidence)\n",
                    p.id,
                    item(p).state,
                    p.role,
                    p.reach_before,
                    p.reach_after,
                    p.days
                ));
            }
            if text.is_empty() {
                text.push_str("No narrowing proposals yet.\n");
            }
        }
        Ok(SelfNarrowResult {
            text,
            proposals: rec.proposals.iter().map(item).collect(),
            state: self.self_state(),
        })
    }

    /// `run`: look at the roles now, the weekly cadence aside.
    async fn narrow_run(&self, profile: Option<&str>, dry: bool, now: u64) -> Result<String> {
        // A dry look reads AWS and keeps and posts nothing, so the gate that
        // holds self-directed work does not hold the owner's own look.
        match gate(&self.self_ctx()) {
            _ if dry => {}
            Gate::Allowed => {}
            Gate::Off => bail!("self-improvement is off: set [self] mode = \"act\" first"),
            Gate::Halted { why, .. } => bail!("self-improvement is halted: {why}"),
        }
        let only =
            match profile {
                Some(n) => Some(apply::profile(n).filter(|p| p.name != "basic").ok_or_else(
                    || {
                        bad(format!(
                            "`{n}` is not a profile with a policy to narrow: owner or read"
                        ))
                    },
                )?),
                None => None,
            };
        let live = self.narrow_live()?;
        let universe = crate::aws::narrow::live::universe();
        Ok(self
            .narrow_pass(&live, &universe, now, only, true, dry)
            .await?
            .join("\n"))
    }

    /// `self.narrow`.
    pub async fn self_narrow(
        &self,
        p: &SelfNarrowParams,
        who: &Answerer,
    ) -> Result<SelfNarrowResult> {
        let action = if p.action.is_empty() {
            "list"
        } else {
            p.action.as_str()
        };
        let name = p.proposal.as_deref().unwrap_or("");
        match action {
            "list" | "show" => self.narrow_read(action, name),
            "run" | "apply" | "decline" | "revert" => {
                let what = p
                    .proposal
                    .as_deref()
                    .or(p.profile.as_deref())
                    .unwrap_or(action);
                let act = Act::SelfNarrow {
                    method: theseus_protocol::method::SELF_NARROW,
                    what,
                    from_job: p.from_job.as_deref(),
                };
                self.judge_act(who, act)?;
                let now = theseus_protocol::now_unix_ms();
                let _one = one().lock().await;
                let text = match action {
                    "decline" => self.narrow_decline(name, &who.who(), now)?,
                    "run" => self.narrow_run(p.profile.as_deref(), p.dry, now).await?,
                    _ => {
                        let live = self.narrow_live()?;
                        self.narrow_answer(&live, action, name, &who.who(), now)
                            .await?
                    }
                };
                Ok(SelfNarrowResult {
                    text: text + "\n",
                    proposals: self.narrow_record()?.proposals.iter().map(item).collect(),
                    state: self.self_state(),
                })
            }
            other => Err(bad(format!(
                "`{other}` is no action: list, show, run, apply, decline or revert"
            ))),
        }
    }
}

/// A file only the daemon's user reads.
fn write_private(path: &std::path::Path, body: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(body.as_bytes())?;
    Ok(())
}
