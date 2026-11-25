//! Each look back's writer writes its text and its parts in one pass (issue 49): what the one renderer makes of
//! the parts is byte for byte the text it wrote, and that text is the one a build before parts wrote for the same
//! store (its golden, a time in it as the daemon's zone says it). Every word of a message it shows is a quote:
//! none is in a code part. All words invented.

use super::*;
use crate::history_tool::{imports, session};
use crate::quotes::{self, Part};

/// The times of the fixture's exchanges.
const T: [u64; 5] = [
    1_790_000_000_000,
    1_790_000_600_000,
    1_790_001_200_000,
    1_790_001_800_000,
    1_790_002_400_000,
];
const SID: &str = "ses_parts";
/// Words of a message that no code part may hold.
const WORDS: [&str; 6] = [
    "kittiwake",
    "harbour",
    "guest's",
    "moorings are fine",
    "lantern",
    "reply to",
];

/// What the renderer makes of `parts` from `s`, read as first written.
fn rendered(s: &Store, parts: &[Part]) -> String {
    quotes::render_with(parts, |q| quotes::read(s, q))
}

/// That `parts` render as `text`, quote `quotes` times at least, and hold none of the messages' `words` in code.
fn by_reference_of(s: &Store, text: &str, parts: &[Part], quotes: usize, words: &[&str]) {
    assert_eq!(rendered(s, parts), text, "the parts render the text");
    let n = parts.iter().filter(|p| matches!(p, Part::Quote(_))).count();
    assert!(n >= quotes, "{n} quotes: {parts:#?}");
    for p in parts {
        if let Part::Code(c) = p {
            for w in words {
                assert!(!c.contains(w), "a message's word {w:?} in code: {c:?}");
            }
        }
    }
}

/// [`by_reference_of`] the fixture's words.
fn by_reference(s: &Store, text: &str, parts: &[Part], quotes: usize) {
    by_reference_of(s, text, parts, quotes, &WORDS);
}

/// `when` of each fixture time, as a page says it.
fn at(i: usize) -> String {
    super::super::page::when(T[i])
}

/// A golden, the text a build before parts wrote for the fixture, with its times as this daemon's zone says them
/// and `sid` for the session it names.
fn golden(text: &str, sid: &str) -> String {
    let mut g = text.to_string();
    for (i, t) in T.iter().enumerate() {
        let said = crate::recall::stamp::said_in(*t, &crate::recall::stamp::system);
        g = g.replace(&format!("<AT{i}>"), &at(i));
        g = g.replace(&format!("<SAID{i}>"), &said);
    }
    g.replace("<SID>", sid)
}

/// One exchange of `e`, its nodes made at its own time.
fn put_at(store: &Store, e: &Ex<'_>, reply_text: &str) -> (Node, Node) {
    let mut ask = Node::user(e.session, Some(&e.turn), "owner", &e.words);
    ask.created_at_ms = e.at;
    let mut said = reply(e.session, &e.turn, reply_text);
    said.created_at_ms = e.at + 100;
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
    recs.extend(mark_of(&ask, e.person, e.place).and_then(|m| mark_record(&m).ok()));
    recs.push(exchange_record(&x).unwrap());
    recs.extend(touch(Root::Place(e.place), &e.turn, e.session, e.at));
    store.append(&recs).unwrap();
    (ask, said)
}

/// A `did` line for `turn`, by a writer of `writer`.
fn did(store: &Store, turn: &str, line: &str, writer: u32) {
    let d = crate::ladder::did::DidLine {
        turn_id: turn.into(),
        session_id: SID.into(),
        line: line.into(),
        model: "m".into(),
        writer,
        marks: crate::view::Marks {
            place: Some(HERE.into()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        cost_usd: 0.0,
        at_ms: 0,
    };
    store
        .append(&[crate::ladder::did::did_record(&d).unwrap()])
        .unwrap();
}

/// Five exchanges of one place: words with the page's own marks and line breaks; words past a line's cut; a
/// guest's words; one summarised by a writer that knew authors, and one by a writer before them.
fn fixture(s: &Store) {
    let long = format!("The harbour log: {}", "kittiwake count rising, ".repeat(16));
    let words = [
        "first \u{ab}kittiwake\u{bb} [lantern]\n\ttwo\r\nthree".to_string(),
        long.clone(),
        "a guest's question about the lantern".to_string(),
        "is the harbour calm".to_string(),
        "check the lantern wicks".to_string(),
    ];
    for (i, w) in words.iter().enumerate() {
        let e = Ex {
            at: T[i],
            words: w.clone(),
            person: Some(if i == 2 { "guest" } else { OWNER }),
            ..ex(i as u64, HERE, SID)
        };
        let reply = match i {
            1 => format!("reply to {}", long.repeat(2)),
            _ => format!("reply to {w}"),
        };
        put_at(s, &e, &reply);
    }
    let turn = |i: u64| ex(i, HERE, SID).turn;
    did(s, &turn(3), "You said the moorings are fine.", 2);
    did(s, &turn(4), "You checked the moorings are fine.", 1);
}

#[test]
fn page_parts_render_byte_identical() {
    let (_d, s) = store();
    fixture(&s);
    let l = lens(&s, HERE);
    let p = walk(&l, &HistoryArgs::default(), None, 0);
    let cuts = crate::scrub::Cuts::none();
    let w = super::super::page::written_page(&p, (HERE, None), 0, None, &cuts);
    let (text, parts) = w.into_parts();
    assert_eq!(text, render(&p, (HERE, None), 0, None, &cuts));
    assert_eq!(text, golden(include_str!("parts/page.txt"), ""));
    // Each line's words, and its reply's beginning or its `did` line.
    by_reference(&s, &text, &parts, 10);
}

#[test]
fn open_parts_render_byte_identical() {
    let (_d, s) = store();
    fixture(&s);
    let l = lens(&s, LOBBY);
    let goldens = [
        (1, include_str!("parts/open1.txt")),
        (2, include_str!("parts/open2.txt")),
    ];
    for (i, g) in goldens {
        let o = super::super::open::open_marked(&l, &Handle(ex(i, HERE, SID).turn)).unwrap();
        assert_eq!(o.text, golden(g, ""), "exchange {i}");
        // The person's words and the reply's, the guest's inside its outside wrapping.
        by_reference(&s, &o.text, &o.parts, 2);
    }
}

#[test]
fn session_parts_render_byte_identical() {
    let (_d, s) = store();
    fixture(&s);
    let o = session::open(&lens(&s, HERE), SID, None).unwrap();
    assert_eq!(o.text, golden(include_str!("parts/session.txt"), ""));
    by_reference(&s, &o.text, &o.parts, 10);
}

/// An open's newest message past its allowance is cut in its words: the quote of it keeps the head the text
/// keeps, its line breaks and margins as code.
#[test]
fn a_session_opens_cut_message_renders_byte_identical() {
    let (_d, s) = store();
    let big = format!("BIG kittiwake\n{}", "harbour lantern line\n".repeat(4_000));
    let e = Ex {
        words: "a short ask".into(),
        at: T[0],
        ..ex(1, HERE, SID)
    };
    put_at(&s, &e, &big);
    let o = session::open(&lens(&s, HERE), SID, None).unwrap();
    assert!(
        o.text.contains("[cut here: this message is about"),
        "{}",
        o.text
    );
    // The newest page holds the cut reply alone: one quote of the lines it keeps.
    by_reference(&s, &o.text, &o.parts, 1);
}

#[test]
fn imports_parts_render_byte_identical() {
    use crate::import::Integrity;
    use crate::memory_lookup::ImportHit;
    let (_d, s) = store();
    let sid = format!("{}{}", crate::import::SESSION_PREFIX, "b".repeat(64));
    let msg = |i: u32, integrity: Integrity, text: &str| {
        Node::imported(
            format!("imp_p{i}"),
            &sid,
            "openclaw",
            T[i as usize],
            Body::Imported {
                text: text.into(),
                integrity,
                source: "openclaw-store".into(),
                unit: format!("u{i}"),
                sha256: "0".repeat(64),
                idx: i,
            },
        )
    };
    let long = format!("the harbour lantern {}", "kittiwake ".repeat(60));
    s.append(&[
        msg(
            0,
            Integrity::Operator,
            "the kittiwake [nests]\nby the harbour",
        )
        .record()
        .unwrap(),
        msg(1, Integrity::Outside, &long).record().unwrap(),
    ])
    .unwrap();
    let hit = |i: u32, text: &str| ImportHit {
        node_id: format!("imp_p{i}"),
        session_id: sid.clone(),
        at_ms: T[i as usize],
        book: Some("casebook".into()),
        text: text.into(),
    };
    // A search's excerpt: the start of the long message, cut by the search with its `…`.
    let excerpt = format!("{}\u{2026}", &long[..150]);
    let (lines, sources, dropped) = imports::project(
        &s,
        vec![
            hit(0, "the kittiwake [nests]\nby the harbour"),
            hit(1, &excerpt),
        ],
        &crate::scrub::Cuts::none(),
    );
    assert_eq!((sources.len(), dropped), (2, 0));
    let p = HistoryPage {
        lines,
        next: None,
        end: PageEnd::HistoryExhausted,
    };
    let a = args(json!({"place": "import", "words": "kittiwake"}));
    let cuts = crate::scrub::Cuts::none();
    let (text, parts) = imports::written_page(&p, &a, None, None, &cuts).into_parts();
    assert_eq!(text, imports::render(&p, &a, None, None, &cuts));
    assert_eq!(text, golden(include_str!("parts/imports.txt"), &sid));
    // The words searched for are the call's, said in its heading: the messages' are quotes.
    by_reference_of(&s, &text, &parts, 2, &["harbour", "lantern", "nests"]);
}

/// A hit whose words its node no longer holds (an index older than the node's newest text) shows nothing: its
/// words are no quote of anything.
#[test]
fn an_import_hit_its_node_does_not_hold_is_not_shown() {
    use crate::memory_lookup::ImportHit;
    let (_d, s) = store();
    let sid = format!("{}{}", crate::import::SESSION_PREFIX, "c".repeat(64));
    let n = Node::imported(
        "imp_q0".into(),
        &sid,
        "openclaw",
        T[0],
        Body::Imported {
            text: "the words as they are now".into(),
            integrity: crate::import::Integrity::Operator,
            source: "openclaw-store".into(),
            unit: "u0".into(),
            sha256: "0".repeat(64),
            idx: 0,
        },
    );
    s.append(&[n.record().unwrap()]).unwrap();
    let hit = ImportHit {
        node_id: "imp_q0".into(),
        session_id: sid,
        at_ms: T[0],
        book: None,
        text: "words an older index held".into(),
    };
    let (lines, sources, dropped) = imports::project(&s, vec![hit], &crate::scrub::Cuts::none());
    assert!(lines.is_empty() && sources.is_empty(), "{lines:?}");
    assert_eq!(dropped, 1);
}
