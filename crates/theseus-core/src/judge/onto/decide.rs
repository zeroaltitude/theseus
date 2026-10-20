//! What `onto.v1`'s answers come to (theseus-xzxqv): a pure decision over a
//! judgment, the ontology as it stands and the bands. Nothing is written or
//! sent here; the sink makes the frame from the plan ([`super::stage`]) and
//! the reply's footer is its words.
//!
//! For each kind the judgment asked, its Choice names a listed candidate
//! (`c3`), `new`, or `none`. The confidence picks a band:
//!
//! - **act** (`[ontology] act`): the session is filed under the candidate, or
//!   under a new category when `new` is paired with a `<kind>_name` answer at
//!   the act band (the name is one code mined, `n2`). `new` at the act band
//!   with no such name asks for the naming call and writes nothing.
//! - **confirm**: a question for the owner, and nothing written.
//! - **below**: nothing; the judgment's own row is the record.
//!
//! The operator's memberships are never touched: a list the operator filled
//! to its limit refuses Jev's entry (a disagreement), and the ontology's own
//! checks refuse the rest. A refusal is a step of the plan, never a panic.

use serde_json::Value;
use theseus_judge::client::Answer;
use theseus_judge::Judgment;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Ontology, Origin, Record};

/// The bands, from `[ontology]`.
#[derive(Debug, Clone, Copy)]
pub struct Bands {
    pub act: f64,
    pub confirm: f64,
}

/// What one kind's answer came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// The session was filed (the plan holds the records).
    Filed {
        kind: String,
        id: CategoryId,
        /// The category and its ancestors, root first.
        path: Vec<String>,
        minted: bool,
        confidence: f64,
    },
    /// The confirm band: ask, write nothing.
    Ask {
        kind: String,
        text: String,
        /// The category it asks to file under; none for a new one.
        category: Option<CategoryId>,
        confidence: f64,
    },
    /// `new` at the act band, and no mined name fits: the naming call.
    Unnamed { kind: String, confidence: f64 },
    /// The ontology refused, or the operator's entries won.
    Refused {
        kind: String,
        why: String,
        /// The operator's membership is what stood in the way.
        disagrees: bool,
    },
}

impl Step {
    pub fn kind(&self) -> &str {
        match self {
            Step::Filed { kind, .. }
            | Step::Ask { kind, .. }
            | Step::Unnamed { kind, .. }
            | Step::Refused { kind, .. } => kind,
        }
    }
}

/// The decision for one judgment.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub session: String,
    pub judgment: String,
    pub steps: Vec<Step>,
    /// The records to write as Jev, categories before the lists that name
    /// them.
    pub records: Vec<Record>,
}

impl Plan {
    /// Nothing to write, ask or say: the plain turn's case.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The reply's footer: `filed: topic › theseus › ontology (new)`, one
    /// clause a kind that was filed, joined with ` · `.
    pub fn footer(&self) -> Option<String> {
        let clauses: Vec<String> = self
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Filed {
                    kind, path, minted, ..
                } => Some(format!(
                    "{kind} › {}{}",
                    path.join(" › "),
                    if *minted { " (new)" } else { "" }
                )),
                _ => None,
            })
            .collect();
        (!clauses.is_empty()).then(|| format!("filed: {}", clauses.join(" · ")))
    }

    /// The kinds whose categories were minted.
    pub fn minted(&self) -> impl Iterator<Item = &Step> {
        self.steps
            .iter()
            .filter(|s| matches!(s, Step::Filed { minted: true, .. }))
    }

    /// The kinds that asked for a name.
    pub fn unnamed(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().filter_map(|s| match s {
            Step::Unnamed { kind, .. } => Some(kind.as_str()),
            _ => None,
        })
    }
}

/// A Choice answer of the judgment: the option and its confidence.
fn choice(j: &Judgment, question: &str) -> Option<(String, f64)> {
    match &j.answer(question)?.answer {
        Answer::Choice {
            choice, confidence, ..
        } => Some((choice.clone(), *confidence)),
        _ => None,
    }
}

/// The strings of a JSON array.
fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|v| {
            v.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The entry of `options` an option id `<prefix><n>` names (n from 1).
fn nth<'a>(options: &'a [String], id: &str, prefix: char) -> Option<&'a String> {
    let n: usize = id.strip_prefix(prefix)?.parse().ok()?;
    options.get(n.checked_sub(1)?)
}

/// The decision for `j`, as the ontology `o` stands and at `now_ms`.
pub fn decide(j: &Judgment, o: &Ontology, bands: Bands, now_ms: u64) -> Plan {
    let mut plan = Plan {
        session: j.context["session"].as_str().unwrap_or_default().into(),
        judgment: j.id.clone(),
        steps: Vec::new(),
        records: Vec::new(),
    };
    let ctx = &j.context["onto"];
    let names = strings(&ctx["names"]);
    let Some(kinds) = ctx.get("kinds").and_then(Value::as_object) else {
        return plan;
    };
    // Memberships written by this plan, so a second kind's parent sees them.
    for kind in theseus_judge::builders::ONTO_KINDS {
        let Some(candidates) = kinds.get(kind) else {
            continue;
        };
        let candidates = strings(candidates);
        let Some((answer, confidence)) = choice(j, kind) else {
            continue;
        };
        if answer == "none" || confidence < bands.confirm {
            continue;
        }
        let act = confidence >= bands.act;
        let step = if answer == "new" {
            new_category(
                j,
                o,
                &plan,
                kind,
                &candidates,
                &names,
                bands,
                now_ms,
                confidence,
            )
        } else {
            let Some(id) = nth(&candidates, &answer, 'c') else {
                continue;
            };
            existing(j, o, &plan, kind, id, act, confidence, now_ms)
        };
        match step {
            Ok(Done::Step(s)) => plan.steps.push(s),
            Ok(Done::Write(s, records)) => {
                plan.records.extend(records);
                plan.steps.push(s);
            }
            Ok(Done::Nothing) => {}
            Err(why) => plan.steps.push(Step::Refused {
                kind: kind.into(),
                why,
                disagrees: false,
            }),
        }
    }
    plan
}

enum Done {
    Step(Step),
    Write(Step, Vec<Record>),
    Nothing,
}

/// The session's list of `kind` with `category` added as Jev's, the
/// lowest-confidence Jev entry dropped when the kind's limit is reached.
fn with_member(
    o: &Ontology,
    plan: &Plan,
    kind: &str,
    category: &CategoryId,
    confidence: f64,
    now_ms: u64,
) -> Result<Option<MemberList>, (String, bool)> {
    let Some(k) = o.kind(kind) else {
        return Err((format!("there is no kind `{kind}`"), false));
    };
    let current = plan
        .records
        .iter()
        .rev()
        .find_map(|r| match r {
            Record::Members(l) if l.session == plan.session && l.kind == kind => Some(l),
            _ => None,
        })
        .or_else(|| o.member_list(&plan.session, kind))
        .map_or_else(Vec::new, |l| l.members.clone());
    if current.iter().any(|m| &m.category == category) {
        return Ok(None);
    }
    let mut members = current;
    while !k.per_session.allows(members.len() + 1) {
        let weakest = members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.origin == Origin::Jev)
            .min_by(|(_, a), (_, b)| {
                a.confidence
                    .unwrap_or(0.0)
                    .total_cmp(&b.confidence.unwrap_or(0.0))
            })
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                members.remove(i);
            }
            None => {
                return Err((
                    format!(
                        "the session already holds the most {kind} categories ({}), and they are \
                         not Jev's to replace",
                        k.per_session
                    ),
                    true,
                ))
            }
        }
    }
    members.push(Membership {
        category: category.clone(),
        origin: Origin::Jev,
        confidence: Some(confidence as f32),
        as_of_ms: now_ms,
    });
    Ok(Some(MemberList {
        session: plan.session.clone(),
        kind: kind.into(),
        members,
    }))
}

fn path_of(o: &Ontology, id: &CategoryId) -> Vec<String> {
    o.path(id).iter().map(|c| c.name.clone()).collect()
}

/// The candidate `id`, filed, asked about, or left.
#[allow(clippy::too_many_arguments)]
fn existing(
    j: &Judgment,
    o: &Ontology,
    plan: &Plan,
    kind: &str,
    id: &str,
    act: bool,
    confidence: f64,
    now_ms: u64,
) -> Result<Done, String> {
    let id = CategoryId::parse(id).map_err(|e| e.to_string())?;
    let Some(c) = o.category(&id) else {
        return Ok(Done::Nothing);
    };
    if !act {
        return Ok(Done::Step(Step::Ask {
            kind: kind.into(),
            text: format!(
                "File this session under the {kind} «{}»? (nothing is written until you do)",
                path_of(o, &id).join(" › ")
            ),
            category: Some(id),
            confidence,
        }));
    }
    let list = match with_member(o, plan, kind, &id, confidence, now_ms) {
        Ok(Some(l)) => l,
        Ok(None) => return Ok(Done::Nothing),
        Err((why, disagrees)) => {
            return Ok(Done::Step(Step::Refused {
                kind: kind.into(),
                why,
                disagrees,
            }))
        }
    };
    let _ = (j, c);
    check_write(
        o,
        plan,
        Step::Filed {
            kind: kind.into(),
            id: id.clone(),
            path: path_of(o, &id),
            minted: false,
            confidence,
        },
        vec![Record::Members(list)],
    )
}

/// The records checked as Jev's against the ontology (with what the plan
/// has already added): a refusal is a step.
fn check_write(
    o: &Ontology,
    plan: &Plan,
    step: Step,
    records: Vec<Record>,
) -> Result<Done, String> {
    let mut next = o.clone();
    for r in plan.records.iter().chain(&records) {
        if let Err(e) = next.put(r.clone(), Origin::Jev) {
            return Ok(Done::Step(Step::Refused {
                kind: step.kind().to_string(),
                why: e.to_string(),
                disagrees: false,
            }));
        }
    }
    Ok(Done::Write(step, records))
}

/// `new`: a minted category when a mined name is named at the act band.
#[allow(clippy::too_many_arguments)]
fn new_category(
    j: &Judgment,
    o: &Ontology,
    plan: &Plan,
    kind: &str,
    _candidates: &[String],
    names: &[String],
    bands: Bands,
    now_ms: u64,
    confidence: f64,
) -> Result<Done, String> {
    let named = choice(j, &format!("{kind}_name"))
        .filter(|(_, c)| *c >= bands.confirm)
        .and_then(|(a, c)| Some((nth(names, &a, 'n')?.clone(), c)));
    if confidence < bands.act {
        let what = named.as_ref().map_or_else(
            || format!("a new {kind}"),
            |(n, _)| format!("a new {kind} «{n}»"),
        );
        return Ok(Done::Step(Step::Ask {
            kind: kind.into(),
            text: format!(
                "Start {what} and file this session under it? (nothing is written until you do)"
            ),
            category: None,
            confidence,
        }));
    }
    let Some((name, name_confidence)) = named.filter(|(_, c)| *c >= bands.act) else {
        return Ok(Done::Step(Step::Unnamed {
            kind: kind.into(),
            confidence,
        }));
    };
    let confidence = confidence.min(name_confidence);
    // A name the kind already holds files under that category.
    let held = o
        .categories()
        .find(|c| c.kind() == kind && c.name.eq_ignore_ascii_case(&name));
    if let Some(c) = held {
        let id = c.id.clone();
        return existing(j, o, plan, kind, id.as_str(), true, confidence, now_ms);
    }
    let id = o.mint_id(kind, &name).map_err(|e| e.to_string())?;
    let parent = parent_for(o, plan, kind);
    let category = Category {
        parent: parent.clone(),
        description: j.context["onto"]["described"][&name]
            .as_str()
            .unwrap_or_default()
            .into(),
        ..Category::new(id.clone(), name.clone(), format!("jev:{}", j.id))
    };
    // The list is built against the ontology as it will be, with the category.
    let mut with = o.clone();
    with.put(Record::Category(category.clone()), Origin::Jev)
        .map_err(|e| e.to_string())?;
    let list = match with_member(&with, plan, kind, &id, confidence, now_ms) {
        Ok(Some(l)) => l,
        Ok(None) => return Ok(Done::Nothing),
        Err((why, disagrees)) => {
            return Ok(Done::Step(Step::Refused {
                kind: kind.into(),
                why,
                disagrees,
            }))
        }
    };
    let mut path = parent.as_ref().map_or_else(Vec::new, |p| path_of(o, p));
    path.push(name);
    check_write(
        o,
        plan,
        Step::Filed {
            kind: kind.into(),
            id,
            path,
            minted: true,
            confidence,
        },
        vec![Record::Category(category), Record::Members(list)],
    )
}

/// The parent a new category of `kind` nests under: the session's own first
/// category of the kind's parent kind (the kind itself, for topics).
fn parent_for(o: &Ontology, plan: &Plan, kind: &str) -> Option<CategoryId> {
    let parent_kind = o.kind(kind)?.parent.clone()?;
    o.member_list(&plan.session, &parent_kind)?
        .members
        .iter()
        .map(|m| m.category.clone())
        .find(|c| o.category(c).is_some())
}
