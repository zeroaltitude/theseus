//! The import search reads the catalog of imported episodes. When the catalog says that some of the import's
//! tag records could not be read, the search answers with a fixed refusal that counts them: never a hit, and
//! never that nothing was found. The catalog's own reader, `import.sessions`, still answers with what it read.

use serde_json::json;
use theseus_protocol::import::ImportSessionsParams;

use super::xs2_ask;
use crate::import::tests_unread::{unread_tag, TAG_WORDS};
use crate::tests_imports::{episode, import, index_over};
use crate::tests_places::{rig_with, session};

#[tokio::test]
async fn an_import_search_past_a_tag_record_that_does_not_read_is_refused_with_its_count() {
    let r = rig_with(|c| c.memory.mode = crate::config::MemoryMode::Live);
    let c = &r.core;
    let ids = import(
        c,
        &[episode(
            71,
            "public",
            "2026-03-03T18:00:00Z",
            "the HERRING-GULL count",
        )],
    );
    c.runner.memory.set_ask(index_over(c, ids.clone()));
    let reader = session(c, None);
    let ask = |place: &str| xs2_ask(c, &reader, json!({"place": place, "words": "HERRING-GULL"}));
    // Every tag record reads: the search finds the words, over every book and in the episode's own.
    for place in ["import", "import:casebook"] {
        let found = ask(place).await.unwrap();
        assert!(
            found.contains("HERRING-GULL") && found.contains(&ids[0]),
            "{place}: {found}"
        );
    }
    let key = unread_tag(c);
    // One tag record does not read: every search that reads the catalog is refused, with the count.
    for place in ["import", "import:casebook", "import:no-such-book"] {
        let got = ask(place).await;
        let said = got.clone().unwrap_or_else(|e| e);
        assert!(got.is_err(), "{place}: {said}");
        assert!(
            said.contains(
                "1 of the import's tag records could not be read, so the imported conversations are not \
                 known in full: nothing was searched, and none is said to be absent"
            ),
            "{place}: {said}"
        );
        for quoted in [TAG_WORDS, "invalid type", "No imported conversation", "HERRING-GULL"] {
            assert!(!said.contains(quoted), "{place}: {said}");
        }
        assert!(!said.contains(&key), "{place}: {said}");
    }
    // The catalog's own reader answers with what it read, and counts the record it passed over.
    let page = c
        .import_sessions(&ImportSessionsParams::default(), true)
        .unwrap();
    assert_eq!(page.tags_unread, Some(1), "{page:?}");
    assert_eq!(page.episodes.len(), 1, "{page:?}");
}
