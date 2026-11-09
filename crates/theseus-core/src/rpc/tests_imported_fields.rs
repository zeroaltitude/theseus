//! The third fail-closed bar of `docs/design/imported-surface.md`: a new
//! text-bearing field on `SessionInfo` or `NodeInfo` must not default to
//! shown. Each type's own definition is walked, and every `pub` field must
//! be on exactly one of two lists, `WITHHELD` (the helper clears it) or
//! `SHOWN` (with the reason it carries none of an imported session's words).
//! A field added without a decision fails, by name. A second test ties the
//! lists to the helpers' behaviour, so a name on `WITHHELD` that the helper
//! does not clear, or a `SHOWN` one it does, fails too.

use serde_json::{json, Value};
use theseus_protocol::{NodeInfo, SessionInfo};

const SESSION_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../theseus-protocol/src/lib.rs"
));
const NODE_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../theseus-protocol/src/nodes.rs"
));

// The worlds the lists describe, counted at base b93c885f: `SessionInfo` has 31
// `pub` fields (5 withheld, 26 shown) and `NodeInfo` 12 (3 withheld, 9 shown).
// The `least` floors below are 28 and 10, a little under, so a census that
// walks almost nothing still fails.

/// `SessionInfo` fields `withhold_session_words` clears.
const SESSION_WITHHELD: &[&str] = &["title", "title_was", "first_text", "last_text", "prompts"];

/// Every other `SessionInfo` field, and why it holds none of the words.
const SESSION_SHOWN: &[(&str, &str)] = &[
    ("session_id", "an id"),
    ("kind", "a two-valued kind"),
    ("label", "the constant `imported` the import writes"),
    ("created_at_unix_ms", "a time"),
    ("turns", "a count"),
    ("usage", "token counts"),
    ("execution_id", "an id"),
    ("execution_state", "a state word"),
    ("last_active_ms", "a time"),
    ("cost_usd", "a sum"),
    ("tool_calls", "a count"),
    ("profile", "a profile's name, chosen by the owner"),
    ("model", "a model's name"),
    ("compilation_id", "an id"),
    ("pending_confirms", "a count"),
    ("parent_session_id", "an id"),
    ("limit_usd", "a sum"),
    (
        "attention",
        "a level, a place's label and a time (DOUBT: label)",
    ),
    ("state", "live, quiet or retired"),
    ("retired", "a reason word and a time"),
    ("superseded_by", "an id, a time and a place's name"),
    ("supersedes", "an id, a time and a place's name"),
    ("dir", "a path the session ran in, not its words (DOUBT)"),
    ("started_on", "a date"),
    ("data", "a data mark: open, private or sensitive"),
    ("labels", "label kinds and scores, not text (DOUBT)"),
];

/// `NodeInfo` fields `withheld_node` clears.
const NODE_WITHHELD: &[&str] = &["text", "thinking", "detail"];

/// Every other `NodeInfo` field, and why it holds none of the words.
const NODE_SHOWN: &[(&str, &str)] = &[
    ("node_id", "an id"),
    ("kind", "a kind word"),
    ("session_id", "an id"),
    ("position", "a count"),
    ("at_unix_ms", "a time"),
    ("turn_id", "an id"),
    ("loop_index", "a count"),
    (
        "author",
        "who spoke, not what was said (DOUBT: an import's agent)",
    ),
    ("bytes", "a size"),
];

/// The names of the `pub` fields of `pub struct <name> {` in `src`.
fn fields_of(src: &str, name: &str) -> Vec<String> {
    let head = format!("pub struct {name} {{\n");
    let start = src
        .find(&head)
        .unwrap_or_else(|| panic!("`{head}` is not in the source"))
        + head.len();
    let body = &src[start..];
    let end = body.find("\n}").expect("the struct has no end");
    body[..end]
        .lines()
        .filter_map(|l| l.strip_prefix("    pub "))
        .filter_map(|l| l.split_once(':').map(|(n, _)| n.trim().to_string()))
        .collect()
}

fn census(src: &str, name: &str, withheld: &[&str], shown: &[(&str, &str)], least: usize) -> usize {
    let spec = crate::complete::MustBeComplete {
        item: "a pub field of an imported session's wire type",
        list: "tests_imported_fields's WITHHELD and SHOWN lists",
        world: "the type's own definition in theseus-protocol",
        add: "a field was added to the type and nobody said whether it can \
              carry an imported session's words. If it can, clear it in \
              `withhold_session_words` / `withheld_node` \
              (rpc/methods.rs) and put it on WITHHELD; if not, put it on \
              SHOWN with the reason. docs/design/imported-surface.md says why",
        drop: "this field is gone from the type, or was renamed: delete its \
               row from the list that still names it",
        least,
        stale_fails: true,
    };
    let listed = withheld
        .iter()
        .map(|n| ((*n).to_string(), "withheld".to_string()))
        .chain(
            shown
                .iter()
                .map(|(n, why)| ((*n).to_string(), (*why).into())),
        );
    // A name on both lists is the list lying about itself.
    let mut seen = std::collections::BTreeSet::new();
    for n in withheld.iter().chain(shown.iter().map(|(n, _)| n)) {
        assert!(seen.insert(*n), "`{n}` is on both lists, or twice");
    }
    crate::complete::check(
        &spec,
        listed,
        fields_of(src, name)
            .into_iter()
            .map(|f| (f, format!("pub field of {name}"))),
    )
}

#[test]
fn every_session_info_field_is_withheld_or_said_to_be_safe() {
    let n = census(
        SESSION_SRC,
        "SessionInfo",
        SESSION_WITHHELD,
        SESSION_SHOWN,
        28,
    );
    assert!(n >= 28, "{n} fields walked");
}

#[test]
fn every_node_info_field_is_withheld_or_said_to_be_safe() {
    let n = census(NODE_SRC, "NodeInfo", NODE_WITHHELD, NODE_SHOWN, 10);
    assert!(n >= 10, "{n} fields walked");
}

/// A new `Option<String>` field fails the census by name, for good: the
/// census runs over a copy of the source with one spliced in (the real
/// definition is never edited for this).
#[test]
#[should_panic(expected = "planted_words")]
fn a_new_text_field_on_session_info_fails_the_census_by_name() {
    let planted = SESSION_SRC.replace(
        "pub struct SessionInfo {\n",
        "pub struct SessionInfo {\n    pub planted_words: Option<String>,\n",
    );
    assert_ne!(planted, SESSION_SRC, "the splice found no struct");
    census(&planted, "SessionInfo", SESSION_WITHHELD, SESSION_SHOWN, 28);
}

#[test]
#[should_panic(expected = "planted_words")]
fn a_new_text_field_on_node_info_fails_the_census_by_name() {
    let planted = NODE_SRC.replace(
        "pub struct NodeInfo {\n",
        "pub struct NodeInfo {\n    pub planted_words: String,\n",
    );
    assert_ne!(planted, NODE_SRC, "the splice found no struct");
    census(&planted, "NodeInfo", NODE_WITHHELD, NODE_SHOWN, 10);
}

/// Every field of the value, planted, so a field the helper changes (or
/// leaves) shows against its own plant.
fn planted_session() -> Value {
    json!({
        "session_id": "import:00c0ffee", "kind": "conversation",
        "label": "imported", "created_at_unix_ms": 11, "turns": 3,
        "usage": {"input_tokens": 5, "output_tokens": 6, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
        "execution_id": "x1", "execution_state": "done",
        "last_active_ms": 12, "cost_usd": 1.5, "tool_calls": 2,
        "profile": "plain", "model": "fake", "compilation_id": "c1",
        "title": "planted words title", "pending_confirms": 1,
        "parent_session_id": "p1", "limit_usd": 9.0,
        "attention": {"level": "idle", "label": "here", "since_ms": 7},
        "state": "quiet",
        "retired": {"reason": "by_hand", "at_ms": 8},
        "superseded_by": {"session_id": "s2", "at_ms": 9, "place": "dm"},
        "supersedes": {"session_id": "s0", "at_ms": 4, "place": "dm"},
        "title_was": ["planted words earlier"], "dir": "/work/tides",
        "started_on": "2026-01-02", "data": "private",
        "labels": [{"flag": "f", "kind": "k", "source": "s",
                    "confidence": 0.5, "band": "b", "at_ms": 1}],
        "first_text": "planted words first", "last_text": "planted words last",
        "prompts": "planted words prompts"
    })
}

fn planted_node() -> Value {
    json!({
        "node_id": "n1", "kind": "user_message", "session_id": "import:00c0ffee",
        "position": 4, "at_unix_ms": 13, "turn_id": "t1", "loop_index": 2,
        "author": "someone", "text": "planted words text",
        "thinking": "planted words thinking", "detail": {"note": "planted words"},
        "bytes": 20
    })
}

/// The two lists say what the helpers do: each `WITHHELD` field moves, each
/// `SHOWN` one is as it was. Every field is planted first, so an unplanted
/// `None` cannot read as "unchanged".
fn behaves(before: &Value, after: &Value, withheld: &[&str], shown: &[(&str, &str)]) {
    let (before, after) = (before.as_object().unwrap(), after.as_object().unwrap());
    for f in withheld {
        assert!(before.contains_key(*f), "`{f}` was not planted");
        assert!(
            !after
                .get(*f)
                .is_some_and(|v| v.to_string().contains("planted words")),
            "`{f}` is on WITHHELD and the helper left its words: {:?}",
            after.get(*f)
        );
        assert_ne!(
            after.get(*f),
            before.get(*f),
            "`{f}` is on WITHHELD and was not cleared"
        );
    }
    for (f, _) in shown {
        assert!(before.contains_key(*f), "`{f}` was not planted");
        assert_eq!(
            after.get(*f),
            before.get(*f),
            "`{f}` is on SHOWN and the helper changed it"
        );
    }
}

#[test]
fn the_lists_say_what_withhold_session_words_does() {
    let before = planted_session();
    let mut info: SessionInfo = serde_json::from_value(before.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&info).unwrap(),
        before,
        "a field is not planted"
    );
    super::methods::withhold_session_words(&mut info);
    behaves(
        &before,
        &serde_json::to_value(&info).unwrap(),
        SESSION_WITHHELD,
        SESSION_SHOWN,
    );
}

#[test]
fn the_lists_say_what_withheld_node_does() {
    let before = planted_node();
    let info: NodeInfo = serde_json::from_value(before.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&info).unwrap(),
        before,
        "a field is not planted"
    );
    let after = super::methods::withheld_node(info);
    behaves(
        &before,
        &serde_json::to_value(&after).unwrap(),
        NODE_WITHHELD,
        NODE_SHOWN,
    );
}
