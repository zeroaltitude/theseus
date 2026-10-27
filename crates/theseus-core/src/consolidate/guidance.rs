//! The dream writes guidance (theseus-olt3h): after the nightly consolidation,
//! a category of the ontology that five sessions or more hold and that has
//! none gets its first, written from what those sessions say. The compile
//! composes a category's guidance into every session that holds it, so a
//! category written this way changes the next compile of each of its
//! sessions' system block (a session's next recompile, not a turn in flight).
//!
//! - **Which**: a category of an interpreted kind that a machine wrote (never
//!   the operator's or the import's), no guidance yet, held by [`MIN_SESSIONS`]
//!   sessions of private places that a person wrote (never a task's, an
//!   import's or a shared place's); at most [`PER_NIGHT`] a night, most held
//!   first.
//! - **The call**: one, through the provider path consolidation uses, so the
//!   data rule's gate and the China-origin rule apply unchanged. Its profile
//!   is `[memory] synth_profile`'s: `"session"` takes the profile all the
//!   sampled sessions last used, and a category whose sessions disagree
//!   waits, counted; so does a category whose model thinks whether asked or
//!   not (Opus 5.5, Fable 5.x, GLM, DeepSeek), whose 250 cap would go to thinking
//!   (theseus-ohiu8). The request holds the category's name and path, and for
//!   its newest [`SAMPLE`] sessions the title and the first and last message
//!   the person wrote, each cut to [`TEXT_CHARS`] characters and scrubbed. A
//!   session whose first or last message cannot be read is left out, and the
//!   category waits only when fewer than [`MIN_SESSIONS`] remain.
//! - **Money**: no new budget. The worst case is reserved at the profile's
//!   prices first, and the night stops before a call would pass `[memory]
//!   synth_limit_usd_per_day`, counting consolidation's spend today and these
//!   calls' (the META record [`MARK`], so a restart reads it back; a refused
//!   answer's call is counted too).
//! - **The write**: an answer that is empty, or over 400 characters once
//!   trimmed, is refused and counted, never cut to fit. Else one machine
//!   write of the dream's (`dream-<ms>-<n>`), undone by `theseus ontology
//!   undo`, which restores the empty guidance. No card goes out at night: the
//!   write is in the morning note.
//!
//! Never run by the owner's `memory.consolidate`: only the nightly tender calls
//! it, and only while `[ontology] dream_guidance` is on (the default; off is
//! its rollback: nothing is read, called or written).

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_ontology::{
    Basis, Category, CategoryId, Guidance, Ontology, Origin, Record, DREAM_GUIDANCE_MAX,
};
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind_session;
use theseus_store::{kinds, Page, Store as _};

use crate::judge::spend;
use crate::node::{Body, Node, Origin as NodeOrigin};
use crate::places::PlaceClass;
use crate::provider::{Provider, ProviderRequest};
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The sessions that make a category worth a guidance.
pub const MIN_SESSIONS: usize = 5;
/// The categories one night writes for at most.
pub const PER_NIGHT: usize = 3;
/// The newest sessions a call reads.
pub const SAMPLE: usize = 12;
/// A title or a message, in characters at most.
pub const TEXT_CHARS: usize = 300;
/// The answer's most tokens: 400 characters and some to spare, asked with
/// thinking off (theseus-050g2: Haiku 5.5's default thinking spent the cap,
/// and every night's answer came back empty). A profile whose model cannot
/// turn thinking off waits instead (theseus-ohiu8).
pub const MAX_TOKENS: u32 = 250;
/// The META record of the night's spend, by local day.
pub const MARK: &str = "onto.dream.last";

/// What the profile is told, before the sessions.
pub const INSTRUCTIONS: &str = "The sessions below belong to one category of a person's work. \
State what they settled, as plain facts, in at most 400 characters.";

/// The dream's spend today, where a restart reads it back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DreamMark {
    pub day: String,
    pub spent_usd: f64,
}

/// What one night did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dreamed {
    /// The ids of the writes made (`ontology undo` takes each).
    pub written: Vec<String>,
    /// Answers refused: empty, or over 400 characters.
    pub refused: u64,
    /// Categories that waited, for any cause in [`Wait`]: asked on a later
    /// night. `waits` says why, by reason.
    pub waiting: u64,
    /// `waiting` by reason ([`Wait`]'s words), so a night that wrote nothing
    /// says what held it.
    pub waits: BTreeMap<String, u64>,
    pub stopped: Option<String>,
    pub spent: Micros,
}

/// Why a category waits for a later night: the one cause of each `None` the
/// request used to give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    /// The sampled sessions last used different profiles.
    Profiles,
    /// The profile does not resolve to a model.
    Profile,
    /// The profile's provider is not configured.
    Provider,
    /// The model thinks whether asked or not, so its small cap would go to
    /// thinking (theseus-ohiu8); the model's name.
    ThinksUnasked(String),
    /// Fewer than [`MIN_SESSIONS`] sessions have text that can be read.
    Unreadable,
    /// The category is gone from the ontology.
    NoCategory,
    /// The model has no price, so no worst case can be reserved.
    NoPrice,
}

impl std::fmt::Display for Wait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::Profiles => f.write_str("sessions last used different profiles"),
            Wait::Profile => f.write_str("the profile does not resolve"),
            Wait::Provider => f.write_str("the profile's provider is missing"),
            Wait::ThinksUnasked(m) => write!(f, "{m} thinks unasked"),
            Wait::Unreadable => f.write_str("too few sessions with readable text"),
            Wait::NoCategory => f.write_str("the category is gone"),
            Wait::NoPrice => f.write_str("the model has no price"),
        }
    }
}

/// A session a call reads: its title and the first and last message the
/// person wrote.
struct Seen {
    title: String,
    first: String,
    last: String,
}

fn cut(text: &str) -> String {
    text.trim().chars().take(TEXT_CHARS).collect()
}

/// The request for one category: the instructions, then the category and its
/// sessions, numbered, scrubbed.
fn request_for(
    (model, thinking): (&str, Option<Value>),
    category: &Category,
    path: &str,
    seen: &[Seen],
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let sessions: Vec<String> = seen
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "[{}] {}\nFirst: {}\nLast: {}",
                i + 1,
                scrub(&s.title),
                scrub(&s.first),
                scrub(&s.last)
            )
        })
        .collect();
    let content = format!(
        "Category: {} ({})\n\nSessions:\n\n{}",
        scrub(&category.name),
        scrub(path),
        sessions.join("\n\n")
    );
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: Vec::new(),
        thinking,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra: Default::default(),
        image_tokens: 0,
    }
}

impl Core {
    /// The first or the last message a person wrote in `session`, by the
    /// index's pages; `None` while its shape is built, or when there is none.
    fn human_end(&self, session: &str, first: bool) -> Option<String> {
        let page = Page {
            kind: kinds::NODE,
            tags: vec![ledger_kind_session("user_message", session)],
            after: first.then_some(0),
            limit: 8,
            ..Page::default()
        };
        let out = self.store.inner().page(&page).ok()??;
        let mut nodes = out.records.iter().filter_map(|r| r.decode::<Node>().ok());
        let human = |n: &Node| match &n.body {
            Body::UserMessage { text, .. } if n.origin == NodeOrigin::Operator => {
                Some(text.clone())
            }
            _ => None,
        };
        match first {
            true => nodes.find_map(|n| human(&n)),
            false => nodes.filter_map(|n| human(&n)).next_back(),
        }
    }

    /// The sessions that hold `id`, newest first, that a call may read: a
    /// person's, in a private place, not a task's nor an import's.
    fn dream_sessions(&self, o: &Ontology, id: &CategoryId) -> Vec<SessionRecord> {
        let mut found: Vec<SessionRecord> = o
            .lists()
            .filter(|l| l.kind == id.kind() && l.members.iter().any(|m| &m.category == id))
            .filter_map(|l| self.store.get_session::<SessionRecord>(&l.session).ok()?)
            .filter(|s| {
                s.task.is_none()
                    && s.imported.is_none()
                    && self.runner.class_of(&s.session_id) == PlaceClass::Private
            })
            .collect();
        found.sort_by(|a, b| {
            (b.last_active_ms, &b.session_id).cmp(&(a.last_active_ms, &a.session_id))
        });
        found
    }

    /// The categories the dream may write for, most held first, each with its
    /// sessions.
    fn dream_wanted(&self, o: &Ontology) -> Vec<(CategoryId, Vec<SessionRecord>)> {
        let mut out: Vec<(CategoryId, Vec<SessionRecord>)> = o
            .categories()
            .filter(|c| c.by_machine())
            .filter(|c| {
                o.kind(c.kind())
                    .is_some_and(|k| k.basis == Basis::Interpreted && k.retired_ms.is_none())
            })
            .filter(|c| o.guidance(&c.id).is_none_or(|g| g.is_empty()))
            .map(|c| (c.id.clone(), self.dream_sessions(o, &c.id)))
            .filter(|(_, s)| s.len() >= MIN_SESSIONS)
            .collect();
        out.sort_by(|(a, sa), (b, sb)| sb.len().cmp(&sa.len()).then_with(|| a.cmp(b)));
        out
    }

    /// What consolidation spent today: its `synthesis.proposed` rows since
    /// local midnight.
    fn consolidation_spent(&self, now_ms: u64) -> Result<Micros> {
        let midnight = crate::learning::local_midnight(now_ms);
        Ok(self
            .synthesis_rows(LedgerKind::SynthesisProposed, None)?
            .iter()
            .filter(|r| r.at_unix_ms >= midnight)
            .map(|r| usd_to_micros(r.data["cost_usd"].as_f64().unwrap_or(0.0)))
            .sum())
    }

    /// The request for `id`'s newest sessions, and where to send it: the
    /// profile's target and provider. A [`Wait`] says why the category waits:
    /// its sessions last used different profiles, the profile or its provider
    /// is not there, its model thinks unasked, fewer than [`MIN_SESSIONS`] of
    /// them have text that can be read, or the model has no price.
    fn dream_request(
        &self,
        o: &Ontology,
        id: &CategoryId,
        sessions: &[SessionRecord],
    ) -> Result<
        (
            ProviderRequest,
            crate::turn::Target,
            Arc<dyn Provider>,
            Micros,
        ),
        Wait,
    > {
        let sample: Vec<&SessionRecord> = sessions.iter().take(SAMPLE).collect();
        let ids: Vec<&str> = sample.iter().map(|s| s.session_id.as_str()).collect();
        let profile = self.session_profile(&ids).map_err(|_| Wait::Profiles)?;
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&profile), None, None)
            .map_err(|_| Wait::Profile)?;
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or(Wait::Provider)?;
        // A model that cannot turn thinking off spends the 250 cap thinking
        // and is cut or empty every time: the category waits, counted, rather
        // than reserve, send and pay for the same refusal each night
        // (theseus-ohiu8). Stateless on purpose: a try count would pay for
        // its first tries and need a record kept; this one costs nothing and
        // lifts the night the profile changes.
        if self.runner.catalog.thinks_unasked(&target.model) {
            return Err(Wait::ThinksUnasked(target.model));
        }
        // A session whose first or last human text cannot be read is left out
        // of the call (theseus-23wd8); the category waits only when fewer than
        // [`MIN_SESSIONS`] readable ones remain.
        let seen: Vec<Seen> = theseus_store::blocking(|| {
            sample
                .iter()
                .filter_map(|s| {
                    Some(Seen {
                        title: cut(s.title.as_deref().unwrap_or_default()),
                        first: cut(&self.human_end(&s.session_id, true)?),
                        last: cut(&self.human_end(&s.session_id, false)?),
                    })
                })
                .collect()
        });
        if seen.len() < MIN_SESSIONS {
            return Err(Wait::Unreadable);
        }
        let path = o
            .path(id)
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(" › ");
        let scrubber = self.tools.scrubber.clone();
        let thinking = self.runner.catalog.thinking_off(&target.model);
        let model = (target.model.as_str(), thinking);
        let category = o.category(id).ok_or(Wait::NoCategory)?;
        let request = request_for(model, category, &path, &seen, &|t| scrubber.scrub(t).0);
        let need = self.audit_reserve(&request).ok_or(Wait::NoPrice)?;
        Ok((request, target, provider, need))
    }

    /// One night's guidance at `now_ms`: for each category the dream may write
    /// for, a call under the day's limit and a write of the answer.
    pub async fn dream_guidance(&self, now_ms: u64) -> Result<Dreamed> {
        let mut out = Dreamed::default();
        if !self.runner.cfg.ontology.dream_guidance {
            return Ok(out);
        }
        let limit = usd_to_micros(self.runner.memory.cfg().synth_limit_usd_per_day);
        let today = spend::local_day(now_ms);
        let stored: Option<DreamMark> = theseus_store::blocking(|| self.store.get_meta(MARK))?;
        let mut mark = stored.filter(|m| m.day == today).unwrap_or(DreamMark {
            day: today,
            spent_usd: 0.0,
        });
        let consolidated = theseus_store::blocking(|| self.consolidation_spent(now_ms))?;
        let mut spent = consolidated + usd_to_micros(mark.spent_usd);
        let o = self.runner.ontology.snapshot(&self.store)?;
        let mut called = 0;
        let wanted = theseus_store::blocking(|| self.dream_wanted(&o));
        for (id, sessions) in wanted {
            if called >= PER_NIGHT || self.outbox.stopping() {
                break;
            }
            let (request, target, provider, need) = match self.dream_request(&o, &id, &sessions) {
                Ok(r) => r,
                Err(why) => {
                    out.waiting += 1;
                    *out.waits.entry(why.to_string()).or_default() += 1;
                    continue;
                }
            };
            if spent.saturating_add(need) > limit {
                out.stopped = Some(format!(
                    "the next guidance would reserve {}, past [memory] synth_limit_usd_per_day \
                     ({}) with {} spent today",
                    crate::narrative::dollars(need),
                    crate::narrative::dollars(limit),
                    crate::narrative::dollars(spent)
                ));
                break;
            }
            let hold = match self.runner.day_hold(need, "dream guidance") {
                Ok(h) => h,
                Err(why) => {
                    out.stopped = Some(why);
                    break;
                }
            };
            called += 1;
            let mut quiet = |_: crate::provider::Delta<'_>| {};
            let answered = provider.stream_message(&request, &mut quiet).await;
            let cost = answered
                .as_ref()
                .map_or(need, |r| self.runner.priced(r, &target.model, need));
            hold.settle(cost);
            spent += cost;
            out.spent += cost;
            mark.spent_usd += micros_to_usd(cost);
            theseus_store::blocking(|| self.store.put_meta(MARK, &mark))?;
            let text = match answered {
                // A cut-off answer is refused, never written (theseus-050g2).
                Ok(resp) if crate::provider::hit_cap(resp.stop_reason.as_deref()) => {
                    tracing::warn!(category = %id, output_tokens = resp.usage.output_tokens,
                        "dream: the guidance stopped on max_tokens; refused");
                    out.refused += 1;
                    continue;
                }
                Ok(resp) => resp.text.trim().to_string(),
                Err(e) => {
                    tracing::warn!(category = %id, error = %format!("{e:#}"), "dream: the guidance call failed");
                    continue;
                }
            };
            match self.dream_write(&o, &id, &text, now_ms, out.written.len()) {
                Some(write) => out.written.push(write),
                None => out.refused += 1,
            }
        }
        Ok(out)
    }

    /// `text` as `id`'s first guidance, as one machine write of the dream's:
    /// its id, or none when the answer is refused (empty, or over 400
    /// characters, never cut) or the ontology refuses the record.
    fn dream_write(
        &self,
        o: &Ontology,
        id: &CategoryId,
        text: &str,
        now_ms: u64,
        n: usize,
    ) -> Option<String> {
        if text.is_empty() || text.chars().count() > DREAM_GUIDANCE_MAX {
            return None;
        }
        let write = format!("dream-{now_ms}-{n}");
        let version = o.guidance(id).map_or(1, |g| g.version + 1);
        let by = format!("dream:{write}");
        let record = Record::Guidance(Guidance::new(id.clone(), text, version, &by));
        let done = theseus_store::blocking(|| {
            self.ontology_machine_write(Origin::Dream, &write, vec![record])
        });
        match done {
            Ok(()) => Some(write),
            Err(e) => {
                tracing::warn!(write, error = %format!("{e:#}"), "dream: the guidance was not written");
                None
            }
        }
    }
}
