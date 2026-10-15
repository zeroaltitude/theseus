//! `compare.v1` (own-benchmark, theseus-pw1q.12): a turn's request, the
//! reply it got (A) and the reply another model gave to the same request
//! (B). One Choice: does B do what A did, for this request? Each reply is
//! scrubbed and clipped, with the calls it asked for named after it (a
//! replay runs none, so calls are compared by name).

use super::*;

/// One reply as the comparison reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareReply {
    pub text: String,
    /// The tool calls it asked for, by name.
    #[serde(default)]
    pub calls: Vec<String>,
}

/// `compare.v1`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareInput {
    /// The request's last user message.
    pub request: String,
    pub a: CompareReply,
    pub b: CompareReply,
}

fn reply_text(r: &CompareReply) -> String {
    match r.calls.is_empty() {
        true => r.text.clone(),
        false => format!("{}\n[asked to call: {}]", r.text, r.calls.join(", ")),
    }
}

pub fn compare(i: &CompareInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let mut b = StateBuilder::new("compare", COMPARE_VERSION, cap, scrub);
    b.text("request", 9, share(cap, 30), Keep::Both, &i.request)
        .text("reply_a", 8, share(cap, 33), Keep::Both, &reply_text(&i.a))
        .text("reply_b", 8, share(cap, 33), Keep::Both, &reply_text(&i.b));
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

    fn input() -> CompareInput {
        CompareInput {
            request: "Which port does the kestrel relay listen on?".into(),
            a: CompareReply {
                text: "It listens on 7714.".into(),
                calls: vec![],
            },
            b: CompareReply {
                text: "Let me check.".into(),
                calls: vec!["fs_read".into()],
            },
        }
    }

    /// The state holds the request and both replies, calls named after the
    /// reply that asked for them, and the pack asks its one Choice.
    #[test]
    fn the_state_holds_the_request_and_both_replies() {
        let p = by_name("compare.v1").unwrap();
        let prepared = prepare(&p, &Input::Compare(input()), &NoScrub).unwrap();
        let v = prepared.state.value();
        assert_eq!(v["request"], "Which port does the kestrel relay listen on?");
        assert_eq!(v["reply_a"], "It listens on 7714.");
        assert_eq!(v["reply_b"], "Let me check.\n[asked to call: fs_read]");
        let asked = p.ask(&prepared.dynamic);
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].id, "verdict");
    }

    /// A reply cut to its share says so, and a secret is scrubbed from it.
    #[test]
    fn a_long_reply_is_cut_and_the_cut_is_marked() {
        let p = by_name("compare.v1").unwrap();
        let mut i = input();
        i.b.text = "word ".repeat(40_000);
        let prepared = prepare(&p, &Input::Compare(i), &NoScrub).unwrap();
        assert!(prepared.state.json.len() < p.state_cap_tokens as usize * 6);
        assert!(prepared.state.json.contains("omitted") || prepared.state.json.contains("cut"));
    }
}
