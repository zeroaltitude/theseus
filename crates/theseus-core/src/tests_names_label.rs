//! `[privacy] sensitive_names` (theseus-gyjyo.4, theseus-oejh3): a name is a
//! label source, not a tripwire. An item that holds one raises `confidential`
//! on its session where it enters, and a pack whose text holds one is said at
//! start by the pack, never by the name. The rig is `tests_route`'s.

use crate::content::{Flag, ItemKind};
use crate::node::Node;
use crate::session::SessionRecord;
use crate::tests_route::rig;
use theseus_protocol::SessionKind;

/// Test 13: a names hit is a label (theseus-gyjyo.4). An item that holds a
/// sensitive name raises `confidential` on its session at its entry, with no
/// judge behind it, and names the word as its source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_names_hit_is_a_label() {
    let named = "Rename the call sites in Examplecorp's billing module.";
    let r = rig(None, 1, |c| {
        c.privacy.sensitive_names = vec!["Examplecorp".into()];
    });
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;
    let node = Node::user(&sid, Some("trn_1"), "cli", named);
    r.core.store.append(&[node.record().unwrap()]).unwrap();
    let item = crate::judge::content::ContentItem::of_message(
        &node,
        ItemKind::OperatorMessage,
        "operator message",
    )
    .unwrap();
    r.core.runner.judge.ask_content(&sid, vec![item]);
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let flags = rec.data.as_deref().cloned().unwrap_or_default();
    assert_eq!(
        flags.label(Flag::Confidential).map(|l| l.source.as_str()),
        Some("name: Examplecorp")
    );
}

/// A configured name that an embedded pack's text holds is said at start by
/// the pack and the number of hits, never by the name (theseus-oejh3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pack_naming_a_sensitive_name_is_reported_at_start() {
    let (_, text) = theseus_judge::pack::EMBEDDED
        .iter()
        .find(|(name, _)| *name == "route.v12")
        .expect("route.v12 is embedded");
    let word = text
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.chars().count() > 9)
        .expect("a long word in the pack");
    let word = word.to_string();
    let r = rig(None, 0, |c| c.privacy.sensitive_names = vec![word.clone()]);
    r.core.check_pack_text_after_serving();
    let t0 = std::time::Instant::now();
    let phase = loop {
        let found = r
            .core
            .startup_log
            .snapshot()
            .into_iter()
            .find(|p| p.name == crate::pack_names::PHASE);
        if let Some(p) = found {
            break p;
        }
        assert!(t0.elapsed().as_secs() < 10, "no phase for the hit");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    let said = phase.detail.to_string();
    assert!(said.contains("route.v12"), "{said}");
    assert!(
        !said.to_lowercase().contains(&word.to_lowercase()),
        "the name is never said: {said}"
    );
}
