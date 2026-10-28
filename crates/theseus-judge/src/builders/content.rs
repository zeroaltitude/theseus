//! `content.v2` (theseus-9ymz0): one item that entered a session, asked about
//! by itself: what it was, where it came from, and its text, scrubbed and
//! capped. The pack's four questions read the same state. `injection` is not
//! asked of the operator's own words: this builder sets the `untrusted` source
//! from the item's kind, and the pack asks that question only when the source
//! has an item, so the rule is the code's and not a sentence the item's own
//! text could argue with.

use super::*;

/// The most characters of an item one state holds: the pack's cap less the
/// room for the rest of the state, at four characters a token and with a
/// margin for text that is not plain ASCII. The caller splits a longer item
/// into pieces of this size (`content::pieces`, theseus-core), each asked in
/// its own state, so nothing a flag looks for is cut away.
pub const PIECE_CHARS: usize = 6000;

const SOURCE_CHARS: usize = 200;
const KIND_CHARS: usize = 24;

/// The kind of an item that is the operator's own words.
pub const OPERATOR_MESSAGE: &str = "operator_message";

/// The kind of a task's brief: the instructions a parent session gave its
/// child.
pub const TASK_BRIEF: &str = "task_brief";

/// Whether an item of `kind` is asked the `injection` question. **This is the
/// one list**, and `theseus_core::content::ItemKind::asks_injection` calls it,
/// so the rule cannot be stated twice and drift (it did: until the review of
/// 2026-10-26 the core's method was dead code and this site had its own check,
/// so a brief was asked after the design said it must not be).
///
/// Two kinds are exempt, for two different reasons:
/// - the operator's own words (the owner, 2026-10-17: "my own DM or channel
///   text should never cause a flag"). His own, and no one else's: anyone
///   else's message is `person_message` and is asked.
/// - a task's brief, which is a parent instructing its child. It is steering by
///   design, so the question is a guaranteed false positive, and at the act
///   band it would freeze every task session on its first item. Nothing is
///   lost: a parent carrying `injection` is frozen and cannot start a task at
///   all, and a brief quoting foreign text is still asked the other three.
///
/// A task's REPORT is not exempt: a report reports rather than instructs, and a
/// child that read a poisoned page can write its parent a report shaped by it.
pub fn asks_injection(kind: &str) -> bool {
    !matches!(kind, OPERATOR_MESSAGE | TASK_BRIEF)
}

/// `content.v3`'s input: one item, and what the daemon knows about where it
/// came from. The five fields after `data_names` are v3's (theseus-gyjyo.11):
/// a v1 or v2 pack builds its state without them, so a case that carries them
/// reaches those packs exactly as it did before.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentInput {
    /// What entered the session, scrubbed by the caller's `Scrub`.
    pub text: String,
    /// The item's kind, which the pack reads: `operator_message`,
    /// `tool_result`, `fetched_page`, `search_result`, `task_brief`,
    /// `task_report`.
    pub kind: String,
    /// Where it came from, in one line: the tool's name and the resource or
    /// address it named, or the place a message arrived in. Never a secret.
    pub source: String,
    /// Which piece of a long item this is, and of how many.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piece: Option<(u32, u32)>,
    /// The operator's own names for open and private matter.
    #[serde(default, skip_serializing_if = "DataNames::is_empty")]
    pub data_names: DataNames,
    /// Who put it there: `owner` (the operator's own words), `person` (anyone
    /// else's message) or `tool` (what a tool, a page or a search returned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Where the session speaks: `private` or `shared`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// The session's directory, with the home directory shown as `~`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The operator lists the item's tree as open source
    /// (`[privacy] open_source_paths`). Only ever stated when true: a path not
    /// on the list is not thereby private.
    #[serde(default, skip_serializing_if = "is_false")]
    pub public: bool,
    /// `[content_based_security] context`, verbatim: the operator's own
    /// paragraph on what the assistant works on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The state builder's version for a pack of `pack_version`. `content.v1` and
/// `content.v2` share the first (they differ only in wording), so a stored
/// judgment of either is served to the other as it was sent; v3's state carries
/// the five fields and so is a second.
pub fn builder_version(pack_version: u32) -> u32 {
    if pack_version <= 2 {
        CONTENT_VERSION
    } else {
        CONTENT_V3_VERSION
    }
}

const AUTHOR_CHARS: usize = 12;
const PLACE_CHARS: usize = 12;
const PROJECT_CHARS: usize = 160;
const CONTEXT_CHARS: usize = 800;

/// `content.v3`: the item's kind and source, who wrote it, where the session
/// speaks and works, whether the operator calls that tree public, the
/// operator's own paragraph, which piece it is when it is one of several, the
/// operator's names, and the text. A pack of version 2 or less leaves the five
/// fields out.
pub fn content(i: &ContentInput, pack_version: u32, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("content", builder_version(pack_version), cap, scrub);
    b.scalar("kind", c.clip(&i.kind, KIND_CHARS))
        .scalar("source", c.clip(i.source.trim(), SOURCE_CHARS))
        .cut_if("source", c.cut());
    if pack_version > 2 {
        // v3's five (theseus-gyjyo.11), each only when the daemon knows it, so
        // a missing one is absent rather than an empty string the pack must read.
        for (key, value, max) in [
            ("author", &i.author, AUTHOR_CHARS),
            ("place", &i.place, PLACE_CHARS),
            ("project", &i.project, PROJECT_CHARS),
        ] {
            if let Some(v) = value.as_deref().filter(|v| !v.trim().is_empty()) {
                b.scalar(key, c.clip(v.trim(), max)).cut_if(key, c.cut());
            }
        }
        if i.public {
            b.scalar("public", true);
        }
        if let Some(v) = i.context.as_deref().filter(|v| !v.trim().is_empty()) {
            b.scalar("context", c.clip(v.trim(), CONTEXT_CHARS))
                .cut_if("context", c.cut());
        }
    }
    if let Some((n, of)) = i.piece.filter(|(_, of)| *of > 1) {
        b.scalar("piece", n).scalar("pieces", of);
    }
    if !i.data_names.is_empty() {
        let names = |v: &[String]| -> Vec<String> {
            v.iter().take(DATA_NAMES).map(|n| c.clip(n, 60)).collect()
        };
        b.scalar(
            "data_names",
            json!({"open": names(&i.data_names.open), "private": names(&i.data_names.private)}),
        )
        .cut_if("data_names", c.cut());
    }
    b.text("text", 9, share(cap, 75), Keep::Both, &i.text);
    let mut dynamic = Dynamic::default();
    if asks_injection(&i.kind) {
        dynamic.sources.insert(
            Source::Untrusted,
            vec![Item {
                key: "untrusted".into(),
                text: "not the operator's own words".into(),
            }],
        );
    }
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::band::Thresholds;
    use crate::pack::by_name;
    use crate::state::NoScrub;

    fn input(kind: &str) -> ContentInput {
        ContentInput {
            text: "ordinary text".into(),
            kind: kind.into(),
            source: "fs.read notes/plan.md".into(),
            piece: None,
            ..ContentInput::default()
        }
    }

    fn asked(i: &ContentInput) -> Vec<String> {
        let p = by_name("content.v2").unwrap();
        let prepared = prepare(&p, &Input::Content(i.clone()), &NoScrub).unwrap();
        p.ask(&prepared.dynamic).into_iter().map(|a| a.id).collect()
    }

    /// The pack is what design §1 says: where it runs, what builds its state,
    /// four deciding yes-or-no questions with `no` when nothing fits, and the
    /// ordinary bands. The bars the flags raise at are the code's
    /// (`content::Flag::bar`), so no number here differs by question.
    #[test]
    fn the_pack_is_what_the_design_says() {
        let p = by_name("content.v2").expect("content.v2 is embedded");
        assert_eq!(p.jev_model, "jev-1.13.0");
        assert_eq!(p.point, crate::pack::Point::Item);
        assert_eq!(p.builder, Builder::Content);
        assert_eq!(p.state_cap_tokens, 3000);
        assert_eq!(p.sample, 1.0);
        assert_eq!(p.baseline, crate::pack::Baseline::None);
        assert_eq!(p.action, crate::pack::Action::None);
        assert!(
            p.rollback.is_empty(),
            "it starts nothing, so nothing rolls it back"
        );
        let ids: Vec<&str> = p.questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, ["confidential", "injection", "personal", "secret"]);
        for q in &p.questions {
            assert_eq!(q.kind, crate::client::Kind::Choice, "{}", q.id);
            assert!(q.decides, "{}", q.id);
            assert_eq!(
                q.thresholds,
                Thresholds {
                    act: 0.90,
                    confirm: 0.60
                },
                "{}",
                q.id
            );
            assert_eq!(q.no_match.as_deref(), Some("no"), "{}", q.id);
            let options: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
            assert_eq!(options, ["yes", "no"], "{}", q.id);
        }
        assert_eq!(
            p.question("injection").unwrap().only_when,
            Some(Source::Untrusted),
            "injection alone is conditional"
        );
        for q in ["confidential", "personal", "secret"] {
            assert_eq!(
                p.question(q).unwrap().only_when,
                None,
                "{q} is always asked"
            );
        }
    }

    /// The operator's own message is never asked about injection, and is asked
    /// the other three; every other kind is asked all four. The rule is in the
    /// state's `untrusted` source, so it is the builder's and not the pack's
    /// wording.
    #[test]
    fn the_injection_question_is_asked_of_everything_but_the_operators_words() {
        // The two exempt kinds, for two different reasons: the owner's own
        // words, and a brief, which is a parent instructing its child and so
        // is steering by design.
        for kind in ["operator_message", "task_brief"] {
            assert_eq!(
                asked(&input(kind)),
                ["confidential", "personal", "secret"],
                "{kind}"
            );
            assert!(!asks_injection(kind), "{kind}");
        }
        // Everything else is asked, a REPORT included: a report reports rather
        // than instructs, and a child that read a poisoned page can write its
        // parent a report shaped by it.
        for kind in [
            "person_message",
            "tool_result",
            "fetched_page",
            "search_result",
            "task_report",
        ] {
            assert!(asks_injection(kind), "{kind}");
            assert_eq!(
                asked(&input(kind)),
                ["confidential", "injection", "personal", "secret"],
                "{kind}"
            );
        }
        assert_eq!(
            asked(&input("some_kind_nobody_wrote_yet")),
            ["confidential", "injection", "personal", "secret"],
            "an unknown kind is asked everything"
        );
    }

    /// The state holds the kind, the source and the text, says which piece of
    /// how many when the item is one of several, carries the operator's names
    /// only when given, and holds the text in a state of a fixed size.
    #[test]
    fn the_state_holds_the_item_its_kind_its_source_and_its_piece() {
        let p = by_name("content.v2").unwrap();
        let mut i = input("tool_result");
        let v = prepare(&p, &Input::Content(i.clone()), &NoScrub)
            .unwrap()
            .state
            .value();
        assert_eq!(v["kind"], "tool_result");
        assert_eq!(v["source"], "fs.read notes/plan.md");
        assert_eq!(v["text"], "ordinary text");
        for absent in ["piece", "pieces", "data_names"] {
            assert!(v.get(absent).is_none(), "{absent} is left out when unused");
        }
        i.piece = Some((2, 3));
        i.data_names = DataNames {
            open: vec!["Widgetco".into()],
            private: vec!["Acme".into()],
        };
        let v = prepare(&p, &Input::Content(i.clone()), &NoScrub)
            .unwrap()
            .state
            .value();
        assert_eq!(
            (v["piece"].as_u64(), v["pieces"].as_u64()),
            (Some(2), Some(3))
        );
        assert_eq!(v["data_names"]["private"], serde_json::json!(["Acme"]));
        // One piece of one is not a piece.
        i.piece = Some((1, 1));
        let v = prepare(&p, &Input::Content(i), &NoScrub)
            .unwrap()
            .state
            .value();
        assert!(v.get("piece").is_none());
    }

    /// `content.v3`'s state carries the five fields the operator's daemon knows
    /// (theseus-gyjyo.11) and a v2 pack's leaves them out, so a case that carries
    /// them reaches v2 as it did before. `public` is stated only when true.
    #[test]
    fn the_v3_state_carries_the_five_fields_and_the_v2_state_does_not() {
        let mut i = input("tool_result");
        i.author = Some("tool".into());
        i.place = Some("private".into());
        i.project = Some("~/projects/example-app".into());
        i.public = true;
        i.context = Some("A personal assistant. ~/projects/example-app is open source.".into());
        let build = |pack: &str| {
            let p = by_name(pack).unwrap();
            prepare(&p, &Input::Content(i.clone()), &NoScrub)
                .unwrap()
                .state
        };
        let v3 = build("content.v3");
        let v = v3.value();
        assert_eq!(v["author"], "tool");
        assert_eq!(v["place"], "private");
        assert_eq!(v["project"], "~/projects/example-app");
        assert_eq!(v["public"], true);
        assert_eq!(
            v["context"],
            "A personal assistant. ~/projects/example-app is open source."
        );
        assert_eq!(v3.builder_version, CONTENT_V3_VERSION);
        for old in ["content.v1", "content.v2"] {
            let s = build(old);
            for absent in ["author", "place", "project", "public", "context"] {
                assert!(s.value().get(absent).is_none(), "{old} holds {absent}");
            }
            assert_eq!(s.builder_version, CONTENT_VERSION, "{old}");
        }
        // Left unset, and `public` false, none is stated.
        let bare = prepare(
            &by_name("content.v3").unwrap(),
            &Input::Content(input("tool_result")),
            &NoScrub,
        )
        .unwrap()
        .state
        .value();
        for absent in ["author", "place", "project", "public", "context"] {
            assert!(bare.get(absent).is_none(), "{absent}");
        }
    }

    /// The five fields are scrubbed like the rest, and the paragraph is cut so a
    /// long one cannot crowd the item out of its state.
    #[test]
    fn the_operators_paragraph_is_scrubbed_and_cut() {
        struct Hide;
        impl Scrub for Hide {
            fn scrub(&self, text: &str) -> String {
                text.replace("hunter2", "[hidden]")
            }
        }
        let mut i = input("tool_result");
        i.context = Some(format!("password hunter2 {}", "word ".repeat(1000)));
        i.project = Some("~/projects/hunter2".into());
        let built = prepare(&by_name("content.v3").unwrap(), &Input::Content(i), &Hide)
            .unwrap()
            .state;
        assert!(!built.json.contains("hunter2"), "{}", built.json);
        assert!(
            built.value()["context"].as_str().unwrap().chars().count() <= CONTEXT_CHARS + 1,
            "the paragraph is cut"
        );
    }

    /// Which builder version a pack's states carry.
    #[test]
    fn the_builder_version_is_the_packs() {
        assert_eq!(builder_version(1), CONTENT_VERSION);
        assert_eq!(builder_version(2), CONTENT_VERSION);
        assert_eq!(builder_version(3), CONTENT_V3_VERSION);
        assert_eq!(
            builder_version(101),
            CONTENT_V3_VERSION,
            "a learned candidate of v3"
        );
        assert_ne!(CONTENT_VERSION, CONTENT_V3_VERSION);
    }

    /// A text past the cap is cut by the builder, never the state's size, and
    /// a piece the caller made (`PIECE_CHARS`) fits whole.
    #[test]
    fn a_piece_fits_the_state_whole() {
        let p = by_name("content.v2").unwrap();
        let line = "the quick brown fox jumps over the lazy dog\n";
        let text: String = line.repeat(PIECE_CHARS / line.len());
        assert!(text.len() <= PIECE_CHARS);
        let mut i = input("fetched_page");
        i.text = text.clone();
        let built = prepare(&p, &Input::Content(i), &NoScrub).unwrap().state;
        assert_eq!(
            built.value()["text"],
            text.as_str(),
            "a piece is held whole"
        );
        assert!(!built.json.is_empty());
        let mut big = input("fetched_page");
        big.text = line.repeat(10 * PIECE_CHARS / line.len());
        let built = prepare(&p, &Input::Content(big), &NoScrub).unwrap().state;
        assert!(
            built.json.len() <= p.state_cap_tokens as usize * 4,
            "{} bytes",
            built.json.len()
        );
    }

    /// The input round-trips through its JSON, as a fixture's does, and an
    /// unknown field is refused.
    #[test]
    fn the_input_round_trips_through_json() {
        let mut i = input("search_result");
        i.piece = Some((1, 2));
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(
            Input::parse(Builder::Content, &json).unwrap(),
            Input::Content(i)
        );
        assert!(Input::parse(
            Builder::Content,
            r#"{"text":"x","kind":"tool_result","source":"s","extra":1}"#
        )
        .is_err());
    }

    /// The operator's words are scrubbed before they are cut, as every
    /// builder's are.
    #[test]
    fn a_secret_is_scrubbed_before_the_state_is_built() {
        struct Hide;
        impl Scrub for Hide {
            fn scrub(&self, text: &str) -> String {
                text.replace("hunter2", "[hidden]")
            }
        }
        let p = by_name("content.v2").unwrap();
        let mut i = input("tool_result");
        i.text = "password is hunter2".into();
        let built = prepare(&p, &Input::Content(i), &Hide).unwrap().state;
        assert!(!built.json.contains("hunter2"), "{}", built.json);
        assert!(built.json.contains("[hidden]"));
    }
}
