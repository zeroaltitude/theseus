//! `heard.v1` (theseus-s3dwj): one short or unclear transcript from a voice
//! call that began over Theseus's speech, just after it, or at the join, with
//! the sentences it may be an echo of and the timing. One Choice: speech to
//! answer, the assistant's own echo, or noise. The word count is computed
//! here, since Jev is weak at counting. Every string is scrubbed and clipped.

use super::*;

/// The assistant's sentences a state offers at most, and a sentence's
/// characters.
pub const SAID: usize = 3;
const SAID_CHARS: usize = 300;
const TRANSCRIPT_CHARS: usize = 300;

/// When the utterance began.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeardOverlap {
    /// While a sentence played, or in a gap between its sentences.
    During,
    /// Within the echo tail after the last sentence, or before a queued reply began.
    Tail,
    /// At the join, before the first turn.
    Join,
}

/// `heard.v1`'s input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeardInput {
    pub transcript: String,
    /// The assistant's last spoken sentences, oldest first.
    #[serde(default)]
    pub said: Vec<String>,
    pub overlap: HeardOverlap,
    /// How long after the sentence playing began the utterance began.
    #[serde(default)]
    pub after_start_ms: Option<u64>,
    /// How long after the last sentence ended it began.
    #[serde(default)]
    pub after_end_ms: Option<u64>,
    pub length_ms: u64,
    /// The recogniser's confidence in the transcript, when it gave one.
    #[serde(default)]
    pub confidence: Option<f64>,
    /// The speaker's microphone has already played the assistant back.
    pub echo_prone: bool,
    /// `private` or `shared`.
    pub place: String,
}

pub fn heard(i: &HeardInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("heard", HEARD_VERSION, cap, scrub);
    let transcript = c.clip(i.transcript.trim(), TRANSCRIPT_CHARS);
    b.scalar("transcript", transcript)
        .cut_if("transcript", c.cut());
    b.scalar("words", i.transcript.split_whitespace().count());
    let kept = newest(&i.said, SAID);
    let said: Vec<Value> = kept
        .iter()
        .map(|s| Value::String(c.clip(s.trim(), SAID_CHARS)))
        .collect();
    b.list_after(
        "said",
        10,
        share(cap, 60),
        said,
        left_out(&i.said, kept.len()),
    )
    .cut_if("said", c.cut());
    b.scalar("overlap", json!(i.overlap))
        .scalar("length_ms", i.length_ms)
        .scalar("echo_prone", i.echo_prone)
        .scalar("place", c.clip(&i.place, 16));
    if let Some(ms) = i.after_start_ms {
        b.scalar("after_speech_start_ms", ms);
    }
    if let Some(ms) = i.after_end_ms {
        b.scalar("after_speech_end_ms", ms);
    }
    if let Some(p) = i.confidence.filter(|p| p.is_finite()) {
        b.scalar("recognizer_confidence", (p * 100.0).round() / 100.0);
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    fn input() -> HeardInput {
        HeardInput {
            transcript: "yes deploy it".into(),
            said: vec!["Looking good.".into(), "Shall I deploy it now?".into()],
            overlap: HeardOverlap::Tail,
            after_start_ms: None,
            after_end_ms: Some(300),
            length_ms: 900,
            confidence: Some(0.91),
            echo_prone: false,
            place: "private".into(),
        }
    }

    /// The state holds the words counted, the sentences, the timing, and the
    /// pack asks its one Choice.
    #[test]
    fn the_state_holds_the_transcript_the_sentences_and_the_timing() {
        let p = by_name("heard.v1").unwrap();
        let prepared = prepare(&p, &Input::Heard(input()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["transcript"], "yes deploy it");
        assert_eq!(v["words"], 3);
        assert_eq!(v["said"][1], "Shall I deploy it now?");
        assert_eq!(v["overlap"], "tail");
        assert_eq!(v["after_speech_end_ms"], 300);
        assert!(v.get("after_speech_start_ms").is_none());
        assert_eq!(v["recognizer_confidence"], 0.91);
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, "heard");
    }

    /// Only the newest three sentences go, and the input round-trips through JSON.
    #[test]
    fn the_newest_three_sentences_stay_and_the_input_round_trips() {
        let mut i = input();
        i.said = (1..=5).map(|n| format!("Sentence {n}.")).collect();
        let p = by_name("heard.v1").unwrap();
        let prepared = prepare(&p, &Input::Heard(i.clone()), &NoScrub).unwrap();
        let said = prepared.state.value()["said"].clone();
        assert!(said.to_string().contains("Sentence 5."));
        assert!(!said.to_string().contains("Sentence 2."));
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            Input::parse(Builder::Heard, &json).unwrap(),
            Input::Heard(i)
        );
    }
}
