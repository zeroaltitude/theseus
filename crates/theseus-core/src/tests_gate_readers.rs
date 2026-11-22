//! The readers on the old path behind the gate (issue 44, round 2): what an
//! earlier request sent is admitted again before it is sent again.

use theseus_protocol::threads::{ThreadNewParams, ThreadSealParams};

use crate::approval::{Answerer, Surface};
use crate::provider::ProviderRequest;
use crate::tests_places::{rig, session, Rig};
use crate::tests_view_gate::turn_as;
use crate::turn::env_tail::HEAD;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// How many date lines the latest request carries: its own tail's, and one
/// for each earlier tail put back.
fn dates(r: &Rig) -> usize {
    let last: ProviderRequest = r.requests().pop().expect("a request");
    serde_json::to_string(&last.messages)
        .unwrap()
        .matches(HEAD)
        .count()
}

fn thread(r: &Rig, slug: &str) {
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: slug.into(),
                title: Some(slug.into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
}

/// A tail is put back for the presence it was made for, and only that: the
/// owner's second turn at the terminal carries his first turn's tail where
/// it was sent; an agent's turn after him, at the same terminal, is not sent
/// it; and the owner, back, is sent his again.
#[tokio::test]
async fn a_kept_tail_is_sent_again_only_to_who_it_was_made_for() {
    let r = rig();
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "the first word", true).await;
    turn_as(&r.core, &desk, "the second word", true).await;
    assert_eq!(dates(&r), 2, "the owner's own tail goes back");
    turn_as(&r.core, &desk, "an agent's word", false).await;
    assert_eq!(dates(&r), 1, "an agent's turn is not sent the owner's tails");
    turn_as(&r.core, &desk, "the owner again", true).await;
    assert!(dates(&r) >= 2, "the owner's tails are his again");
}

/// A tail made before a thread was sealed is let go: it is not sent again,
/// to the owner either; one made under the seal goes back. A seal on the
/// conversation's own thread is not new to it, and lets nothing go.
#[tokio::test]
async fn a_kept_tail_is_let_go_when_a_thread_is_sealed_after_it() {
    let r = rig();
    thread(&r, "kelp");
    let own = r.core.thread_session("kelp", cli()).unwrap().session_id;
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "the first word", true).await;
    turn_as(&r.core, &own, "the thread's first word", true).await;
    r.core
        .thread_seal(
            &ThreadSealParams {
                slug: "kelp".into(),
                reason: Some("embargoed".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    turn_as(&r.core, &desk, "the second word", true).await;
    assert_eq!(dates(&r), 1, "the tail from before the seal is let go");
    turn_as(&r.core, &desk, "the third word", true).await;
    assert_eq!(dates(&r), 2, "a tail made under the seal goes back");
    turn_as(&r.core, &own, "the thread's second word", true).await;
    assert_eq!(dates(&r), 2, "a thread's own seal is not new to it");
}
