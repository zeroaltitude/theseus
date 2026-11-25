//! The look's history part written with its parts (issue 49): what the one renderer makes of them is byte for
//! byte its text, which is the text a build before parts wrote; a message's words are quotes, and the whole
//! answer's cuts keep the head of a quote they fall in. All words invented.

use serde_json::json;

use super::*;
use crate::node::{Body, Node};
use crate::quotes::{self, Part};
use crate::tests_places::{rig_scripted, session};

/// A reply of `model` saying `text`.
fn reply(sid: &str, text: &str) -> Node {
    Node::assistant(
        sid,
        "trn_1",
        0,
        Body::AssistantMessage {
            blocks: vec![
                json!({"type": "thinking", "thinking": "not shown"}),
                json!({"type": "text", "text": text}),
            ],
            model: "m".into(),
            provider: "p".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    )
}

#[tokio::test]
async fn look_parts_render_byte_identical() {
    let r = rig_scripted(|_| {}, |_| crate::provider::Scripted::text("ok"));
    let sid = session(&r.core, None);
    let long = "the kittiwake lantern log ".repeat(30);
    r.core
        .store
        .append(&[
            Node::user(&sid, Some("trn_1"), "cli", "check the\n\tharbour  lantern")
                .record()
                .unwrap(),
            reply(&sid, &long).record().unwrap(),
        ])
        .unwrap();
    let cuts = crate::scrub::Cuts::none();
    let (w, shown) = history::history(&r.core, &sid, MESSAGES, &cuts);
    let (text, parts) = w.into_parts();
    // The text a build before parts wrote for these messages.
    assert_eq!(text, include_str!("tests_parts/look.txt"));
    assert_eq!(shown.len(), 2);
    let rendered = quotes::render_with(&parts, |q| quotes::read(&r.core.store, q));
    assert_eq!(rendered, text);
    let quoted = parts.iter().filter(|p| matches!(p, Part::Quote(_))).count();
    assert_eq!(quoted, 2, "{parts:#?}");
    for p in &parts {
        if let Part::Code(c) = p {
            assert!(!c.contains("lantern"), "a message's word in code: {c:?}");
        }
    }
    // The answer's cut falls inside the reply's words: its parts keep the head the text keeps.
    let (whole, _) = assembled(
        vec![(history::history(&r.core, &sid, MESSAGES, &cuts).0, shown)],
        &cuts,
    );
    let (cut, _) = cut_kept(whole, 100, &cuts);
    let (text, parts) = cut.into_parts();
    assert!(
        text.ends_with("[cut at 100 characters; ask for fewer messages or one part]\n"),
        "{text}"
    );
    let rendered = quotes::render_with(&parts, |q| quotes::read(&r.core.store, q));
    assert_eq!(rendered, text);
}

/// The look's message line and the whole answer's cut, through its writer, with a made-up value across each: the line
/// clipped at its 600 characters and the answer cut at its allowance never store a range that begins or ends inside
/// the value, and what they show holds none of it but whole.
#[tokio::test]
async fn a_looks_quotes_never_begin_or_end_inside_a_known_value() {
    const VALUE: &str = "kelp-ferry-0417-vault-token";
    let r = rig_scripted(|_| {}, |_| crate::provider::Scripted::text("ok"));
    let sid = session(&r.core, None);
    let said = format!("{} {VALUE} by the weir", "t".repeat(LINE_CHARS - 10));
    let answer = format!("{} {VALUE} and the mill", "w".repeat(300));
    r.core
        .store
        .append(&[
            Node::user(&sid, Some("trn_1"), "cli", &said)
                .record()
                .unwrap(),
            reply(&sid, &answer).record().unwrap(),
        ])
        .unwrap();
    let cuts = crate::scrub::Scrubber::with_values(vec![(VALUE.into(), "v".into())]).cuts();
    let read = |q: &crate::quotes::Quote| quotes::read_as_written(&r.core.store, q);
    let (w, shown) = history::history(&r.core, &sid, MESSAGES, &cuts);
    let (text, parts) = w.clone().into_parts();
    assert_eq!(quotes::render_with(&parts, read), text);
    assert!(
        quotes::cut_through(&parts, read, &[VALUE]).is_empty(),
        "{parts:#?}"
    );
    assert!(quotes::no_part_of(&text, VALUE), "{text}");
    // The whole answer cut inside the reply's value.
    let (whole, _) = assembled(vec![(w, shown)], &cuts);
    let at = whole.text().rfind(VALUE).unwrap() + 9;
    let max = whole.text()[..at].chars().count();
    let (cut, _) = cut_kept(whole, max, &cuts);
    let (text, parts) = cut.into_parts();
    assert!(text.contains("[cut at "), "{text}");
    assert_eq!(quotes::render_with(&parts, read), text);
    assert!(
        quotes::cut_through(&parts, read, &[VALUE]).is_empty(),
        "{parts:#?}"
    );
    assert!(quotes::no_part_of(&text, VALUE), "{text}");
}
