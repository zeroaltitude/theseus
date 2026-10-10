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

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::band::band;
use crate::builders::{
    ATTRIBUTION_VERSION, CATEGORIZE_VERSION, CITATION_VERSION, CONTINUE_VERSION, INBOUND_VERSION,
    LOOP_VERSION, MEMORY_VERSION, PEOPLE_SEEN_VERSION, PEOPLE_VERSION, PROBE_VERSION,
    RERANK_VERSION, SECURITY2_VERSION, SECURITY_VERSION,
};
use crate::judge::{AnswerRecord, StateRecord};
use crate::pack::{Builder, Pack, QuestionDef};
use crate::state::BuiltState;

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

/// A builder's name and version, as its states record them.
pub fn builder_identity(b: Builder) -> (&'static str, u32) {
    match b {
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
    }
}

/// Why a stored state cannot go to `candidate` as it was sent, or `None`
/// when it can: the candidate's builder, its version, and its cap must each
/// equal the judgment's.
pub fn stored_state_differs(candidate: &Pack, state: &StateRecord) -> Option<String> {
    let (name, version) = builder_identity(candidate.builder);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::band::Band;
    use crate::client::Answer;
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
