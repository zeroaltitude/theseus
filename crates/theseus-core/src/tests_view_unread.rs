//! A view record this build cannot read (issue 44): a later build may write names of bands and reasons this one
//! does not know, and an install rolled back to this build still meets those records. Every reader passes over
//! such a record: the history tool still answers a bare page, a page by date and an open (its bare page begins
//! where that view was made, and hides nothing the view's window would have paged), the owner's `view show`
//! says which view it cannot read and lists the turn's others, and the model's look at its view answers.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::method;
use theseus_protocol::view::ViewShowResult;
use theseus_store::{kinds, NewRecord};

use crate::approval::Surface;
use crate::history_tool::page::{self, Lens};
use crate::node::Body;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_places::{answers_a_call, last_user, rig_scripted, session, turn, Rig};
use crate::view::history::HistoryArgs;
use crate::view::record;
use crate::Core;

/// What the warning says, once a call, when the history tool meets a view record it cannot read.
const WARNED: &str = "a bare history call begins where that view was made";

/// What the warning says when a listing of a turn's views leaves out one it cannot read.
const LEFT_OUT: &str = "a view record cannot be read; it is left out";

/// The names a later build wrote into the record: no log line quotes them.
const PLANTED: [&str; 2] = ["a_band_of_a_later_build", "a_reason_of_a_later_build"];

/// A model whose message `CALL {…}` calls `context_history` with those arguments.
fn rig() -> Rig {
    rig_scripted(
        |_| {},
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Read.");
            }
            match last_user(req).split_once("CALL ") {
                Some((_, a)) => Scripted::tools(
                    "Reading.",
                    &[(
                        "h1",
                        "context_history",
                        serde_json::from_str(a.trim()).unwrap(),
                    )],
                ),
                None => Scripted::text("Noted."),
            }
        },
    )
}

/// `sid`'s turn calls the tool with `args`: the call's stored result.
async fn read_in(core: &Arc<Core>, sid: &str, args: Value) -> String {
    turn(core, sid, &format!("CALL {args}")).await;
    (core.store.session_nodes(sid).unwrap().into_iter())
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool_use_id,
                content,
                ..
            } if tool_use_id == "h1" => Some(content),
            _ => None,
        })
        .expect("the call's result")
}

/// The view record of `sid`'s last compilation stored again as a later build would write it: its first piece
/// in a band, and for a reason, this build has no name for. Its compilation.
fn written_by_a_later_build(core: &Core, sid: &str) -> String {
    let rec = (core.store.get_session::<SessionRecord>(sid).unwrap()).expect("the session");
    let cid = rec.compilation_id.expect("the session's last compilation");
    let key = format!("{}{cid}", record::VIEW_PREFIX);
    let mut v: Value = core.store.get_meta(&key).unwrap().expect("its view record");
    let first = (v["shown"].as_array_mut())
        .and_then(|s| s.first_mut())
        .expect("a piece shown");
    first["piece"]["band"] = json!("a_band_of_a_later_build");
    first["piece"]["reasons"] = json!(["a_reason_of_a_later_build"]);
    let again = NewRecord::json(kinds::META, Some(&key), &v).unwrap();
    core.store.append(&[again]).unwrap();
    assert!(
        record::read(&core.store, &cid).is_err(),
        "this build cannot read it"
    );
    cid
}

/// The turn of `sid`'s exchange whose person said `words`.
fn turn_of(core: &Core, sid: &str, words: &str) -> String {
    (core.store.session_nodes(sid).unwrap().into_iter())
        .find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } if text.contains(words) => n.turn_id.clone(),
            _ => None,
        })
        .expect("the person's message")
}

/// The turns of a bare page of place `here`, read from `session`'s last compilation.
fn bare_page(core: &Core, session: &str, here: &str) -> Vec<String> {
    let lens = Lens {
        store: &core.store,
        here: here.into(),
        in_view: page::in_view(&core.store, session, here).unwrap(),
        words: None,
        span: None,
        cut: Default::default(),
        scope: None,
        cuts: crate::scrub::Cuts::none(),
    };
    let walked = lens.walk(&HistoryArgs::default(), None, 0).unwrap();
    walked.page.lines.into_iter().map(|l| l.handle.0).collect()
}

/// The log's text, kept for a test.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

impl Log {
    /// The lines that hold `needle`.
    fn lines(&self, needle: &str) -> Vec<String> {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        (text.lines().filter(|l| l.contains(needle)))
            .map(String::from)
            .collect()
    }

    /// No line quotes a name the later build wrote into the record.
    fn quotes_nothing_planted(&self) {
        for name in PLANTED {
            let quoted = self.lines(name);
            assert!(quoted.is_empty(), "the log quotes {name}: {quoted:?}");
        }
    }
}

/// The session's last view record holds a band and a reason this build does not know: the history tool still
/// answers a bare page, a page by date and an open, through the model's own call, and says once a call that it
/// could not read that view.
#[tokio::test]
async fn the_history_tool_answers_when_the_last_view_record_cannot_be_read() {
    let r = rig();
    let c = &r.core;
    let sid = session(c, None);
    turn(c, &sid, "the zircon heron nests by the north weir").await;
    let first = turn_of(c, &sid, "zircon heron");
    let cid = written_by_a_later_build(c, &sid);
    let log = Log::default();
    let _g = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .finish(),
    );
    let bare = read_in(c, &sid, json!({})).await;
    assert!(bare.contains("zircon heron"), "{bare}");
    assert!(!bare.contains("a_band_of_a_later_build"), "{bare}");
    let warned = log.lines(WARNED);
    assert_eq!(warned.len(), 1, "one warning for the call: {warned:?}");
    assert!(
        warned[0].contains(&cid),
        "it names the compilation: {warned:?}"
    );
    log.quotes_nothing_planted();
    // The pointer moved to the call's own turn, whose record this build wrote: a later build's is laid again.
    let cid = written_by_a_later_build(c, &sid);
    let dated = read_in(c, &sid, json!({ "from": "2020-01-01" })).await;
    assert!(dated.contains("zircon heron"), "{dated}");
    let warned = log.lines(WARNED);
    assert_eq!(warned.len(), 2, "one warning a call: {warned:?}");
    assert!(warned[1].contains(&cid), "{warned:?}");
    written_by_a_later_build(c, &sid);
    let opened = read_in(c, &sid, json!({ "open": first })).await;
    assert!(opened.contains("zircon heron"), "{opened}");
    assert!(opened.contains("Noted."), "{opened}");
    assert_eq!(log.lines(WARNED).len(), 3, "one warning a call");
    log.quotes_nothing_planted();
}

/// A bare page read past a view record this build cannot read begins where that view was made: every exchange
/// of the place begun before it is on the page, so nothing the record's own window would have paged is hidden
/// (some the model saw in full may be paged again).
#[tokio::test]
async fn a_bare_page_past_an_unreadable_view_hides_nothing_its_window_would_have_paged() {
    let r = rig();
    let c = &r.core;
    let sid = session(c, None);
    let said = ["the tide", "the moon", "the weir", "the heron"];
    for s in said {
        turn(c, &sid, &format!("note: {s}")).await;
    }
    let turns: Vec<String> = said.iter().map(|s| turn_of(c, &sid, s)).collect();
    let here = (crate::exchange::exchange(&c.store, &turns[0]).unwrap())
        .expect("its exchange")
        .place;
    let read = bare_page(c, &sid, &here);
    written_by_a_later_build(c, &sid);
    let unread = bare_page(c, &sid, &here);
    assert!(
        read.iter().all(|t| unread.contains(t)),
        "{read:?} {unread:?}"
    );
    assert!(turns.iter().all(|t| unread.contains(t)), "{unread:?}");
}

/// A window exchange begun in the very millisecond its view was made, laid thinned, with nothing cleared: the
/// readable record's bare page holds it, and so does the bare page past that record once this build cannot
/// read it. (A page begun just before the view was made would hide it, and every page after.)
#[tokio::test]
async fn a_bare_page_past_an_unreadable_view_holds_an_exchange_of_the_views_own_millisecond() {
    use crate::view::{Form, Pos, Slot, SourceRange, Space};
    let r = rig();
    let c = &r.core;
    let sid = session(c, None);
    // Two turns: the second's view lays the first in its window.
    turn(c, &sid, "note: the tide").await;
    turn(c, &sid, "note: the moon").await;
    let tide = turn_of(c, &sid, "the tide");
    let here = (crate::exchange::exchange(&c.store, &tide).unwrap())
        .expect("its exchange")
        .place;
    let rec = (c.store.get_session::<SessionRecord>(&sid).unwrap()).expect("the session");
    let cid = rec.compilation_id.expect("the session's last compilation");
    let comp = (c.store.get_compilation(&cid).unwrap()).expect("its compilation");
    assert!(
        (comp.budget.iter())
            .flat_map(|b| &b.dropped)
            .all(|d| d.tier == "recall"),
        "the compiler cleared nothing: {:?}",
        comp.budget
    );
    let made = comp.created_at_ms;
    let lamp = "turn_at_the_views_millisecond".to_string();
    crate::tests_rebase_view::put_exchange(&c.store, &sid, &here, &lamp, made, "note: the lamp");
    let mut view = (record::read(&c.store, &cid).unwrap()).expect("its view record");
    let is_window = |s: &crate::view::budget::Shown| {
        s.piece.slot == Slot::Here && matches!(s.piece.range.space, Space::Exchanges { .. })
    };
    let mut laid = (view.shown.iter().find(|s| is_window(s)))
        .cloned()
        .expect("a window piece");
    let at = Pos {
        at_ms: made,
        id: lamp.clone(),
    };
    laid.piece.range = SourceRange {
        space: Space::Exchanges {
            place: here.clone(),
        },
        first: at.clone(),
        last: at,
    };
    laid.form = Form::Spoken;
    view.shown.retain(|s| !is_window(s));
    view.shown.push(laid);
    c.store.put_meta(&view.key(), &view).unwrap();
    let read = bare_page(c, &sid, &here);
    assert!(read.contains(&lamp), "{read:?}");
    written_by_a_later_build(c, &sid);
    let unread = bare_page(c, &sid, &here);
    assert!(unread.contains(&lamp), "{unread:?}");
    assert!(
        read.iter().all(|t| unread.contains(t)),
        "{read:?} {unread:?}"
    );
}

/// The owner's `view show` of a turn whose view this build cannot read says so, by its compilation; another
/// turn's view is still listed.
#[tokio::test]
async fn view_show_says_which_view_this_build_cannot_read() {
    let r = rig();
    let c = &r.core;
    let sid = session(c, None);
    turn(c, &sid, "the zircon heron nests by the north weir").await;
    let first = turn_of(c, &sid, "zircon heron");
    let cid = written_by_a_later_build(c, &sid);
    turn(c, &sid, "and the plum orchard is due a visit").await;
    let second = turn_of(c, &sid, "plum orchard");
    let shown = |t: &str| {
        let core = c.clone();
        let t = t.to_string();
        async move {
            let v = rpc_as(
                &core,
                surface("sock#1", Surface::Cli),
                method::VIEW_SHOW,
                json!({ "turn_id": t }),
            )
            .await
            .unwrap();
            serde_json::from_value::<ViewShowResult>(v).unwrap()
        }
    };
    let log = Log::default();
    let g = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_ansi(false)
            .finish(),
    );
    let unread = shown(&first).await;
    drop(g);
    let left_out = log.lines(LEFT_OUT);
    assert_eq!(left_out.len(), 1, "one warning: {left_out:?}");
    assert!(left_out[0].contains(&cid), "names it: {left_out:?}");
    log.quotes_nothing_planted();
    assert!(
        !unread.views.iter().any(|v| v.compilation == cid),
        "{unread:?}"
    );
    assert_eq!(unread.unread, vec![cid], "{unread:?}");
    let read = shown(&second).await;
    assert_eq!(read.views.len(), 1, "{read:?}");
    assert!(read.unread.is_empty(), "{read:?}");
}

/// The model's look at a turn's view answers when this build cannot read that view: it is passed over, as
/// every listing of views passes over it, and the look says what it says of a turn with no view.
#[tokio::test]
async fn the_look_at_a_view_this_build_cannot_read_answers() {
    let r = rig();
    let c = &r.core;
    let sid = session(c, None);
    turn(c, &sid, "the zircon heron nests by the north weir").await;
    let first = turn_of(c, &sid, "zircon heron");
    let before = crate::look::view::words(c, &sid, &first);
    assert!(!before.contains("no view record"), "{before}");
    written_by_a_later_build(c, &sid);
    let after = crate::look::view::words(c, &sid, &first);
    assert!(!after.contains("cannot be read"), "{after}");
    assert!(after.contains("no view record"), "{after}");
}
