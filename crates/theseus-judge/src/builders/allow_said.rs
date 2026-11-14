//! `allow_said.v1` (chat-acts-are-tools, S43-1): the operator's latest
//! message, whole, and the held call the assistant has decided it means.
//! One Noul: does the message say to run that call as it was first proposed?
//!
//! The state is deliberately small and one-sided. It carries the operator's
//! words and the call as he was offered it, and **nothing of the model's
//! reasoning**: the question is about what he said, so a model that argued
//! itself into the call must not be able to argue the judge into it too.
//! The other holds in the same conversation go in because "run the original"
//! said while two are held is a different fact from the same words said while
//! one is.

use super::*;

/// The operator's message, clipped: past this it is not an instruction about
/// one call.
const MESSAGE_CHARS: usize = 2_000;
/// One held call's summary, and the most other holds the state names.
const SUMMARY_CHARS: usize = 300;
pub const OTHERS: usize = 8;

/// `allow_said.v1`'s input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowSaidInput {
    /// The operator's latest message, whole.
    pub message: String,
    /// The held call's tool, as the hold names it (`fs.write`).
    pub tool: String,
    /// Its summary, as the owner was offered it.
    pub summary: String,
    /// Its id.
    pub id: String,
    /// The other calls held in the same conversation, each `tool — summary (id)`.
    #[serde(default)]
    pub others: Vec<String>,
}

pub fn allow_said(i: &AllowSaidInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("allow_said", ALLOW_SAID_VERSION, cap, scrub);
    b.scalar("message", c.clip(i.message.trim(), MESSAGE_CHARS))
        .cut_if("message", c.cut());
    b.scalar("held_tool", c.clip(i.tool.trim(), 64));
    b.scalar("held_summary", c.clip(i.summary.trim(), SUMMARY_CHARS))
        .cut_if("held_summary", c.cut());
    b.scalar("held_id", c.clip(i.id.trim(), 64));
    let kept = newest(&i.others, OTHERS);
    let others: Vec<Value> = kept
        .iter()
        .map(|s| Value::String(c.clip(s.trim(), SUMMARY_CHARS)))
        .collect();
    b.list_after(
        "others_held",
        10,
        share(cap, 30),
        others,
        left_out(&i.others, kept.len()),
    )
    .cut_if("others_held", c.cut());
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

    fn input() -> AllowSaidInput {
        AllowSaidInput {
            message: "run the original".into(),
            tool: "fs.write".into(),
            summary: "fs.write notes.txt".into(),
            id: "hld_1a2b3c4d".into(),
            others: vec![],
        }
    }

    /// The state holds his words and the call, and the pack asks its one Noul.
    #[test]
    fn the_state_holds_the_message_and_the_held_call() {
        let p = by_name("allow_said.v1").unwrap();
        let prepared = prepare(&p, &Input::AllowSaid(input()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["message"], "run the original");
        assert_eq!(v["held_tool"], "fs.write");
        assert_eq!(v["held_id"], "hld_1a2b3c4d");
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, "says_run");
    }

    /// The other holds reach the state, and the input round-trips through JSON.
    #[test]
    fn the_other_holds_reach_the_state_and_the_input_round_trips() {
        let mut i = input();
        i.others = vec!["proc.run — rm -rf build (hld_9z)".into()];
        let p = by_name("allow_said.v1").unwrap();
        let prepared = prepare(&p, &Input::AllowSaid(i.clone()), &NoScrub).unwrap();
        let others = prepared.state.value()["others_held"].clone();
        assert!(others.to_string().contains("hld_9z"), "{others}");
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            Input::parse(Builder::AllowSaid, &json).unwrap(),
            Input::AllowSaid(i)
        );
    }
}
