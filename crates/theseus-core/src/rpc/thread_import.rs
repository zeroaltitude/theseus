//! `thread.import` (theseus-71ib, T11): candidate threads from the owner's
//! earlier harness, and the apply that makes the ticked ones.
//!
//! - **One method, two halves.** With no `apply` it is the dry run: the scan
//!   of the imported episodes (`threads_import`), on the blocking pool, and
//!   no write. With `apply` it makes the named candidates, one at a time:
//!   the card's model call, `thread.new`, `thread.edit` (the card, the
//!   working set, the persona), and one `thread.imported` row with the day's
//!   import spend in one frame. A candidate is looked up by slug in a fresh
//!   scan, so a plan file stays good while the history grows.
//! - **The owner's act**, from a private place (`judge_act(Act::Thread)`, as
//!   every `thread.*` is), refused inside a job by the CLI.
//! - **The card is best effort and budgeted.** A call that would pass
//!   `[threads] import_day_usd` with its worst case is not made, nor one the
//!   daemon's day ceiling refuses; the thread is made without a card and the
//!   result says why. A model that fails leaves the same.
//! - **Nothing of the history enters the thread.** No node is written into
//!   any session; the card is a derived summary on the thread's record,
//!   citing the episodes by handle.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, Micros};
use theseus_protocol::method;
use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams};
use theseus_protocol::threads_import::{
    ThreadApplied, ThreadImportParams, ThreadImportResult, ThreadSkipped, MIN_CLUSTER, SINCE_DAYS,
};
use theseus_store::{kinds, NewRecord};

use super::import::blocking;
use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::judge::spend;
use crate::threads_import::candidates::{candidates, Draft, Opts};
use crate::threads_import::{card, source};

/// The day's import spend, a META record: `day` is the local day it counts.
pub(super) const DAY_KEY: &str = "threads.import.day";
const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct ImportDay {
    pub(super) day: String,
    pub(super) spent_usd: f64,
}

/// What a card's call came to.
#[derive(Default)]
struct Carded {
    lines: Vec<String>,
    cited: Vec<Value>,
    dropped: u32,
    /// The profile the call ran on (empty when none was resolved).
    profile: String,
    model: String,
    cost: Micros,
    input_tokens: u64,
    output_tokens: u64,
    failed: Option<String>,
}

impl Core {
    /// `thread.import` as the dispatcher calls it.
    pub(super) async fn rpc_thread_import(
        self: Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p: ThreadImportParams = parse(params)?;
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        let r = self.thread_import(&p, who).await?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    pub async fn thread_import(
        self: &Arc<Self>,
        p: &ThreadImportParams,
        who: Answerer,
    ) -> Result<ThreadImportResult, RpcFailure> {
        self.judge_thread(&who, method::THREAD_IMPORT, "import")?;
        self.threads_on()?;
        let from = p.from.trim().to_lowercase();
        if from.is_empty()
            || !from
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(RpcFailure::invalid(anyhow!(
                "--from names a source: lower-case letters, digits and hyphens (`openclaw`)"
            )));
        }
        let since_days = p.since_days.unwrap_or(SINCE_DAYS).clamp(1, 3650);
        let min_cluster = p.min_cluster.unwrap_or(MIN_CLUSTER).max(1);
        let now = theseus_protocol::now_unix_ms();
        let existing: BTreeSet<String> = self.threads().all().map(|t| t.slug.clone()).collect();
        let personas: BTreeSet<String> = self.personas().keys().into_iter().collect();
        let core = self.clone();
        let scan_from = from.clone();
        let plan = blocking(move || {
            let since = now.saturating_sub(u64::from(since_days) * DAY_MS);
            let seen = source::scan(&core.store, &scan_from, since)?;
            Ok(candidates(
                &seen,
                &Opts {
                    now_ms: now,
                    since_days,
                    min_cluster,
                    personas: &personas,
                    existing: &existing,
                },
            ))
        })
        .await?;
        let cap_usd = self.cfg.threads.import_day_usd;
        let today = spend::local_day(now);
        let mut day = self.import_day(&today);
        let mut result = ThreadImportResult {
            from: from.clone(),
            since_days,
            min_cluster,
            candidates: plan.drafts.iter().map(|d| d.cand.clone()).collect(),
            applied: Vec::new(),
            skipped: Vec::new(),
            spent_usd: 0.0,
            spent_today_usd: day.spent_usd,
            cap_usd,
            notes: plan.notes.clone(),
        };
        let Some(slugs) = &p.apply else {
            return Ok(result);
        };
        let mut done = BTreeSet::new();
        for slug in slugs {
            let slug = slug.trim().to_string();
            if !done.insert(slug.clone()) {
                continue;
            }
            let skip = |why: String| ThreadSkipped {
                slug: slug.clone(),
                why,
            };
            let Some(draft) = plan.drafts.iter().find(|d| d.cand.slug == slug) else {
                result.skipped.push(skip(
                    "no such candidate in this scan: run the dry run again".into(),
                ));
                continue;
            };
            if draft.cand.exists || self.threads().get(&slug).is_some() {
                result
                    .skipped
                    .push(skip("a thread of this slug exists".into()));
                continue;
            }
            if self.threads().open() >= self.cfg.threads.max_live as usize {
                result.skipped.push(skip(format!(
                    "{} threads are open ([threads] max_live): finish one first",
                    self.cfg.threads.max_live
                )));
                continue;
            }
            match self.import_one(draft, &from, &who, &mut day).await {
                Ok(applied) => {
                    result.spent_usd += applied.cost_usd;
                    result.applied.push(applied);
                }
                Err(e) => result.skipped.push(skip(e.message)),
            }
            result.spent_today_usd = day.spent_usd;
        }
        Ok(result)
    }

    fn import_day(&self, today: &str) -> ImportDay {
        self.store
            .get_meta::<ImportDay>(DAY_KEY)
            .ok()
            .flatten()
            .filter(|d| d.day == today)
            .unwrap_or_else(|| ImportDay {
                day: today.to_string(),
                spent_usd: 0.0,
            })
    }

    /// One thread: the card's call, the thread, the card on it, the row.
    async fn import_one(
        &self,
        draft: &Draft,
        from: &str,
        who: &Answerer,
        day: &mut ImportDay,
    ) -> Result<ThreadApplied, RpcFailure> {
        let c = &draft.cand;
        let episodes = card::episodes(&draft.inputs);
        let carded = match episodes.is_empty() {
            true => Carded {
                failed: Some("no episode text to read (all partner-confidential, or empty)".into()),
                ..Carded::default()
            },
            false => self.import_card(&c.title, &episodes, day).await,
        };
        let cost_usd = micros_to_usd(carded.cost);
        day.spent_usd += cost_usd;
        let made = self.thread_new(
            &ThreadNewParams {
                slug: c.slug.clone(),
                title: Some(c.title.clone()),
                topics: (!c.topics.is_empty()).then(|| c.topics.clone()),
                ..Default::default()
            },
            who.clone(),
        );
        let made = made.and_then(|_| {
            self.thread_edit(
                &ThreadEditParams {
                    slug: c.slug.clone(),
                    working_set: (!c.working_set.is_empty()).then(|| c.working_set.clone()),
                    card: (!carded.lines.is_empty()).then(|| carded.lines.clone()),
                    persona: c.persona.clone(),
                    ..Default::default()
                },
                who.clone(),
            )
        });
        let profile = match carded.profile.as_str() {
            "" => self.cfg.threads.import_profile.clone().unwrap_or_default(),
            p => p.to_string(),
        };
        let f = crate::fact::thread_import::ThreadImported {
            slug: &c.slug,
            from,
            profile: &profile,
            model: &carded.model,
            cost_usd,
            input_tokens: carded.input_tokens,
            output_tokens: carded.output_tokens,
            lines: carded.lines.len() as u32,
            dropped: carded.dropped,
            cited: &carded.cited,
            failed: carded.failed.as_deref(),
        };
        // The call was paid for whether or not the thread could be made.
        if carded.cost > 0 || made.is_ok() {
            let row = crate::fact::row(&f, None, None).map_err(RpcFailure::invalid)?;
            let mark =
                NewRecord::json(kinds::META, Some(DAY_KEY), day).map_err(RpcFailure::invalid)?;
            self.people_write(&[row, mark])
                .await
                .map_err(RpcFailure::invalid)?;
            self.rec(None).announce(&f);
        }
        made?;
        Ok(ThreadApplied {
            slug: c.slug.clone(),
            card: carded.lines,
            cited: carded
                .cited
                .iter()
                .filter_map(|c| c["handle"].as_str().map(str::to_string))
                .collect(),
            cost_usd,
            card_failed: carded.failed,
        })
    }

    /// The model a card over `episodes` episodes is written on: `[threads]
    /// import_profile` when it is set (a pin), else Jev's choice for a job over
    /// the owner's conversations (route.v10, theseus-ej6bj).
    pub(crate) async fn import_model(&self, episodes: u32) -> Result<crate::turn::Target, String> {
        let (live, _) = self.live_profile();
        let site = crate::job_model::Site {
            name: "threads",
            pin: self.cfg.threads.import_profile.as_deref(),
            legacy: Some(crate::config::legacy::CHEAP),
            job: theseus_judge::builders::JobKind::Threads { episodes },
            restricted: theseus_judge::builders::RestrictedModels::NotAllowed,
        };
        match self.runner.job_target(&live, site).await {
            Ok((t, _)) => Ok(t),
            Err(e) => Err(format!("[threads] import_profile {e}")),
        }
    }

    /// The card's one model call, under the day's import line and the
    /// daemon's day ceiling.
    async fn import_card(
        &self,
        title: &str,
        episodes: &[card::Episode],
        day: &ImportDay,
    ) -> Carded {
        let fail = |why: String| Carded {
            failed: Some(why),
            ..Carded::default()
        };
        let target = match self.import_model(episodes.len() as u32).await {
            Ok(t) => t,
            Err(why) => return fail(why),
        };
        let profile = target.profile.clone();
        let Some(provider) = self.runner.providers.get(&target.provider).cloned() else {
            return fail(format!("no provider {:?} for {profile}", target.provider));
        };
        let scrubber = self.tools.scrubber.clone();
        let how = self.runner.catalog.forcing(&target.model);
        let request = card::request(&target.model, how, title, episodes, &|t| {
            scrubber.scrub(t).0
        });
        let Some(need) = self.audit_reserve(&request) else {
            return fail(format!("no price for {}", target.model));
        };
        let cap = self.cfg.threads.import_day_usd;
        let left = (cap - day.spent_usd).max(0.0);
        if micros_to_usd(need) > left {
            return fail(format!(
                "the day's import budget (${cap:.2}) leaves ${left:.2}, under this card's worst case (${:.2})",
                micros_to_usd(need)
            ));
        }
        let hold = match self.runner.day_hold(need, "thread-import") {
            Ok(h) => h,
            Err(why) => return fail(why),
        };
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(&request, &mut quiet).await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &target.model, need));
        hold.settle(cost);
        let mut out = Carded {
            profile: profile.clone(),
            model: target.model.clone(),
            cost,
            ..Carded::default()
        };
        let resp = match answered {
            Ok(r) => r,
            Err(e) => {
                out.failed = Some(format!("the card's call failed: {e:#}"));
                return out;
            }
        };
        out.model = resp.model.clone();
        out.input_tokens = resp.usage.input_tokens;
        out.output_tokens = resp.usage.output_tokens;
        if crate::provider::hit_cap(resp.stop_reason.as_deref()) {
            out.failed = Some(crate::provider::cap_phrase(card::MAX_TOKENS));
            return out;
        }
        match card::parse(&resp.content, episodes) {
            Some(parsed) if !parsed.lines.is_empty() => {
                let (lines, cited) = card::render(&parsed.lines, episodes);
                out.lines = lines;
                out.dropped = parsed.dropped;
                out.cited = cited
                    .iter()
                    .map(|c| json!({"handle": c.handle, "node": c.node, "session": c.session_id}))
                    .collect();
            }
            Some(parsed) => {
                out.dropped = parsed.dropped;
                out.failed = Some(format!(
                    "the answer held no cited line ({} dropped)",
                    parsed.dropped
                ));
            }
            None => out.failed = Some("the answer held no write_card call".into()),
        }
        out
    }
}
