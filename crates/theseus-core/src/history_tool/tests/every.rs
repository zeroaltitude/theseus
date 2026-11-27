//! A look-back that narrows (a span, or words) and names no place reads every place (issue 44): every place's
//! exchanges, merged in the touch index's own order (newest first, then by turn id ascending within one
//! millisecond), each line naming its place, history whose place was never recorded marked so. A bare call
//! still reads here; the self loop's scope, a fork's cut and an erased message's newest record hold on every
//! line; and a page costs about its hundred reads and a small chunk a place, never a hundred a place. `place:
//! "*"` asks every place outright, with nothing to narrow it.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::{answer, args, ex, lens, noon_nov, pov, put, reply, self_session, store, turns, walk};
use super::{Board, Ex, DM, HERE, LOBBY};
use crate::exchange::{exchange_record, touch, Exchange, Opener, Root, OWNER};
use crate::history_tool::page::TOUCHES_READ;
use crate::node::Node;
use crate::store::Store;
use crate::view::history::HistoryArgs;
use crate::view::timeline::Cursor;
use crate::view::{PointOfView, Present};

/// A span holding all the tests' times.
fn all_time() -> crate::recall::when::When {
    crate::recall::when::When {
        said: "all time".into(),
        from_ms: Some(1),
        to_ms: Some(4_000_000_000_000),
    }
}

/// `answer` for session `me`'s turn, from `here`, as of 22 November 2026.
async fn call(core: &std::sync::Arc<crate::Core>, me: &str, here: &str, v: Value) -> String {
    let at = PointOfView {
        now_ms: noon_nov(22),
        ..pov(here, Present::Open)
    };
    answer(core, &Board::default(), (me, "t"), &at, &args(v))
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .text
}

/// [`put`], with the message and the reply dated at the exchange's start, as an index reads them.
fn put_dated(store: &Store, e: &Ex<'_>) {
    let mut ask = Node::user(e.session, Some(&e.turn), "owner", &e.words);
    let mut said = reply(e.session, &e.turn, &format!("reply to {}", e.words));
    ask.created_at_ms = e.at;
    said.created_at_ms = e.at + 1;
    let x = Exchange {
        turn_id: e.turn.clone(),
        session_id: e.session.into(),
        place: e.place.into(),
        private: e.private,
        opened_by: Opener::Message,
        person: e.person.map(str::to_string),
        start_ms: e.at,
        end_ms: Some(e.at + 500),
        ask: Some(ask.id.clone()),
        reply: Some(said.id.clone()),
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    let mut recs = vec![ask.record().unwrap(), said.record().unwrap()];
    recs.push(exchange_record(&x).unwrap());
    recs.extend(touch(Root::Place(e.place), &e.turn, e.session, e.at));
    store.append(&recs).unwrap();
}

/// A date with no place reads every place: here, a DM, a channel, a task's own place and history whose place
/// was never recorded, newest first, each line naming its place; a day outside the span is not on it.
#[tokio::test]
async fn a_span_with_no_place_reads_every_place_newest_first() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let other = session(&r.core, None);
    let day = noon_nov(11);
    let said = [
        (1, HERE, me.as_str(), Some(OWNER)),
        (2, DM, other.as_str(), Some(OWNER)),
        (3, LOBBY, other.as_str(), Some("discord:7")),
        (4, "task:tsk_kelp", other.as_str(), None),
        (5, crate::exchange::rebuild::UNKNOWN, other.as_str(), None),
    ];
    for (i, place, session, person) in said {
        let e = Ex {
            at: day + i * 1_000,
            person,
            ..ex(i, place, session)
        };
        put(&r.core.store, &e);
    }
    put(
        &r.core.store,
        &Ex {
            at: noon_nov(12),
            ..ex(6, DM, &other)
        },
    );
    let page = call(&r.core, &me, HERE, json!({"when": "2026-11-11"})).await;
    let at: Vec<usize> = (1..=5)
        .map(|i| {
            page.find(&format!("message {i}"))
                .unwrap_or_else(|| panic!("message {i}: {page}"))
        })
        .collect();
    assert!(at.windows(2).all(|w| w[0] > w[1]), "newest first: {page}");
    assert!(!page.contains("message 6"), "outside the span: {page}");
    assert!(page.contains("every place's history"), "{page}");
    for place in [
        format!(" · {HERE} · "),
        format!(" · {DM} · "),
        format!(" · {LOBBY} · "),
        " · task:tsk_kelp · ".into(),
        " · place not recorded · ".into(),
    ] {
        assert!(page.contains(&place), "{place}: {page}");
    }
}

/// Every place's exchanges page by `before` with no gap and no repeat, in the index's own order: four places,
/// two whose names share a prefix (`local:/a`, `local:/ab`), three exchanges of different places in one
/// millisecond, turn ids that do not follow time.
#[test]
fn every_places_pages_page_by_before_with_no_gap_or_repeat() {
    let (_d, s) = store();
    let places = ["local:/a", "local:/ab", DM, LOBBY];
    let mut want: Vec<(u64, String)> = Vec::new();
    for i in 0..250u64 {
        let turn = format!("turn_{:04}", (i * 7_919) % 10_000);
        let e = Ex {
            turn: turn.clone(),
            at: 1_000_000 + (i / 3) * 1_000,
            ..ex(i, places[(i % 4) as usize], "ses_a")
        };
        put(&s, &e);
        want.push((e.at, turn));
    }
    want.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let want: Vec<String> = want.into_iter().map(|(_, t)| t).collect();
    let mut l = lens(&s, "local:/a");
    l.span = Some(all_time());
    let (mut got, mut before, mut looks) = (Vec::<String>::new(), None::<Cursor>, 0);
    loop {
        let p = walk(&l, &HistoryArgs::default(), before.clone(), looks);
        got.extend(turns(&p));
        looks += 1;
        match p.next {
            Some(c) if looks < 5 => before = Some(c),
            _ => break,
        }
    }
    assert_eq!(got.len(), 250, "{got:?}");
    assert_eq!(got.iter().collect::<HashSet<_>>().len(), 250, "no repeat");
    assert_eq!(got, want, "the index's own order, no gap");
}

/// The cost of a page of every place: about its hundred reads, and a small chunk of each place's touches,
/// never a hundred of each. Eighty places of twenty exchanges each: a hundred a place would be eight thousand.
#[test]
fn a_page_of_every_place_reads_a_hundred_and_a_small_chunk_a_place() {
    let (_d, s) = store();
    let places: Vec<String> = (0..80).map(|p| format!("local:/p{p:02}")).collect();
    for i in 0..1_600u64 {
        let place = &places[(i % 80) as usize];
        put(&s, &ex(i, place, "ses_a"));
    }
    let mut l = lens(&s, "local:/p00");
    l.span = Some(all_time());
    TOUCHES_READ.with(|n| n.set(0));
    let p = walk(&l, &HistoryArgs::default(), None, 0);
    let read = TOUCHES_READ.with(std::cell::Cell::get);
    assert_eq!(p.lines.len(), 100);
    let shown: HashSet<&str> = p.lines.iter().map(|l| l.place.as_str()).collect();
    assert_eq!(shown.len(), 80, "every place: {shown:?}");
    assert!(
        read <= 200 + 80 * 10,
        "{read} touches read for a page of 80 places"
    );
}

/// A bare call still reads this place only: before what is in view, here.
#[tokio::test]
async fn a_bare_call_still_reads_this_place_only() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let other = session(&r.core, None);
    put(&r.core.store, &ex(1, HERE, &me));
    put(&r.core.store, &ex(2, DM, &other));
    let page = call(&r.core, &me, HERE, json!({})).await;
    assert!(page.contains("message 1"), "{page}");
    assert!(!page.contains("message 2"), "{page}");
    assert!(!page.contains("every place"), "{page}");
}

/// A session the self loop opened to judge (a reviewer's) reads its own place by date, never every place.
#[tokio::test]
async fn a_self_loop_span_with_no_place_reads_its_own_place_only() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = self_session(&r.core, Some(crate::rsi::roles::Role::Reviewer));
    let mine =
        crate::view_gate::place_name(&r.core.runner, &me).unwrap_or_else(|| "unknown".into());
    let other = session(&r.core, None);
    let day = noon_nov(11);
    put(
        &r.core.store,
        &Ex {
            at: day,
            ..ex(1, &mine, &me)
        },
    );
    put(
        &r.core.store,
        &Ex {
            at: day + 1_000,
            ..ex(2, DM, &other)
        },
    );
    let page = call(&r.core, &me, &mine, json!({"when": "2026-11-11"})).await;
    assert!(page.contains("turn_0001"), "its own: {page}");
    assert!(!page.contains("turn_0002"), "another place: {page}");
}

/// An erased message in another place shows no words on a date's page: each node is read by its newest
/// record, wherever the line comes from.
#[tokio::test]
async fn an_erased_message_in_another_place_shows_no_words_on_a_span_page() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let other = session(&r.core, None);
    let day = noon_nov(11);
    let (ask, _) = put(
        &r.core.store,
        &Ex {
            at: day,
            words: "the kelp boat".into(),
            ..ex(2, DM, &other)
        },
    );
    r.core
        .store
        .append(&[ask.erased(day + 5_000, "asked").record().unwrap()])
        .unwrap();
    let page = call(&r.core, &me, HERE, json!({"when": "2026-11-11"})).await;
    assert!(page.contains("turn_0002"), "the line is there: {page}");
    assert!(
        !page.contains("\u{ab}the kelp boat\u{bb}"),
        "no words: {page}"
    );
}

/// Words and a span in the past find a hit in another place: the words search is scoped to the span's own
/// exchanges of every place, and the span is passed to the index.
#[tokio::test]
async fn words_and_a_past_span_find_a_hit_in_another_place() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let other = session(&r.core, None);
    let day = noon_nov(11);
    let said = [
        (1, HERE, me.as_str(), day, "the ferry to kelp island"),
        (2, DM, other.as_str(), day + 1_000, "the ferry tide table"),
        (3, DM, other.as_str(), noon_nov(12), "the ferry a day later"),
    ];
    for (i, place, session, at, words) in said {
        put_dated(
            &r.core.store,
            &Ex {
                at,
                words: words.into(),
                ..ex(i, place, session)
            },
        );
    }
    r.core
        .runner
        .memory
        .set_ask(crate::tests_imports::index_over(
            &r.core,
            vec![me.clone(), other.clone()],
        ));
    let page = call(
        &r.core,
        &me,
        HERE,
        json!({"words": "ferry", "when": "2026-11-11"}),
    )
    .await;
    assert!(page.contains("turn_0001"), "{page}");
    assert!(page.contains("turn_0002"), "another place: {page}");
    assert!(!page.contains("turn_0003"), "outside the span: {page}");
}

/// `place: "*"`, bare, reads every place: here, a DM and a channel, newest first, each line naming its place,
/// the page headed as every place's.
#[tokio::test]
async fn a_bare_every_place_call_pages_every_place_newest_first() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let other = session(&r.core, None);
    for (i, place, session) in [(1, HERE, &me), (2, DM, &other), (3, LOBBY, &other)] {
        put(&r.core.store, &ex(i, place, session));
    }
    let page = call(&r.core, &me, HERE, json!({"place": "*"})).await;
    let at: Vec<usize> = (1..=3)
        .map(|i| {
            page.find(&format!("message {i}"))
                .unwrap_or_else(|| panic!("message {i}: {page}"))
        })
        .collect();
    assert!(at.windows(2).all(|w| w[0] > w[1]), "newest first: {page}");
    assert!(page.contains("every place's history"), "{page}");
    for place in [HERE, DM, LOBBY] {
        assert!(page.contains(&format!(" · {place} · ")), "{place}: {page}");
    }
}

/// Every place's pages, asked by `place: "*"` with no span and no words, go on from the cursor each was given
/// with no gap and no repeat, in the index's own order.
#[test]
fn an_every_place_page_resumes_before_the_cursor_it_was_given_with_no_gap_or_repeat() {
    let (_d, s) = store();
    let places = ["local:/a", "local:/ab", DM, LOBBY];
    let mut want: Vec<(u64, String)> = Vec::new();
    for i in 0..250u64 {
        let turn = format!("turn_{:04}", (i * 7_919) % 10_000);
        let e = Ex {
            turn: turn.clone(),
            at: 1_000_000 + (i / 3) * 1_000,
            ..ex(i, places[(i % 4) as usize], "ses_a")
        };
        put(&s, &e);
        want.push((e.at, turn));
    }
    want.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let want: Vec<String> = want.into_iter().map(|(_, t)| t).collect();
    let l = lens(&s, "local:/a");
    let every = args(json!({"place": "*"}));
    let (mut got, mut before, mut looks) = (Vec::<String>::new(), None::<Cursor>, 0);
    loop {
        let p = walk(&l, &every, before.clone(), looks);
        got.extend(turns(&p));
        looks += 1;
        match p.next {
            Some(c) if looks < 5 => before = Some(c),
            _ => break,
        }
    }
    assert_eq!(got.len(), 250, "{got:?}");
    assert_eq!(got.iter().collect::<HashSet<_>>().len(), 250, "no repeat");
    assert_eq!(got, want, "the index's own order, no gap");
}

/// A session the self loop opened to judge (a reviewer's), asking every place, keeps to its own place, as a bare
/// call does.
#[tokio::test]
async fn a_self_loop_session_asking_every_place_keeps_to_its_own() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = self_session(&r.core, Some(crate::rsi::roles::Role::Reviewer));
    let mine =
        crate::view_gate::place_name(&r.core.runner, &me).unwrap_or_else(|| "unknown".into());
    let other = session(&r.core, None);
    put(&r.core.store, &ex(1, &mine, &me));
    put(&r.core.store, &ex(2, DM, &other));
    let page = call(&r.core, &me, &mine, json!({"place": "*"})).await;
    assert!(page.contains("turn_0001"), "its own: {page}");
    assert!(!page.contains("turn_0002"), "another place: {page}");
    assert!(!page.contains("every place"), "{page}");
}

/// `[context] history_page_exchanges` sizes the page of the real tool: at 20, a bare call and a `place: "*"`
/// call each give 20 lines a page, and each page's `next`, passed back as `before`, gives the next ones with no
/// gap and no repeat.
#[tokio::test]
async fn the_history_page_setting_sizes_the_real_tools_pages() {
    use crate::tests_places::{rig_with, session};
    let r = rig_with(|c| c.context.history_page_exchanges = Some(20));
    let me = session(&r.core, None);
    let other = session(&r.core, None);
    for i in 1..=50u64 {
        let place = if i % 2 == 0 { HERE } else { DM };
        put(&r.core.store, &ex(i, place, &other));
    }
    let at = PointOfView {
        now_ms: noon_nov(22),
        ..pov(HERE, Present::Open)
    };
    let page = |v: Value| {
        let (core, me, at) = (&r.core, &me, &at);
        async move {
            let o = answer(core, &Board::default(), (me, "t"), at, &args(v))
                .await
                .unwrap_or_else(|e| panic!("{e}"));
            // Each line of a page names its exchange: `turn_` and four digits.
            let lines: Vec<String> = (o.text.lines())
                .filter(|l| l.starts_with("- "))
                .filter_map(|l| l.find("turn_").and_then(|i| l.get(i..i + 9)))
                .map(str::to_string)
                .collect();
            (lines, o.meta)
        }
    };
    for (asked, all) in [(json!({}), 25), (json!({"place": "*"}), 50)] {
        let (first, meta) = page(asked.clone()).await;
        assert_eq!(meta["page"], 20, "{asked}: {meta}");
        assert_eq!(
            (first.len(), &meta["lines"]),
            (20, &json!(20)),
            "{asked}: {first:?}"
        );
        let mut next = asked.clone();
        next["before"] = meta["next"].clone();
        let (second, meta) = page(next).await;
        assert_eq!(
            second.len(),
            (all - 20).min(20),
            "{asked}: {second:?} {meta}"
        );
        let both: HashSet<&String> = first.iter().chain(&second).collect();
        assert_eq!(both.len(), first.len() + second.len(), "{asked}: no repeat");
        let newest: Vec<String> = (1..=50u64)
            .rev()
            .filter(|i| all == 50 || i % 2 == 0)
            .take(first.len() + second.len())
            .map(|i| format!("turn_{i:04}"))
            .collect();
        assert_eq!(
            [first, second].concat(),
            newest,
            "{asked}: newest first, no gap"
        );
    }
}
