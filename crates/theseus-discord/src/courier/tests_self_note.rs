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

/// Both sites draw the same buttons: three Drops and a Halt on the create, and again on the edit
/// of the same key (planted: wire one site only, and the other shows none).
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
    // The same key, other words: the message is edited, and the buttons are sent again.
    lane.write(&write("The night's note, later.", 3))
        .await
        .unwrap();
    let m = fake.messages(CHANNEL).pop().unwrap();
    assert_eq!(m.content, "The night's note, later.");
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
