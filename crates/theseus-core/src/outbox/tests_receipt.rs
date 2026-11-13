//! Delivery receipts (theseus-vug; M5's clause in `docs/spec/06-part1-s10.md`,
//! "delivery recorded as `posted | transport_failed | never_posted`, never
//! `seen`").
//!
//! What these hold: the word is the settler's, written into the post's own
//! record; the index counts the settles on both axes and the two totals
//! agree, so no settled post ends in no state; and the line every surface
//! shows says which of the three it was.

use serde_json::json;
use theseus_kernel::{ActionState, Kernel, Outcome};
use theseus_protocol::outbox::Receipt;

use super::tests::outbox;
use super::{receipt_of, Outbox};

/// One post to a Discord DM, written and dispatched.
fn post(o: &Outbox, n: u32) -> String {
    let a = o
        .post(
            "ses_wren",
            "exe_wren",
            "discord:dm:4210",
            json!({"kind": "notice", "text": format!("note {n}")}),
        )
        .expect("the post is written");
    o.dispatch(&a.correlation_id).expect("dispatched");
    a.correlation_id
}

/// Every settled post carries the receipt its settler wrote, read back from
/// the post's own record and never rebuilt from its state.
#[test]
fn each_settle_records_the_word_its_settler_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let o = outbox(&dir);
    let cases = [
        (Outcome::Succeeded, Receipt::Posted),
        (Outcome::Failed, Receipt::TransportFailed),
        (Outcome::Succeeded, Receipt::NeverPosted),
    ];
    assert_eq!(cases.len(), Receipt::ALL.len(), "one case per receipt");
    for (n, (outcome, receipt)) in cases.into_iter().enumerate() {
        let id = post(&o, n as u32);
        o.settle(&id, outcome, receipt, None, json!({}), "discord")
            .expect("the settle is taken");
        let a = o.kernel.outbox_action(&id).unwrap().expect("its record");
        assert_eq!(
            receipt_of(&a),
            Some(receipt),
            "post {id} records what settled it"
        );
    }
}

/// The index counts every settled post on both axes — the outcome's, which
/// health has always shown, and the receipt's — and the two totals agree.
/// That is "no post ends in no state", counted: a settle that recorded no
/// receipt would be `unsaid` and the test names it.
#[test]
fn the_receipts_account_for_every_settled_post() {
    let dir = tempfile::tempdir().unwrap();
    let o = outbox(&dir);
    for (n, (outcome, receipt)) in [
        (Outcome::Succeeded, Receipt::Posted),
        (Outcome::Succeeded, Receipt::Posted),
        (Outcome::Failed, Receipt::TransportFailed),
        (Outcome::Failed, Receipt::NeverPosted),
    ]
    .into_iter()
    .enumerate()
    {
        let id = post(&o, n as u32);
        o.settle(&id, outcome, receipt, None, json!({}), "discord")
            .unwrap();
    }
    // One post left open: it has no receipt yet and is in neither total.
    let open = post(&o, 9);
    let c = o
        .peek(|ix| ix.counts.get("discord").cloned().unwrap_or_default())
        .expect("the index is read");
    assert_eq!((c.posted, c.transport_failed, c.never_posted), (2, 1, 1));
    assert_eq!(c.unsaid, 0, "every settle here said its receipt");
    assert_eq!(c.sent + c.failed, 4, "four posts settled");
    assert_eq!(c.settled(), c.sent + c.failed, "the receipts total them");
    let st = o.status("discord");
    assert_eq!((st.sent, st.failed, st.pending), (2, 2, 1));
    assert!(
        receipt_of(&o.kernel.outbox_action(&open).unwrap().unwrap()).is_none(),
        "an open post has no receipt"
    );
}

/// A post for a place no longer bound is settled by the core, which sent
/// nothing: the honest word is `never_posted`, and the line every surface
/// shows leads with it.
#[test]
fn a_post_for_an_unbound_place_is_never_posted_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let o = outbox(&dir);
    let id = post(&o, 1);
    assert_eq!(o.refuse_unbound("discord", |_| false), 1);
    let a = o.kernel.outbox_action(&id).unwrap().unwrap();
    assert_eq!(a.state, ActionState::Failed);
    assert_eq!(receipt_of(&a), Some(Receipt::NeverPosted));
    let st = o.status("discord");
    assert_eq!((st.sent, st.failed), (0, 1));
    let said = st.last_error.expect("a refusal is an error line");
    assert!(
        said.starts_with("never posted: ") && said.contains("not bound here any more"),
        "the receipt leads the line a surface shows: {said}"
    );
}

/// A transport failure says the other thing: it was sent and the channel
/// refused it. The distinction is the whole point of the vocabulary, so the
/// line a surface shows must carry it.
#[test]
fn a_transport_failure_says_it_was_sent() {
    let dir = tempfile::tempdir().unwrap();
    let o = outbox(&dir);
    let id = post(&o, 1);
    o.settle(
        &id,
        Outcome::Failed,
        Receipt::TransportFailed,
        None,
        json!({"error": "50013: missing permissions"}),
        "discord",
    )
    .unwrap();
    let said = o.status("discord").last_error.expect("an error line");
    assert_eq!(said, "transport failed: 50013: missing permissions");
}

/// The index is a projection: read again from the store, a settled post's
/// receipt is the one its record carries, not one rebuilt from its state.
/// A post settled before receipts existed has none, and is counted `unsaid`
/// rather than given a word nothing wrote.
#[test]
fn a_fresh_index_reads_the_recorded_receipts_and_invents_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = {
        let o = outbox(&dir);
        let posted = post(&o, 1);
        o.settle(
            &posted,
            Outcome::Succeeded,
            Receipt::Posted,
            None,
            json!({}),
            "discord",
        )
        .unwrap();
        let never = post(&o, 2);
        o.settle(
            &never,
            Outcome::Failed,
            Receipt::NeverPosted,
            None,
            json!({"error": "no channel"}),
            "discord",
        )
        .unwrap();
        // A post settled the way a build before receipts settled it: the
        // kernel refuses one now, so the record is written here as that
        // build left it.
        let old = post(&o, 3);
        let mut a = o.kernel.outbox_action(&old).unwrap().unwrap();
        a.state = ActionState::Failed;
        a.settled_at_ms = Some(1_000);
        a.detail = Some(json!({"error": "a refusal from before receipts"}));
        o.store
            .append(&[theseus_store::NewRecord::json(
                theseus_store::kinds::OUTBOX,
                Some(&a.correlation_id),
                &a,
            )
            .unwrap()
            .scoped("ses_wren")])
            .unwrap();
        o.store.clone()
    };
    let kernel = std::sync::Arc::new(Kernel::new(
        store.shared(),
        std::sync::Arc::new(theseus_kernel::RealClock),
        Default::default(),
    ));
    let again = Outbox::new(store, kernel);
    again.warm();
    let c = again
        .peek(|ix| ix.counts.get("discord").cloned().unwrap_or_default())
        .expect("the index is read");
    assert_eq!((c.posted, c.never_posted), (1, 1), "read back as recorded");
    assert_eq!(c.transport_failed, 0, "nothing invents the other word");
    assert_eq!(c.unsaid, 1, "the one from before receipts says nothing");
    assert_eq!(c.settled(), 3);
    assert_eq!(c.sent + c.failed, 3, "both axes count every settled post");
}
