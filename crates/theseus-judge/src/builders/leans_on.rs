//! `leans_on.v1` (issue 44, the design's step 3b): a person's message in a
//! conversation others can see, and the last lines said in that conversation.
//! One Choice: the message stands alone, leans on something said here, leans
//! on something not said here, or cannot be told.
//!
//! The state holds only what the place itself already shows: no other place,
//! no memory, no recall, and not who wrote the message (the caller asks only of
//! the owner's own messages, so the state need not say so). Every string is
//! scrubbed and clipped.

use super::*;

/// The lines of the place a state offers at most, and a line's characters.
pub const HERE: usize = 8;
const HERE_CHARS: usize = 400;
const MESSAGE_CHARS: usize = 2_000;

/// One line said in the place, as the place shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HereLine {
    /// `person` or `assistant`: who said it, as the place shows it, with no
    /// name.
    pub who: String,
    pub text: String,
}

/// `leans_on.v1`'s input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeansOnInput {
    /// The message, whole.
    pub message: String,
    /// What was said in the place before it, oldest first.
    #[serde(default)]
    pub here: Vec<HereLine>,
}

pub fn leans_on(i: &LeansOnInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("leans_on", LEANS_ON_VERSION, cap, scrub);
    b.scalar("message", c.clip(i.message.trim(), MESSAGE_CHARS))
        .cut_if("message", c.cut());
    let kept = newest(&i.here, HERE);
    let here: Vec<Value> = kept
        .iter()
        .map(|l| json!({"who": c.clip(&l.who, 16), "text": c.clip(l.text.trim(), HERE_CHARS)}))
        .collect();
    b.list_after(
        "here",
        10,
        share(cap, 60),
        here,
        left_out(&i.here, kept.len()),
    )
    .cut_if("here", c.cut());
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

    fn line(who: &str, text: &str) -> HereLine {
        HereLine {
            who: who.into(),
            text: text.into(),
        }
    }

    /// The state holds the message and the place's own lines, newest kept,
    /// and the pack asks its one Choice.
    #[test]
    fn the_state_holds_the_message_and_the_places_newest_lines() {
        let p = by_name("leans_on.v1").unwrap();
        let mut here: Vec<HereLine> = (0..12)
            .map(|n| line("person", &format!("line {n}")))
            .collect();
        here.push(line("assistant", "the newest line"));
        let i = LeansOnInput {
            message: "do what I told you".into(),
            here,
        };
        let prepared = prepare(&p, &Input::LeansOn(i.clone()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["message"], "do what I told you");
        let s = v["here"].to_string();
        assert!(s.contains("the newest line"), "{s}");
        assert!(!s.contains("line 0\""), "the oldest are left out: {s}");
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, "leans_on");
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            Input::parse(Builder::LeansOn, &json).unwrap(),
            Input::LeansOn(i)
        );
    }

    #[derive(Deserialize)]
    struct Cases {
        pack: String,
        case: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        id: String,
        message: String,
        here: Vec<(String, String)>,
        label: String,
    }

    /// The labelled cases (`packs/leans_on.v1.cases.toml`): each label is one
    /// of the pack's options, the traps are there (a reference satisfied in
    /// the place, a general question), and each case's state holds its
    /// message and its place's lines alone.
    #[test]
    fn leans_on_cases() {
        let cases: Cases =
            toml::from_str(include_str!("../../packs/leans_on.v1.cases.toml")).unwrap();
        assert_eq!(cases.pack, "leans_on.v1");
        let p = by_name(&cases.pack).unwrap();
        let options: Vec<String> = p.questions[0]
            .options
            .iter()
            .map(|o| o.id.clone())
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        for c in &cases.case {
            assert!(
                seen.insert(c.id.clone()),
                "{}: a second case of that id",
                c.id
            );
            assert!(
                options.contains(&c.label),
                "{}: {} is no option",
                c.id,
                c.label
            );
            let i = LeansOnInput {
                message: c.message.clone(),
                here: c.here.iter().map(|(w, t)| line(w, t)).collect(),
            };
            let v = prepare(&p, &Input::LeansOn(i), &NoScrub)
                .unwrap()
                .state
                .value();
            assert_eq!(v["message"], c.message.as_str(), "{}", c.id);
            assert!(v.as_object().unwrap().len() <= 2, "{}: {v}", c.id);
        }
        for label in ["stands_alone", "said_here", "not_said_here"] {
            assert!(
                cases.case.iter().filter(|c| c.label == label).count() >= 4,
                "too few {label} cases"
            );
        }
    }

    /// Nothing but the input reaches the state: two inputs alike in the
    /// message and the place's lines build byte-identical states.
    #[test]
    fn the_state_is_the_input_and_nothing_else() {
        let p = by_name("leans_on.v1").unwrap();
        let i = LeansOnInput {
            message: "the plan from earlier".into(),
            here: vec![line("person", "morning")],
        };
        let a = prepare(&p, &Input::LeansOn(i.clone()), &NoScrub).unwrap();
        let b = prepare(&p, &Input::LeansOn(i), &NoScrub).unwrap();
        assert_eq!(a.state.json, b.state.json);
        let keys: Vec<String> = a
            .state
            .value()
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| !k.starts_with('_'))
            .cloned()
            .collect();
        assert!(
            keys.iter().all(|k| k == "message" || k == "here"),
            "{keys:?}"
        );
    }
}
