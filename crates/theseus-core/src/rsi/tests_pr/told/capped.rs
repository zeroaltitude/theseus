//! A red gate is announced once a night per item, and the try that reaches the cap is the
//! item's one notice (issue 6, conditions 4 and 5), through the whole call.

use super::*;

/// KEY: the second and third red of a night are not posted on their own account; the third
/// reaches the cap and is the one notice, with every reason; and a fourth call is refused.
#[tokio::test]
async fn reds_inside_a_night_are_one_post_and_the_cap_is_one_notice() {
    let (r, sid) = rig_with_task();
    r.open_and_edit(&sid, "broken.txt").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(
        thread_posts(&r, "self_build_red").len(),
        1,
        "the first red is told"
    );
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(
        thread_posts(&r, "self_build_red").len(),
        1,
        "the second, in the same night, is only in the note"
    );
    assert!(
        operator_posts(&r, "self_digest").is_empty(),
        "inside the cap, no notice"
    );
    assert_eq!(
        r.core.folded_read().ended.len(),
        1,
        "the item is in the note"
    );
    // The third reaches the cap of three.
    r.turn(&sid, WIRE, propose()).await;
    assert_eq!(thread_posts(&r, "self_build_red").len(), 1, "still the one");
    let notices = operator_posts(&r, "self_digest");
    assert_eq!(notices.len(), 1, "{notices:?}");
    let text = notices[0]["text"].as_str().unwrap();
    for want in [
        "did not work out after 3 tries",
        "- try 3:",
        "theseus self retry trim-the-sails",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    assert!(
        task_of(&r)
            .evidence
            .iter()
            .any(|e| e.identity == "failed:cap"),
        "the item says it was capped"
    );
    // No fourth build: the tool says why instead of making a tree.
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("has used all its tries"), "{said}");
    assert_eq!(
        operator_posts(&r, "self_digest").len(),
        1,
        "and says nothing more"
    );
}
