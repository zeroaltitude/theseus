//! A fold keeps each message's author (issue 44): in a channel, the owner's words, another person's and
//! an unmarked message's are each named for what their own mark proves, in the rendered fold and in what
//! the summary's writer reads; a fold of the owner's words alone reads as before.

use super::fold::NoFlags;
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::view::StoreLadder;
use super::{DayPurse, N};
use crate::exchange::{self, OwnerMark, Peer};
use crate::store::Store;
use crate::view::fold::Ladder as _;
use crate::view::{Anchor, PointOfView, Present};

const CHANNEL: &str = "discord:channel:4242";

/// `turn`'s message marked as said by the owner in person, or by chat author `other`.
fn mark(s: &Store, turn: &str, other: Option<&str>) {
    let ex = exchange::exchange(s, turn).unwrap().unwrap();
    let m = OwnerMark {
        node: ex.ask.clone().unwrap(),
        in_person: other.is_none(),
        owner_handle: other.map(|_| false),
        peer: if other.is_some() {
            Peer::Transport
        } else {
            Peer::Person
        },
        agent: None,
        surface: if other.is_some() { "discord" } else { "cli" }.into(),
        place: CHANNEL.into(),
        arrived_session: ex.session_id.clone(),
        ran_session: ex.session_id.clone(),
        author: other.map(str::to_string),
        at_ms: ex.start_ms,
    };
    s.append(&[exchange::mark_record(&m).unwrap()]).unwrap();
}

fn none(_: &str) -> bool {
    false
}

/// `2N` exchanges of the channel, marked by `who(i)`, the oldest `N` folded: the rendered fold, and what
/// the fold's writer read.
async fn fold(who: impl Fn(u64) -> Option<Option<&'static str>>) -> (String, String) {
    let (_d, s) = store();
    for i in 0..2 * N as u64 {
        let t = exchange_at(&s, "ses_ch", CHANNEL, 1_000 + i, false);
        if let Some(other) = who(i) {
            mark(&s, &t, other);
        }
    }
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    let step = Step {
        writer: &w,
        flags: &NoFlags,
        hidden: &none,
        purse: &purse,
    };
    let out = read::fold_step(&s, CHANNEL, None, &step).await.unwrap();
    assert_eq!(out.made.len(), 1, "{:?}", out.failed);
    let read = w.asks.lock().unwrap().last().unwrap().text.clone();
    let nodes = StoreLadder(&s).read(CHANNEL, N, None).unwrap().nodes;
    let renderer = crate::view::wire_text::TextRenderer {
        store: &s,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
    };
    let pov = PointOfView {
        anchor: Anchor::Place(CHANNEL.into()),
        person: Some(exchange::OWNER.into()),
        present: Present::Private,
        at_hand: None,
        now_ms: 0,
    };
    let blocks = renderer.ladder(&pov, &nodes).unwrap();
    assert_eq!(blocks.len(), 1);
    (blocks[0].text.clone(), read)
}

#[tokio::test]
async fn a_channel_fold_names_each_speaker_and_never_relabels_another_as_the_owner() {
    let (text, read) = fold(|i| match i {
        1 => Some(Some("discord:7")),
        2 => None,
        _ => Some(None),
    })
    .await;
    assert!(!text.contains("the owner's words in them"), "{text}");
    assert!(text.contains("each by its author:"), "{text}");
    assert!(
        text.contains("- the owner: Recalibrate gauge 1000"),
        "{text}"
    );
    let other = "- another person (not the owner): Recalibrate gauge 1001";
    assert!(text.contains(other), "{text}");
    let unknown = "- author unknown (not the owner's instruction): Recalibrate gauge 1002";
    assert!(text.contains(unknown), "{text}");
    assert!(
        read.contains("Another person's message (not the owner's): Recalibrate gauge 1001"),
        "{read}"
    );
    assert!(read.contains(
        "author is unknown (not the owner's, nor his instruction): Recalibrate gauge 1002"
    ));
    assert!(
        read.contains("The person's message: Recalibrate gauge 1000"),
        "{read}"
    );
}

#[tokio::test]
async fn a_fold_over_unmarked_messages_says_their_author_is_unknown() {
    let (text, read) = fold(|_| None).await;
    assert!(!text.contains("the owner's words in them"), "{text}");
    assert!(!text.contains("- the owner: "), "{text}");
    let unknown = "- author unknown (not the owner's instruction): Recalibrate gauge 1000";
    assert!(text.contains(unknown), "{text}");
    assert!(!read.contains("The person's message"), "{read}");
}

#[tokio::test]
async fn a_fold_of_the_owners_words_alone_reads_as_before() {
    let (text, _) = fold(|_| Some(None)).await;
    assert!(text.contains(
        "the owner's words in them, word for word (selected):\n- Recalibrate gauge 1000"
    ));
}
