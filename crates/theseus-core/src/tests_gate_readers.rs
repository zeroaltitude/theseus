//! The readers on the old path behind the gate (issue 44, round 2): what an
//! earlier request sent is admitted again before it is sent again; the
//! place digest says nothing of a thread held from its reader; a listed
//! title made from a moved message is not shown; health counts a thread's
//! binding that does not read; and a turn whose presence is not known reads
//! nothing of history.

use serde_json::json;
use theseus_memory::gate::Present;
use theseus_protocol::threads::{ThreadNewParams, ThreadSealParams};

use crate::approval::{Answerer, Surface};
use crate::provider::ProviderRequest;
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
/// carries: its own tail's, and one for each earlier tail put back.
fn dates(r: &Rig, input: &str) -> usize {
    let req: ProviderRequest = r
        .requests()
        .into_iter()
        .rfind(|q| last_user(q).contains(input))
        .expect("the turn's request");
    serde_json::to_string(&req.messages)
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

/// The place digest says nothing of a thread held from its reader, not even
/// that it exists: the owner alone in one thread is shown the other open
/// thread and no count of the sealed one, with the ledger on or off; the
/// sealed thread itself still reads its own place.
#[tokio::test]
async fn the_place_digest_does_not_count_a_sealed_thread() {
    use crate::compiler::place_digest::render;
    use crate::places::PlaceClass;
    for ledger in [true, false] {
        let r = crate::tests_places::rig_with(|c| c.context.ledger = Some(ledger));
        for slug in ["kelp", "tide", "reef"] {
            thread(&r, slug);
        }
        seal(&r, "reef");
        let kelp = r.core.thread_session("kelp", cli()).unwrap().session_id;
        view_gate::seat_owner(&r.core.runner, &kelp);
        let board = r.core.runner.threads.all(&r.core.store).unwrap();
        let live = view_gate::digest_threads(
            &r.core.runner,
            &kelp,
            "kelp",
            board.live(PlaceClass::Private),
        );
        let d = render(&live, "kelp", 280).expect("tide is shown");
        assert_eq!(
            (d.shown, d.embargoed),
            (1, 0),
            "ledger {ledger}: {}",
            d.text
        );
        assert!(!d.text.contains("embargoed"), "{}", d.text);

        // Seal tide too: nothing is left to show, and nothing is said.
        seal(&r, "tide");
        let board = r.core.runner.threads.all(&r.core.store).unwrap();
        let live = view_gate::digest_threads(
            &r.core.runner,
            &kelp,
            "kelp",
            board.live(PlaceClass::Private),
        );
        assert_eq!(render(&live, "kelp", 280), None, "ledger {ledger}");

        // The sealed thread's own turn is not held from its own place.
        let reef = r.core.thread_session("reef", cli()).unwrap().session_id;
        let own = view_gate::digest_threads(
            &r.core.runner,
            &reef,
            "reef",
            board.live(PlaceClass::Private),
        );
        assert!(own.iter().any(|t| t.slug == "reef"), "ledger {ledger}");
    }
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
    r.core.thread_session("kelp", cli()).unwrap();
    let h = r.core.threads_health().expect("the threads are said");
    assert_eq!(h.unread_bindings, 0, "bound again, it reads");
}

/// A turn whose presence is not known (a wake set by a wake, whose set-up
/// turn had no message to carry a mark) reads nothing of history: not
/// another conversation, nor its own place's, by recall's places or by a
/// lookup's words; the owner alone, at the same terminal, reads both.
#[tokio::test]
async fn a_turn_whose_presence_is_not_known_reads_nothing_of_history() {
    use crate::view_gate::RecallPlaces;
    const HIS: &str = "the harbour gate code is 0451";
    let r = rig();
    let home = session(&r.core, None);
    turn_as(&r.core, &home, HIS, true).await;
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "plain words", true).await;
    let place = view_gate::place_name(&r.core.runner, &desk).unwrap();
    view_gate::seat_for_tests(&r.core.runner, &desk, "turn_w", Present::Unknown, &place);
    let places = RecallPlaces::of(&r.core.runner, Some(&desk));
    for s in [&home, &desk] {
        assert!(!places.may_read(&r.core.runner, s), "{s}");
    }
    r.core
        .runner
        .memory
        .set_ask(crate::tests_memory_lookup::index_over(
            &r.core,
            vec![home.clone(), desk.clone()],
        ));
    let words = json!({"words": "harbour"});
    let got = crate::memory_lookup::call(r.core.clone(), &desk, words.clone())
        .await
        .map(|o| o.text)
        .unwrap_or_else(|e| e);
    assert!(!got.contains(HIS), "{got}");

    view_gate::seat_owner(&r.core.runner, &desk);
    let places = RecallPlaces::of(&r.core.runner, Some(&desk));
    assert!(places.may_read(&r.core.runner, &home));
    let got = crate::memory_lookup::call(r.core.clone(), &desk, words)
        .await
        .unwrap()
        .text;
    assert!(got.contains(HIS), "the owner alone reads it: {got}");
}
