//! The backfill (theseus-wy7y): `theseus import people TAG --propose`, the
//! owner's act. Over the tag's live imported sessions, in id order, each one
//! with human-facing text is one pass (`run.rs`): the extraction and Jev's
//! judgments, proposals only, never memberships. A session with none is
//! skipped. Paced by the machine's quiet between sessions, the daemon's stop
//! looked for there; resumable from the tag's META mark
//! (`import.people.propose.<tag>`, the last session passed), which moves in
//! each pass's own frame; and under a spend cap for the run (`cap_usd`,
//! default $5): a session whose worst case would pass it is not begun, and
//! the run says how to go on. `dry_run` reads the sessions and prices them,
//! the extractor at its profile's price and Jev at about $0.042 per million
//! tokens, with no call.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use theseus_judge::price::Micros;
use theseus_kernel::{micros_to_usd, usd_to_micros};
use theseus_protocol::import::{ImportPeopleParams, ImportPeopleResult, PeopleProposeReport};

use super::lines;
use super::run::{meta, Session};
use crate::import::tag_scope;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The tag's mark's key prefix.
pub const MARK_PREFIX: &str = "import.people.propose.";
/// The run's cap when the owner names none.
pub const CAP_USD: f64 = 5.0;
/// Jev's price, per million state tokens.
pub const JEV_USD_PER_MTOK: f64 = 0.042;
/// What the projection assumes a session gives Jev: candidates, and each
/// one's state.
pub const EST_CANDIDATES: u64 = 4;
pub const EST_STATE_TOKENS: u64 = 1500;
/// The extractor's answer the projection assumes (its worst case is
/// `extract::MAX_TOKENS`, which the cap holds against).
pub const EST_OUTPUT_TOKENS: u32 = 300;

/// The tag's mark.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TagMark {
    /// The last session passed (sessions go in id order).
    pub after: String,
    pub sessions: u64,
    pub at_ms: u64,
}

/// Jev's projected cost of one session.
fn jev_projected() -> Micros {
    usd_to_micros(JEV_USD_PER_MTOK * (EST_CANDIDATES * EST_STATE_TOKENS) as f64 / 1e6)
}

/// A pass's counts, added to the run's.
fn tally(r: &mut PeopleProposeReport, pass: &super::run::Pass) {
    r.candidates += pass.candidates;
    r.excluded += pass.excluded;
    r.judged += pass.judged;
    r.failed += u64::from(pass.failed);
}

/// The words of a stop at the cap, and how to go on.
fn at_cap(worst: Micros, spent: Micros, cap_usd: f64, left: u64, tag: &str) -> String {
    format!(
        "stopped at the cap: the next session's worst case ({}) would pass ${cap_usd:.2} with {} \
         spent; {left} sessions left. Go on from the tag's mark: theseus import people {tag} \
         --propose --cap <dollars>",
        crate::narrative::dollars(worst),
        crate::narrative::dollars(spent),
    )
}

/// A run's state between its sessions.
struct Run {
    r: PeopleProposeReport,
    spent: Micros,
    projected: Micros,
    passed: u64,
    known: std::collections::HashMap<String, std::collections::HashSet<String>>,
    price: crate::catalog::CatalogEntry,
    model: String,
    cap: Micros,
}

type Plan = (
    Option<TagMark>,
    Vec<(String, String)>,
    std::collections::HashMap<String, std::collections::HashSet<String>>,
);

impl Core {
    /// The tag's mark, its live imported sessions in id order with their
    /// titles, and the names each was asked about before.
    fn propose_plan(&self, tag: &str, key: &str) -> anyhow::Result<Plan> {
        let mark: Option<TagMark> = self.store.get_meta(key)?;
        let mut sessions: Vec<(String, String)> = Vec::new();
        for r in self.store.scope_after(&tag_scope(tag), 0)? {
            if r.kind != theseus_store::kinds::SESSION {
                continue;
            }
            let rec: SessionRecord = r.decode()?;
            if rec.imported.as_ref().is_some_and(|i| i.erased.is_none()) {
                sessions.push((
                    rec.session_id.clone(),
                    rec.title.clone().unwrap_or_default(),
                ));
            }
        }
        sessions.sort();
        sessions.dedup_by(|a, b| a.0 == b.0);
        Ok((mark, sessions, self.people_known()?))
    }

    /// `import.people { propose }`.
    pub async fn import_people_propose(
        self: &Arc<Self>,
        p: &ImportPeopleParams,
    ) -> anyhow::Result<ImportPeopleResult> {
        let t0 = std::time::Instant::now();
        let cap_usd = p.cap_usd.unwrap_or(CAP_USD);
        if !(cap_usd.is_finite() && cap_usd > 0.0) {
            anyhow::bail!("--cap is a positive number of dollars, not {cap_usd}");
        }
        let cap = usd_to_micros(cap_usd);
        let key = format!("{MARK_PREFIX}{}", p.tag);
        let (target, _) = self.people_extractor().map_err(anyhow::Error::msg)?;
        let price = self
            .runner
            .catalog
            .get(&target.model)
            .ok_or_else(|| anyhow::anyhow!("no price for {}", target.model))?
            .clone();
        let (tag, core) = (p.tag.clone(), self.clone());
        let (mark, sessions, known) =
            theseus_store::blocking(move || core.propose_plan(&tag, &key))?;
        let mut run = Run {
            r: PeopleProposeReport {
                sessions: sessions.len() as u64,
                profile: target.profile.clone(),
                model: target.model.clone(),
                cap_usd,
                ..PeopleProposeReport::default()
            },
            spent: 0,
            projected: 0,
            passed: mark.as_ref().map_or(0, |m| m.sessions),
            known,
            price,
            model: target.model.clone(),
            cap,
        };
        let after = mark.map(|m| m.after).unwrap_or_default();
        let todo: Vec<&(String, String)> = sessions.iter().filter(|(s, _)| *s > after).collect();
        run.r.done_before = run.r.sessions - todo.len() as u64;
        for (i, (sid, title)) in todo.iter().enumerate() {
            if i > 0 && !p.dry_run {
                theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await;
            }
            let left = (todo.len() - i) as u64;
            let stop = match self.outbox.stopping() {
                true => Some(format!(
                    "the daemon is stopping: {left} sessions left; run the same command again to \
                     go on from the tag's mark"
                )),
                false => self.propose_one(p, sid, title, left, &mut run).await?,
            };
            if let Some(why) = stop {
                (run.r.left, run.r.stopped) = (left, Some(why));
                break;
            }
        }
        let mut r = run.r;
        r.spent_usd = micros_to_usd(run.spent);
        r.projected_usd = micros_to_usd(run.projected);
        Ok(ImportPeopleResult {
            tag: p.tag.clone(),
            dry_run: p.dry_run,
            sessions: r.sessions,
            ms: t0.elapsed().as_secs_f64() * 1000.0,
            propose: Some(r),
            ..ImportPeopleResult::default()
        })
    }

    /// One session of the run: priced (a dry run), or passed under the cap
    /// with the tag's mark moved in its frame. `Some` says why the run stops.
    async fn propose_one(
        &self,
        p: &ImportPeopleParams,
        sid: &str,
        title: &str,
        left: u64,
        run: &mut Run,
    ) -> anyhow::Result<Option<String>> {
        let id = sid.to_string();
        let nodes = theseus_store::blocking(|| self.store.session_nodes(&id))?;
        let found = lines(&nodes);
        let request = self.people_request(&run.model, title, &found);
        let est = crate::compiler::estimate(&request, run.price.bytes_per_token, None).tokens;
        let r = &mut run.r;
        match found.is_empty() {
            true => r.no_text += 1,
            false => (r.tokens, r.read) = (r.tokens + est, r.read + 1),
        }
        if p.dry_run {
            if !found.is_empty() {
                run.projected += run.price.reserve_micros(EST_OUTPUT_TOKENS, est) + jev_projected();
            }
            return Ok(None);
        }
        let mark = TagMark {
            after: sid.to_string(),
            sessions: run.passed + 1,
            at_ms: theseus_protocol::now_unix_ms(),
        };
        let mark = meta(&format!("{MARK_PREFIX}{}", p.tag), &mark)?;
        if found.is_empty() {
            self.people_write(&[mark]).await?;
            run.passed += 1;
            return Ok(None);
        }
        let worst = run.price.reserve_micros(super::extract::MAX_TOKENS, est) + jev_projected();
        if run.spent.saturating_add(worst) > run.cap {
            (r.read, r.tokens) = (r.read - 1, r.tokens - est);
            return Ok(Some(at_cap(worst, run.spent, r.cap_usd, left, &p.tag)));
        }
        let purpose = format!("backfill:{}", p.tag);
        let known = run.known.remove(sid).unwrap_or_default();
        let session = Session {
            sid,
            title,
            lines: found,
            purpose: &purpose,
            mark: Some(mark),
            known,
        };
        match self.propose_people(session).await {
            Ok(pass) => {
                run.passed += 1;
                run.spent += pass.spent;
                tally(&mut run.r, &pass);
                Ok(None)
            }
            Err(why) => Ok(Some(format!(
                "stopped: {why}; {left} sessions left, from the tag's mark"
            ))),
        }
    }
}
