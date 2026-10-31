//! The quick route knows the owner's words (theseus-z706m): "Can you explain
//! how RSI works?" went to Haiku in the v1 voice check and was answered as the
//! trading indicator, because a quick detour then carried no recall; the
//! owner's RSI is recursive self-improvement, which only a record says.
//! theseus-pa1xx made a quick detour carry the turn's recall. These tests hold
//! that against the case the issue feared: the term is a short all-caps token
//! the stand-in index matches by word (as BM25 does: letters and digits,
//! lowercased), so a recall query that dropped the acronym, or a quick route
//! that threw the note away, finds nothing and the request shows it.
//! The rig is `tests_route`'s.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_judge::fake::FakeJev;

use crate::recall::{Ask, AskFuture};
use crate::tests_route::{glm_placements, mode, rig, turn, Rig};

/// The record that defines the owner's term, in a session of its own.
const RSI_NOTE: &str = "Remember: RSI is recursive self-improvement, the owner's name for \
                        Theseus's own loop that reviews its work and improves itself.";

/// A distractor the index also holds, which no message below names.
const OTHER_NOTE: &str = "Remember: the keel guard refuses a raised budget.";

/// Words too common to match on, as an index's weights give them none.
const COMMON: [&str; 14] = [
    "the", "a", "is", "can", "you", "how", "does", "what", "s", "of", "and", "it", "explain",
    "work",
];

/// A text's index words: split on anything but letters and digits,
/// lowercased, without the common ones (the engine's default tokenizer).
fn words(text: &str) -> BTreeSet<String> {
    let lower = text.to_lowercase();
    let split = lower.split(|c: char| !c.is_alphanumeric());
    let all = split.filter(|w| !w.is_empty() && !COMMON.contains(w));
    all.map(str::to_string).collect()
}

/// `inner`, answering only the hits that share a word with the query, and
/// noting each query's text.
fn by_words(inner: Ask, asked: Arc<Mutex<Vec<String>>>) -> Ask {
    Arc::new(move |p| -> AskFuture {
        asked.lock().unwrap().push(p.text.clone());
        let query = words(&p.text);
        let answer = inner(p);
        Box::pin(async move {
            let mut r = answer.await?;
            r.hits.retain(|h| !words(&h.text).is_disjoint(&query));
            Ok(r)
        })
    })
}

/// A rig whose recall is live, the stand-in index holding the two notes, and
/// the queries it is asked.
fn rig_with_records(jev: &FakeJev) -> (Rig, Arc<Mutex<Vec<String>>>) {
    let r = rig(Some(jev), 1, |c| {
        c.memory.mode = crate::config::MemoryMode::Live;
        c.routing.max_wait_ms = 5_000;
        glm_placements(c);
    });
    let notes = crate::tests_recall::session(&r.core, None, &[RSI_NOTE, OTHER_NOTE]);
    let index = crate::tests_recall::index_of(&r.core, vec![notes]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    r.core.runner.memory.set_ask(by_words(index, asked.clone()));
    (r, asked)
}

/// Whether a request message carries the definition.
fn defines_rsi(m: &Value) -> bool {
    m.to_string().contains("recursive self-improvement")
}

/// The v1 voice check's message, on the quick route: the Haiku request holds
/// the owner's definition of RSI after the message, and not the distractor;
/// the recall query kept the acronym; the row says `ran`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_quick_route_gets_the_owners_definition_of_rsi() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.95);
    let (r, asked) = rig_with_records(&jev);
    let ask = "Can you explain how RSI works?";
    let res = turn(&r.core, None, ask, None).await;
    let route = res.route.as_ref().unwrap();
    assert_eq!(
        (
            res.profile.as_str(),
            route.reason.as_str(),
            route.mode.as_deref()
        ),
        ("haiku", "verdict", Some("haiku"))
    );
    let q = &r.claude.requests()[0];
    assert_eq!(q.model, "claude-haiku-5-5");
    let (last, before) = q.messages.split_last().unwrap();
    assert!(
        defines_rsi(last),
        "the definition, after the message: {last}"
    );
    assert!(!before.iter().any(defines_rsi), "{before:?}");
    assert!(
        !q.messages
            .iter()
            .any(|m| m.to_string().contains("keel guard")),
        "the distractor: {:?}",
        q.messages
    );
    let queries = asked.lock().unwrap().clone();
    assert!(
        queries.iter().any(|t| t.contains("RSI")),
        "the recall query kept the acronym: {queries:?}"
    );
    assert_eq!(res.recalled, 1, "the footer counts what the model saw");
    let rows = crate::tests_recall::recalls(&r.core, &res.session_id);
    assert_eq!(rows.len(), 1, "one recall row");
    assert_eq!(rows[0].outcome, "ran");
}

/// The shapes the term comes in: a bare acronym, lowercase, a short question,
/// one inside a longer sentence. Each is a quick detour whose request carries
/// the definition; a query that dropped short or all-caps tokens finds no
/// note for any of them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_or_all_caps_term_still_reaches_the_quick_route() {
    for ask in [
        "RSI?",
        "rsi?",
        "What's RSI?",
        "Does RSI run overnight, and when did it last change anything?",
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "quick", 0.95);
        let (r, _) = rig_with_records(&jev);
        let res = turn(&r.core, None, ask, None).await;
        let route = res.route.as_ref().unwrap();
        assert_eq!(
            (route.reason.as_str(), route.mode.as_deref()),
            ("verdict", Some("haiku")),
            "{ask}"
        );
        let q = &r.claude.requests()[0];
        let (last, _) = q.messages.split_last().unwrap();
        assert!(defines_rsi(last), "{ask}: {:?}", q.messages);
        assert_eq!(res.recalled, 1, "{ask}");
    }
}

