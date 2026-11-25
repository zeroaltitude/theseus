//! EVERYWHERE through real turns (issue 48; `view::select::everywhere`): a line from another place, a guest's
//! in a shared place too, is laid beside the message headed with its speaker's name; an exchange elsewhere
//! leaves the cached front as it was.

use serde_json::Value;

use crate::provider::binding::unmarked;
use crate::provider::ProviderRequest;
use crate::tests_places::session;
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, texts, turn_as, view_rig};
use crate::tests_view_people::{beside, far_session, guest, hold, said_by, GUEST};

/// The words said in another place.
const WORDS: &str = "far: the tide turns at noon";

fn plain(q: &ProviderRequest) -> Vec<Value> {
    q.messages.iter().map(unmarked).collect()
}

/// KEY: a guest's words in a shared place are among the lines from every place on the owner's turn, headed
/// with their name, in the place they were said in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_places_words_are_in_everywhere_attributed_to_their_speaker() {
    let r = view_rig();
    let c = &r.core;
    hold(c, GUEST, "Wren Hallam");
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: who else?", true).await;
    let far = far_session(&c.store);
    let now = theseus_protocol::now_unix_ms();
    let t = said_by(
        &c.store,
        (&far, "discord:channel:509", false),
        &guest(),
        now - 60_000,
        "Ferry at four?",
    );
    turn_as(c, &desk, "desk: and now?", true).await;
    let b = beside(&asked(&r, "desk: and now?").pop().unwrap());
    let end = (b.find(&format!("(open {t})"))).unwrap_or_else(|| panic!("{t} is not laid: {b}"));
    let start = (b[..end].rfind("\n\noutside text from "))
        .unwrap_or_else(|| panic!("{t}'s line is not headed with its words: {b}"))
        + 2;
    let line = &b[start..end];
    assert!(
        line.starts_with("outside text from \"Wren Hallam\""),
        "{line}"
    );
    assert!(line.contains("Ferry at four?"), "{line}");
    assert!(b.contains("discord:channel:509"), "{b}");
}

/// KEY: the lines from every place are beside the message, never in the front: an exchange elsewhere between
/// two turns here is in the later turn's block beside the message, and the front before it is the earlier
/// turn's, byte for byte.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn everywhere_is_beside_the_message_and_the_front_is_unchanged_by_activity_elsewhere() {
    let r = view_rig();
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: first light", true).await;
    turn_as(c, &desk, "desk: second light", true).await;
    let far = session(c, Some("local:/far"));
    turn_as(c, &far, WORDS, true).await;
    turn_as(c, &desk, "desk: third light", true).await;
    let two = asked(&r, "desk: second light").pop().unwrap();
    let three = asked(&r, "desk: third light").pop().unwrap();
    assert!(!texts(&two).contains(WORDS), "{}", texts(&two));
    assert!(beside(&three).contains(WORDS), "{}", beside(&three));
    let (a, b) = (plain(&two), plain(&three));
    let front = a.len() - 1;
    assert_eq!(b[..front], a[..front], "the front holds");
    assert_eq!(three.system, two.system);
    let fronts = serde_json::to_string(&b[..front]).unwrap();
    assert!(!fronts.contains(WORDS), "{fronts}");
}
