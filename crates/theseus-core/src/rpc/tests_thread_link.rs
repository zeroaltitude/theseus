//! Linked threads (theseus-98bz.11) through the core: a link is the owner's, from a private place,
//! written on both threads in one frame; a linked thread's notes reach the asking thread's recall
//! with no distance and no question to Jev; a seal still holds them out, even one set after the
//! link; a link follows a merge. The key test is `a_linked_threads_note_passes_the_lens_unpaid`.

use std::collections::BTreeSet;

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::error_code::REFUSED;
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::threads::{
    Seal, ThreadEditParams, ThreadInfo, ThreadLinkParams, ThreadMergeParams, ThreadSealParams,
};

use super::tests_cross_judge::{admitted_texts, cross_requests, rigged, sent_text, world, CUTOFF};
use super::tests_thread_recall::{ambient, cli, new_thread, say, scored, thread_session};
use super::Core;
use crate::approval::{Answerer, Surface};
use crate::config::MemoryMode;
use crate::tests_recall::{rig_with, Rig};
use crate::tests_rerank::{recalls, sent, turn};

const ASK: &str = "Regenerate the report with the new cutoff.";

fn link(c: &Core, a: &str, b: &str) -> Result<ThreadInfo, String> {
    let p = ThreadLinkParams {
        slug: a.into(),
        other: b.into(),
        ..Default::default()
    };
    c.thread_link(&p, cli())
        .map(|r| r.thread)
        .map_err(|e| e.message)
}

fn unlink(c: &Core, a: &str, b: &str) -> Result<ThreadInfo, String> {
    let p = ThreadLinkParams {
        slug: a.into(),
        other: b.into(),
        ..Default::default()
    };
    c.thread_unlink(&p, cli())
        .map(|r| r.thread)
        .map_err(|e| e.message)
}

fn rows(c: &Core, kind: &str) -> Vec<crate::ledger::LedgerRow> {
    crate::tests_day_ceiling::rows(c, kind)
}

fn frames<T>(f: impl FnOnce() -> T) -> (u64, T) {
    let before = theseus_store::frames_written_here();
    let out = f();
    (theseus_store::frames_written_here() - before, out)
}

/// A thread's record as stored (META `thread:<slug>`), not the board's.
fn stored(c: &Core, slug: &str) -> ThreadInfo {
    c.store
        .get_meta::<ThreadInfo>(&crate::threads::key(slug))
        .unwrap()
        .unwrap()
}

fn set(c: &Core, slug: &str, f: impl FnOnce(&mut ThreadInfo)) {
    c.runner
        .threads
        .write(
            &c.store,
            |b| {
                let mut t = b.get(slug).unwrap().clone();
                f(&mut t);
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
}

fn seal_of(slug: &str) -> Option<Seal> {
    Some(Seal {
        by: "owner".into(),
        at_ms: 1,
        reason: format!("{slug} is private"),
    })
}

fn drop_reason<'a>(m: &'a RecallManifest, node: &str) -> Option<&'a str> {
    m.dropped
        .iter()
        .find(|d| d.node_id == node)
        .map(|d| d.reason.as_str())
}

fn node_with(c: &Core, m: &RecallManifest, text: &str) -> Option<String> {
    m.admitted
        .iter()
        .map(|a| a.node_id.clone())
        .chain(m.dropped.iter().map(|d| d.node_id.clone()))
        .find(|n| sent_text(c, n).contains(text))
}

fn stranger() -> Answerer {
    Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "314159".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    }
}

/// A thread asks about the new cutoff; another's note holds it. Before the link the note drops as
/// another stream's; linked, the same recall admits it with its score unchanged and no question
/// about it goes to Jev; unlinked, it drops again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_linked_threads_note_passes_the_lens_unpaid() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_subject", Jev::Noul(0.05));
    let r = rigged(&jev, |_| {});
    let c = &r.core;
    let pipe = world(c, 0);
    let note = |c: &Core| node_with(c, &recalls(c, &pipe).pop().unwrap(), "0.55").unwrap();

    turn(c, &pipe, ASK).await;
    let m = recalls(c, &pipe).pop().unwrap();
    let cutoff = note(c);
    assert!(
        drop_reason(&m, &cutoff).is_some_and(|r| r.starts_with("thread")),
        "dropped before the link: {:?}",
        m.dropped
    );
    assert!(
        cross_requests(&jev).iter().any(|b| b.contains("0.55")),
        "Jev was asked about it"
    );

    link(c, "pipeline", "stats").unwrap();
    let asked = cross_requests(&jev).len();
    turn(c, &pipe, ASK).await;
    let m = recalls(c, &pipe).pop().unwrap();
    let item = m
        .admitted
        .iter()
        .find(|a| a.node_id == cutoff)
        .expect("admitted");
    assert!(
        (item.fused - 0.097).abs() < 1e-9,
        "no penalty: {}",
        item.fused
    );
    assert!(admitted_texts(&r).contains("0.55"));
    assert!(
        cross_requests(&jev)[asked..]
            .iter()
            .all(|b| !b.contains("0.55")),
        "no question about a linked thread's note"
    );

    unlink(c, "stats", "pipeline").unwrap();
    turn(c, &pipe, ASK).await;
    let m = recalls(c, &pipe).pop().unwrap();
    assert!(
        drop_reason(&m, &cutoff).is_some_and(|r| r.starts_with("thread")),
        "{:?}",
        m.dropped
    );
}

/// The lens alone, without Jev: the linked note ranks above the asking thread's own lesser note
/// (it pays nothing), where an unlinked one, at its paid score, ranks below and drops.
#[tokio::test]
async fn a_linked_note_ranks_at_its_own_score() {
    let r: Rig = rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.threads.own_top_admissible = true;
    });
    let c = &r.core;
    for (s, t) in [
        ("pipeline", "datatidy pipeline"),
        ("stats", "datatidy stats"),
    ] {
        new_thread(c, s, t);
    }
    let pipe = thread_session(c, "pipeline");
    say(c, &pipe, "the report groups rows by region");
    say(c, &pipe, "the report is written to report.md");
    let stats = thread_session(c, "stats");
    let cutoff = say(c, &stats, CUTOFF);
    let picks = || vec![(pipe.clone(), 1, 0.08), (stats.clone(), 0, 0.097)];
    let before = ambient(c, &pipe, ASK, scored(c, picks())).await;
    assert!(drop_reason(&before, &cutoff).is_some_and(|r| r.starts_with("thread")));
    link(c, "pipeline", "stats").unwrap();
    let after = ambient(c, &pipe, ASK, scored(c, picks())).await;
    let item = after
        .admitted
        .iter()
        .find(|a| a.node_id == cutoff)
        .expect("admitted");
    assert_eq!(
        item.rank, 1,
        "unpaid, it outranks the own note: {:?}",
        after.admitted
    );
}

/// Linked, then sealed: the note drops `sealed`, and none of the sealed thread's title, goal or
/// card is in any part of the asking thread's request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seal_after_a_link_still_holds_the_notes_out() {
    let jev = FakeJev::start().unwrap();
    jev.script("same_subject", Jev::Noul(0.95));
    let r = rigged(&jev, |_| {});
    let c = &r.core;
    let pipe = world(c, 0);
    c.thread_edit(
        &ThreadEditParams {
            slug: "stats".into(),
            goal: Some("STATSGOAL tune the quokka cutoff".into()),
            card: Some(vec!["STATSCARD cutoff moved in march".into()]),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    link(c, "pipeline", "stats").unwrap();
    c.thread_seal(
        &ThreadSealParams {
            slug: "stats".into(),
            reason: Some("before disclosure".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    turn(c, &pipe, ASK).await;
    let m = recalls(c, &pipe).pop().unwrap();
    let cutoff = node_with(c, &m, "0.55").expect("a candidate");
    assert_eq!(drop_reason(&m, &cutoff), Some("sealed"), "{:?}", m.admitted);
    for req in sent(&r.model) {
        for held in ["datatidy stats", "STATSGOAL", "STATSCARD", "0.55"] {
            assert!(!req.contains(held), "{held:?} reached the request");
        }
    }
    for body in cross_requests(&jev) {
        assert!(!body.contains("0.55"), "a sealed note reached Jev");
    }
}

/// A sealed end, or one with a seal proposed, cannot be linked, whichever end it is: refused with
/// the reason, and nothing is written.
#[test]
fn linking_a_sealed_or_pending_thread_is_refused() {
    let r = rig_with(MemoryMode::Off, |_| {});
    let c = &r.core;
    for s in ["a", "b", "c"] {
        new_thread(c, s, &format!("Title of {s}"));
    }
    set(c, "b", |t| t.sealed = seal_of("b"));
    set(c, "c", |t| t.seal_pending = Some("looks private".into()));
    let (n, e) = frames(|| {
        [
            link(c, "a", "b"),
            link(c, "b", "a"),
            link(c, "a", "c"),
            link(c, "c", "a"),
        ]
    });
    assert_eq!(n, 0, "nothing written");
    for (i, e) in e.iter().enumerate() {
        let e = e.as_ref().unwrap_err();
        assert!(
            e.contains("sealed") || e.contains("seal is proposed"),
            "{i}: {e}"
        );
    }
    assert!(link(c, "a", "b")
        .unwrap_err()
        .contains("the thread b is sealed"));
    assert!(link(c, "a", "c")
        .unwrap_err()
        .contains("a seal is proposed for the thread c"));
    assert!(rows(c, "thread.linked").is_empty());
    assert!(stored(c, "a").links.is_empty());
}

/// One frame holds both records and the row; an unlink is one frame the same way (and works on a
/// sealed thread); a link that holds writes nothing.
#[test]
fn a_link_is_written_on_both_threads_in_one_frame() {
    let r = rig_with(MemoryMode::Off, |_| {});
    let c = &r.core;
    for s in ["a", "b"] {
        new_thread(c, s, &format!("Title of {s}"));
    }
    let (n, got) = frames(|| link(c, "a", "b"));
    assert_eq!(n, 1, "one frame");
    assert_eq!(got.unwrap().links, ["b"]);
    assert_eq!(stored(c, "a").links, ["b"]);
    assert_eq!(stored(c, "b").links, ["a"]);
    let linked = rows(c, "thread.linked");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].data["slug"], "a");
    assert_eq!(linked[0].data["other"], "b");
    let (n, again) = frames(|| link(c, "b", "a"));
    assert_eq!(n, 0, "linking twice writes nothing");
    assert_eq!(again.unwrap().links, ["a"]);
    assert_eq!(rows(c, "thread.linked").len(), 1);

    // A seal set after the link does not stop the unlink.
    set(c, "b", |t| t.sealed = seal_of("b"));
    let (n, got) = frames(|| unlink(c, "b", "a"));
    assert_eq!(n, 1);
    assert!(got.unwrap().links.is_empty());
    assert!(stored(c, "a").links.is_empty() && stored(c, "b").links.is_empty());
    assert_eq!(rows(c, "thread.unlinked").len(), 1);
    let (n, _) = frames(|| unlink(c, "a", "b"));
    assert_eq!(n, 0, "an absent link writes nothing");
    assert_eq!(rows(c, "thread.unlinked").len(), 1);
}

/// Not the owner's, not a link: a person in a shared channel is refused and nothing changes.
#[test]
fn only_the_owner_links() {
    let r = rig_with(MemoryMode::Off, |_| {});
    let c = &r.core;
    for s in ["a", "b"] {
        new_thread(c, s, &format!("Title of {s}"));
    }
    let p = ThreadLinkParams {
        slug: "a".into(),
        other: "b".into(),
        ..Default::default()
    };
    let e = c.thread_link(&p, stranger()).unwrap_err();
    assert_eq!(e.code, REFUSED, "{}", e.message);
    assert!(stored(c, "a").links.is_empty());
    assert!(rows(c, "thread.linked").is_empty());
    link(c, "a", "b").unwrap();
    let e = c.thread_unlink(&p, stranger()).unwrap_err();
    assert_eq!(e.code, REFUSED, "{}", e.message);
    assert_eq!(stored(c, "a").links, ["b"]);
    assert!(rows(c, "thread.unlinked").is_empty());
}

/// A link to a thread later merged away follows the live thread: A, linked with B, finds the
/// notes of C once B is merged into C (and still those of B, which the merge left in place).
#[tokio::test]
async fn a_link_follows_a_merged_thread() {
    let r: Rig = rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.threads.own_top_admissible = true;
    });
    let c = &r.core;
    for s in ["a", "b", "c"] {
        new_thread(c, s, &format!("datatidy {s}"));
    }
    let a = thread_session(c, "a");
    say(c, &a, "the report groups rows by region");
    let b = thread_session(c, "b");
    let b_note = say(c, &b, "the b stage keeps the old cutoff 0.40");
    let cs = thread_session(c, "c");
    let c_note = say(c, &cs, CUTOFF);
    link(c, "a", "b").unwrap();
    c.thread_merge(
        &ThreadMergeParams {
            from: "b".into(),
            into: "c".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    assert_eq!(
        c.threads().linked_with("a"),
        BTreeSet::from(["b".to_string(), "c".to_string()])
    );
    assert!(
        c.threads().linked_with("c").contains("a"),
        "read from the other end too"
    );
    let picks = vec![
        (a.clone(), 0, 0.08),
        (cs.clone(), 0, 0.097),
        (b.clone(), 0, 0.096),
    ];
    let m = ambient(c, &a, ASK, scored(c, picks)).await;
    for note in [&c_note, &b_note] {
        assert!(
            m.admitted.iter().any(|i| &i.node_id == note),
            "{:?}",
            m.dropped
        );
    }
    let item = m.admitted.iter().find(|i| i.node_id == c_note).unwrap();
    assert_eq!(item.rank, 1);
}

/// A link to oneself, to an unknown thread or to a merged one is refused; a thread holds at most 8.
#[test]
fn a_link_to_self_unknown_or_merged_is_refused_and_eight_is_the_most() {
    let r = rig_with(MemoryMode::Off, |_| {});
    let c = &r.core;
    for s in ["a", "b", "c"] {
        new_thread(c, s, &format!("Title of {s}"));
    }
    let (n, e) = frames(|| link(c, "a", "a"));
    assert!(e.unwrap_err().contains("cannot be linked to itself"));
    assert!(link(c, "a", "nope").unwrap_err().contains("no thread nope"));
    assert!(link(c, "nope", "a")
        .unwrap_err()
        .contains("`theseus thread list`"));
    c.thread_merge(
        &ThreadMergeParams {
            from: "c".into(),
            into: "b".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let e = link(c, "a", "c").unwrap_err();
    assert!(
        e.contains("the thread c was merged into b: link that one"),
        "{e}"
    );
    assert_eq!(n, 0);
    for k in 0..8 {
        let s = format!("x{k}");
        new_thread(c, &s, &format!("Title of {s}"));
        link(c, "a", &s).unwrap();
    }
    assert_eq!(stored(c, "a").links.len(), 8);
    let e = link(c, "a", "b").unwrap_err();
    assert!(e.contains("holds 8 links already"), "{e}");
    assert!(stored(c, "b").links.is_empty());
    let e = link(c, "b", "a").unwrap_err();
    assert!(e.contains("the thread a holds 8 links"), "{e}");
}
