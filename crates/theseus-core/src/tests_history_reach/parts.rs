//! A look back kept by reference (issue 49): each form of `context.history`'s read, through the runtime, stores
//! its parts and no words of the messages it shows; what it shows is the words as they were written, and after
//! an erase its quotes of the erased messages show one fixed line, the rest as it was.

use theseus_store::Store as _;

use super::*;
use crate::quotes::{Part, ERASED_QUOTE};

/// The stored record of result `id`: its bytes as the WAL holds them, its content and its parts.
fn stored(core: &Core, id: &str) -> (String, String, Vec<Part>) {
    let (pos, n) = core.store.get_node(id).unwrap().expect("the result");
    let rec = core.store.inner().get(pos).unwrap().expect("its record");
    let Body::ToolResult { content, parts, .. } = n.body else {
        panic!("not a result: {n:?}");
    };
    (
        String::from_utf8_lossy(&rec.payload).into_owned(),
        content,
        parts,
    )
}

/// A page, an open of an exchange and an open of a session, each through the tool: the record holds no word of
/// the message, its content is empty, and its parts quote the message and show its words.
#[tokio::test]
async fn a_page_stores_no_words() {
    let r = rig();
    let said = say(&r.core).await;
    let other = session(&r.core, Some("channel:lab"));
    for args in [
        json!({ "place": said.place }),
        json!({ "open": said.turn }),
        json!({ "open": said.session }),
    ] {
        let (copy, text) = read_in(&r.core, &other, args.clone()).await;
        assert!(text.contains(SAID), "{args}: {text}");
        let (bytes, content, parts) = stored(&r.core, &copy);
        assert!(
            !bytes.contains(WORD),
            "{args}: the record holds the words: {bytes}"
        );
        assert_eq!(content, "", "{args}");
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, Part::Quote(q) if q.node == said.node)),
            "{args}: {parts:#?}"
        );
    }
}

/// An imported conversation's session, opened and then erased: its result shows the erased words' fixed line
/// in their place, and the rest of what it showed as it was.
#[tokio::test]
async fn an_opened_import_shows_its_erased_words_as_one_line() {
    use crate::tests_imports::{episode, import};
    let r = rig();
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let other = session(&r.core, None);
    let (copy, before) = read_in(&r.core, &other, json!({ "open": ids[0] })).await;
    assert!(before.contains(WORD), "{before}");
    crate::import::write::erase(&r.core.store, "kelp-2026", Some("a test"), "cli").unwrap();
    let (_, _, parts) = stored(&r.core, &copy);
    let after = crate::tests_places::kept_text(&r.core, String::new(), &parts);
    assert!(!after.contains(WORD), "{after}");
    assert!(after.contains(ERASED_QUOTE), "{after}");
    let heading = before.lines().next().unwrap();
    assert!(after.starts_with(heading), "the code's words stay: {after}");
}
