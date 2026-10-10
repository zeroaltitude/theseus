//! `people.v1` (theseus-wy7y): one candidate person a generative model found
//! in a session's text, judged by Jev. Jev answers no names, so the
//! extractor writes the text (name, handles, role line, evidence) and this
//! state carries it: the session's title, the candidate, the evidence lines,
//! and the held people it may be, which are also the `match` options (at
//! most [`PEOPLE`]). The role line is its own item too (`RoleLine`), so the
//! `evaluative` Noul is asked only of a candidate that has one. Every string
//! is scrubbed here and clipped, like every string a state holds.

use super::*;

/// The held people a state offers at most.
pub const PEOPLE: usize = 50;
/// Evidence lines at most, and each line's characters.
pub const EVIDENCE: usize = 6;
const EVIDENCE_CHARS: usize = 400;

/// The candidate as the extractor wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonCandidate {
    pub name: String,
    #[serde(default)]
    pub handles: Vec<String>,
    /// What the person does or owns; empty when the text did not say.
    #[serde(default)]
    pub role_line: String,
    /// The lines of the session that name them, oldest first.
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// A held person: its local id (the option's id) and how it reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldPerson {
    pub id: String,
    /// The name, its handles, and its description, in a line.
    pub description: String,
}

/// `people.v1`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeopleInput {
    pub session_title: String,
    pub candidate: PersonCandidate,
    /// Nearest first.
    #[serde(default)]
    pub held: Vec<HeldPerson>,
}

pub fn people(i: &PeopleInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let clip = |s: &str, n: usize| clip_with(scrub, s, n);
    let mut b = StateBuilder::new("people", PEOPLE_VERSION, cap, scrub);
    b.scalar("session_title", c.clip(&i.session_title, 120))
        .cut_if("session_title", c.cut());
    let cand = &i.candidate;
    b.scalar("candidate_name", c.clip(cand.name.trim(), 120))
        .cut_if("candidate_name", c.cut());
    let handles: Vec<Value> = cand
        .handles
        .iter()
        .take(8)
        .map(|h| Value::String(c.clip(h.trim(), 120)))
        .collect();
    b.list_head("candidate_handles", 9, share(cap, 5), handles, 0)
        .cut_if("candidate_handles", c.cut());
    let role = cand.role_line.trim();
    if !role.is_empty() {
        b.scalar("candidate_role_line", c.clip(role, 300))
            .cut_if("candidate_role_line", c.cut());
    }
    let evidence = newest(&cand.evidence, EVIDENCE);
    let lines: Vec<Value> = evidence
        .iter()
        .map(|e| Value::String(c.clip(e.trim(), EVIDENCE_CHARS)))
        .collect();
    b.list_after(
        "evidence",
        10,
        share(cap, 45),
        lines,
        left_out(&cand.evidence, evidence.len()),
    )
    .cut_if("evidence", c.cut());
    let held = &i.held[..i.held.len().min(PEOPLE)];
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::People,
        held.iter()
            .map(|p| Item {
                key: p.id.clone(),
                text: clip(&p.description, 200),
            })
            .collect(),
    );
    if !role.is_empty() {
        dynamic.sources.insert(
            Source::RoleLine,
            vec![Item {
                key: "role_line".into(),
                text: clip(role, 300),
            }],
        );
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}
