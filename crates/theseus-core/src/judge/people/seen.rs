//! The owner's gate on `people.v1`'s live point (theseus-u5n8, "combine,
//! gated by Jev"). At each due exchange end of a private conversation, Jev
//! alone first, in one call (`people_seen.v1`): the state is the session's
//! title and its human-facing lines since people's mark; one Noul per held
//! person the exchange may involve ([`listed`]: the session's own first,
//! then those whose name or handle the lines hold; never the owner's nor
//! one [`NotPeople`] excludes; at most [`SEEN`]), and one Noul whether a
//! person not listed is involved. Code reads the answers:
//!
//! - a listed person's Noul at `[people] confirm` or above is a proposal of
//!   that person for the session (`rpc/proposals.rs`, read from the
//!   judgment as people.v1's are), never a membership written here;
//! - only the `unlisted` Noul at `[people] gate` or above opens the gate:
//!   the extractor reads the exchange and people.v1 judges what it finds
//!   (`run.rs`). Under it, or with no answer (the pack off, no client, the
//!   day's budget spent, a failed call), no model is called.
//!
//! The judgment is the sink's `judge.call` row, scoped `judge:people_seen`,
//! as every pack's. The backfill (`import people TAG --propose`) has no
//! gate: it extracts each session under its cap.

use std::collections::HashSet;

use serde_json::json;
use theseus_judge::builders::{HeldPerson, PeopleSeenInput, SEEN};
use theseus_judge::{Answer, Ask, DecisionPoint, Input, Judge, Judgment, Outcome, Urgency};
use theseus_ontology::{handles_of, Category, Ontology};

use super::{Line, NotPeople};
use crate::config::PackMode;
use crate::judge::{spend, JudgeService, ScrubWith};

/// The pack.
pub const SEEN_PACK: &str = "people_seen.v1";
/// Its judgments' scope, and their labels'.
pub const SEEN_SCOPE: &str = "judge:people_seen";
/// The questions a listed person's Noul comes from (the first ten, the next).
pub const SEEN_DEFS: [&str; 2] = ["seen", "seen_more"];
/// The question that opens the gate.
pub const UNLISTED: &str = "unlisted";

/// The words of a text, folded: lowercase, split at anything not a letter
/// or a digit.
fn words(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the lines name `p`: a word of its name (or of a `name:` handle)
/// of three letters or more among their words, or another handle's value
/// within their text.
fn named_in(p: &Category, said: &HashSet<String>, text: &str) -> bool {
    let mut names = vec![p.name.clone()];
    let handles = handles_of(p);
    names.extend(
        handles
            .iter()
            .filter_map(|h| h.strip_prefix("name:").map(String::from)),
    );
    names
        .iter()
        .flat_map(|n| words(n))
        .any(|w| w.chars().count() > 2 && said.contains(&w))
        || handles
            .iter()
            .filter(|h| !theseus_ontology::person::is_name(h))
            .filter_map(|h| h.split_once(':').map(|(_, v)| v.to_lowercase()))
            .any(|v| v.chars().count() > 2 && text.contains(&v))
}

/// The held people a gate lists, at most [`SEEN`]: the people `sid` holds,
/// then those the lines name, each part by name; never a retired or merged
/// one, the owner's (a person holding a `[places] owner` handle), or one
/// `not` excludes by name or handle. Each with its name.
pub fn listed(
    o: &Ontology,
    sid: &str,
    lines: &[Line],
    not: &NotPeople,
) -> Vec<(HeldPerson, String)> {
    let text = lines
        .iter()
        .map(|l| l.text.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let said = words(&text);
    let held: HashSet<_> = o
        .memberships(sid)
        .iter()
        .map(|m| m.category.clone())
        .collect();
    let mut people: Vec<(u8, &Category)> = o
        .categories()
        .filter(|c| c.kind() == theseus_ontology::person::KIND)
        .filter(|c| c.retired_ms.is_none() && c.merged_into.is_none())
        .filter(|c| !not.excludes_person(c))
        .filter_map(|c| match held.contains(&c.id) {
            true => Some((0, c)),
            false => named_in(c, &said, &text).then_some((1, c)),
        })
        .collect();
    people.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));
    people
        .into_iter()
        .take(SEEN)
        .map(|(_, p)| {
            let held = HeldPerson {
                id: p.id.local().to_string(),
                description: super::describe(p),
            };
            (held, p.name.clone())
        })
        .collect()
}

/// The gate's `unlisted` Noul, when Jev answered it.
pub fn unlisted(j: &Judgment) -> Option<f64> {
    if j.outcome != Outcome::Answered {
        return None;
    }
    j.answers
        .iter()
        .find(|a| a.def == UNLISTED)
        .and_then(|a| match a.answer {
            Answer::Noul { noul } => Some(noul),
            _ => None,
        })
}

/// Each listed person's Noul: `(question id, person's local id, noul)`.
pub fn seen(j: &Judgment) -> Vec<(String, String, f64)> {
    if j.pack != SEEN_PACK || j.outcome != Outcome::Answered {
        return Vec::new();
    }
    j.answers
        .iter()
        .filter(|a| SEEN_DEFS.contains(&a.def.as_str()))
        .filter_map(|a| match (&a.about, &a.answer) {
            (Some(id), Answer::Noul { noul }) => Some((a.question.clone(), id.clone(), *noul)),
            _ => None,
        })
        .collect()
}

/// A listed person's name, as the judgment's context holds it.
pub fn listed_name(j: &Judgment, id: &str) -> Option<String> {
    j.context["listed"]
        .as_array()?
        .iter()
        .find(|p| p["id"] == id)?["name"]
        .as_str()
        .map(str::to_string)
}

impl JudgeService {
    /// The gate's one call for a private exchange: Jev's judgment, or
    /// `None` when none was asked (the pack off, the client not built, the
    /// day's budget spent).
    pub(crate) async fn judge_seen(
        &self,
        sid: &str,
        title: &str,
        lines: &[Line],
        listed: Vec<(HeldPerson, String)>,
    ) -> Option<Judgment> {
        if self.mode_for(SEEN_PACK, sid).mode == PackMode::Off {
            return None;
        }
        let pack = theseus_judge::pack::by_name(SEEN_PACK)?;
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let built = match self.built() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built; the people gate stays shut");
                return None;
            }
        };
        let input = PeopleSeenInput {
            session_title: title.to_string(),
            lines: lines
                .iter()
                .map(|l| format!("{}: {}", l.author, l.text))
                .collect(),
            held: listed.iter().map(|(h, _)| h.clone()).collect(),
        };
        let state = theseus_judge::prepare(
            &pack,
            &Input::PeopleSeen(input),
            &ScrubWith(self.scrubber.clone()),
        )
        .ok()?;
        let blob = theseus_store::blocking(|| self.store.blobs().put(state.state.json.as_bytes()))
            .map_err(|e| tracing::warn!(error = %e, "judge: the gate's state blob was not written"))
            .ok()?;
        let listed: Vec<_> = listed
            .iter()
            .map(|(h, name)| json!({"id": h.id, "name": name}))
            .collect();
        let mut context = json!({
            "session": sid, "purpose": "live", "baseline": "no_membership",
            "decision": "no_membership", "blob": blob, "on_path_ms": 0, "listed": listed,
        });
        let mode = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack.clone(), &state, mode, context);
        ask.id = Some(format!("jdg_{}", uuid::Uuid::now_v7().simple()));
        let asks = vec![ask];
        let need = built.judge.inner().reserve_micros(&asks).unwrap_or(0);
        if !self.reserve(&today, need) {
            return None;
        }
        let j = built
            .judge
            .judge(DecisionPoint {
                asks,
                urgency: Urgency::Shadow,
            })
            .await
            .into_iter()
            .next()?;
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        self.budget.settle(&today, need, spent, called, failed);
        Some(j)
    }
}
