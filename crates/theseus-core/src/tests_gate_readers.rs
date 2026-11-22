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
/// it; and each change of presence starts a new chain.
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
    // The agent's turn started a new chain: the owner's tails were let go
    // with the thinking they were under, and the agent's are not his.
    turn_as(&r.core, &desk, "the owner again", true).await;
    assert_eq!(dates(&r, "the owner again"), 1, "a new chain again");
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
    seal_core(&r.core, slug);
}

fn seal_core(core: &crate::Core, slug: &str) {
    core.thread_seal(
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

/// Health tells the owner in person when a thread's binding cannot be read,
/// by count, and stops once the thread is bound again; an agent's process at
/// the terminal and the MCP server are told nothing of it, as before.
#[tokio::test]
async fn health_counts_a_binding_that_cannot_be_read_until_it_is_bound_again() {
    use crate::approval::{Client, Surface};
    use crate::peer_proc::Behind;
    use crate::tests_m3::rpc_as;
    let r = crate::tests_places::rig_setup(
        |_| {},
        |s| {
            s.put_meta("thread:kelp", &json!({"slug": "kelp", "title": "kelp"}))
                .unwrap();
            s.put_meta("discord.session.thread:kelp", &json!(17))
                .unwrap();
            s.put_meta("discord.session.thread:reef", &json!(18))
                .unwrap();
        },
    );
    r.core.runner.threads.all(&r.core.store).unwrap();
    let unread = |c: Client| {
        let core = r.core.clone();
        async move {
            let h = rpc_as(&core, c, theseus_protocol::method::HEALTH, json!({}))
                .await
                .unwrap();
            h["threads"]["unread_bindings"].as_u64().unwrap_or(0)
        }
    };
    let owner = || Client::new("sock#1", Surface::Cli);
    assert_eq!(unread(owner()).await, 2, "the owner in person");
    let agent = Client::new("sock#2", Surface::Cli).behind(Behind::Agent("claude".into()));
    assert_eq!(unread(agent).await, 0, "an agent's process");
    assert_eq!(
        unread(Client::new("mcp", Surface::Mcp)).await,
        0,
        "the MCP server"
    );
    assert_eq!(
        r.core
            .health()
            .threads
            .map(|t| t.unread_bindings.unwrap_or(0)),
        Some(0),
        "health read inside the daemon is as before"
    );
    // The thread bound again, as opening its session binds it.
    let sid = session(&r.core, None);
    r.core
        .runner
        .threads
        .bind(&r.core.store, "kelp", |_| Ok(((), Some(sid.clone()))))
        .unwrap();
    assert_eq!(unread(owner()).await, 1, "bound again, it reads");
}

/// A turn of `sid` in the binding's rig, its input marked as the owner's in
/// person or an agent's.
async fn said_as(
    r: &crate::tests_binding::Rig,
    sid: &str,
    input: &str,
    in_person: bool,
) -> theseus_protocol::TurnSubmitResult {
    use crate::session::SessionRecord;
    let core = &r.core;
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = view_gate::test_mark(&core.runner, sid, in_person);
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

/// Letting a tail go never sends a request the provider's binding refuses:
/// across a new seal, and across a narrowing (an agent's turn after the
/// owner's), the next request starts a new chain, its thinking stripped, and
/// no request is refused or repeated with the provider's remedy.
#[tokio::test]
async fn a_tail_let_go_starts_a_chain_the_binding_accepts() {
    use crate::provider::Binding;
    use crate::provider::Scripted;
    use crate::tests_binding::{private_session, rig, thinks_then_reads};
    for change in ["seal", "narrow"] {
        let r = rig(
            Binding::Refuse,
            vec![
                thinks_then_reads("toolu_a"),
                Scripted::text("Four."),
                thinks_then_reads("toolu_b"),
                Scripted::text("Still four."),
            ],
        );
        let sid = private_session(&r.core);
        let one = said_as(&r, &sid, "what does the tide table say?", true).await;
        assert_eq!(one.output, "Four.", "{change}: {one:?}");
        if change == "seal" {
            r.core
                .thread_new(
                    &ThreadNewParams {
                        slug: "kelp".into(),
                        title: Some("kelp".into()),
                        ..Default::default()
                    },
                    cli(),
                )
                .unwrap();
            seal_core(&r.core, "kelp");
        }
        let two = said_as(&r, &sid, "and again?", change != "narrow").await;
        assert_eq!(two.output, "Still four.", "{change}: {two:?}");
        let reqs = r.fake.requests();
        assert_eq!(reqs.len(), 4, "{change}: no request refused and repeated");
        for q in &reqs {
            let remedy = q.thinking.as_ref().and_then(|t| t.get("block_binding"));
            assert!(remedy.is_none(), "{change}: a repeat with the remedy");
        }
        // The new chain carries none of the first turn's thinking.
        assert!(
            !serde_json::to_string(&reqs[2].messages)
                .unwrap()
                .contains("\"thinking\""),
            "{change}: {:?}",
            reqs[2].messages
        );
    }
}

/// A board that reads in part holds every thread: one thread record that
/// does not decode among good ones lets every kept tail go, as a board that
/// does not read at all does.
#[tokio::test]
async fn a_partly_unread_board_lets_every_kept_tail_go() {
    let r = crate::tests_places::rig_setup(
        |c| c.project.tail_facts = Some(true),
        |s| {
            s.put_meta("thread:kelp", &json!({"slug": "kelp", "title": "kelp"}))
                .unwrap();
            s.put_meta("thread:reef", &json!("not a thread")).unwrap();
        },
    );
    let desk = session(&r.core, None);
    assert_eq!(view_gate::held_threads(&r.core.runner, &desk), None);
    turn_as(&r.core, &desk, "the first word", true).await;
    turn_as(&r.core, &desk, "the second word", true).await;
    assert_eq!(dates(&r, "the second word"), 1, "nothing kept goes back");
}

/// A tail is let go when a message moved into a thread since it was made,
/// though no thread joined the held ones: thread T was sealed before the
/// tail was made, then a message of another conversation (the source a
/// standing record in that tail could have been captured from) is moved
/// into T. The held set is the same; the move lets the tail go.
#[tokio::test]
async fn a_tail_is_let_go_when_a_message_moved_into_a_held_thread_since() {
    let r = tailed();
    thread(&r, "kelp");
    seal(&r, "kelp");
    let home = session(&r.core, None);
    turn_as(&r.core, &home, "the mooring ledger", true).await;
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "the first word", true).await;
    let before = view_gate::held_threads(&r.core.runner, &desk);
    let (_, moved) = r.core.store.session_nodes(&home).unwrap().remove(0);
    r.core
        .store
        .put_meta(
            &crate::moved::moved_of_key(&moved.id),
            &json!({"thread": "kelp", "split": "spl_x"}),
        )
        .unwrap();
    assert_eq!(view_gate::held_threads(&r.core.runner, &desk), before);
    turn_as(&r.core, &desk, "the second word", true).await;
    assert_eq!(dates(&r, "the second word"), 1, "the move let it go");
    turn_as(&r.core, &desk, "the third word", true).await;
    assert_eq!(
        dates(&r, "the third word"),
        2,
        "a tail made after it goes back"
    );
}

/// The core a test's model script reaches back into, set once it is built.
type Back = std::sync::Arc<std::sync::OnceLock<std::sync::Weak<crate::Core>>>;

/// What a turn's first call does between its two calls, in
/// `the_fresh_tail_admits_its_records_again_on_every_call`.
#[derive(Clone, Copy)]
enum Between {
    Steer,
    Seal,
}

/// A record scoped to the terminal, kept from the owner's message in his
/// DM, rides the first call's tail of his own turn at the terminal; a
/// narrowing steer (an agent's message absorbed) or a seal of the thread its
/// message was moved into, between that call and the next, leaves it out of
/// the second call's tail.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_fresh_tail_admits_its_records_again_on_every_call() {
    use crate::provider::Scripted;
    use theseus_protocol::standing::{StandingAddParams, StandingKind, StandingScope};
    const ORDER: &str = "the north quay gauge reads in feet";
    const ASK: &str = "check the gauge for me";
    for between in [Between::Steer, Between::Seal] {
        let back: Back = Default::default();
        let hook = back.clone();
        let desk_at: std::sync::Arc<std::sync::OnceLock<String>> = Default::default();
        let desk_in = desk_at.clone();
        let r = crate::tests_places::rig_setup_scripted(
            |_| {},
            |_| {},
            move |req: &crate::provider::ProviderRequest| {
                if crate::tests_places::answers_a_call(req) {
                    return Scripted::text("Done.");
                }
                if !last_user(req).contains(ASK) {
                    return Scripted::text("Noted.");
                }
                let core = hook.get().and_then(std::sync::Weak::upgrade).unwrap();
                let desk = desk_in.get().unwrap();
                match between {
                    Between::Steer => {
                        let m = view_gate::test_mark(&core.runner, desk, false);
                        view_gate::steered(&core.runner, desk, Some(&m));
                    }
                    Between::Seal => seal_core(&core, "kelp"),
                }
                Scripted::tools(
                    "Looking.",
                    &[("k1", "proc_run", json!({"argv": ["cat", "notes-copy.md"]}))],
                )
            },
        );
        let _ = back.set(std::sync::Arc::downgrade(&r.core));
        thread(&r, "kelp");
        let dm = session(&r.core, Some(&format!("dm:{}", crate::tests_places::OWNER)));
        turn_as(&r.core, &dm, "the gauge is in feet", true).await;
        let (_, said) = r
            .core
            .store
            .session_nodes(&dm)
            .unwrap()
            .into_iter()
            .find(|(_, n)| crate::recall::text_of(n).contains("gauge"))
            .unwrap();
        if let Between::Seal = between {
            r.core
                .store
                .put_meta(
                    &crate::moved::moved_of_key(&said.id),
                    &json!({"thread": "kelp", "split": "spl_x"}),
                )
                .unwrap();
        }
        r.core
            .standing_add(
                &StandingAddParams {
                    kind: StandingKind::Preference,
                    text: ORDER.into(),
                    scope: StandingScope::Surface("terminal".into()),
                    source: Some(format!("capture:{}", said.id)),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap();
        let desk = session(&r.core, None);
        let _ = desk_at.set(desk.clone());
        turn_on_terminal(&r.core, &desk, ASK).await;
        let calls: Vec<_> = r
            .requests()
            .into_iter()
            .filter(|q| serde_json::to_string(&q.messages).unwrap().contains(ASK))
            .collect();
        assert!(calls.len() >= 2, "{} calls", calls.len());
        let tail = |q: &crate::provider::ProviderRequest| {
            serde_json::to_string(q.messages.last().unwrap()).unwrap()
        };
        assert!(tail(&calls[0]).contains(ORDER), "the first call carries it");
        assert!(
            !tail(&calls[1]).contains(ORDER),
            "the second call leaves it out: {}",
            tail(&calls[1])
        );
    }
}

/// The owner's turn at the terminal surface, marked as typed in person.
async fn turn_on_terminal(core: &std::sync::Arc<crate::Core>, sid: &str, input: &str) {
    use crate::session::SessionRecord;
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = view_gate::test_mark(&core.runner, sid, true);
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: Some("terminal"),
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// A wake set by a wake (its set-up known, its provenance absent) reads
/// nothing of history: at the terminal, and in a channel bound private,
/// whose shortcut would otherwise draw on the channel; the history tool reads
/// the gate's record of it. An input with no mark, set up by nothing, keeps
/// today's path.
#[tokio::test]
async fn a_wake_set_by_a_wake_reads_nothing_of_history() {
    use crate::places::BoundPlace;
    use crate::tests_places::LAB;
    use crate::view_gate::{narrowest, set_up_proof, Proof, RecallPlaces};
    const HIS: &str = "the harbour gate code is 0451";
    let r = rig();
    r.core.bind_places(vec![BoundPlace {
        persona: None,
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: true,
        guild: Some("100000000000000001".into()),
        ceiling: None,
    }]);
    let home = session(&r.core, None);
    turn_as(&r.core, &home, HIS, true).await;
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "the harbour plan", true).await;
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    turn_as(&r.core, &lab, "the harbour crane", true).await;
    // A wake's turn whose set-up turn had no message: no presence found.
    assert!(matches!(set_up_proof(true, narrowest([None])), Proof::Deny));
    assert!(matches!(
        set_up_proof(false, narrowest([])),
        Proof::SetUp(None)
    ));
    for sid in [&desk, &lab] {
        let place = view_gate::place_name(&r.core.runner, sid).unwrap();
        view_gate::begin(&r.core.runner, sid, "turn_w", place, Proof::Deny);
        let places = RecallPlaces::of(&r.core.runner, Some(sid));
        assert_eq!(places.channel(), None, "{sid}");
        for s in [&home, &desk, &lab] {
            assert!(!places.may_read(&r.core.runner, s), "{sid} reads {s}");
        }
        let got = crate::memory_lookup::call(r.core.clone(), sid, json!({"words": "harbour"}))
            .await
            .map(|o| o.text)
            .unwrap_or_else(|e| e);
        assert!(!got.contains(HIS), "{sid}: {got}");
        let pov = crate::history_tool::pov_of(
            &r.core,
            sid,
            "turn_w",
            None,
            crate::places::PlaceClass::Private,
        );
        assert_eq!(pov.present, crate::view::Present::Unknown, "{sid}");
    }
}

/// Kelp sealed, with a turn in it, and a home session whose first message
/// was moved into kelp (its `moved.of` and the split's record, as a split
/// writes them): `(kelp's session, home)`.
async fn sealed_with_a_move(r: &Rig, title: &str) -> (String, String) {
    thread(r, "kelp");
    let kelp = r.core.thread_session("kelp", cli()).unwrap().session_id;
    turn_as(&r.core, &kelp, "the kelp beds are embargoed", true).await;
    let home = session(&r.core, None);
    turn_as(&r.core, &home, title, true).await;
    turn_as(&r.core, &home, "and the ebb table too", true).await;
    let first = r
        .core
        .store
        .session_nodes(&home)
        .unwrap()
        .into_iter()
        .find(|(_, n)| crate::recall::text_of(n).contains(title))
        .map(|(_, n)| n.id)
        .unwrap();
    let store = &r.core.store;
    store
        .put_meta(
            &crate::moved::moved_of_key(&first),
            &json!({"thread": "kelp", "split": "spl_x"}),
        )
        .unwrap();
    store
        .put_meta(
            "thread.split.spl_x",
            &json!({"id": "spl_x", "source_session": home, "thread": "kelp"}),
        )
        .unwrap();
    seal(r, "kelp");
    (kelp, home)
}

/// `session.list` through a real connection: a sealed thread's session is
/// not listed for a reader who is not the owner in person, and is listed
/// with none of its words for him; a session a message was moved out of into
/// the sealed thread shows none of its words (its title was made from that
/// message) to anyone; any other session is as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_list_shows_a_reader_nothing_of_a_held_thread() {
    use crate::approval::Surface;
    const TITLE: &str = "the mooring ledger for the north quay";
    let r = rig();
    let (kelp, home) = sealed_with_a_move(&r, TITLE).await;
    let plain = session(&r.core, None);
    turn_as(&r.core, &plain, "the open tide log", true).await;
    let ask = |surface: Surface| {
        let params = json!({"ids": [kelp, home, plain]});
        let c = r.core.clone();
        async move {
            crate::tests_mcp_memory::rpc(
                &c,
                surface,
                theseus_protocol::method::SESSION_LIST,
                params,
            )
            .await
            .unwrap()
        }
    };
    let ids = |v: &serde_json::Value| -> Vec<String> {
        v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["session_id"].as_str().unwrap().to_string())
            .collect()
    };
    let owner = ask(Surface::Cli).await;
    assert_eq!(ids(&owner).len(), 3, "the owner lists all three: {owner}");
    let text = owner.to_string();
    assert!(!text.contains("kelp beds"), "{text}");
    assert!(!text.contains(TITLE), "{text}");
    assert!(text.contains("the open tide log"), "{text}");
    for surface in [Surface::Unnamed, Surface::Discord] {
        let other = ask(surface).await;
        assert!(!ids(&other).contains(&kelp), "{surface:?}: {other}");
        assert!(ids(&other).contains(&home), "{surface:?}: {other}");
        assert!(!other.to_string().contains(TITLE), "{surface:?}: {other}");
    }
}

/// `memory.lookup`'s listing dates, counts and labels a session by the
/// messages its reader may read: a session one of whose messages was moved
/// into a sealed thread is listed with no title and one turn fewer, and one
/// whose every message was moved is not listed.
#[tokio::test]
async fn the_memory_listing_counts_only_admitted_messages() {
    const TITLE: &str = "the mooring ledger for the north quay";
    let r = rig();
    let (_, home) = sealed_with_a_move(&r, TITLE).await;
    let desk = session(&r.core, None);
    view_gate::seat_owner(&r.core.runner, &desk);
    let got = crate::memory_lookup::call(r.core.clone(), &desk, json!({"when": "today"}))
        .await
        .unwrap()
        .text;
    assert!(!got.contains(TITLE), "{got}");
    assert!(got.contains(&home), "listed: {got}");
    assert!(
        got.contains("1 turn)"),
        "the moved turn is not counted: {got}"
    );
    // Every message of it moved: not listed.
    for (_, n) in r.core.store.session_nodes(&home).unwrap() {
        r.core
            .store
            .put_meta(
                &crate::moved::moved_of_key(&n.id),
                &json!({"thread": "kelp", "split": "spl_x"}),
            )
            .unwrap();
    }
    let got = crate::memory_lookup::call(r.core.clone(), &desk, json!({"when": "today"}))
        .await
        .unwrap()
        .text;
    assert!(!got.contains(&home), "not listed: {got}");
}

/// No request of `r` carried the provider's remedy for a refused prefix: a
/// refusal is always answered by one repeat that names it.
fn none_refused(reqs: &[crate::provider::ProviderRequest], what: &str) {
    for q in reqs {
        let remedy = q.thinking.as_ref().and_then(|t| t.get("block_binding"));
        assert!(remedy.is_none(), "{what}: a repeat with the remedy");
    }
}

/// A warm read that lets a tail go leaves the session needing a new chain:
/// after a thinking turn and a seal, two warm reads send nothing (the second
/// finds the kept tails emptied, and still sends nothing), and the next turn
/// starts a new chain the binding accepts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_warm_read_after_a_seal_sends_nothing_and_leaves_a_new_chain_owed() {
    use crate::keep_warm::Outcome;
    use crate::provider::{Binding, FakeProvider, Scripted};
    use crate::tests_binding::{say, thinks_then_reads};
    use crate::tests_keep_warm::{build_with, one_hour, reads};
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let mut c = one_hour(24);
    c.project.tail_facts = Some(true);
    c.tools.projects_dir = Some(work.canonicalize().unwrap().to_string_lossy().into_owned());
    let fake = std::sync::Arc::new(FakeProvider {
        binding: Some(Binding::Refuse),
        ..FakeProvider::scripted(vec![
            Scripted::Blocks {
                blocks: vec![
                    json!({"type": "thinking", "thinking": "a tide question"}),
                    json!({"type": "text", "text": "Four."}),
                ],
                stop_reason: "end_turn".into(),
            },
            thinks_then_reads("toolu_b"),
            Scripted::text("Still four."),
        ])
    });
    let core = build_with(dir.path(), c, fake.clone());
    let sid =
        crate::tests_keep_warm::session(&core, theseus_protocol::SessionKind::Conversation, None);
    core.runner.keep_warm.message(&sid, true);
    say(&core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    core.thread_new(
        &ThreadNewParams {
            slug: "kelp".into(),
            title: Some("kelp".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    seal_core(&core, "kelp");
    let live = core.live_profile().0;
    for n in 0..2 {
        let read = core.runner.keep_warm_once(&sid, &live).await;
        assert!(
            !matches!(read, Outcome::Read { .. }),
            "warm read {n}: {read:?}"
        );
    }
    assert!(reads(&fake).is_empty(), "no warm read was sent");
    let second = say(&core, &sid, "and again?").await.unwrap();
    assert_eq!(second.output, "Still four.", "{second:?}");
    none_refused(&fake.requests(), "the turn after");
}

/// A seal that lands after a request's snapshot was read, while its compile
/// awaits, is seen before the request leaves: the first send of the next
/// turn holds no old tail and needs no remedy from the provider.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seal_during_the_compile_is_seen_before_the_send() {
    use crate::provider::{Binding, Scripted};
    use crate::tests_binding::{private_session, rig as strict, thinks_then_reads};
    let r = strict(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            Scripted::text("Four."),
            thinks_then_reads("toolu_b"),
            Scripted::text("Still four."),
        ],
    );
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "kelp".into(),
                title: Some("kelp".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    let sid = private_session(&r.core);
    said_as(&r, &sid, "what does the tide table say?", true).await;
    let core = std::sync::Arc::downgrade(&r.core);
    r.core.runner.tails.hook_for_tests(move || {
        if let Some(c) = core.upgrade() {
            seal_core(&c, "kelp");
        }
    });
    let two = said_as(&r, &sid, "and again?", true).await;
    assert_eq!(two.output, "Still four.", "{two:?}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 4, "no request refused and repeated");
    none_refused(&reqs, "the seal during the compile");
    let first = serde_json::to_string(&reqs[2].messages).unwrap();
    assert_eq!(first.matches(HEAD).count(), 1, "no old tail: {first}");
    assert!(!first.contains("\"thinking\""), "a new chain: {first}");
}

/// A real split, written by the kernel's frame, into an open thread counts
/// as a move: an older tail is let go though no thread joined the held ones.
/// (A split into a sealed thread from an unsealed conversation is refused:
/// turns never cross a seal.)
#[tokio::test]
async fn a_split_through_the_kernel_lets_older_tails_go() {
    use theseus_protocol::threads::ThreadSplitParams;
    let r = tailed();
    thread(&r, "kelp");
    r.core.thread_session("kelp", cli()).unwrap();
    let home = session(&r.core, None);
    turn_as(&r.core, &home, "the mooring ledger", true).await;
    let desk = session(&r.core, None);
    turn_as(&r.core, &desk, "the first word", true).await;
    let held = view_gate::held_threads(&r.core.runner, &desk);
    let before = r.core.store.moves();
    let nodes = r.core.store.session_nodes(&home).unwrap();
    let first = nodes
        .iter()
        .find(|(_, n)| crate::recall::text_of(n).contains("mooring"))
        .map(|(p, _)| *p)
        .unwrap();
    let last = nodes.last().map(|(p, _)| *p).unwrap();
    r.core
        .thread_split(
            &ThreadSplitParams {
                from: home.clone(),
                into: Some("kelp".into()),
                range: Some([first, last]),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(
        r.core.store.moves() > before,
        "the split's frame is counted"
    );
    assert_eq!(view_gate::held_threads(&r.core.runner, &desk), held);
    turn_as(&r.core, &desk, "the second word", true).await;
    assert_eq!(dates(&r, "the second word"), 1, "the split let it go");
}
