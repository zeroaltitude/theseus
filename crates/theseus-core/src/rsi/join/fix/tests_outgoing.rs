//! The outgoing check on the real paths (issue 53; `rsi::outgoing`): a builder's push of its branch, a fix at
//! the gate, a fix at review and the join, each against the fixture's bare upstream. Words a session working on
//! the item read from another conversation, ten or more in a row, never leave: in a comment, in a commit
//! message, in a commit a later one removed, in a fixer's commit, in the join's message. Nothing reaches the
//! upstream, the item is held for the owner, and no word of what matched is said anywhere. The repository's own
//! words pass; a read that cannot be resolved refuses.
//!
//! A builder's and a fixer's look back is a real history call in their own turn. The join's and the fix at
//! review's are the records such a call leaves (its result and its copy edge), written into the seeded
//! builder's session, which that rig never runs a turn in.

use std::sync::Arc;

use serde_json::json;

use super::tests::StandIn;
use super::tests_gate::{
    built, green, input, item, onto_main, rig, rows, tree_of, write, ITEM, LINTED, REPAIR, WIRE,
};
use crate::rpc::Core;
use crate::rsi::roles::Role;
use crate::rsi::tests_pr::Rig;
use crate::rsi::tests_tree::git;

/// What the owner said in another conversation: thirteen words.
pub(super) const SAID: &str =
    "kelp oars drift past the third bollard where the heron waits each grey morning";
/// The words of [`SAID`] no refusal may hold.
const TELLS: [&str; 4] = ["kelp", "bollard", "heron", "morning"];

/// A clean change the gate passes.
pub(super) const CLEAN: &[(&str, &str)] = &[
    ("fix.txt", "fixed\n"),
    ("tests/fix.sh", "test -f fix.txt\n"),
];

pub(super) fn holds_none(text: &str) {
    for w in TELLS {
        assert!(!text.to_lowercase().contains(w), "{w}: {text}");
    }
}

/// Another conversation in which the owner said `words`: one exchange at its own place; its handle.
pub(super) fn said_elsewhere(core: &Core, words: &str) -> String {
    use crate::exchange::{exchange_record, touch, Exchange, Opener, Root, OWNER};
    let other =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&other.session_id, &other).unwrap();
    let turn = crate::new_id("trn");
    let place = "local:/harbour";
    let at = theseus_protocol::now_unix_ms() - 60_000;
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
    core.store.append(&recs).unwrap();
    turn
}

/// `sid`'s own turn opens the other conversation's exchange through the history tool: a real look back.
pub(super) async fn looks_back(r: &Rig, sid: &str, words: &str) {
    let handle = said_elsewhere(&r.core, words);
    let read = r
        .turn(sid, "context_history", json!({ "open": handle }))
        .await;
    let word = words.split(' ').nth(5).unwrap();
    assert!(read.contains(word), "it read the words: {read}");
}

/// The owner's DM bound, so a hold's card has somewhere to go (`rsi::tests_pr`'s `bind_the_owners_dm`).
pub(super) fn bind_dm(r: &Rig) {
    r.core
        .runner
        .place_rule
        .bind_one(crate::places::BoundPlace {
            target: "discord:dm:7".into(),
            name: "DM".into(),
            private: true,
            ..Default::default()
        });
}

/// The record a look back leaves in `sid`: its result, kept by reference, quoting all of node `source` where it
/// is stored (a node never written is quoted at position zero, which cannot be read): for a session no turn runs
/// in.
pub(super) fn looked_back(core: &Core, sid: &str, source: &str) {
    use crate::node::{Body, Node, ResultStatus};
    use crate::quotes::{Field, Form, Part, Quote};
    let (position, to) = match core.store.get_node(source).unwrap() {
        Some((at, n)) => (at, crate::recall::text_of(&n).len() as u32),
        None => (0, 4),
    };
    let quote = Part::Quote(Quote {
        node: source.into(),
        position,
        field: Field::Text,
        from: 0,
        to,
        form: Form::Verbatim,
    });
    let turn = crate::new_id("trn");
    let result = Node::tool_result(
        sid,
        Some(&turn),
        Some(0),
        Body::ToolResult {
            tool_use_id: "tu_look".into(),
            tool: crate::view::history::HISTORY_TOOL.into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: String::new(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: Some(5),
            late: false,
            meta: json!({}),
            image: None,
            external: None,
            payload: None,
            parts: vec![quote],
        },
    );
    core.store.append(&[result.record().unwrap()]).unwrap();
}

/// [`looked_back`] at the owner's words in another conversation.
pub(super) fn read_elsewhere(core: &Core, sid: &str, words: &str) {
    let turn = said_elsewhere(core, words);
    let x = crate::exchange::exchange(&core.store, &turn)
        .unwrap()
        .unwrap();
    looked_back(core, sid, &x.ask.unwrap());
}

/// The builder of [`ITEM`], wide, its tree open.
pub(super) async fn builder(r: &Rig) -> String {
    bind_dm(r);
    let sid = item(r);
    r.core.tools.self_pr.set_role(&sid, Role::Builder).unwrap();
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("Tree open"), "{said}");
    sid
}

/// Nothing of the item's branches is at the upstream.
pub(super) fn nothing_left(r: &Rig) {
    let left = git(&r.f.upstream, &["for-each-ref", "refs/heads/self/"]);
    assert!(left.is_empty(), "at the upstream: {left}");
}

/// The attempt was held for the owner: its reply, its held stop row and its one card hold no word of what matched
/// (the attempt's own row keeps the builder's why, as every attempt's always has: a local row, never sent).
pub(super) fn held(r: &Rig, said: &str) {
    held_as(r, said, ITEM);
}

/// [`held`], of item `item`.
pub(super) fn held_as(r: &Rig, said: &str, item: &str) {
    assert!(said.contains("It was not pushed"), "{said}");
    holds_none(said);
    let b = built(r);
    assert_eq!(b["stopped_at"], "push", "{b}");
    let mark =
        r.f.place
            .trees()
            .join(format!("{}.keel", crate::rsi::tree::slug(item)));
    assert!(mark.exists(), "held by the keel stop's mark");
    let stops = rows(r, "self.join.stopped");
    let [stop] = stops.as_slice() else {
        panic!("one held stop row: {stops:?}");
    };
    assert_eq!(stop["numbers"]["held"], true, "{stop}");
    assert_eq!(stop["head"], b["head"], "{stop}");
    holds_none(&stop["why"].to_string());
    holds_none(&stop["numbers"].to_string());
    let cards = crate::tests_day_ceiling::posts(&r.core, "self_keel");
    let [card] = cards.as_slice() else {
        panic!("one card: {cards:?}");
    };
    holds_none(&card.to_string());
}

/// A builder that read the owner's words in a look back and copied twelve of them into a comment: refused at
/// the push of its branch, nothing at the upstream, the item held, and no word of them in the refusal. Its next
/// call is refused while it is held.
#[tokio::test]
async fn a_copied_run_in_a_comment_is_refused_at_the_branch_push() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    let twelve: Vec<&str> = SAID.split(' ').take(12).collect();
    let mut files = CLEAN.to_vec();
    let comment = format!("// {}\nfn sail() {{}}\n", twelve.join(" "));
    files.push(("src/sail.rs", &comment));
    write(&tree_of(&r), &files);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("a look back"), "{said}");
    held(&r, &said);
    nothing_left(&r);
    let again = r.turn(&sid, WIRE, input()).await;
    assert!(again.contains("is held for the owner"), "{again}");
}

/// The same run in the commit's message (the change's why): refused.
#[tokio::test]
async fn a_copied_run_in_a_commit_message_is_refused() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    write(&tree_of(&r), CLEAN);
    let why = "Kelp oars drift past the third bollard where the heron waits";
    let said = r
        .turn(
            &sid,
            WIRE,
            json!({"item": ITEM, "issue": "theseus-sail1", "why": why}),
        )
        .await;
    held(&r, &said);
    nothing_left(&r);
    // The attempt's own row keeps the builder's why, as every attempt's always has (a local row, never sent).
    assert_eq!(built(&r)["why"], why);
}

/// The run in a commit a later commit removed: the tip's net change holds none of it, but the commit it was
/// added in is sent, so it is refused.
#[tokio::test]
async fn a_copied_run_in_a_commit_removed_at_the_tip_is_refused() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    let tree = tree_of(&r);
    write(&tree, &[("notes.md", SAID)]);
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "-m", "notes"]);
    std::fs::remove_file(tree.join("notes.md")).unwrap();
    write(&tree, CLEAN);
    let said = r.turn(&sid, WIRE, input()).await;
    held(&r, &said);
    nothing_left(&r);
}

/// A fixer that read the owner's words, and whose fix at the gate copies them: refused at the push, naming the
/// fixer's commit.
#[tokio::test]
async fn a_fixers_copied_run_is_refused() {
    let r = rig(|_| {});
    let fixer_sid = r.session("self");
    (r.core.tools.self_pr)
        .set_role(&fixer_sid, Role::Fixer)
        .unwrap();
    looks_back(&r, &fixer_sid, SAID).await;
    let mut writes = REPAIR.to_vec();
    let comment = format!("# {SAID}\n");
    writes.push(("fix-notes.txt", &comment));
    r.core.tools.self_pr.set_fix(crate::rsi::pr::GateSeams {
        fixer: Arc::new(Reading {
            inner: StandIn::writing(&writes),
            session: fixer_sid,
        }),
        main: green(),
    });
    let sid = builder(&r).await;
    write(&tree_of(&r), LINTED);
    let said = r.turn(&sid, WIRE, input()).await;
    let made = rows(&r, "self.fix.made").pop().expect("the fix was made");
    let fix = made["head"].as_str().unwrap();
    assert!(said.contains(&fix[..12]), "the fixer's commit: {said}");
    held(&r, &said);
    nothing_left(&r);
}

/// A fixer that is the stand-in's, its session the one given.
struct Reading {
    inner: Arc<StandIn>,
    session: String,
}

impl super::Fixer for Reading {
    fn fix<'a>(&'a self, core: &'a Arc<Core>, ask: &'a super::FixAsk) -> super::FixFut<'a> {
        Box::pin(async move {
            let mut f = super::Fixer::fix(&*self.inner, core, ask).await?;
            f.session = Some(self.session.clone());
            Ok(f)
        })
    }
    fn approve<'a>(
        &'a self,
        core: &'a Arc<Core>,
        ask: &'a super::ApproveAsk,
    ) -> crate::rsi::join::review::Fut<'a> {
        super::Fixer::approve(&*self.inner, core, ask)
    }
}

/// The repository already holds the run at the base: the builder quoting it is quoting the code, and the branch
/// is pushed.
#[tokio::test]
async fn a_run_the_repository_holds_at_the_base_is_pushed() {
    let r = rig(|_| {});
    onto_main(&r, "harbour-notes", &[("docs/harbour.md", SAID)]);
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    let mut files = CLEAN.to_vec();
    let comment = format!("// {SAID}\n");
    files.push(("src/sail.rs", &comment));
    write(&tree_of(&r), &files);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("Pushed to origin"), "{said}");
}

/// Nine words of what was read are not a match: the branch is pushed, as a clean one is today.
#[tokio::test]
async fn nine_words_of_what_was_read_are_pushed() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looks_back(&r, &sid, SAID).await;
    let nine: Vec<&str> = SAID.split(' ').take(9).collect();
    let mut files = CLEAN.to_vec();
    let comment = format!("// {}\n", nine.join(" "));
    files.push(("src/sail.rs", &comment));
    write(&tree_of(&r), &files);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("Pushed to origin"), "{said}");
    let b = built(&r);
    let head = b["head"].as_str().unwrap();
    let branch = b["branch"].as_str().unwrap();
    assert_eq!(
        git(
            &r.f.upstream,
            &["rev-parse", &format!("refs/heads/{branch}")]
        ),
        head
    );
}

/// A look back whose source cannot be found cannot be cleared: the push is refused, as for a match.
#[tokio::test]
async fn a_read_that_cannot_be_resolved_refuses_the_push() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    looked_back(&r.core, &sid, "msg_never_written");
    write(&tree_of(&r), CLEAN);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("cannot be read as it was written"), "{said}");
    held(&r, &said);
    nothing_left(&r);
}

/// A check put off (here its read past its deadline; the ledger's index not yet built is the same answer,
/// `rsi::outgoing::read`) puts the branch's push off: not pushed, no hold, no mark, no held row, no card; the join
/// checks it again.
#[tokio::test]
async fn a_check_put_off_at_the_branch_holds_nothing_and_posts_no_card() {
    let r = rig(|_| {});
    let sid = builder(&r).await;
    write(&tree_of(&r), CLEAN);
    let _deadline = crate::rsi::outgoing::clear::tests_deadline::set(std::time::Duration::ZERO);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("put off"), "{said}");
    nothing_left(&r);
    let mark = r.f.place.trees().join(format!("{ITEM}.keel"));
    assert!(!mark.exists(), "no hold");
    assert!(rows(&r, "self.join.stopped").is_empty(), "no held row");
    assert!(
        crate::tests_day_ceiling::posts(&r.core, "self_keel").is_empty(),
        "no card"
    );
}

mod join {
    //! The join and the fix at review, over the join's rig.
    use std::sync::Arc;
    use std::time::Duration;

    use theseus_protocol::rsi::SelfMode;
    use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};

    use super::super::tests::{scripted, StandIn};
    use super::super::tests_gate::GATE;
    use super::{holds_none, read_elsewhere, SAID};
    use crate::rsi::join::tests::Scripts;
    use crate::rsi::join::tests::{git, rig_on, Rig, PROVED, SONNET};
    use crate::rsi::join::Seams;
    use crate::rsi::tests_tree::fixture_with;

    fn rig() -> Rig {
        rig_on(fixture_with(GATE), SelfMode::Act, |_| {}, |_| {})
    }

    fn seams(review: &[&'static str], fixer: Arc<StandIn>) -> Seams {
        Seams {
            reviewer: scripted(review),
            tests: Arc::new(Scripts),
            fixer,
            lock_every: Duration::from_millis(50),
            lock_wait: Duration::from_secs(60),
            ..Seams::default()
        }
    }

    fn item(r: &Rig, id: &str) {
        let t = TaskRecord {
            id: id.into(),
            version: 1,
            title: "Trim the sails".into(),
            state: TaskState::InProgress,
            origin: TaskOrigin {
                session: "self:backlog".into(),
                principal: "self".into(),
                ..Default::default()
            },
            ..TaskRecord::default()
        };
        (r.core.store)
            .append(&[crate::task_graph::record(&t).unwrap()])
            .unwrap();
    }

    /// The session that built `branch`, as its built row names it.
    fn builder_of(r: &Rig, branch: &str) -> String {
        (r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000))
        .unwrap()
        .into_iter()
        .find(|(_, row)| row.kind == "self.branch.built" && row.data["branch"] == branch)
        .and_then(|(_, row)| row.session_id)
        .expect("its built row")
    }

    /// The held stop, its row and the owner's card, none holding a word of what matched.
    fn held_at_push(r: &Rig) {
        let stop = r.rows("self.join.stopped").pop().expect("a stop");
        assert_eq!(stop["phase"], "push", "{stop}");
        assert_eq!(stop["numbers"]["held"], true, "{stop}");
        holds_none(&stop.to_string());
        let cards = crate::tests_day_ceiling::posts(&r.core, "self_join");
        assert_eq!(cards.len(), 1, "one card: {cards:?}");
        let card = cards.last().expect("the owner's card").to_string();
        assert!(card.contains("This push was stopped"), "{card}");
        holds_none(&card);
    }

    /// The join's own message carries the change's why: words the builder read, past what its commit's subject
    /// kept, are refused there. Main is not moved, and the head is held.
    #[tokio::test]
    async fn a_copied_run_in_the_joins_why_is_refused() {
        let r = rig();
        let main = r.upstream_main();
        let why = format!("Trim the sails before the wind, since {SAID}.");
        let (branch, _) = r.seed_why("trim", PROVED, SONNET, &why);
        item(&r, "trim");
        read_elsewhere(&r.core, &builder_of(&r, &branch), SAID);
        let pass = r
            .core
            .self_join_pass(&seams(&["accept"], StandIn::writing(&[])))
            .await
            .unwrap();
        assert!(pass.joined.is_empty(), "{pass:?}");
        assert_eq!(r.upstream_main(), main, "main did not move");
        held_at_push(&r);
    }

    /// A fix at review that copies the words is not pushed, and the pass no longer goes on as if it were: the
    /// head is held for the owner, and the next pass reviews nothing of it.
    #[tokio::test]
    async fn a_refused_push_of_a_fix_at_review_holds_and_does_not_go_on() {
        let r = rig();
        let (branch, _) = r.seed("trim", PROVED, SONNET);
        item(&r, "trim");
        read_elsewhere(&r.core, &builder_of(&r, &branch), SAID);
        let fixer = StandIn::writing(&[("note.txt", SAID)]);
        let s = seams(&["fix", "accept"], fixer);
        let one = r.core.self_join_pass(&s).await.unwrap();
        assert!(one.joined.is_empty(), "{one:?}");
        let left = git(&r.f.upstream, &["for-each-ref", "refs/heads/self/"]);
        assert!(left.is_empty(), "at the upstream: {left}");
        held_at_push(&r);
        let two = r.core.self_join_pass(&s).await.unwrap();
        assert!(two.reviewed.is_empty() && two.joined.is_empty(), "{two:?}");
    }

    /// A read past its deadline puts the join off: a stop row and no card twice, and the third time in a row the
    /// head is held as unresolved, with one card; a pass after it does nothing more.
    #[tokio::test]
    async fn a_read_past_its_deadline_is_put_off_twice_and_held_the_third_time() {
        let r = rig();
        let main = r.upstream_main();
        let (branch, _) = r.seed("trim", PROVED, SONNET);
        item(&r, "trim");
        read_elsewhere(&r.core, &builder_of(&r, &branch), SAID);
        let _deadline = crate::rsi::outgoing::clear::tests_deadline::set(Duration::ZERO);
        let s = seams(&["accept"], StandIn::writing(&[]));
        for pass in 1..=4 {
            let p = r.core.self_join_pass(&s).await.unwrap();
            assert!(p.joined.is_empty(), "pass {pass}: {p:?}");
            let stops = r.rows("self.join.stopped");
            let cards = crate::tests_day_ceiling::posts(&r.core, "self_join");
            match pass {
                1 | 2 => {
                    assert_eq!(stops.len(), pass, "pass {pass}: {stops:?}");
                    assert_eq!(stops[pass - 1]["phase"], "outgoing", "{stops:?}");
                    assert_ne!(stops[pass - 1]["numbers"]["held"], true, "{stops:?}");
                    assert!(cards.is_empty(), "pass {pass}: no card");
                }
                _ => {
                    assert_eq!(stops.len(), 3, "pass {pass}: {stops:?}");
                    assert_eq!(stops[2]["numbers"]["held"], true, "{stops:?}");
                    assert_eq!(cards.len(), 1, "pass {pass}: one card");
                }
            }
        }
        assert_eq!(r.upstream_main(), main, "main did not move");
    }

    /// What the item's sessions read is gathered once in a pass: a join retried when main moved under its push
    /// clears its second graph against the same reads.
    #[tokio::test]
    async fn what_was_read_is_gathered_once_a_pass() {
        use std::sync::atomic::Ordering;
        let r = rig();
        let (branch, _) = r.seed("trim", PROVED, SONNET);
        item(&r, "trim");
        read_elsewhere(
            &r.core,
            &builder_of(&r, &branch),
            "the nets were mended at dawn",
        );
        let (root, up) = (
            r.f.place.state.parent().unwrap().to_path_buf(),
            r.f.upstream.clone(),
        );
        let s = Seams {
            before_push: Some(Arc::new(move |attempt| {
                if attempt == 1 {
                    let other = root.join("mover");
                    git(
                        &root,
                        &["clone", "-q", up.to_str().unwrap(), other.to_str().unwrap()],
                    );
                    std::fs::write(other.join("elsewhere.txt"), "elsewhere\n").unwrap();
                    git(&other, &["add", "-A"]);
                    git(&other, &["commit", "-q", "-m", "elsewhere"]);
                    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
                }
            })),
            ..seams(&["accept"], StandIn::writing(&[]))
        };
        let before = crate::rsi::outgoing::read::tests_hooks::READS.load(Ordering::SeqCst);
        let p = r.core.self_join_pass(&s).await.unwrap();
        assert_eq!(p.joined.len(), 1, "{p:?}");
        assert_eq!(
            r.rows("self.joined")[0]["numbers"]["tries"],
            2,
            "two graphs cleared"
        );
        let read = crate::rsi::outgoing::read::tests_hooks::READS.load(Ordering::SeqCst) - before;
        assert_eq!(read, 1, "read once for the pass");
    }
}
