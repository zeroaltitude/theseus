//! `rerank.v1` (M6 step 32c, the `+rerank` arm): the new message, trimmed,
//! and recall's top notes in the fused order, numbered from 1. The core
//! hands only notes that passed every recall filter, the place rule first;
//! each is scrubbed here and clipped, like every string a state holds.
//!
//! Its dynamic items are the notes the state kept: the first ten as
//! `notes`, the next ten as `more_notes` (a per-item Noul asks at most ten),
//! each keyed by the note's recall key (`<node>#<chunk>`) and named by its
//! number. A note the cap left out of the state has no Noul, so Jev is never
//! asked about a note it cannot see.

use super::*;

/// The most notes one rerank asks about (design M6 §2.7: the top 20).
pub const RERANK_NOTES: usize = 20;
/// A note's text at most, in characters: twenty fit the notes' share.
const NOTE_CHARS: usize = 1000;
/// The message at most, in characters, before the state's own cap.
const MESSAGE_CHARS: usize = 4000;
/// Notes per source: a per-item Noul asks at most ten.
const PER_SOURCE: usize = 10;

/// One candidate recall would rank: its key and its excerpt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RerankNote {
    /// `<node>#<chunk>`, what the answers come back about.
    pub key: String,
    pub text: String,
}

/// `rerank.v1`'s input: the message, and the notes in the fused order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RerankInput {
    pub message: String,
    #[serde(default)]
    pub notes: Vec<RerankNote>,
}

pub fn rerank(i: &RerankInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let notes = &i.notes[..i.notes.len().min(RERANK_NOTES)];
    let mut b = StateBuilder::new("rerank", RERANK_VERSION, cap, scrub);
    b.text(
        "message",
        10,
        share(cap, 15),
        Keep::Both,
        &c.clip(i.message.trim(), MESSAGE_CHARS),
    )
    .cut_if("message", c.cut());
    let items: Vec<Value> = notes
        .iter()
        .enumerate()
        .map(|(n, note)| json!({"note": n + 1, "text": c.clip(note.text.trim(), NOTE_CHARS)}))
        .collect();
    b.list_head(
        "notes",
        8,
        share(cap, 80),
        items,
        left_out(&i.notes, notes.len()),
    )
    .cut_if("notes", c.cut());
    let state = b.build();
    // The notes the state kept, by number: only they are asked about.
    let kept: Vec<usize> = state.value()["notes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["note"].as_u64())
                .map(|n| n as usize)
                .collect()
        })
        .unwrap_or_default();
    let item = |n: usize| Item {
        key: notes[n - 1].key.clone(),
        text: n.to_string(),
    };
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Notes,
        kept.iter()
            .filter(|&&n| (1..=PER_SOURCE).contains(&n))
            .map(|&n| item(n))
            .collect(),
    );
    dynamic.sources.insert(
        Source::MoreNotes,
        kept.iter()
            .filter(|&&n| (PER_SOURCE + 1..=RERANK_NOTES).contains(&n))
            .map(|&n| item(n))
            .collect(),
    );
    Prepared {
        state: Arc::new(state),
        dynamic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    fn input(n: usize, chars: usize) -> RerankInput {
        RerankInput {
            message: "  Where does the grey heron nest?  ".into(),
            notes: (0..n)
                .map(|i| RerankNote {
                    key: format!("nod_{i}#0"),
                    text: format!("note {i}: {}", "w".repeat(chars)),
                })
                .collect(),
        }
    }

    /// Twenty-five notes: the state keeps the first twenty, numbered, and
    /// the pack asks one Noul each, ten under each question, keyed by the
    /// note's recall key.
    #[test]
    fn the_top_twenty_are_asked_one_noul_each() {
        let p = by_name("rerank.v1").unwrap();
        let prepared = prepare(&p, &Input::Rerank(input(25, 40)), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["message"], "Where does the grey heron nest?", "trimmed");
        let notes = v["notes"].as_array().unwrap();
        assert_eq!(notes[0]["note"], 1);
        assert_eq!(notes[19]["note"], 20);
        assert!(prepared.state.tokens <= p.state_cap_tokens);
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 20);
        assert_eq!(asked[0].id, "helps.1");
        assert_eq!(asked[0].about.as_deref(), Some("nod_0#0"));
        assert_eq!(asked[10].id, "helps_more.1");
        assert_eq!(asked[10].about.as_deref(), Some("nod_10#0"));
        assert_eq!(asked[19].about.as_deref(), Some("nod_19#0"));
        assert_eq!(
            asked[3].question.instructions(),
            "Does the note numbered 4 hold information that would help answer the message?"
        );
    }

    /// Twenty long notes fit the state whole once clipped; a note the cap
    /// still left out would get no Noul.
    #[test]
    fn every_note_asked_about_is_in_the_state() {
        let p = by_name("rerank.v1").unwrap();
        for (n, chars) in [(20, 5000), (3, 10), (0, 0)] {
            let prepared = prepare(&p, &Input::Rerank(input(n, chars)), &NoScrub).unwrap();
            let v = prepared.state.value();
            let shown: Vec<u64> = v["notes"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x["note"].as_u64()).collect())
                .unwrap_or_default();
            let asked = p.ask(&prepared.dynamic);
            assert_eq!(asked.len(), shown.len(), "{n} notes of {chars}");
            assert_eq!(asked.len(), n, "{n} notes of {chars}: all fit");
            assert!(prepared.state.tokens <= p.state_cap_tokens);
        }
        // A smaller cap keeps fewer notes, and asks only of those.
        let mut small = (*p).clone();
        small.state_cap_tokens = 1500;
        let prepared = prepare(&small, &Input::Rerank(input(20, 1000)), &NoScrub).unwrap();
        let shown = prepared.state.value()["notes"]
            .as_array()
            .map_or(0, |a| a.iter().filter(|x| x["note"].is_u64()).count());
        assert!(shown < 20, "{shown}");
        assert_eq!(small.ask(&prepared.dynamic).len(), shown);
    }

    /// Every note's text and the message pass the scrubber.
    #[test]
    fn the_notes_are_scrubbed() {
        struct NoHeron;
        impl Scrub for NoHeron {
            fn scrub(&self, text: &str) -> String {
                text.replace("heron", "[scrubbed]")
            }
        }
        let p = by_name("rerank.v1").unwrap();
        let mut i = input(2, 10);
        i.notes[1].text = "the heron's key".into();
        let prepared = prepare(&p, &Input::Rerank(i), &NoHeron).unwrap();
        assert!(
            !prepared.state.json.contains("heron"),
            "{}",
            prepared.state.json
        );
    }
}
