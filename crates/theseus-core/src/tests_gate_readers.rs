//! The readers on the old path behind the gate (issue 44, round 2): what an
//! earlier request sent is admitted again before it is sent again; the
//! place digest says nothing of a thread held from its reader; a listed
//! title made from a moved message is not shown; and health counts a
//! thread's binding that does not read.

use serde_json::json;
use theseus_protocol::threads::{ThreadNewParams, ThreadSealParams};

use crate::approval::{Answerer, Surface};
use crate::tests_places::{last_user, rig, session, Rig};
use crate::tests_view_gate::turn_as;
use crate::turn::env_tail::HEAD;
use crate::view_gate;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// How many date lines the request of the turn whose input was `input`
/// carries: its own tail's, and one for each earlier tail put back. The
/// most of any request whose last message is that input (the rig's driver
/// sends others with no tail).
fn dates(r: &Rig, input: &str) -> usize {
    r.requests()
        .into_iter()
        .filter(|q| last_user(q).contains(input))
        .map(|q| {
            serde_json::to_string(&q.messages)
                .unwrap()
                .matches(HEAD)
                .count()
        })
        .max()
        .expect("the turn's request")
}

/// A core whose requests carry the date line in their tail (a test's core
/// has it off).
fn tailed() -> Rig {
    crate::tests_places::rig_with(|c| c.project.tail_facts = Some(true))
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
    let r = tailed();
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "the first word", true).await;
    turn_as(&r.core, &desk, "the second word", true).await;
    assert_eq!(
        dates(&r, "the second word"),
        2,
        "the owner's own tail goes back"
    );
    turn_as(&r.core, &desk, "an agent's word", false).await;
    assert_eq!(
        dates(&r, "an agent's word"),
        1,
        "an agent's turn is not sent the owner's tails"
    );
    turn_as(&r.core, &desk, "the owner again", true).await;
    assert!(
        dates(&r, "the owner again") >= 2,
        "the owner's tails are his again"
    );
}

/// A tail made before a thread was sealed is let go: it is not sent again,
/// to the owner either; one made under the seal goes back. A seal on the
/// conversation's own thread is not new to it, and lets nothing go.
#[tokio::test]
async fn a_kept_tail_is_let_go_when_a_thread_is_sealed_after_it() {
    let r = tailed();
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
    assert_eq!(
        dates(&r, "the second word"),
        1,
        "the tail from before the seal is let go"
    );
    turn_as(&r.core, &desk, "the third word", true).await;
    assert_eq!(
        dates(&r, "the third word"),
        2,
        "a tail made under the seal goes back"
    );
    turn_as(&r.core, &own, "the thread's second word", true).await;
    assert_eq!(
        dates(&r, "the thread's second word"),
        2,
        "a thread's own seal is not new to it"
    );
}

fn seal(r: &Rig, slug: &str) {
    r.core
        .thread_seal(
            &ThreadSealParams {
                slug: slug.into(),
                reason: Some("embargoed".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
}

/// The place digest says nothing of a thread held from a reader that is not
/// the owner alone, not even that it exists: with the ledger off (no turn's
/// presence is computed) such a turn is shown the other open thread and no
/// count of the sealed one; the owner alone is told how many, as the
/// digest's own rule has it; and the sealed thread still reads its own.
#[tokio::test]
async fn the_place_digest_does_not_count_a_sealed_thread_but_to_the_owner_alone() {
    use crate::compiler::place_digest::render;
    use crate::places::PlaceClass;
    let r = crate::tests_places::rig_with(|c| c.context.ledger = Some(false));
    for slug in ["kelp", "tide", "reef"] {
        thread(&r, slug);
    }
    seal(&r, "reef");
    let kelp = r.core.thread_session("kelp", cli()).unwrap().session_id;
    let board = r.core.runner.threads.all(&r.core.store).unwrap();
    let digest = |sid: &str, here: &str| {
        let live =
            view_gate::digest_threads(&r.core.runner, sid, here, board.live(PlaceClass::Private));
        render(&live, here, 280)
    };
    let d = digest(&kelp, "kelp").expect("tide is shown");
    assert_eq!((d.shown, d.embargoed), (1, 0), "{}", d.text);
    assert!(!d.text.contains("embargoed"), "{}", d.text);

    view_gate::seat_owner(&r.core.runner, &kelp);
    let d = digest(&kelp, "kelp").expect("tide is shown");
    assert_eq!(
        (d.shown, d.embargoed),
        (1, 1),
        "the owner alone: {}",
        d.text
    );

    // The sealed thread's own turn is not held from its own place.
    let reef = r.core.thread_session("reef", cli()).unwrap().session_id;
    let own = view_gate::digest_threads(
        &r.core.runner,
        &reef,
        "reef",
        board.live(PlaceClass::Private),
    );
    assert!(own.iter().any(|t| t.slug == "reef"));
}

/// A session's title in a span's listing, made from its first message, is
/// not shown once that message is moved into a sealed thread: the session is
/// still listed, under no title, as its own read shows it.
#[tokio::test]
async fn a_listed_title_made_from_a_moved_message_is_not_shown() {
    const TITLE: &str = "the mooring ledger for the north quay";
    let r = rig();
    thread(&r, "kelp");
    seal(&r, "kelp");
    let home = session(&r.core, None);
    turn_as(&r.core, &home, TITLE, true).await;
    let desk = session(&r.core, None);
    view_gate::seat_owner(&r.core.runner, &desk);
    let list = || async {
        crate::memory_lookup::call(r.core.clone(), &desk, json!({"when": "today"}))
            .await
            .unwrap()
            .text
    };
    let before = list().await;
    assert!(before.contains(TITLE), "{before}");
    let first = r
        .core
        .store
        .session_nodes(&home)
        .unwrap()
        .into_iter()
        .find(|(_, n)| crate::recall::text_of(n).contains(TITLE))
        .map(|(_, n)| n.id)
        .unwrap();
    r.core
        .store
        .put_meta(
            &crate::moved::moved_of_key(&first),
            &json!({"thread": "kelp", "split": "spl_x"}),
        )
        .unwrap();
    let after = list().await;
    assert!(!after.contains(TITLE), "{after}");
    assert!(after.contains("this harness"), "still listed: {after}");
}

/// Health says when a thread's binding cannot be read, by count, and stops
/// saying it once the thread is bound again.
#[tokio::test]
async fn health_counts_a_binding_that_cannot_be_read_until_it_is_bound_again() {
    let r = crate::tests_places::rig_setup(
        |_| {},
        |s| {
            s.put_meta("thread:kelp", &json!({"slug": "kelp", "title": "kelp"}))
                .unwrap();
            s.put_meta("discord.session.thread:kelp", &json!(17))
                .unwrap();
        },
    );
    r.core.runner.threads.all(&r.core.store).unwrap();
    let h = r.core.threads_health().expect("the threads are said");
    assert_eq!(h.unread_bindings, 1);
    // The thread bound again, as opening its session binds it.
    let sid = session(&r.core, None);
    r.core
        .runner
        .threads
        .bind(&r.core.store, "kelp", |_| Ok(((), Some(sid.clone()))))
        .unwrap();
    let h = r.core.threads_health().expect("the threads are said");
    assert_eq!(h.unread_bindings, 0, "bound again, it reads");
}
