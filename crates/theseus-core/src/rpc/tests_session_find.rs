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

/// The fourth door of theseus-a7pr, as it actually is on this base.
///
/// The follow-up said `session.list { with_text }` returns an imported
/// session's `first_text`, `last_text`, `prompts` and `title` to every
/// surface. Three of those four it cannot: `with_text` is filled only on the
/// scanning path (`session_list_in`), and every scanning path reads
/// `live_sessions`/`sessions_paged`, which step past the imported keys
/// (theseus-7087) — so an imported session is in no `with_text` answer at
/// all. The path that is open is `session.list { ids }`, which reads each
/// named record straight from the store with no import filter and no
/// surface, and hands back its `title`: `session::title_from` of its summary
/// or its first message, the same sixty characters `session.history` was
/// patched for one door up. This test holds both halves, so neither the
/// closed door nor the open one can change in silence.
#[test]
fn an_imported_sessions_words_reach_only_a_surface_that_reads_private_text() {
    let c = core();
    let imported = format!("{}{}", crate::import::SESSION_PREFIX, "00c0ffee");
    {
        let mut r =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        r.session_id = imported.clone();
        r.title = Some("the kestrel count on the north ridge".into());
        r.title_was = vec!["the first heading".into()];
        r.dir = Some("/work/tides".into());
        r.last_active_ms = theseus_protocol::now_unix_ms();
        c.store.put_session(&imported, &r).unwrap();
    }
    let plain = open_in(&c, "/work/tides", theseus_protocol::now_unix_ms(), 1);
    {
        let mut r: crate::session::SessionRecord = c.store.get_session(&plain).unwrap().unwrap();
        r.title = Some("the tide log".into());
        c.store.put_session(&plain, &r).unwrap();
    }
    for (id, said) in [
        (&imported, "forty kestrels over the ridge"),
        (&plain, "the tide is out"),
    ] {
        let n = Node::user(id, None, "operator", said).record().unwrap();
        c.store.append(&[n]).unwrap();
    }

    let by_ids = |private: bool| {
        c.session_list_for(
            SessionListParams {
                ids: Some(vec![imported.clone(), plain.clone()]),
                ..Default::default()
            },
            private,
        )
        .unwrap()
        .sessions
    };
    let row = |rows: &[theseus_protocol::SessionInfo], id: &str| {
        rows.iter()
            .find(|s| s.session_id == id)
            .unwrap_or_else(|| panic!("{id} is not in the list"))
            .clone()
    };

    // The open door: `ids` reads the record itself, so the imported session
    // is there, and its title is its first message.
    let owner = by_ids(true);
    assert_eq!(owner.len(), 2, "both sessions are read by id");
    let mine = row(&owner, &imported);
    assert_eq!(
        mine.title.as_deref(),
        Some("the kestrel count on the north ridge"),
        "the owner's own surface reads it"
    );
    assert_eq!(
        mine.title_was.as_deref(),
        Some(["the first heading".to_string()].as_slice())
    );

    let untrusted = by_ids(false);
    assert_eq!(untrusted.len(), 2, "the row is listed, not dropped");
    let withheld = row(&untrusted, &imported);
    assert_eq!(withheld.session_id, imported);
    assert_eq!(withheld.title, None, "its first message's words leaked");
    assert_eq!(
        withheld.title_was, None,
        "an earlier title is the same words"
    );

    // A session that is not imported is untouched on either surface.
    for rows in [&owner, &untrusted] {
        assert_eq!(row(rows, &plain).title.as_deref(), Some("the tide log"));
    }

    // The door the follow-up named, which is shut one level down: the
    // scanning path steps past every imported key, so `with_text` cannot
    // reach one. Asserted with the positive count, so an empty answer, or a
    // scan that found nothing, cannot meet the bar.
    for private in [true, false] {
        let scanned = c
            .session_list_for(
                SessionListParams {
                    with_text: true,
                    ..Default::default()
                },
                private,
            )
            .unwrap()
            .sessions;
        assert!(
            scanned.iter().any(|s| s.session_id == plain),
            "the scan found the ordinary session"
        );
        assert!(
            scanned
                .iter()
                .all(|s| !crate::import::is_imported(&s.session_id)),
            "an imported session reached a with_text answer: {:?}",
            scanned.iter().map(|s| &s.session_id).collect::<Vec<_>>()
        );
        let other = row(&scanned, &plain);
        assert!(
            other
                .first_text
                .as_deref()
                .is_some_and(|t| t.contains("the tide is out")),
            "with_text still fills an ordinary session's words: {:?}",
            other.first_text
        );
    }
}
