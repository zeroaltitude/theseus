//! What an item's sessions read (`super`), gathered from a store laid out as the real writers lay it: every view
//! of a turn to the index's end, a view the index lists that is gone, a quote read where it was shown, a laid
//! exchange followed through its parts, an erased source, a fold by its handle (and an older key), a child's and
//! an earlier start's reads, a result stored before quotes, and the cost and the deadline of the read.

use std::time::{Duration, Instant};

use serde_json::json;

use super::super::leave::{Leaving, Out};
use super::super::{Deferral, Heard, Refusal, Source, Unread, Why};
use crate::node::{Body, Node, ResultStatus};
use crate::quotes::{Field, Form, Part as QuotePart, Quote};
use crate::view::{Handle, Pos, Space};
use crate::Core;

/// Words of the owner's in another conversation: thirteen.
const SAID: &str = "kelp oars drift past the third bollard where the heron waits each grey morning";
/// The item the reads are gathered for: no task, no rows, so only the sessions named are read.
const ITEM: &str = "kelp-nets";
const PLACE: &str = "local:/harbour";

fn rig() -> crate::tests_places::Rig {
    crate::tests_places::rig_with(|_| {})
}

fn far() -> Instant {
    Instant::now() + Duration::from_secs(600)
}

/// One commit leaving whose message is `text`.
fn leaving(text: &str) -> Leaving {
    Leaving {
        branch: "self/20261125-kelp".into(),
        commits: vec![Out {
            id: "c0ffee00c0ffee00".into(),
            message: text.into(),
            ..Out::default()
        }],
    }
}

/// Whether `h` refuses [`SAID`] copied into a commit, and from which kind of read.
fn refuses(h: &Heard) -> Option<Source> {
    match h.check(&leaving(SAID), |_| Ok(Default::default())) {
        Err(Refusal::Words { source, .. }) => Some(source),
        _ => None,
    }
}

/// A new session of its own.
pub(crate) fn session(c: &Core) -> String {
    let s = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&s.session_id, &s).unwrap();
    s.session_id
}

/// `words` said in `sid` at [`PLACE`], as one exchange: its turn and its message's node.
pub(crate) fn said(c: &Core, sid: &str, words: &str) -> (String, String) {
    use crate::exchange::{exchange_record, Exchange, Opener, OWNER};
    let turn = crate::new_id("trn");
    let ask = Node::user(sid, Some(&turn), "owner", words);
    let x = Exchange {
        turn_id: turn.clone(),
        session_id: sid.into(),
        place: PLACE.into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(OWNER.into()),
        start_ms: 1_000,
        end_ms: Some(2_000),
        ask: Some(ask.id.clone()),
        reply: None,
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    c.store
        .append(&[ask.record().unwrap(), exchange_record(&x).unwrap()])
        .unwrap();
    (turn, ask.id)
}

/// A turn of `sid`'s own, with a message of its own: what its views are kept under.
fn own_turn(c: &Core, sid: &str) -> String {
    let turn = crate::new_id("trn");
    let n = Node::user(sid, Some(&turn), "owner", "trim the sails");
    c.store.append(&[n.record().unwrap()]).unwrap();
    turn
}

/// A look back's result in `sid` (in `turn`, else a turn of its own), kept by reference in `parts` (none: its
/// words `content` inline).
fn look(c: &Core, sid: &str, turn: Option<&str>, content: &str, parts: Vec<QuotePart>) -> Node {
    let turn = turn.map_or_else(|| crate::new_id("trn"), str::to_string);
    let tool = crate::view::history::HISTORY_TOOL;
    let n = Node::tool_result(
        sid,
        Some(&turn),
        Some(0),
        Body::ToolResult {
            tool_use_id: "tu_look".into(),
            tool: tool.into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: content.into(),
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
            parts,
        },
    );
    c.store.append(&[n.record().unwrap()]).unwrap();
    n
}

/// A quote of all of node `id`'s text, at the position it is stored at now.
fn quote_of(c: &Core, id: &str) -> QuotePart {
    let (position, n) = c.store.get_node(id).unwrap().unwrap();
    let text = crate::recall::text_of(&n);
    QuotePart::Quote(Quote {
        node: id.into(),
        position,
        field: Field::Text,
        from: 0,
        to: text.len() as u32,
        form: Form::Verbatim,
    })
}

/// A view piece laying exchange `turn`.
fn exchange_piece(turn: &str) -> crate::view::Piece {
    let mut p = crate::tests_view_record::fold_piece(PLACE, 1, (turn, turn), turn);
    p.range.space = Space::Exchanges {
        place: PLACE.into(),
    };
    p.handle = Handle(turn.into());
    p.range.first = Pos {
        at_ms: 1_000,
        id: turn.into(),
    };
    p.range.last = p.range.first.clone();
    p
}

/// Every view of a turn is read, to the index's end: a turn with forty views, only the first of which laid the
/// other conversation, is still refused. (The display reader shows only the newest thirty-two.)
#[tokio::test]
async fn every_view_of_a_turn_is_read_to_the_index_end() {
    let r = rig();
    let c = &r.core;
    let (other, builder) = (session(c), session(c));
    let (x, _) = said(c, &other, SAID);
    let turn = own_turn(c, &builder);
    crate::tests_view_record::keep(c, &turn, "cmp_000", PLACE, vec![exchange_piece(&x)]);
    for i in 1..40 {
        crate::tests_view_record::keep(c, &turn, &format!("cmp_{i:03}"), PLACE, vec![]);
    }
    assert_eq!(crate::view::record::of_turn(&c.store, &turn).unwrap().len(), 32);
    let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
    assert_eq!(refuses(&h), Some(Source::View));
}

/// A view the turn's index lists whose record is not there is unresolved, never an empty read.
#[tokio::test]
async fn an_indexed_view_that_is_missing_is_unresolved() {
    let r = rig();
    let c = &r.core;
    let builder = session(c);
    let turn = own_turn(c, &builder);
    let entry = crate::view::record::turn_record(&turn, "cmp_gone", 1).unwrap();
    c.store.append(&[entry]).unwrap();
    let got = Heard::of(&c.store, ITEM, &[&builder], far()).map(|_| ());
    assert_eq!(got, Err(Unread::Unresolved(Why::ViewMissing)));
}

/// A look back's quote is read where it was shown (its record's position), not as its source reads now: a source
/// rewritten under its id since still refuses the words shown, and the new words, never shown, pass.
#[tokio::test]
async fn a_quote_is_read_where_it_was_shown_not_as_its_source_reads_now() {
    let r = rig();
    let c = &r.core;
    let (other, builder) = (session(c), session(c));
    let (_, node) = said(c, &other, SAID);
    look(c, &builder, None, "", vec![quote_of(c, &node)]);
    const LATER: &str = "an entirely different sentence now stands where those words once were written down";
    let (_, mut n) = c.store.get_node(&node).unwrap().unwrap();
    if let Body::UserMessage { text, .. } = &mut n.body {
        *text = LATER.into();
    }
    c.store.append(&[n.record().unwrap()]).unwrap();
    let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
    assert_eq!(refuses(&h), Some(Source::History));
    assert_eq!(h.check(&leaving(LATER), |_| Ok(Default::default())), Ok(()));
}

/// A quote that cannot be read as it was written is unresolved.
#[tokio::test]
async fn a_quote_that_cannot_be_read_is_unresolved() {
    let r = rig();
    let c = &r.core;
    let builder = session(c);
    let gone = QuotePart::Quote(Quote {
        node: "msg_never_written".into(),
        position: 0,
        field: Field::Text,
        from: 0,
        to: 4,
        form: Form::Verbatim,
    });
    look(c, &builder, None, "", vec![gone]);
    let got = Heard::of(&c.store, ITEM, &[&builder], far()).map(|_| ());
    assert_eq!(got, Err(Unread::Unresolved(Why::Quote)));
}

/// An exchange of another conversation a view laid, which holds that conversation's own look back kept by
/// reference, is read through its parts: the words it quoted from a third are refused.
#[tokio::test]
async fn a_laid_exchange_holding_a_look_back_is_followed_through_its_parts() {
    let r = rig();
    let c = &r.core;
    let (third, other, builder) = (session(c), session(c), session(c));
    let (_, node) = said(c, &third, SAID);
    let (x, _) = said(c, &other, "show me what was said in the harbour");
    look(c, &other, Some(&x), "", vec![quote_of(c, &node)]);
    let turn = own_turn(c, &builder);
    crate::tests_view_record::keep(c, &turn, "cmp_x", PLACE, vec![exchange_piece(&x)]);
    let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
    assert_eq!(refuses(&h), Some(Source::View));
}

/// A source erased since is nothing to match: its words are never read again, and the read is not unresolved.
#[tokio::test]
async fn an_erased_source_is_nothing_to_match() {
    let r = rig();
    let c = &r.core;
    let (other, builder) = (session(c), session(c));
    let (_, node) = said(c, &other, SAID);
    look(c, &builder, None, "", vec![quote_of(c, &node)]);
    let (_, n) = c.store.get_node(&node).unwrap().unwrap();
    c.store
        .append(&[n.erased(9_000, "the owner erased it").record().unwrap()])
        .unwrap();
    let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
    assert_eq!(refuses(&h), None);
}

/// A result stored before results were kept by reference is read by its own words, with no copy edge at all.
#[tokio::test]
async fn a_result_stored_before_quotes_is_read_by_its_own_words() {
    let r = rig();
    let c = &r.core;
    let builder = session(c);
    let inline = format!("An opened exchange:\n> {SAID}");
    look(c, &builder, None, &inline, vec![]);
    let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
    assert_eq!(refuses(&h), Some(Source::History));
}

/// A fold a view laid is read by its handle, through the ladder's own reader: one the real writer wrote (its key
/// holds its range's digest), and one under an older key its handle still names. An unknown handle is unresolved.
#[tokio::test]
async fn a_fold_is_read_by_its_handle_whatever_its_key() {
    use crate::ladder::fold::{self, FoldNode, HANDLE_PREFIX};
    let r = rig();
    let c = &r.core;
    let other = session(c);
    let (x, _) = said(c, &other, "the nets were mended at dawn");
    let node = |id: &str| FoldNode {
        id: id.into(),
        place: PLACE.into(),
        level: 1,
        exchanges: vec![x.clone()],
        first_ms: 1_000,
        last_ms: 1_000,
        his: vec![],
        counted: 0,
        children: vec![],
        title: "the harbour".into(),
        account: format!("ACCOUNT: {SAID}"),
        marks: crate::view::Marks {
            place: Some(PLACE.into()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        model: None,
        writer: 1,
        at_ms: 1_000,
    };
    theseus_store::blocking(|| fold::write_once(&c.store, node("fold-new"))).unwrap();
    // An older key: the view contract's, with no range digest after it.
    let old_key = format!("{}{PLACE}\u{1}1\u{1}{:020}", fold::FOLD_PREFIX, 1_000u64);
    let old = node("fold-old");
    c.store
        .append(&[
            theseus_store::NewRecord::json(theseus_store::kinds::META, Some(&old_key), &old).unwrap(),
            theseus_store::NewRecord::json(
                theseus_store::kinds::META,
                Some(&format!("{HANDLE_PREFIX}fold-old")),
                &old_key,
            )
            .unwrap(),
        ])
        .unwrap();
    for handle in ["fold-new", "fold-old"] {
        let builder = session(c);
        let turn = own_turn(c, &builder);
        let p = crate::tests_view_record::fold_piece(PLACE, 1, (&x, &x), handle);
        crate::tests_view_record::keep(c, &turn, &format!("cmp_{handle}"), PLACE, vec![p]);
        let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
        assert_eq!(refuses(&h), Some(Source::View), "{handle}");
    }
    let builder = session(c);
    let turn = own_turn(c, &builder);
    let p = crate::tests_view_record::fold_piece(PLACE, 1, (&x, &x), "fold-unknown");
    crate::tests_view_record::keep(c, &turn, "cmp_unknown", PLACE, vec![p]);
    let got = Heard::of(&c.store, ITEM, &[&builder], far()).map(|_| ());
    assert_eq!(got, Err(Unread::Unresolved(Why::Fold)));
}

/// A task a contributing session started, and an earlier start of the item that only a row of its builds names,
/// are among the sessions read.
#[tokio::test]
async fn a_childs_and_an_earlier_starts_reads_are_counted() {
    use theseus_protocol::tasks::{TaskOrigin, TaskRecord};
    let r = rig();
    let c = &r.core;
    let builder = session(c);
    let inline = format!("An opened exchange:\n> {SAID}");
    // A helper the builder started, which looked back.
    let child = session(c);
    look(c, &child, None, &inline, vec![]);
    let t = TaskRecord {
        id: crate::task_graph::of_session(&child),
        session: Some(child.clone()),
        origin: TaskOrigin {
            session: builder.clone(),
            principal: "self".into(),
            ..Default::default()
        },
        ..TaskRecord::default()
    };
    c.store.append(&[crate::task_graph::record(&t).unwrap()]).unwrap();
    let h = Heard::of(&c.store, ITEM, &[&builder], far()).unwrap();
    assert_eq!(refuses(&h), Some(Source::History), "the child");
    // An earlier start, named by its build's row alone.
    let earlier = session(c);
    look(c, &earlier, None, &inline, vec![]);
    let row = crate::fact::row(
        &crate::fact::rsi_pr::SelfBranchBuilt {
            item: ITEM,
            issue: "theseus-kelp1",
            branch: "self/20261124-kelp-nets",
            head: Some("abc"),
            gate: "red",
            report: None,
            cost_usd: 0.0,
            stopped_at: None,
            why: "mend",
            numbers: json!({}),
        },
        Some(&earlier),
        None,
    )
    .unwrap();
    c.store.append(&[row]).unwrap();
    let later = session(c);
    let h = Heard::of(&c.store, ITEM, &[&later], far()).unwrap();
    assert_eq!(refuses(&h), Some(Source::History), "the earlier start");
}

/// What one read costs is bounded by what the item's sessions hold (several starts, an old fork and a long
/// session), and a read past its deadline is put off, not refused.
#[tokio::test]
async fn the_read_is_bounded_and_put_off_at_its_deadline() {
    use theseus_protocol::tasks::{TaskEvidence, TaskRecord};
    let r = rig();
    let c = &r.core;
    let starts: Vec<String> = (0..4).map(|_| session(c)).collect();
    // A long session: four hundred turns of its own.
    let long = &starts[0];
    for i in 0..400 {
        let turn = crate::new_id("trn");
        let n = Node::user(long, Some(&turn), "owner", &format!("line {i}"));
        c.store.append(&[n.record().unwrap()]).unwrap();
    }
    // A fork made before forks were given a role: its origin alone names its source.
    let fork = session(c);
    let mut rec: crate::session::SessionRecord = c.store.get_session(&fork).unwrap().unwrap();
    rec.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: starts[1].clone(),
        node: "msg_point".into(),
        position: 0,
        compilation: None,
        at_ms: 0,
    }));
    c.store.put_session(&fork, &rec).unwrap();
    // The fork is named; the start it was made from is found through it.
    let evidence: Vec<TaskEvidence> = (starts.iter().filter(|s| *s != &starts[1]).chain([&fork]))
        .map(|s| TaskEvidence {
            identity: format!("session:{s}"),
            ..Default::default()
        })
        .collect();
    let t = TaskRecord {
        id: ITEM.into(),
        evidence,
        ..TaskRecord::default()
    };
    c.store.append(&[crate::task_graph::record(&t).unwrap()]).unwrap();
    let before = theseus_store::records_read_here();
    let at = Instant::now();
    Heard::of(&c.store, ITEM, &[], far()).unwrap();
    let (reads, took) = (theseus_store::records_read_here() - before, at.elapsed());
    eprintln!("outgoing read cost: {reads} records, {took:?}, for 5 sessions and 404 nodes");
    assert!(reads < 2_000, "{reads} records read");
    let got = Heard::of(&c.store, ITEM, &[], Instant::now()).map(|_| ());
    assert_eq!(got, Err(Unread::Defer(Deferral::Deadline)));
}

/// The ledger's index still being built puts the read off: it is never an empty read, nor a hold.
#[tokio::test]
async fn the_ledger_index_not_ready_puts_the_read_off() {
    let r = rig();
    let c = &r.core;
    let builder = session(c);
    c.store.shape_window(true);
    let got = Heard::of(&c.store, ITEM, &[&builder], far()).map(|_| ());
    c.store.shape_window(false);
    assert_eq!(got, Err(Unread::Defer(Deferral::NotReady)));
}
