//! What leaves, as git sends it (round 4 of issue 53; `rsi::outgoing::leave`), through a builder's real push
//! against the fixture's bare upstream: a rename that holds the words in a file's name, a file that cannot be
//! read as text (a NUL, the `binary` attribute), a run new words complete across words the file already held, and
//! the branch's own name; and a hold that is a hold everywhere: a branch held at its push is never reviewed,
//! joined or carded again.

use serde_json::json;

use super::tests_gate::{built, input, onto_main, rig, rows, tree_of, write, WIRE};
use super::tests_outgoing::{
    bind_dm, builder, held, held_as, holds_none, looks_back, nothing_left, CLEAN, SAID,
};
use crate::rsi::tests_pr::Rig;
use crate::rsi::tests_tree::git;

/// Ten of [`SAID`]'s words, hyphenated, as a file's name: no word of them on a `+` line.
fn hyphened() -> String {
    SAID.split(' ').take(10).collect::<Vec<_>>().join("-")
}

/// A commit in the builder's tree of what is there now.
fn commit_all(r: &Rig, message: &str) {
    let tree = tree_of(r);
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "-m", message]);
}

/// A commit that only renames a file, to a name holding ten of the words: refused for its path.
#[tokio::test]
async fn a_rename_only_commit_with_a_ten_word_name_is_refused() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    write(&tree_of(&r), CLEAN);
    write(&tree_of(&r), &[("docs/notes.md", "tide tables\n")]);
    commit_all(&r, "notes");
    let tree = tree_of(&r);
    git(
        &tree,
        &["mv", "docs/notes.md", &format!("docs/{}.md", hyphened())],
    );
    commit_all(&r, "rename");
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("the name of a path it changes"), "{said}");
    held(&r, &said);
    nothing_left(&r);
}

/// A changed file holding a NUL cannot be read as text: it is not inspected, and the branch is held with a card
/// that says so. The NUL is past the first 8000 bytes, which is all git's own binary rule looks at: only the
/// check's own reading of the blob finds it.
#[tokio::test]
async fn a_file_with_a_nul_is_not_inspected_and_is_held() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    let tree = tree_of(&r);
    write(&tree, CLEAN);
    let mut data = "tide tables\n".repeat(800).into_bytes();
    data.extend(b"\0and more tables\n");
    std::fs::write(tree.join("data.txt"), data).unwrap();
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("cannot be read as text"), "{said}");
    held(&r, &said);
    nothing_left(&r);
    let card = crate::tests_day_ceiling::posts(&r.core, "self_keel")
        .pop()
        .unwrap();
    assert!(card.to_string().contains("could not be checked"), "{card}");
}

/// A file the repository's attributes mark binary is not inspected either, though it is text.
#[tokio::test]
async fn a_file_marked_binary_is_not_inspected_and_is_held() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    let tree = tree_of(&r);
    write(&tree, CLEAN);
    write(
        &tree,
        &[
            (".gitattributes", "notes.txt binary\n"),
            ("notes.txt", "tide tables\n"),
        ],
    );
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("cannot be read as text"), "{said}");
    held(&r, &said);
    nothing_left(&r);
}

/// Five of the words already a line of a file on main, five more added as the next line: neither main nor the
/// lines added hold the run, the whole file the commit sends does, and it is refused.
#[tokio::test]
async fn a_run_completed_across_old_and_new_words_is_refused() {
    let r = rig(|_| {});
    let w: Vec<&str> = SAID.split(' ').collect();
    onto_main(
        &r,
        "harbour-notes",
        &[("docs/harbour.md", &format!("{}\n", w[..5].join(" ")))],
    );
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    let mut files = CLEAN.to_vec();
    let line = format!("{}\n{}\n", w[..5].join(" "), w[5..10].join(" "));
    files.push(("docs/harbour.md", &line));
    write(&tree_of(&r), &files);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("a file it changes"), "{said}");
    held(&r, &said);
    nothing_left(&r);
}

/// Words of a conversation in the branch's own name (its item's): refused before any push, and nothing written
/// for the refusal names the branch, the item or the words.
#[tokio::test]
async fn a_ten_word_branch_name_is_refused_before_any_push() {
    use theseus_protocol::tasks::{TaskEvidence, TaskOrigin, TaskRecord, TaskState};
    const SHORT: &str = "the owner said go to sea at six as we do on it each day";
    const NAMED: &str = "go-to-sea-at-six-as-we-do-on-it";
    let r = rig(|_| {});
    bind_dm(&r);
    let sid = r.session("self");
    let t = TaskRecord {
        id: NAMED.into(),
        version: 1,
        title: "Sail".into(),
        state: TaskState::InProgress,
        session: Some(sid.clone()),
        origin: TaskOrigin {
            session: "self:backlog".into(),
            principal: "self".into(),
            ..Default::default()
        },
        evidence: vec![TaskEvidence {
            identity: format!("session:{sid}"),
            by: "self-backlog".into(),
            at_ms: 1,
            ..Default::default()
        }],
        ..TaskRecord::default()
    };
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    r.core
        .tools
        .self_pr
        .set_role(&sid, crate::rsi::roles::Role::Builder)
        .unwrap();
    let ask = json!({"item": NAMED, "issue": "theseus-sail2", "why": "Sail."});
    let said = r.turn(&sid, WIRE, ask.clone()).await;
    assert!(said.contains("Tree open"), "{said}");
    looks_back(&r, &sid, SHORT).await;
    let tree =
        r.f.place
            .trees()
            .canonicalize()
            .unwrap()
            .join(crate::rsi::tree::slug(NAMED));
    write(&tree, CLEAN);
    let said = r.turn(&sid, WIRE, ask).await;
    assert!(said.contains("the name of the branch"), "{said}");
    held_as(&r, &said, NAMED);
    nothing_left(&r);
    let card = crate::tests_day_ceiling::posts(&r.core, "self_keel")
        .pop()
        .unwrap()
        .to_string();
    // The branch is `self/<date>-<the slug>`: neither its name nor its slug is said.
    for w in ["go-to-sea", "sea at six", "-go-to", "self/2026"] {
        assert!(!card.contains(w), "{w}: {card}");
        assert!(!said.contains(w), "{w}: {said}");
    }
}

/// A branch held at its push is held everywhere, once: a pass of the join after it, and another (a new night, a
/// restart: the hold is in the store, not in memory), neither review it nor join it, and post no card of their
/// own; the one card stays the one.
#[tokio::test]
async fn a_branch_held_at_its_push_is_never_reviewed_joined_or_carded_again() {
    use std::sync::Arc;
    use std::time::Duration;
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    let mut files = CLEAN.to_vec();
    let comment = format!("// {SAID}\n");
    files.push(("src/sail.rs", &comment));
    write(&tree_of(&r), &files);
    let said = r.turn(&sid, WIRE, input()).await;
    held(&r, &said);
    let b = built(&r);
    assert_eq!(b["gate"], "green", "an attempt the join would take: {b}");
    let main = git(&r.f.upstream, &["rev-parse", "refs/heads/main"]);
    let seams = crate::rsi::join::Seams {
        reviewer: super::tests::scripted(&["accept", "accept"]),
        tests: Arc::new(crate::rsi::join::tests::Scripts),
        fixer: super::tests::StandIn::writing(&[]),
        lock_every: Duration::from_millis(50),
        lock_wait: Duration::from_secs(60),
        ..crate::rsi::join::Seams::default()
    };
    for pass in 1..=2 {
        let p = r.core.self_join_pass(&seams).await.unwrap();
        assert!(
            p.reviewed.is_empty() && p.joined.is_empty(),
            "pass {pass}: {p:?}"
        );
    }
    assert_eq!(git(&r.f.upstream, &["rev-parse", "refs/heads/main"]), main);
    assert!(rows(&r, "self.review.verdict").is_empty(), "no review");
    assert_eq!(rows(&r, "self.join.stopped").len(), 1, "the one held stop");
    assert_eq!(
        crate::tests_day_ceiling::posts(&r.core, "self_keel").len(),
        1,
        "one card"
    );
    assert!(
        crate::tests_day_ceiling::posts(&r.core, "self_join").is_empty(),
        "no card of the join's"
    );
    holds_none(&rows(&r, "self.join.stopped")[0].to_string());
}

/// The place a session's first exchange was at.
fn place_of(r: &Rig, sid: &str) -> String {
    let turn = (r.core.store.session_nodes(sid).unwrap().into_iter())
        .find_map(|(_, n)| n.turn_id)
        .unwrap();
    crate::exchange::exchange(&r.core.store, &turn)
        .unwrap()
        .unwrap()
        .place
}

/// `words` said in a session of its own at `place`, as one exchange: its turn.
fn said_at(r: &Rig, place: &str, words: &str) -> String {
    use crate::exchange::{exchange_record, touch, Exchange, Opener, Root, OWNER};
    let c = &r.core;
    let other =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&other.session_id, &other).unwrap();
    let turn = crate::new_id("trn");
    let at = theseus_protocol::now_unix_ms() - 3_600_000;
    let ask = crate::node::Node::user(&other.session_id, Some(&turn), "owner", words);
    let x = Exchange {
        turn_id: turn.clone(),
        session_id: other.session_id.clone(),
        place: place.into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(OWNER.into()),
        start_ms: at,
        end_ms: Some(at + 500),
        ask: Some(ask.id.clone()),
        reply: None,
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    let mut recs = vec![ask.record().unwrap(), exchange_record(&x).unwrap()];
    recs.extend(touch(Root::Place(place), &turn, &other.session_id, at));
    c.store.append(&recs).unwrap();
    turn
}

/// A fold the ladder's own writer wrote over another conversation's exchange at the builder's place, laid by the
/// builder's real view (`ladder::fold::write_once`: its key holds its range's digest, and is found by its handle):
/// a clean branch pushes, and one holding ten words of the fold's account is refused.
#[tokio::test]
async fn a_fold_a_real_view_laid_is_read_by_its_handle() {
    use crate::ladder::fold::{write_once, FoldNode};
    for copied in [false, true] {
        let r = rig(|_| {});
        let sid = builder(&r).await;
        let place = place_of(&r, &sid);
        let turn = said_at(
            &r,
            &place,
            "the nets were mended at dawn by the harbour wall",
        );
        let x = crate::exchange::exchange(&r.core.store, &turn)
            .unwrap()
            .unwrap();
        let fold = FoldNode {
            id: format!("fold-{turn}"),
            place: place.clone(),
            level: 1,
            exchanges: vec![turn.clone()],
            first_ms: x.start_ms,
            last_ms: x.start_ms,
            his: vec![],
            counted: 0,
            children: vec![],
            title: "the harbour".into(),
            account: format!("ACCOUNT: {SAID}"),
            marks: crate::view::Marks {
                place: Some(place.clone()),
                audience: crate::view::Audience::Private,
                author: crate::view::Author::Theseus,
                outside: false,
                sealed: false,
                veiled: false,
            },
            model: None,
            writer: 1,
            at_ms: x.start_ms,
        };
        theseus_store::blocking(|| write_once(&r.core.store, fold)).unwrap();
        let mut files = CLEAN.to_vec();
        let comment = format!("// {SAID}\n");
        if copied {
            files.push(("src/sail.rs", &comment));
        }
        write(&tree_of(&r), &files);
        let said = r.turn(&sid, WIRE, input()).await;
        let last = (r
            .core
            .store
            .get_session::<crate::session::SessionRecord>(&sid))
        .unwrap()
        .unwrap()
        .last_turn_id
        .unwrap();
        let views = crate::view::record::every_of_turn(&r.core.store, &last).unwrap();
        let laid = (views.iter().flat_map(|v| &v.shown))
            .any(|s| matches!(s.piece.range.space, crate::view::Space::Folds { .. }));
        assert!(laid, "the builder's view laid the fold");
        match copied {
            false => assert!(said.contains("Pushed to origin"), "{said}"),
            true => {
                assert!(said.contains("the view of another session"), "{said}");
                held(&r, &said);
                nothing_left(&r);
            }
        }
    }
}
