//! A fold keeps each message's author (issue 44): in a channel, the owner's words, another person's and
//! an unmarked message's are each named for what their own mark proves, in the rendered fold and in what
//! the summary's writer reads; a fold of the owner's words alone reads as before.

use super::fold::NoFlags;
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::view::StoreLadder;
use super::{DayPurse, FOLD_STEP, WINDOW_FLOOR};
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

/// A window's worth of the channel's exchanges, due a fold, marked by `who(i)`, the oldest [`FOLD_STEP`]
/// folded: the rendered fold, and what the fold's writer read.
async fn fold(who: impl Fn(u64) -> Option<Option<&'static str>>) -> (String, String) {
    let (_d, s) = store();
    for i in 0..(WINDOW_FLOOR + FOLD_STEP) as u64 {
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
        purse: &purse,
        floor: WINDOW_FLOOR,
    };
    let out = read::fold_step(&s, CHANNEL, None, &step).await.unwrap();
    assert_eq!(out.made.len(), 1, "{:?}", out.failed);
    let read = w.asks.lock().unwrap().last().unwrap().text.clone();
    let nodes = StoreLadder(&s)
        .read(CHANNEL, FOLD_STEP, None)
        .unwrap()
        .nodes;
    let renderer = crate::view::wire_text::TextRenderer {
        store: &s,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
        since: None,
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

/// A node written before its writer was told each message's author (P1-4): its account says the owner
/// asked for what another person asked for. Its title and account are withheld, said so; its excerpt
/// stays, labelled by its own mark. Nothing stored is changed.
#[tokio::test]
async fn an_older_summarys_account_is_withheld_where_it_could_credit_another_to_the_owner() {
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_ch", CHANNEL, 1_000, false);
    mark(&s, &turn, Some("discord:7"));
    let ex = exchange::exchange(&s, &turn).unwrap().unwrap();
    let node = super::fold::FoldNode {
        id: "fld_old".into(),
        place: CHANNEL.into(),
        level: 1,
        exchanges: vec![turn],
        first_ms: ex.start_ms,
        last_ms: ex.start_ms,
        his: vec![super::fold::HisLine {
            node: ex.ask.clone().unwrap(),
            correction: false,
            standing: false,
        }],
        counted: 0,
        children: vec![],
        title: "The owner's gauge".into(),
        account: "The owner asked for gauge 1000 to be recalibrated.".into(),
        marks: super::marks_of(&ex),
        model: Some("fake-small-1".into()),
        writer: 1,
        at_ms: ex.start_ms,
    };
    s.append(&super::fold::records(&node).unwrap()).unwrap();
    let nodes = StoreLadder(&s)
        .read(CHANNEL, FOLD_STEP, None)
        .unwrap()
        .nodes;
    assert_eq!(nodes.len(), 1);
    let renderer = crate::view::wire_text::TextRenderer {
        store: &s,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
        since: None,
    };
    let pov = PointOfView {
        anchor: Anchor::Place(CHANNEL.into()),
        person: Some(exchange::OWNER.into()),
        present: Present::Private,
        at_hand: None,
        now_ms: 0,
    };
    let b = &renderer.ladder(&pov, &nodes).unwrap()[0];
    assert!(!b.text.contains("The owner asked"), "{}", b.text);
    assert!(!b.heading.contains("The owner's gauge"), "{}", b.heading);
    assert!(b
        .text
        .contains("(written before authorship was recorded; not shown)"));
    assert!(b
        .text
        .contains("- another person (not the owner): Recalibrate gauge 1000"));
    let kept = super::fold::node(&s, "fld_old").unwrap().unwrap();
    assert_eq!(kept.account, node.account, "nothing stored is rewritten");
}

/// An older node whose excerpts are the owner's but one whose source node is gone (item 4): a missing
/// message is not proof that it was his, so the account and the title are withheld.
#[tokio::test]
async fn an_older_summary_over_a_missing_message_is_withheld() {
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_ch", CHANNEL, 1_000, false);
    mark(&s, &turn, None);
    let ex = exchange::exchange(&s, &turn).unwrap().unwrap();
    let line = |node: &str| super::fold::HisLine {
        node: node.into(),
        correction: false,
        standing: false,
    };
    let node = super::fold::FoldNode {
        id: "fld_gone".into(),
        place: CHANNEL.into(),
        level: 1,
        exchanges: vec![turn],
        first_ms: ex.start_ms,
        last_ms: ex.start_ms,
        his: vec![line(ex.ask.as_deref().unwrap()), line("nd_gone")],
        counted: 0,
        children: vec![],
        title: "The owner's gauges".into(),
        account: "The owner asked for both gauges.".into(),
        marks: super::marks_of(&ex),
        model: Some("fake-small-1".into()),
        writer: 1,
        at_ms: ex.start_ms,
    };
    s.append(&super::fold::records(&node).unwrap()).unwrap();
    let nodes = StoreLadder(&s)
        .read(CHANNEL, FOLD_STEP, None)
        .unwrap()
        .nodes;
    let renderer = crate::view::wire_text::TextRenderer {
        store: &s,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
        since: None,
    };
    let pov = PointOfView {
        anchor: Anchor::Place(CHANNEL.into()),
        person: Some(exchange::OWNER.into()),
        present: Present::Private,
        at_hand: None,
        now_ms: 0,
    };
    let b = &renderer.ladder(&pov, &nodes).unwrap()[0];
    assert!(!b.text.contains("The owner asked"), "{}", b.text);
    assert!(b
        .text
        .contains("(written before authorship was recorded; not shown)"));
}

/// An older `did` line (item 4) over another person's message reads as not shown, by the one rule the
/// history page and the view's line forms call (`did::shown_line`); over the owner's own, as written.
#[test]
fn an_older_did_line_is_shown_only_over_the_owners_proven_words() {
    let (_d, s) = store();
    let seeded = |who: Option<&'static str>| {
        let turn = exchange_at(&s, "ses_ch", CHANNEL, 1_000, false);
        mark(&s, &turn, who);
        let ex = exchange::exchange(&s, &turn).unwrap().unwrap();
        super::did::DidLine {
            turn_id: turn,
            session_id: ex.session_id.clone(),
            line: "Recalibrated the gauge the owner asked for.".into(),
            model: "fake-small-1".into(),
            writer: 1,
            marks: super::marks_of(&ex),
            cost_usd: 0.0,
            at_ms: ex.start_ms,
        }
    };
    let other = seeded(Some("discord:7"));
    assert_eq!(
        super::did::shown_line(&s, &other),
        super::did::UNAUTHORED_LINE
    );
    let his = seeded(None);
    assert_eq!(super::did::shown_line(&s, &his), his.line);
    let mut current = other;
    current.writer = super::writer::WRITER_VERSION;
    assert_eq!(super::did::shown_line(&s, &current), current.line);
}
