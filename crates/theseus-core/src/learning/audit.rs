//! The audit (M5 25d; design §2.6, §2.9): a model profile answers a pack's
//! questions over a seeded sample of its answered judgments that carry no
//! audit label yet, each answer an audit label (`judge.label`, `source:
//! audit`, weight 0.5, keyed by judgment, question, and run).
//!
//! - **One request per state**, through the profile's `Provider`, outside
//!   any session: the judgment's state (its blob, as Jev read it) and each
//!   question's instructions and criteria as Jev reads them (a Choice's
//!   options and what each means, a Score's levels, a Noul's `when_true` and
//!   `when_false`), answered as one JSON object, one value per question id.
//!   An answer outside a question's options (or a question the pack does
//!   not ask) is counted and dropped.
//! - **Priced from the catalog, reserved and settled**: each request
//!   reserves its output cap and its input estimate at the model's prices
//!   before it is sent, and settles at what its usage cost (its reservation,
//!   when it failed). The run stops before a request would pass `[judge]
//!   audit_limit_usd`.
//! - **Off the low thread**: the run reads and writes on its `learning`
//!   thread at nice 19; each request is sent on the runtime and waited for
//!   there, so no pool thread it starts takes that thread's priority
//!   (theseus-bgg5).
//! - **Written once**: the run's labels and its `judge.audit` row in one
//!   frame, scoped `judge:<pack id>`, so the report counts them. A run that
//!   asked nothing (every judgment audited already) writes nothing.

use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::{json, Map, Value};
use theseus_judge::client::Kind;
use theseus_judge::pack::{Pack, QuestionDef};
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_protocol::judge_runs::{JudgeAuditParams, JudgeAuditResult};
use theseus_store::NewRecord;

use super::labels::system_key;
use super::{read_scope, Seen, SYSTEM_WEIGHT};
use crate::fact;
use crate::provider::ProviderRequest;
use crate::rpc::Core;

/// What the audit model is told, before the state and the questions.
pub const INSTRUCTIONS: &str = "You audit an automated judge. Read the state, a JSON object that \
describes one event, and answer each question about it. Reply with one JSON object and nothing \
else: a key for each question id, each value as that question says.";

/// An audit's answer most: a few words per question.
pub const MAX_TOKENS: u32 = 1024;

/// A pack version by name, or the wired version of an id.
pub fn pack_named(name: &str) -> anyhow::Result<Arc<Pack>> {
    let name = name.trim();
    let full = if name.contains('.') {
        name.to_string()
    } else {
        crate::judge::WIRED
            .iter()
            .map(|(n, _)| *n)
            .find(|n| n.split('.').next() == Some(name))
            .with_context(|| format!("this build wires no version of {name}"))?
            .to_string()
    };
    theseus_judge::pack::by_name(&full).with_context(|| format!("this build has no pack {full}"))
}

/// The questions an audit asks: the pack's whole ones whose options are its
/// own (a dynamic source's items are not in the record).
fn asked(pack: &Pack) -> Vec<&QuestionDef> {
    pack.questions
        .iter()
        .filter(|q| q.per.is_none() && q.options_from.is_none() && q.only_when.is_none())
        .collect()
}

/// One question as the audit model reads it: Jev's instructions and
/// criteria, and the form of its answer.
fn question_text(q: &QuestionDef) -> String {
    match q.kind {
        Kind::Choice => {
            let options: Vec<String> = q
                .options
                .iter()
                .map(|o| format!("\"{}\" ({})", o.id, o.means.as_deref().unwrap_or_default()))
                .collect();
            format!(
                "- \"{}\": {} Answer one option id, as a string: {}.",
                q.id,
                q.instructions,
                options.join("; ")
            )
        }
        Kind::Noul => {
            let criteria = match (&q.when_true, &q.when_false) {
                (Some(t), Some(f)) => format!(" True when: {t} False when: {f}"),
                _ => String::new(),
            };
            format!(
                "- \"{}\": {} Answer true or false.{criteria}",
                q.id, q.instructions
            )
        }
        Kind::Score => {
            let levels: Vec<String> = q
                .levels
                .iter()
                .enumerate()
                .map(|(i, l)| format!("{i} = {l}"))
                .collect();
            format!(
                "- \"{}\": {} Answer a level's number: {}.",
                q.id,
                q.instructions,
                levels.join("; ")
            )
        }
    }
}

/// The request for one state.
pub fn request_for(pack: &Pack, state: &str, model: &str, max_tokens: u32) -> ProviderRequest {
    let questions: Vec<String> = asked(pack).into_iter().map(question_text).collect();
    let content = format!(
        "State:\n{state}\n\nQuestions:\n{}\n\nReply with one JSON object.",
        questions.join("\n")
    );
    ProviderRequest {
        model: model.to_string(),
        max_tokens: max_tokens.min(MAX_TOKENS),
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: Vec::new(),
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// The answer's JSON object: the reply's text from its first `{` to its
/// last `}`.
fn object_in(text: &str) -> Option<Map<String, Value>> {
    let (a, b) = (text.find('{')?, text.rfind('}')?);
    serde_json::from_str(text.get(a..=b)?).ok()
}

/// Each answer that is one of its question's: `(question, label)`, and how
/// many were dropped.
pub fn labels_of(pack: &Pack, text: &str) -> (Vec<(String, Value)>, u32) {
    let Some(obj) = object_in(text) else {
        return (Vec::new(), asked(pack).len() as u32);
    };
    let mut out = Vec::new();
    let mut dropped = 0;
    for (id, v) in obj {
        let label = asked(pack)
            .into_iter()
            .find(|q| q.id == id)
            .and_then(|q| match q.kind {
                Kind::Choice => v
                    .as_str()
                    .filter(|s| q.options.iter().any(|o| o.id == *s))
                    .map(|s| json!(s)),
                Kind::Noul => v.as_bool().map(|b| json!(b)),
                Kind::Score => v
                    .as_u64()
                    .filter(|n| (*n as usize) < q.levels.len())
                    .map(|n| json!(n)),
            });
        match label {
            Some(l) => out.push((id, l)),
            None => dropped += 1,
        }
    }
    (out, dropped)
}

/// The sample: the judgments ordered by the sha256 of the seed and each id,
/// the first `n`. The same seed draws the same sample.
pub fn sample(seen: &[Seen], seed: u64, n: usize) -> Vec<&Seen> {
    use sha2::Digest;
    let mut keyed: Vec<(Vec<u8>, &Seen)> = seen
        .iter()
        .map(|s| {
            let mut h = sha2::Sha256::new();
            h.update(seed.to_be_bytes());
            h.update(s.judgment.id.as_bytes());
            (h.finalize().to_vec(), s)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().take(n).map(|(_, s)| s).collect()
}

/// One request, sent on the runtime and waited for here: a blocking pool
/// thread it starts (a host name's lookup) is a worker's child, never this
/// low thread's, whose nice value it would keep for life (theseus-bgg5).
/// The reading of the answer stays on the caller's thread.
fn send_on_runtime(
    rt: &tokio::runtime::Handle,
    provider: &Arc<dyn crate::provider::Provider>,
    request: ProviderRequest,
) -> anyhow::Result<crate::provider::ModelResponse> {
    let provider = provider.clone();
    rt.block_on(rt.spawn(async move {
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        provider.stream_message(&request, &mut quiet).await
    }))
    .map_err(anyhow::Error::from)
    .and_then(|r| r)
}

/// One label the run writes.
struct Labeled {
    id: String,
    judgment: String,
    session: Option<String>,
    question: String,
    label: Value,
}

impl Core {
    /// `judge.audit`: the owner's run, from a private place
    /// (`judge_act(Act::JudgeRun)`), on a thread of its own at low priority.
    pub async fn judge_audit(
        self: &Arc<Self>,
        p: JudgeAuditParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<JudgeAuditResult> {
        let who = who.into();
        if !self.cfg.judge.enabled {
            bail!("the judge is off ([judge] enabled = false): nothing is audited");
        }
        let pack = pack_named(&p.pack)?;
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&p.profile), None, None)
            .with_context(|| format!("the profile {:?} does not resolve", p.profile))?;
        if self.runner.catalog.get(&target.model).is_none() {
            bail!(
                "{} has no price in the catalog: an audit is priced before it is sent",
                target.model
            );
        }
        let what = format!("audit of {} with {}", pack.name(), p.profile);
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::JUDGE_AUDIT,
                what: &what,
            },
        )?;
        let core = Arc::downgrade(self);
        let rt = tokio::runtime::Handle::current();
        let (who_s, via) = (who.who(), who.via());
        let rx = super::tender::on_low_thread(move || match core.upgrade() {
            Some(c) => c.audit_run(&rt, &p, &pack, &target, &who_s, &via),
            None => Err(anyhow::anyhow!("the daemon stopped before the audit ran")),
        })?;
        rx.await.context("the audit's thread ended")?.1
    }

    /// What one request reserves at the model's prices.
    pub(crate) fn audit_reserve(&self, request: &ProviderRequest) -> Option<Micros> {
        let price = self.runner.catalog.get(&request.model)?;
        let est = crate::compiler::estimate(request, price.bytes_per_token, None);
        Some(price.reserve_micros(request.max_tokens, est.tokens))
    }

    #[allow(clippy::too_many_arguments)]
    fn audit_run(
        &self,
        rt: &tokio::runtime::Handle,
        p: &JudgeAuditParams,
        pack: &Pack,
        target: &crate::turn::Target,
        who: &str,
        via: &str,
    ) -> anyhow::Result<JudgeAuditResult> {
        let _rt = rt.enter();
        let id = crate::new_id("aud");
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .with_context(|| format!("the provider {:?} is not configured", target.provider))?;
        let mut scope = read_scope(&self.store, &pack.id)?;
        let audited = |s: &Seen| {
            scope
                .labels
                .get(&s.judgment.id)
                .is_some_and(|ls| ls.iter().any(|l| l.source == "audit"))
        };
        let all = scope.judgments.remove(&pack.name()).unwrap_or_default();
        let eligible: Vec<Seen> = all
            .into_iter()
            .filter(|s| s.judgment.outcome == theseus_judge::Outcome::Answered && !audited(s))
            .collect();
        let seed = p.seed.unwrap_or_else(|| {
            use sha2::Digest;
            let h = sha2::Sha256::digest(id.as_bytes());
            u64::from_be_bytes(h[..8].try_into().expect("8 bytes"))
        });
        let picked = sample(&eligible, seed, p.sample as usize);
        let limit = usd_to_micros(self.cfg.judge.audit_limit_usd);
        let mut r = JudgeAuditResult {
            id: id.clone(),
            pack: pack.name(),
            profile: p.profile.clone(),
            model: target.model.clone(),
            seed,
            eligible: eligible.len() as u32,
            sampled: picked.len() as u32,
            limit_usd: micros_to_usd(limit),
            ..Default::default()
        };
        let (mut spent, mut labels): (Micros, Vec<Labeled>) = (0, Vec::new());
        for s in picked {
            let Some(state) = s.judgment.context["blob"]
                .as_str()
                .and_then(|d| self.store.blobs().base64(d))
                .and_then(|b| crate::blobs::decode(&b).ok())
                .and_then(|b| String::from_utf8(b).ok())
            else {
                r.failed += 1;
                continue;
            };
            let request = request_for(pack, &state, &target.model, target.max_tokens);
            let need = self.audit_reserve(&request).unwrap_or(u64::MAX);
            if spent.saturating_add(need) > limit {
                r.stopped = Some(format!(
                    "the next request would reserve {}, past [judge] audit_limit_usd ({}) with \
                     {} spent",
                    crate::narrative::dollars(need),
                    crate::narrative::dollars(limit),
                    crate::narrative::dollars(spent)
                ));
                break;
            }
            r.asked += 1;
            let sent = send_on_runtime(rt, &provider, request);
            match sent {
                Ok(resp) => {
                    spent += self
                        .runner
                        .catalog
                        .get(&resp.model)
                        .or_else(|| self.runner.catalog.get(&target.model))
                        .map_or(need, |e| e.cost_micros(&resp.usage));
                    let (got, dropped) = labels_of(pack, &resp.text);
                    r.dropped += dropped;
                    for (question, label) in got {
                        labels.push(Labeled {
                            id: system_key(&s.judgment.id, &question, &id),
                            judgment: s.judgment.id.clone(),
                            session: s.context("session").map(str::to_string),
                            question,
                            label,
                        });
                    }
                }
                Err(e) => {
                    // A failed request may have been billed: it is booked
                    // at its reservation.
                    spent += need;
                    r.failed += 1;
                    tracing::warn!(error = %format!("{e:#}"), judgment = %s.judgment.id, "audit: a request failed");
                }
            }
        }
        r.labels = labels.len() as u32;
        r.cost_usd = micros_to_usd(spent);
        if r.asked > 0 {
            self.write_audit(pack, &r, &labels, who, via)?;
        }
        Ok(r)
    }

    /// The run's frame: its labels and its `judge.audit` row.
    fn write_audit(
        &self,
        pack: &Pack,
        r: &JudgeAuditResult,
        labels: &[Labeled],
        who: &str,
        via: &str,
    ) -> anyhow::Result<()> {
        let scope = crate::rpc::judge::scope_of(&pack.id);
        let auditor = format!("audit:{}", r.profile);
        let mut records: Vec<NewRecord> = Vec::new();
        for l in labels {
            let f = fact::judge::JudgeLabel {
                id: &l.id,
                judgment: &l.judgment,
                pack: &r.pack,
                question: Some(&l.question),
                // An audit labels a pack's questions whole, by their ids:
                // never one item of a per-item question (32d's `about`).
                about: None,
                label: l.label.clone(),
                source: "audit",
                who: &auditor,
                via: "audit",
                weight: SYSTEM_WEIGHT,
                note: &r.model,
                correlation_id: None,
                rule: Some(&r.id),
            };
            let mut rec = fact::row(&f, l.session.as_deref(), None)?;
            rec.key = Some(l.id.clone());
            records.push(rec.scoped(&scope));
        }
        let f = fact::judge_runs::JudgeAudited {
            result: r,
            who,
            via,
        };
        let mut rec = fact::row(&f, None, None)?;
        rec.key = Some(r.id.clone());
        records.push(rec.scoped(&scope));
        self.store.append(&records)?;
        self.rec(None).announce(&f);
        Ok(())
    }
}
