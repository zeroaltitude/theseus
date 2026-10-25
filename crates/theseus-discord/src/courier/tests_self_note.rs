//! The self notes' buttons reach Discord through both of the lane's component sites
//! (theseus-o1h3f.7): the create (`create`) and the edit (`edit`, a known key written
//! again). A digest naming more items than the cap still makes a valid message.

use theseus_sim::fake_discord::FakeDiscord;

use super::tests_bound::{lane, CHANNEL};
use super::*;

fn items(n: usize) -> Vec<(String, String)> {
    (1..=n)
        .map(|i| (format!("tsk_{i}"), format!("Mend the pier {i}")))
        .collect()
}

fn write(content: &str, n: usize) -> Write {
    Write {
        key: "self:act_one".into(),
        channel: CHANNEL,
        content: content.into(),
        buttons: Buttons::SelfNote(items(n)),
        reply_to: None,
        message: None,
        mentions: vec![],
        ping: false,
    }
}

fn ids(fake: &FakeDiscord) -> Vec<String> {
    let m = fake.messages(CHANNEL).pop().expect("a message");
    m.buttons.into_iter().map(|b| b.custom_id).collect()
}

/// Both sites draw the same buttons: the create of a new key, and the edit of a key whose message
/// had none (an edit leaves components it does not mention, so the edit must carry them itself).
/// Planted: wire one site only, and its message shows none.
#[tokio::test]
async fn both_component_sites_draw_the_buttons() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let want = [
        "snote:drop:tsk_1",
        "snote:drop:tsk_2",
        "snote:drop:tsk_3",
        "snote:halt",
    ];
    lane.write(&write("The night's note.", 3)).await.unwrap();
    assert_eq!(ids(&fake), want, "the create");

    let mut bare = write("A bare note.", 3);
    bare.key = "self:act_two".into();
    bare.buttons = Buttons::Keep;
    lane.write(&bare).await.unwrap();
    assert!(ids(&fake).is_empty(), "made without buttons");
    let mut later = write("A bare note, later.", 3);
    later.key = "self:act_two".into();
    lane.write(&later).await.unwrap();
    let m = fake.messages(CHANNEL).pop().unwrap();
    assert_eq!(m.content, "A bare note, later.");
    assert_eq!(ids(&fake), want, "the edit");
}

/// A note over the cap still posts: at most five buttons a row and five rows, the newest
/// `MAX_DROPS` kept and Halt always there (planted: no cap, and 30 buttons are refused).
#[tokio::test]
async fn a_digest_over_the_cap_still_posts_a_valid_message() {
    let d = tempfile::tempdir().unwrap();
    let fake = FakeDiscord::start();
    let mut lane = lane(d.path(), &fake);
    let cap = crate::runtime::self_note::MAX_DROPS;
    lane.write(&write("The night's note.", 30)).await.unwrap();
    let m = fake.messages(CHANNEL).pop().expect("the note posted");
    let rows = m.components_json.as_array().unwrap();
    assert!(rows.len() <= 5, "{} rows", rows.len());
    for r in rows {
        assert!(r["components"].as_array().unwrap().len() <= 5);
    }
    let got = ids(&fake);
    assert_eq!(got.len(), cap + 1, "{got:?}");
    assert_eq!(got.last().unwrap(), "snote:halt");
    assert_eq!(
        got[0],
        format!("snote:drop:tsk_{}", 30 - cap + 1),
        "the newest kept"
    );
    assert_eq!(got[cap - 1], "snote:drop:tsk_30");
}
