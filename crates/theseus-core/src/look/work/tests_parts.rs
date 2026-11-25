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
