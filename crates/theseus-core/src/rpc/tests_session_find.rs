//! `session.list { dir, with_text }` (theseus-6yka): the conversations of one
//! directory, by the git root of each, newest active first, with the words
//! of each one's first and last message; and where each started.

use std::sync::Arc;

use theseus_protocol::{SessionListParams, SessionOpenParams};

use super::tests::test_core;
use super::Core;
use crate::node::Node;

fn core() -> Arc<Core> {
    test_core("the tide is out")
}

/// A session working in `dir`, last active at `active`, with `turns` turns.
fn open_in(core: &Core, dir: &str, active: u64, turns: u64) -> String {
    // Births are told apart by the millisecond.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let id = core
        .open_session(SessionOpenParams {
            dir: Some(dir.to_string()),
            ..Default::default()
        })
        .unwrap()
        .session_id;
    let mut r: crate::session::SessionRecord = core.store.get_session(&id).unwrap().unwrap();
    (r.turns, r.last_active_ms) = (turns, active);
    core.store.put_session(&id, &r).unwrap();
    id
}

fn ids(core: &Core, dir: &str) -> Vec<String> {
    core.session_list_of(SessionListParams {
        dir: Some(dir.to_string()),
        ..Default::default()
    })
    .unwrap()
    .sessions
    .into_iter()
    .map(|s| s.session_id)
    .collect()
}

#[test]
fn the_filter_keeps_one_directorys_sessions_newest_active_first() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let older = open_in(&c, "/work/tides", now - 5_000, 1);
    let other = open_in(&c, "/work/reef", now - 1_000, 1);
    let newer = open_in(&c, "/work/tides", now - 2_000, 1);
    let nowhere = c
        .open_session(SessionOpenParams::default())
        .unwrap()
        .session_id;
    assert_eq!(ids(&c, "/work/tides"), [newer, older]);
    assert_eq!(ids(&c, "/work/reef"), [other]);
    assert!(ids(&c, "/work/none").is_empty());
    assert!(
        !ids(&c, "/work/tides").contains(&nowhere),
        "a session with no directory is no directory's"
    );
}

#[test]
fn a_directory_finds_the_sessions_started_below_its_git_root() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let t = tempfile::tempdir().unwrap();
    let (repo, other) = (t.path().join("repo"), t.path().join("other"));
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src/deep")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
    let top = open_in(&c, &s(&repo), now - 3_000, 1);
    let sub = open_in(&c, &s(&repo.join("src/deep")), now - 2_000, 1);
    let elsewhere = open_in(&c, &s(&other), now - 1_000, 1);
    // Asked from the root, from a subdirectory, or from a sibling:
    assert_eq!(ids(&c, &s(&repo)), [sub.clone(), top.clone()]);
    assert_eq!(ids(&c, &s(&repo.join("src"))), [sub, top]);
    assert_eq!(ids(&c, &s(&other)), [elsewhere]);
}

#[test]
fn the_words_of_the_first_and_last_message_ride_with_text_only() {
    let c = core();
    let id = open_in(&c, "/work/tides", theseus_protocol::now_unix_ms(), 2);
    let batch: Vec<_> = [
        "survey the log lines",
        "now the second pass",
        "thanks, done",
    ]
    .iter()
    .map(|t| Node::user(&id, None, "operator", t).record().unwrap())
    .collect();
    c.store.append(&batch).unwrap();
    let ask = |with_text| {
        c.session_list_of(SessionListParams {
            dir: Some("/work/tides".into()),
            with_text,
            ..Default::default()
        })
        .unwrap()
        .sessions
        .remove(0)
    };
    let bare = ask(false);
    assert_eq!((bare.first_text, bare.last_text), (None, None));
    let full = ask(true);
    assert_eq!(full.first_text.as_deref(), Some("survey the log lines"));
    assert_eq!(full.last_text.as_deref(), Some("thanks, done"));
}

#[test]
fn without_a_directory_the_text_is_asked_of_every_session() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    open_in(&c, "/work/tides", now - 1, 1);
    let b = open_in(&c, "/work/reef", now - 2, 1);
    c.open_session(SessionOpenParams::default()).unwrap();
    c.store
        .append(&[Node::user(&b, None, "operator", "chart the reef")
            .record()
            .unwrap()])
        .unwrap();
    let all = c
        .session_list_of(SessionListParams {
            with_text: true,
            ..Default::default()
        })
        .unwrap()
        .sessions;
    let by = |id: &str| all.iter().find(|s| s.session_id == id).unwrap();
    assert_eq!(all.len(), 3, "every directory's, and none's");
    assert_eq!(by(&b).first_text.as_deref(), Some("chart the reef"));
    assert!(by(&b)
        .prompts
        .as_deref()
        .unwrap()
        .contains("chart the reef"));
}

#[test]
fn a_session_says_where_it_started() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    let here = open_in(&c, "/work/tides", now, 1);
    let dm = open_in(&c, "/work/tides", now, 1);
    c.outbox.bind_place("dm:4242", &dm).unwrap();
    let by = |id: &str| {
        c.session_list_of(SessionListParams::default())
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.session_id == id)
            .unwrap()
            .started_on
    };
    assert_eq!(by(&here).as_deref(), Some("cli"));
    assert_eq!(by(&dm).as_deref(), Some("discord"), "it speaks to a place");
}

#[test]
fn a_page_goes_back_by_its_cursor_and_the_filter_pages_like_the_list() {
    let c = core();
    let now = theseus_protocol::now_unix_ms();
    for i in 0..5 {
        open_in(&c, "/work/tides", now - i, 1);
        open_in(&c, "/work/reef", now - i, 1);
    }
    // Three births at a step, and two sessions wanted.
    let page = |before| {
        c.session_list_scanning(
            SessionListParams {
                dir: Some("/work/tides".into()),
                n: Some(2),
                before,
                ..Default::default()
            },
            Some("/work/tides"),
            3,
        )
        .unwrap()
    };
    let mut pages = Vec::new();
    let mut at = None;
    loop {
        let p = page(at);
        at = p.older;
        pages.push(p);
        if at.is_none() {
            break;
        }
    }
    assert!(pages.len() >= 2, "the sessions come in more than one page");
    assert!(pages[..pages.len() - 1].iter().all(|p| p.older.is_some()));
    assert!(
        pages.last().unwrap().older.is_none(),
        "the last page has no cursor"
    );
    let mut all: Vec<String> = pages
        .into_iter()
        .flat_map(|p| p.sessions.into_iter().map(|s| s.session_id))
        .collect();
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(
        (n, all.len()),
        (5, 5),
        "each of the directory's sessions once, none of the other's"
    );
}
