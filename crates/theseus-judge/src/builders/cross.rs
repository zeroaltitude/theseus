//! `recall_cross.v1` (theseus-98bz.10): the new message, trimmed, and the
//! notes the thread lens dropped as another stream's, best first, numbered
//! from 1, each with the title of the thread it came from. The core hands
//! only notes of open threads (a sealed or seal-pending one's is dropped
//! before this); each is scrubbed here and clipped, like every string a state
//! holds.
//!
//! Its dynamic items are the notes the state kept, as `notes`, each keyed by
//! the note's node id and named by its number: a note the cap left out has no
//! Noul, so Jev is never asked about a note it cannot see.

use super::*;

/// The most notes one request asks about (a per-item Noul asks at most ten).
pub const CROSS_NOTES: usize = 10;
/// A note's text at most, in characters.
const NOTE_CHARS: usize = 1200;
/// The message at most, in characters, before the state's own cap.
const MESSAGE_CHARS: usize = 2000;

/// One note another thread holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossNote {
    /// The note's node id, what the answer comes back about.
    pub key: String,
    /// The title of the thread it came from.
    pub thread: String,
    pub text: String,
}

/// `recall_cross.v1`'s input: the message, and the notes, best first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossInput {
    pub message: String,
    #[serde(default)]
    pub notes: Vec<CrossNote>,
}

pub fn cross(i: &CrossInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let notes = &i.notes[..i.notes.len().min(CROSS_NOTES)];
    let mut b = StateBuilder::new("recall_cross", RECALL_CROSS_VERSION, cap, scrub);
    b.text(
        "message",
        10,
        share(cap, 20),
        Keep::Both,
        &c.clip(i.message.trim(), MESSAGE_CHARS),
    )
    .cut_if("message", c.cut());
    let items: Vec<Value> = notes
        .iter()
        .enumerate()
        .map(|(n, note)| {
            json!({
                "note": n + 1,
                "from_thread": c.clip(&note.thread, 80),
                "text": c.clip(note.text.trim(), NOTE_CHARS),
            })
        })
        .collect();
    b.list_head(
        "notes",
        8,
        share(cap, 75),
        items,
        left_out(&i.notes, notes.len()),
    )
    .cut_if("notes", c.cut());
    let state = b.build();
    let kept: Vec<usize> = state.value()["notes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["note"].as_u64())
                .map(|n| n as usize)
                .collect()
        })
        .unwrap_or_default();
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Notes,
        kept.iter()
            .filter(|&&n| (1..=CROSS_NOTES).contains(&n))
            .map(|&n| Item {
                key: notes[n - 1].key.clone(),
                text: n.to_string(),
            })
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

    fn input(n: usize) -> CrossInput {
        CrossInput {
            message: "  Regenerate the report with the new cutoff.  ".into(),
            notes: (0..n)
                .map(|i| CrossNote {
                    key: format!("nod_{i}"),
                    thread: "home".into(),
                    text: format!("note {i}: the cutoff for flagging changed: use 0.55"),
                })
                .collect(),
        }
    }

    /// One Noul a note, keyed by its node, with the thread it came from in
    /// the state; at most ten.
    #[test]
    fn each_note_is_asked_once_with_its_thread() {
        let p = by_name("recall_cross.v1").unwrap();
        let prepared = prepare(&p, &Input::RecallCross(input(12)), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["message"], "Regenerate the report with the new cutoff.");
        assert_eq!(v["notes"][0]["from_thread"], "home");
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), CROSS_NOTES);
        assert_eq!(asked[0].id, "same_subject.1");
        assert_eq!(asked[0].about.as_deref(), Some("nod_0"));
        assert!(prepared.state.tokens <= p.state_cap_tokens);
    }
}
