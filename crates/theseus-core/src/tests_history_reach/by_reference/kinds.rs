//! Every kind of page the history tool reads across every place ties its result to the messages its lines
//! show, as a page of one place does: by a span of days, by a label, and by words. Once a message such a page
//! showed is marked wrong, the reader is told once and its window lays the result with the line saying what
//! was said since; once the message is erased, the result is laid with its quote of the message as one line,
//! none of its words (a result stored with its words, before results were kept by reference: the whole result
//! as one line). The stored result is never written again.

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use super::*;
use crate::copies::since::ERASED_RESULT;
use crate::quotes::ERASED_QUOTE;
use crate::recall::{Ask, AskFuture};

/// The words a page by words searches for: in the line said, and not the word the tests look for.
const SEARCH: &str = "heron";

/// What a page reads across every place.
#[derive(Clone, Copy, Debug)]
enum Page {
    /// A span of days.
    Date,
    /// A content label's timeline.
    Label,
    /// Words, through the words index.
    Words,
}

impl Page {
    fn args(self) -> Value {
        match self {
            Page::Date => json!({ "when": "past 3 days" }),
            Page::Label => json!({ "label": "personal" }),
            Page::Words => json!({ "words": SEARCH }),
        }
    }
}

/// A stand-in words index that answers a search of [`SEARCH`] with `node` and any other query (a turn's recall)
/// with nothing: the reader reads the line only by the history tool.
fn index_of(core: &Core, node: String) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        let asked = match p.text.trim() == SEARCH {
            true => vec![node.clone()],
            false => Vec::new(),
        };
        let hits = (asked.iter().enumerate())
            .map(|(i, id)| {
                let (position, n) = store.get_node(id).unwrap().expect("the said node");
                IndexHit {
                    text: crate::recall::text_of(&n),
                    time_ms: n.created_at_ms,
                    node_id: n.id,
                    chunk: 0,
                    session_id: n.session_id,
                    position,
                    kind: "imported".into(),
                    origin: "import".into(),
                    author: None,
                    place: None,
                    tool: None,
                    external: false,
                    entities_matched: vec![],
                    entities: Vec::new(),
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: i + 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: 1.0 / (61 + i) as f64,
                }
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// The line said in one conversation and read from `channel:lab` by a page of `page`: the rig, what was said,
/// the reader, and its result's node and stored text. The page is asserted to be one, and to show the line by
/// a whole line ending with its exchange's handle.
struct Read {
    r: Rig,
    said: Said,
    reader: String,
    copy: String,
    text: String,
}

async fn read_by(page: Page) -> Read {
    let r = rig();
    let said = say(&r.core).await;
    match page {
        Page::Date => {}
        Page::Label => {
            let raised = [(said.node.as_str(), vec![crate::content::Flag::Personal])];
            let recs = crate::item_labels::touches_of(&r.core.store, raised);
            assert!(!recs.is_empty(), "the label touches its exchange");
            r.core.store.append(&recs).unwrap();
        }
        Page::Words => (r.core.runner.memory).set_ask(index_of(&r.core, said.node.clone())),
    }
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, text) = read_in(&r.core, &reader, page.args()).await;
    assert!(text.starts_with("context.history: "), "{page:?}: {text}");
    let handle = format!(" \u{b7} {}", said.turn);
    assert!(
        (text.lines()).any(|l| l.starts_with("- ") && l.ends_with(&handle) && l.contains(WORD)),
        "{page:?}: the page shows the line by its handle: {text}"
    );
    Read {
        r,
        said,
        reader,
        copy,
        text,
    }
}

/// The stored result `copy` renders as `text`, as it was written, through the one renderer (its quotes read at
/// their positions), and its `content` holds none of the message's words: a look back keeps references to what
/// it showed, not a second copy of its words (issue 49).
fn stored_as(core: &Core, copy: &str, text: &str) {
    let (_, n) = core.store.get_node(copy).unwrap().expect("the result");
    let Body::ToolResult { content, parts, .. } = &n.body else {
        panic!("not a result: {n:?}")
    };
    let written =
        crate::quotes::render_with(parts, |q| crate::quotes::read_as_written(&core.store, q));
    assert_eq!(written, text, "the stored result renders as written: {n:?}");
    for word in SAID.split(' ').filter(|w| w.len() > 3) {
        assert!(
            !content.contains(word),
            "the stored result holds {word:?}: {n:?}"
        );
    }
}

/// The stored result `copy` holds `text`, as it was written: the statement for a result stored with its words.
fn stored_as_words(core: &Core, copy: &str, text: &str) {
    let (_, n) = core.store.get_node(copy).unwrap().expect("the result");
    assert!(
        matches!(&n.body, Body::ToolResult { content, .. } if content == text),
        "the stored result is as written: {n:?}"
    );
}

/// The line said is marked wrong: the result's own row is the reader's session, the reader is told once, and
/// its next request lays the result with its words and the line saying what was said since.
async fn marked_wrong(page: Page) {
    let Read {
        r,
        said,
        reader,
        copy,
        text,
    } = read_by(page).await;
    let out = retract(&r.core, &said.node);
    let e = (out.exposures.iter())
        .find(|e| e.session_id == reader && e.node_id == copy)
        .unwrap_or_else(|| panic!("{page:?}: the retraction does not list {copy}: {out:#?}"));
    assert_eq!(e.kind, kind::SESSION, "{page:?}: {e:?}");
    turn(&r.core, &reader, "and the weir?").await;
    assert_eq!(notes_in(&r.core, &reader), 1, "{page:?}");
    let (_, laid) = laid_result(&r);
    let laid = laid.to_string();
    assert!(laid.contains(WORD), "{page:?}: {laid}");
    assert!(laid.contains("[Since this was read: "), "{page:?}: {laid}");
    assert!(laid.contains(WHY), "{page:?}: {laid}");
    stored_as(&r.core, &copy, &text);
}

/// The line said is erased: the reader's next request lays the result's quote of it as one line, none of its
/// words.
async fn erased(page: Page) {
    let Read {
        r,
        said,
        reader,
        copy,
        text,
    } = read_by(page).await;
    let (_, n) = r.core.store.get_node(&said.node).unwrap().unwrap();
    let gone = n.erased(n.created_at_ms + 1, "a test");
    r.core.store.append(&[gone.record().unwrap()]).unwrap();
    turn(&r.core, &reader, "and the weir?").await;
    let (_, laid) = laid_result(&r);
    let laid = laid.to_string();
    assert!(laid.contains(ERASED_QUOTE), "{page:?}: {laid}");
    assert!(!laid.contains(WORD), "{page:?}: {laid}");
    stored_as(&r.core, &copy, &text);
}

#[tokio::test]
async fn a_page_by_date_lays_the_line_said_since_once_a_message_it_showed_is_marked_wrong() {
    marked_wrong(Page::Date).await;
}

#[tokio::test]
async fn a_page_by_date_lays_no_words_once_a_message_it_showed_is_erased() {
    erased(Page::Date).await;
}

#[tokio::test]
async fn a_page_by_label_lays_the_line_said_since_once_a_message_it_showed_is_marked_wrong() {
    marked_wrong(Page::Label).await;
}

#[tokio::test]
async fn a_page_by_label_lays_no_words_once_a_message_it_showed_is_erased() {
    erased(Page::Label).await;
}

#[tokio::test]
async fn a_page_by_words_lays_the_line_said_since_once_a_message_it_showed_is_marked_wrong() {
    marked_wrong(Page::Words).await;
}

#[tokio::test]
async fn a_page_by_words_lays_no_words_once_a_message_it_showed_is_erased() {
    erased(Page::Words).await;
}

/// A result stored with its words, as a build before results were kept by reference wrote it (an old record,
/// written here by hand), is never written again once a message it showed is erased: it still holds its words as
/// written, and is laid as one line with none of them.
#[tokio::test]
async fn a_page_stored_as_words_is_as_written_once_a_message_it_showed_is_erased() {
    let Read {
        r,
        said,
        reader,
        copy,
        text,
    } = read_by(Page::Date).await;
    old_shape(&r.core, &copy, &text);
    let (_, n) = r.core.store.get_node(&said.node).unwrap().unwrap();
    let gone = n.erased(n.created_at_ms + 1, "a test");
    r.core.store.append(&[gone.record().unwrap()]).unwrap();
    turn(&r.core, &reader, "and the weir?").await;
    let (_, laid) = laid_result(&r);
    let laid = laid.to_string();
    assert!(laid.contains(ERASED_RESULT), "{laid}");
    assert!(!laid.contains(WORD), "{laid}");
    stored_as_words(&r.core, &copy, &text);
}
