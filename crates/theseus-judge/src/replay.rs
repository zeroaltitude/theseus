//! Replay's pure half (design §2.9; M5 25d): what a candidate pack version
//! changes beside its incumbent, whether a stored state can be sent to it as
//! it was, and a thresholds-only candidate's answers re-banded without a
//! call. The core reads the record and makes the calls; these say what the
//! record allows.
//!
//! Jev reads a question's instructions and criteria (a Choice's options'
//! `means`, a Score's `levels`, a Noul's `when_true` and `when_false`), the
//! state, and the model asked. A candidate that changes none of these, and
//! only the thresholds (or which questions decide), would draw the same
//! answers: [`what_changes`] says so, and [`reband`] bands the stored answers
//! again under the candidate's thresholds.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::band::{band, Band};
use crate::builders::{prepare, InboundInput, Input};
use crate::builders::{
    ATTRIBUTION_VERSION, CATEGORIZE_VERSION, CITATION_VERSION, COMPARE_VERSION,
    CONTINUE_VERSION, FAMILIES_VERSION, HEARD_VERSION, INBOUND_VERSION, LOOP_VERSION,
    MEMORY_VERSION, NAME_IT_VERSION, PEOPLE_SEEN_VERSION, PEOPLE_VERSION, PROBE_VERSION,
    RECALL_CROSS_VERSION, RERANK_VERSION, SAME_KIND_VERSION, SAME_THING_VERSION, SECURITY2_VERSION,
    SECURITY_VERSION, TWO_THINGS_VERSION,
};
use crate::client::{Answer, Urgency};
use crate::judge::{AnswerRecord, Ask, DecisionPoint, Judge, Mode, StateRecord};
use crate::pack::{Builder, Pack, QuestionDef};
use crate::state::{BuiltState, NoScrub};

/// What a candidate changes beside its incumbent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// What Jev reads changed (a question's wording or criteria, the model,
    /// the builder or its cap): each state is asked again.
    Asks,
    /// Only the thresholds (or which questions decide): the stored answers
    /// are re-banded, and nothing is called.
    ThresholdsOnly,
}

/// A question as Jev reads it: its thresholds and whether it decides set
/// aside.
fn as_read(q: &QuestionDef) -> QuestionDef {
    let mut q = q.clone();
    q.thresholds = crate::band::Thresholds::CONSERVATIVE;
    q.decides = false;
    q.decide_above = None;
    q
}

/// What `candidate` changes beside `incumbent`, by what Jev reads.
pub fn what_changes(incumbent: &Pack, candidate: &Pack) -> Change {
    let same_reads = incumbent.jev_model == candidate.jev_model
        && incumbent.builder == candidate.builder
        && incumbent.state_cap_tokens == candidate.state_cap_tokens
        && incumbent.questions.len() == candidate.questions.len()
        && incumbent
            .questions
            .iter()
            .zip(&candidate.questions)
            .all(|(a, b)| as_read(a) == as_read(b));
    if same_reads {
        Change::ThresholdsOnly
    } else {
        Change::Asks
    }
}

/// A pack's builder's name and version, as its states record them. Most
/// builders have one version for every pack that uses them; the content
/// builder's is the pack's (`content.v3`'s state carries fields the earlier
/// two do not).
pub fn builder_identity(pack: &Pack) -> (&'static str, u32) {
    match pack.builder {
        Builder::Probe => ("probe", PROBE_VERSION),
        Builder::Loop => ("loop", LOOP_VERSION),
        Builder::Security => ("security", SECURITY_VERSION),
        Builder::Security2 => ("security2", SECURITY2_VERSION),
        Builder::Inbound => ("inbound", INBOUND_VERSION),
        Builder::Continue => ("continue", CONTINUE_VERSION),
        Builder::Categorize => ("categorize", CATEGORIZE_VERSION),
        Builder::Rerank => ("rerank", RERANK_VERSION),
        Builder::Memory => ("memory", MEMORY_VERSION),
        Builder::Attribution => ("attribution", ATTRIBUTION_VERSION),
        Builder::Citation => ("citation", CITATION_VERSION),
        Builder::People => ("people", PEOPLE_VERSION),
        Builder::PeopleSeen => ("people_seen", PEOPLE_SEEN_VERSION),
        Builder::Compare => ("compare", COMPARE_VERSION),
        Builder::Heard => ("heard", HEARD_VERSION),
        Builder::SameThing => ("same_thing", SAME_THING_VERSION),
        Builder::TwoThings => ("two_things", TWO_THINGS_VERSION),
        Builder::SameKind => ("same_kind", SAME_KIND_VERSION),
        Builder::NameIt => ("name_it", NAME_IT_VERSION),
        Builder::RecallCross => ("recall_cross", RECALL_CROSS_VERSION),
        Builder::Families => ("families", FAMILIES_VERSION),
        Builder::Content => ("content", crate::builders::content_builder_version(pack.version)),
    }
}

/// Why a stored state cannot go to `candidate` as it was sent, or `None`
/// when it can: the candidate's builder, its version, and its cap must each
/// equal the judgment's.
pub fn stored_state_differs(candidate: &Pack, state: &StateRecord) -> Option<String> {
    let (name, version) = builder_identity(candidate);
    if state.builder != name {
        return Some(format!(
            "the judgment's state was built by {}, the candidate's by {name}",
            state.builder
        ));
    }
    if state.builder_version != version {
        return Some(format!(
            "the judgment's state was built by {name} version {}, the candidate's builder is \
             version {version}",
            state.builder_version
        ));
    }
    if state.cap_tokens != candidate.state_cap_tokens {
        return Some(format!(
            "the judgment's state was capped at {} tokens, the candidate's cap is {}",
            state.cap_tokens, candidate.state_cap_tokens
        ));
    }
    None
}

/// A stored state, as it was sent: its blob's bytes and the judgment's
/// record of it. `Err`: the bytes are not the state the record names.
pub fn stored_state(json: String, record: &StateRecord) -> Result<BuiltState, String> {
    let sha256 = hex::encode(Sha256::digest(json.as_bytes()));
    if sha256 != record.sha256 {
        return Err(format!(
            "its blob's sha256 is {sha256}, not the {} its judgment names",
            record.sha256
        ));
    }
    let fields = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&json)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    Ok(BuiltState {
        bytes: json.len(),
        json,
        sha256,
        tokens: record.tokens,
        cap_tokens: record.cap_tokens,
        builder: record.builder.clone(),
        builder_version: record.builder_version,
        fields,
        truncated: record.truncated.clone(),
        dropped: record.dropped.clone(),
    })
}

/// The stored answers, banded under `candidate`'s thresholds: what a
/// thresholds-only candidate would have said. An answer to a question the
/// candidate does not ask is left out.
pub fn reband(answers: &[AnswerRecord], candidate: &Pack) -> Vec<AnswerRecord> {
    answers
        .iter()
        .filter_map(|a| {
            let def = candidate.question(&a.def)?;
            Some(AnswerRecord {
                band: band(&a.answer, def.thresholds),
                ..a.clone()
            })
        })
        .collect()
}

// ---------------------------------------------------- onto.v1's labelled replay

/// A session the import labelled (`theseus import topics`), as the replay
/// asks it: the input `onto.v1` would have seen, and the import's labels,
/// which Jev is never shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelledCase {
    pub session: String,
    pub input: InboundInput,
    /// Per kind, the ids of the categories the import filed the session under.
    pub labels: BTreeMap<String, BTreeSet<String>>,
}

/// What `onto.v1` chose for one kind of one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "pick", content = "category", rename_all = "snake_case")]
pub enum Pick {
    /// A listed category, by its id.
    Candidate(String),
    /// `new`: none listed covers it. Never a hit (the labels are all listed),
    /// counted as `minted` and as a miss for the label's recall.
    New,
    /// `none`: the message is filed under nothing.
    Nothing,
}

/// One kind's answer, with the band it landed in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chosen {
    pub pick: Pick,
    pub band: Band,
}

/// A replayed session beside its labels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scored {
    pub session: String,
    pub labels: BTreeMap<String, BTreeSet<String>>,
    /// Per kind asked. A kind absent here was not answered (the call failed
    /// or the builder left the kind out).
    pub picks: BTreeMap<String, Chosen>,
}

/// The weakest band a pick counts at: `Act` is what the sink would write on
/// its own, `Confirm` adds what it would put on a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Floor {
    Act,
    Confirm,
}

impl Floor {
    fn admits(self, band: Band) -> bool {
        match self {
            Floor::Act => band == Band::Act,
            Floor::Confirm => band != Band::Escalate,
        }
    }
}

/// Precision and recall of one kind. `precision` is `None` when nothing was
/// picked at the floor, `recall` when nothing was labelled: no number is
/// better than a made-up one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KindScore {
    pub kind: String,
    /// Sessions the kind was answered for.
    pub sessions: usize,
    /// Labels the import holds for the kind, over those sessions.
    pub labelled: usize,
    /// Picks of a listed category at the floor.
    pub picked: usize,
    pub hits: usize,
    pub wrong: usize,
    pub missed: usize,
    /// `new` answers at the floor.
    pub minted: usize,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

fn ratio(n: usize, d: usize) -> Option<f64> {
    (d > 0).then(|| n as f64 / d as f64)
}

/// Each kind's precision and recall against the import's labels, at `floor`.
/// A pick is a hit when its category is one of the session's labels for the
/// kind, wrong otherwise; a label no pick matched is missed. A pick below the
/// floor is as if Jev had said nothing: its labels are missed.
pub fn score_onto(cases: &[Scored], floor: Floor) -> Vec<KindScore> {
    let mut kinds: BTreeMap<String, KindScore> = BTreeMap::new();
    for case in cases {
        for (kind, picked) in &case.picks {
            let k = kinds.entry(kind.clone()).or_insert_with(|| KindScore {
                kind: kind.clone(),
                sessions: 0,
                labelled: 0,
                picked: 0,
                hits: 0,
                wrong: 0,
                missed: 0,
                minted: 0,
                precision: None,
                recall: None,
            });
            let gold = case.labels.get(kind);
            let held = gold.map_or(0, BTreeSet::len);
            k.sessions += 1;
            k.labelled += held;
            let counts = floor.admits(picked.band);
            match &picked.pick {
                Pick::Candidate(id) if counts => {
                    k.picked += 1;
                    if gold.is_some_and(|g| g.contains(id)) {
                        k.hits += 1;
                        k.missed += held - 1;
                    } else {
                        k.wrong += 1;
                        k.missed += held;
                    }
                }
                Pick::New if counts => {
                    k.minted += 1;
                    k.missed += held;
                }
                _ => k.missed += held,
            }
        }
    }
    kinds
        .into_values()
        .map(|mut k| {
            k.precision = ratio(k.hits, k.picked);
            k.recall = ratio(k.hits, k.hits + k.missed);
            k
        })
        .collect()
}

/// The replay's report: both floors, and what could not be scored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OntoReplay {
    pub pack: String,
    pub sessions: usize,
    /// Sessions whose judgment never answered (skipped, failed, or drifted
    /// off the pinned model): they are left out of the scores.
    pub unanswered: usize,
    pub at_act: Vec<KindScore>,
    pub at_confirm: Vec<KindScore>,
}

pub fn onto_report(pack: &str, sessions: usize, scored: &[Scored]) -> OntoReplay {
    OntoReplay {
        pack: pack.to_string(),
        sessions,
        unanswered: sessions - scored.len().min(sessions),
        at_act: score_onto(scored, Floor::Act),
        at_confirm: score_onto(scored, Floor::Confirm),
    }
}

fn cell(v: Option<f64>) -> String {
    v.map_or_else(|| "n/a".into(), |v| format!("{v:.3}"))
}

/// The report as the Markdown a `docs/benchmarks/` page quotes.
pub fn onto_markdown(r: &OntoReplay) -> String {
    let mut out = format!(
        "# {} against the import's labels\n\n{} sessions replayed, {} unanswered (left out).\n",
        r.pack, r.sessions, r.unanswered
    );
    for (title, rows) in [
        ("At the act band", &r.at_act),
        ("At confirm and above", &r.at_confirm),
    ] {
        out.push_str(&format!(
            "\n## {title}\n\n| kind | sessions | labelled | picked | hits | wrong | missed | minted | precision | recall |\n|---|---|---|---|---|---|---|---|---|---|\n"
        ));
        for k in rows {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                k.kind,
                k.sessions,
                k.labelled,
                k.picked,
                k.hits,
                k.wrong,
                k.missed,
                k.minted,
                cell(k.precision),
                cell(k.recall)
            ));
        }
    }
    out
}

/// Hides the import's labels from the input: a label's category is not the
/// session's own membership (the state's `memberships`, and its candidate
/// order, would give the answer away).
pub(crate) fn hide_labels(case: &LabelledCase) -> InboundInput {
    let mut input = case.input.clone();
    if let Some(o) = input.onto.as_mut() {
        for kind in &mut o.kinds {
            if let Some(gold) = case.labels.get(&kind.kind) {
                for item in &mut kind.pool {
                    if gold.contains(&item.id) {
                        item.session = false;
                    }
                }
            }
        }
    }
    input
}

/// One session's ask, labels hidden, with the candidates each kind offered
/// (read back to turn the answers into picks).
#[derive(Clone)]
pub struct OntoAsk {
    pub ask: Ask,
    pub session: String,
    picked_from: Vec<crate::builders::Picked>,
}

/// The asks of `cases` through `pack` (`onto.v1`): one per session, every
/// label hidden, `context`'s fields added to each ask's own.
pub fn onto_asks(
    pack: &Arc<Pack>,
    cases: &[LabelledCase],
    context: &serde_json::Value,
) -> Result<Vec<OntoAsk>, String> {
    let mut out = Vec::new();
    for case in cases {
        let input = hide_labels(case);
        let prepared = prepare(pack, &Input::Inbound(input.clone()), &NoScrub)
            .map_err(|e| format!("{}: {e}", case.session))?;
        let picked_from = input
            .onto
            .as_ref()
            .map(|o| o.pick(&input.message))
            .unwrap_or_default();
        let mut ctx = serde_json::json!({"session": case.session});
        if let (Some(m), Some(c)) = (ctx.as_object_mut(), context.as_object()) {
            m.extend(c.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        out.push(OntoAsk {
            ask: Ask::new(pack.clone(), &prepared, Mode::Shadow, ctx),
            session: case.session.clone(),
            picked_from,
        });
    }
    Ok(out)
}

/// What `onto.v1` chose for each session whose judgment answered, beside the
/// labels. `judgments` holds one per ask, in order; a session whose judgment
/// did not answer is left out (see [`OntoReplay::unanswered`]).
pub fn onto_scored(
    cases: &[LabelledCase],
    asks: &[OntoAsk],
    judgments: &[crate::judge::Judgment],
) -> Vec<Scored> {
    let mut out = Vec::new();
    for ((case, ask), j) in cases.iter().zip(asks).zip(judgments) {
        if !j.actionable() {
            continue;
        }
        let mut picks = BTreeMap::new();
        for p in &ask.picked_from {
            let Some(a) = j.answer(&p.kind) else { continue };
            let Answer::Choice { choice, .. } = &a.answer else {
                continue;
            };
            let pick = match choice.as_str() {
                "new" => Pick::New,
                "none" => Pick::Nothing,
                c => c
                    .strip_prefix('c')
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| p.candidates.get(n.checked_sub(1)?))
                    .map_or(Pick::Nothing, |m| Pick::Candidate(m.id.clone())),
            };
            picks.insert(
                p.kind.clone(),
                Chosen {
                    pick,
                    band: a.band.band,
                },
            );
        }
        out.push(Scored {
            session: case.session.clone(),
            labels: case.labels.clone(),
            picks,
        });
    }
    out
}

/// Replays `cases` through `pack` (`onto.v1`) on `judge`: one decision point,
/// one ask per session, every label hidden. Sessions whose judgment did not
/// answer are left out of the result (see [`OntoReplay::unanswered`]).
pub async fn replay_onto<J: Judge>(
    judge: &J,
    pack: Arc<Pack>,
    cases: &[LabelledCase],
) -> Result<Vec<Scored>, String> {
    let asked = onto_asks(&pack, cases, &serde_json::Value::Null)?;
    let judgments = judge
        .judge(DecisionPoint {
            asks: asked.iter().map(|a| a.ask.clone()).collect(),
            urgency: Urgency::Shadow,
        })
        .await;
    Ok(onto_scored(cases, &asked, &judgments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;

    const LOOP: &str = include_str!("../packs/loop.v1.toml");

    fn candidate(edit: impl Fn(&str) -> String) -> Pack {
        Pack::parse(&edit(LOOP).replace("version = 1", "version = 2")).unwrap()
    }

    #[test]
    fn a_reworded_criterion_asks_and_a_threshold_alone_rebands() {
        let inc = by_name("loop.v1").unwrap();
        let same = candidate(|t| t.to_string());
        assert_eq!(what_changes(&inc, &same), Change::ThresholdsOnly);
        let first_act = LOOP.find("act = ").unwrap();
        let line_end = first_act + LOOP[first_act..].find('\n').unwrap();
        let line = &LOOP[first_act..line_end];
        let lowered = candidate(|t| t.replacen(line, "act = 0.61", 1));
        assert_eq!(what_changes(&inc, &lowered), Change::ThresholdsOnly);
        let means = LOOP.find("means = \"").unwrap() + "means = \"".len();
        let reworded = candidate(|t| {
            let mut s = t.to_string();
            s.insert_str(means, "Plainly, ");
            s
        });
        assert_eq!(what_changes(&inc, &reworded), Change::Asks);
        let model = candidate(|t| t.replace("jev-1.13.0", "jev-1.14.0"));
        assert_eq!(what_changes(&inc, &model), Change::Asks);
        let cap = candidate(|t| t.replace("state_cap_tokens = 4000", "state_cap_tokens = 3000"));
        assert_eq!(what_changes(&inc, &cap), Change::Asks);
    }

    #[test]
    fn a_stored_state_goes_as_it_was_only_to_the_same_builder_and_cap() {
        let inc = by_name("loop.v1").unwrap();
        let mut rec = StateRecord {
            sha256: hex::encode(Sha256::digest(b"{\"ask\":\"x\"}")),
            bytes: 11,
            tokens: 3,
            cap_tokens: inc.state_cap_tokens,
            builder: "loop".into(),
            builder_version: LOOP_VERSION,
            truncated: vec![],
            dropped: vec![],
        };
        assert_eq!(stored_state_differs(&inc, &rec), None);
        let s = stored_state("{\"ask\":\"x\"}".into(), &rec).unwrap();
        assert_eq!((s.bytes, s.fields), (11, vec!["ask".to_string()]));
        assert!(stored_state("{\"ask\":\"y\"}".into(), &rec).is_err());
        rec.builder_version = LOOP_VERSION + 1;
        assert!(stored_state_differs(&inc, &rec)
            .unwrap()
            .contains("version"));
        rec.builder_version = LOOP_VERSION;
        rec.cap_tokens = 1;
        assert!(stored_state_differs(&inc, &rec).unwrap().contains("capped"));
    }

    /// A content state of a v1 or v2 judgment goes to the other of the two as it
    /// was sent, since they share a builder; it does not go to v3, whose state
    /// carries five more fields, so a replay of v2's judgments under v3 rebuilds
    /// them (theseus-gyjyo.11).
    #[test]
    fn a_content_state_of_v2_is_served_to_v1_and_not_to_v3() {
        use crate::builders::{prepare, ContentInput, Input};
        use crate::state::NoScrub;
        let v2 = by_name("content.v2").unwrap();
        let built = prepare(
            &v2,
            &Input::Content(ContentInput {
                text: "A page.".into(),
                kind: "fetched_page".into(),
                source: "http.fetch https://example.test/".into(),
                ..ContentInput::default()
            }),
            &NoScrub,
        )
        .unwrap()
        .state;
        let rec = StateRecord {
            sha256: built.sha256.clone(),
            bytes: built.bytes,
            tokens: built.tokens,
            cap_tokens: built.cap_tokens,
            builder: built.builder.clone(),
            builder_version: built.builder_version,
            truncated: vec![],
            dropped: vec![],
        };
        assert_eq!(stored_state_differs(&by_name("content.v1").unwrap(), &rec), None);
        assert_eq!(stored_state_differs(&v2, &rec), None);
        let why = stored_state_differs(&by_name("content.v3").unwrap(), &rec).unwrap();
        assert!(why.contains("version 1") && why.contains("version 2"), "{why}");
    }

    #[test]
    fn rebanding_reads_the_candidates_thresholds() {
        let inc = by_name("loop.v1").unwrap();
        let answer = Answer::Noul { noul: 0.7 };
        let def = inc.question("announced_unfinished").unwrap();
        let a = AnswerRecord {
            question: def.id.clone(),
            def: def.id.clone(),
            about: None,
            band: band(&answer, def.thresholds),
            answer,
        };
        let mut lower = (*inc).clone();
        for q in &mut lower.questions {
            q.thresholds.act = 0.65;
            q.thresholds.confirm = 0.6;
        }
        let out = reband(std::slice::from_ref(&a), &lower);
        assert_eq!(out[0].band.band, Band::Act);
        assert_ne!(a.band.band, Band::Act);
    }
}
