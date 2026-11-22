//! A message a split or a claim moved into a thread (issue 44, the node seal): no `did` line is written
//! from its exchange's words and its fold is made by code, read by no model; and the helpers the view's
//! tests of the same rule share (a place's history, a fold built before a move, the move itself).

use super::did::{self, Outcome};
use super::fold::NoFlags;
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::{DayPurse, N};
use crate::exchange;
use crate::store::Store;

pub(crate) const PLACE: &str = "discord:dm:42";
pub(crate) const SID: &str = "ses_dm";

/// A place's ended exchanges, `n` of them, oldest first.
pub(crate) fn history(s: &Store, n: u64) -> Vec<String> {
    (0..n)
        .map(|i| exchange_at(s, SID, PLACE, 1_000 + i, false))
        .collect()
}

/// Thread `slug`'s record, sealed when `sealed`, and a session bound to it; that session's id.
pub(crate) fn thread(s: &Store, slug: &str, sealed: bool) -> String {
    let r = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    s.put_session(&r.session_id, &r).unwrap();
    let mut t = serde_json::json!({"slug": slug, "title": slug});
    if sealed {
        t["sealed"] = serde_json::json!({"by": "cli", "at_ms": 1, "reason": "embargo"});
    }
    s.put_meta(&format!("thread:{slug}"), &t).unwrap();
    s.put_meta(&format!("discord.session.thread:{slug}"), &r.session_id)
        .unwrap();
    r.session_id
}

/// `turn`'s message moved into thread `slug`, as a split records it.
pub(crate) fn move_ask(s: &Store, turn: &str, slug: &str) {
    let ask = exchange::exchange(s, turn).unwrap().unwrap().ask.unwrap();
    let to = serde_json::json!({"thread": slug, "split": "spl_x"});
    s.put_meta(&crate::moved::moved_of_key(&ask), &to).unwrap();
}

fn shown(_: &str) -> bool {
    false
}

/// `2N` exchanges, the oldest `N` folded (by the fake writer); the turns.
pub(crate) async fn folded(s: &Store) -> Vec<String> {
    let turns = history(s, 2 * N as u64);
    let w = Fake::default();
    let purse = DayPurse::new(s, None, false);
    let step = Step {
        writer: &w,
        flags: &NoFlags,
        hidden: &shown,
        purse: &purse,
    };
    let out = read::fold_step(s, PLACE, None, &step).await.unwrap();
    assert_eq!(out.made.len(), 1, "{:?}", out.failed);
    turns
}

#[tokio::test]
async fn a_moved_message_gets_no_did_line_from_its_words() {
    let (_d, s) = store();
    let turns = history(&s, 2);
    move_ask(&s, &turns[0], "kelp");
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    let moved = did::write(&s, &w, &turns[0], &shown, &purse).await.unwrap();
    assert!(matches!(moved, Outcome::Hidden), "{moved:?}");
    assert!(w.asks.lock().unwrap().is_empty(), "no model read it");
    let kept = did::write(&s, &w, &turns[1], &shown, &purse).await.unwrap();
    assert!(matches!(kept, Outcome::Written(_)), "{kept:?}");
}

#[tokio::test]
async fn a_fold_over_a_moved_message_is_made_by_code() {
    let (_d, s) = store();
    let turns = history(&s, 2 * N as u64);
    move_ask(&s, &turns[3], "kelp");
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    let step = Step {
        writer: &w,
        flags: &NoFlags,
        hidden: &shown,
        purse: &purse,
    };
    let out = read::fold_step(&s, PLACE, None, &step).await.unwrap();
    assert_eq!(out.made.len(), 1);
    assert!(
        w.asks.lock().unwrap().is_empty(),
        "no model read the moved message"
    );
}
