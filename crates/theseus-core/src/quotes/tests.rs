//! A rendering kept by reference reads what was shown then: its code's words as stored, each quote's bytes as
//! first written, by position; an erase and a "wrong" mark are the only things said since that change it, and a
//! quote that cannot be read shows a fixed line, never a guess. All names and words invented.

use std::collections::BTreeSet;

use serde_json::json;

use super::*;
use crate::copies::since::Since;
use crate::node::Attachment;

const AT_MS: u64 = 1_790_000_000_000;
const WORDS: &str = "the grey heron nests by the old weir";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("store")).unwrap();
    (dir, s)
}

/// `n` stored: its position.
fn put(s: &Store, n: &Node) -> u64 {
    s.append(&[n.record().unwrap()]).unwrap()[0]
}

fn quote(node: &str, position: u64, field: Field, (from, to): (usize, usize), form: Form) -> Part {
    Part::Quote(Quote {
        node: node.into(),
        position,
        field,
        from: from as u32,
        to: to as u32,
        form,
    })
}

/// `parts` as laid with nothing marked wrong.
fn laid(s: &Store, parts: &[Part]) -> String {
    render(s, parts, &Since::new(s, &BTreeSet::new())).unwrap()
}

fn reply(text: &str) -> Node {
    Node::assistant(
        "ses_weir",
        "t1",
        0,
        Body::AssistantMessage {
            blocks: vec![
                json!({"type": "thinking", "thinking": "the owner asks about the weir"}),
                json!({"type": "text", "text": text}),
                json!({"type": "tool_use", "id": "c1", "name": "fs_read", "input": {"path": "a"}}),
                json!({"type": "text", "text": " Noted."}),
            ],
            model: "m".into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    )
}

/// The page line a reference keeps: the code's words around a quote of the message, in a page's form.
#[test]
fn parts_render_the_bytes_first_written_even_after_a_rewrite_under_the_id() {
    let (_d, s) = store();
    let first = Node::user("ses_weir", Some("t0"), "cli", WORDS);
    let p0 = put(&s, &first);
    // A later record under the same id (as an imported summary is written again): the newest by id.
    let mut again = first.clone();
    again.body = Body::UserMessage {
        text: "the heron moved to the mill race".into(),
        attachments: Vec::new(),
        parts: Vec::new(),
    };
    let p1 = put(&s, &again);
    let parts = vec![
        Part::Code("- 2026-03-03 \u{b7} cli \u{b7} the owner: \u{ab}".into()),
        quote(&first.id, p0, Field::Text, (4, 19), Form::Flat1),
        Part::Code("\u{bb} \u{2192} t0".into()),
    ];
    assert_eq!(
        laid(&s, &parts),
        "- 2026-03-03 \u{b7} cli \u{b7} the owner: \u{ab}grey heron nest\u{bb} \u{2192} t0"
    );
    // Read again, the same bytes: nothing is cut again, nothing is run again.
    assert_eq!(laid(&s, &parts), laid(&s, &parts));
    // The position decides which words: the later record's, read at its own.
    let later = [quote(&first.id, p1, Field::Text, (4, 9), Form::Verbatim)];
    assert_eq!(laid(&s, &later), "heron");
}

/// An erase blanks the quotes of the erased node, one fixed line each, and only them.
#[test]
fn an_erased_quote_renders_one_line_and_the_rest_stays() {
    let (_d, s) = store();
    let gone = Node::user("ses_weir", Some("t0"), "cli", WORDS);
    let kept = Node::user("ses_weir", Some("t1"), "cli", "the tide turns at four");
    let (pg, pk) = (put(&s, &gone), put(&s, &kept));
    let parts = vec![
        Part::Code("1: ".into()),
        quote(&gone.id, pg, Field::Text, (0, WORDS.len()), Form::Verbatim),
        Part::Code("\n2: ".into()),
        quote(&kept.id, pk, Field::Text, (0, 8), Form::Verbatim),
        Part::Code("\n3: ".into()),
        quote(&gone.id, pg, Field::Text, (4, 8), Form::Flat1),
    ];
    assert_eq!(
        laid(&s, &parts),
        format!("1: {WORDS}\n2: the tide\n3: grey")
    );
    put(&s, &gone.erased(AT_MS, "a test"));
    let text = laid(&s, &parts);
    assert_eq!(
        text,
        format!("1: {ERASED_QUOTE}\n2: the tide\n3: {ERASED_QUOTE}")
    );
    assert!(!text.contains("heron") && !text.contains("grey"), "{text}");
}

/// A quote that cannot be read as it was written shows one fixed line: never another record's words, never the
/// newer text under its id, never a part of a field it does not hold.
#[test]
fn an_unreadable_quote_fails_closed() {
    let (_d, s) = store();
    let msg = Node::user("ses_weir", Some("t0"), "cli", "caf\u{e9} by the weir");
    let other = Node::user("ses_weir", Some("t1"), "cli", "the mill race floods");
    let (pm, po) = (put(&s, &msg), put(&s, &other));
    let one = |p: Part| laid(&s, &[p]);
    let whole = (0, msg_len(&msg));
    // The position holds another node's record.
    assert_eq!(
        one(quote(&msg.id, po, Field::Text, whole, Form::Verbatim)),
        UNREADABLE_QUOTE
    );
    // A position past the log's end.
    assert_eq!(
        one(quote(
            &msg.id,
            po + 1_000,
            Field::Text,
            whole,
            Form::Verbatim
        )),
        UNREADABLE_QUOTE
    );
    // A node never stored.
    assert_eq!(
        one(quote("msg_none", pm, Field::Text, whole, Form::Verbatim)),
        UNREADABLE_QUOTE
    );
    // A range past the text, backwards, or inside a character (`é` is two bytes, at 3 and 4).
    for range in [(0, 100), (5, 2), (0, 4)] {
        assert_eq!(
            one(quote(&msg.id, pm, Field::Text, range, Form::Verbatim)),
            UNREADABLE_QUOTE
        );
    }
    // A field its body does not hold: a person's message has no reply text.
    assert_eq!(
        one(quote(&msg.id, pm, Field::ReplyText, (0, 3), Form::Verbatim)),
        UNREADABLE_QUOTE
    );
    // An exchange with no `did` line, and a `did` quote whose position holds a node.
    assert_eq!(
        one(quote("t0", pm, Field::Did, (0, 3), Form::Verbatim)),
        UNREADABLE_QUOTE
    );
    // And the readable one, for contrast.
    assert_eq!(
        one(quote(&msg.id, pm, Field::Text, (0, 5), Form::Verbatim)),
        "caf\u{e9}"
    );
}

fn msg_len(n: &Node) -> usize {
    match &n.body {
        Body::UserMessage { text, .. } => text.len(),
        _ => unreachable!(),
    }
}

/// Each field reads its own text: a message's words without its files, a reply's text blocks without thinking or
/// tool traffic, the index's text (files after a line break), an imported message's words, a `did` line.
#[test]
fn each_field_reads_its_own_text() {
    let (_d, s) = store();
    let file = Attachment {
        name: "tides.txt".into(),
        media_type: "text/plain".into(),
        size: 9,
        content: AttachmentContent::Text {
            text: "high at 4".into(),
            cut: false,
        },
    };
    let msg = Node::user_with("ses_weir", Some("t0"), "cli", "the tide table", vec![file]);
    let answer = reply("It turns at four.");
    let imported = Node::imported(
        "imp_00000000000000000000000000000001".into(),
        "ses_old",
        "owner",
        AT_MS,
        Body::Imported {
            text: "an old note on the weir".into(),
            integrity: crate::import::Integrity::Operator,
            source: "notes".into(),
            unit: "u1".into(),
            sha256: "0".repeat(64),
            idx: 0,
        },
    );
    let (pm, pa, pi) = (put(&s, &msg), put(&s, &answer), put(&s, &imported));
    let d = DidLine {
        turn_id: "t0".into(),
        session_id: "ses_weir".into(),
        line: "You told the owner the tide turns at four.".into(),
        model: "m".into(),
        writer: 1,
        marks: crate::view::Marks {
            place: None,
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        cost_usd: 0.0,
        at_ms: AT_MS,
    };
    let pd = s
        .append(&[crate::ladder::did::did_record(&d).unwrap()])
        .unwrap()[0];
    let all = |id: &str, p: u64, f: Field, len: usize| {
        laid(&s, &[quote(id, p, f, (0, len), Form::Verbatim)])
    };
    assert_eq!(all(&msg.id, pm, Field::Text, 14), "the tide table");
    assert_eq!(
        all(&msg.id, pm, Field::Chunk, 24),
        "the tide table\nhigh at 4"
    );
    assert_eq!(
        all(&answer.id, pa, Field::ReplyText, 24),
        "It turns at four. Noted."
    );
    assert_eq!(
        all(&answer.id, pa, Field::Chunk, 24),
        "It turns at four. Noted."
    );
    assert_eq!(
        all(&imported.id, pi, Field::Chunk, 23),
        "an old note on the weir"
    );
    assert_eq!(all(&imported.id, pi, Field::Text, 2), UNREADABLE_QUOTE);
    assert_eq!(all("t0", pd, Field::Did, 42), d.line);
    // A did quote names its exchange: under another turn's key, the record at that position is not its line.
    assert_eq!(all("t9", pd, Field::Did, 42), UNREADABLE_QUOTE);
}

/// What each form makes of one text, byte for byte, and the names a stored record gives forms and fields: both
/// frozen. A change to any line here is a change to what old references show, so it is a new form or field.
#[test]
fn forms_are_frozen() {
    const TEXT: &str = "  a\u{ab}b]\t\tc\r\nd\u{85}e  \n";
    let golden = [
        (Form::Verbatim, "verbatim", TEXT),
        (Form::Flat1, "flat1", " a\u{2039}b) c d e "),
        (Form::Clip1, "clip1", " a\u{ab}b] c d e "),
        (
            Form::Margin1,
            "margin1",
            "\u{2502}   a\u{ab}b]  c\n\u{2502} d e  ",
        ),
        (Form::Line1, "line1", "  a\u{ab}b]  c  d e   "),
    ];
    for (form, name, shown) in golden {
        assert_eq!(form.apply(TEXT), shown, "{form:?}");
        assert_eq!(serde_json::to_value(form).unwrap(), json!(name));
    }
    for (field, name) in [
        (Field::Text, "text"),
        (Field::ReplyText, "reply_text"),
        (Field::Did, "did"),
        (Field::Chunk, "chunk"),
    ] {
        assert_eq!(serde_json::to_value(field).unwrap(), json!(name));
    }
    let parts = vec![
        Part::Code("a".into()),
        quote("msg_1", 7, Field::Did, (1, 2), Form::Line1),
    ];
    assert_eq!(
        serde_json::to_string(&parts).unwrap(),
        r#"[{"code":"a"},{"quote":{"node":"msg_1","position":7,"field":"did","from":1,"to":2,"form":"line1"}}]"#
    );
}

/// A quoted node marked wrong has the since line after the rendering, once however often it is quoted; one that
/// is erased has none (its words are not shown), and one not marked has none.
#[test]
fn a_marked_quote_has_the_since_line_once() {
    let (_d, s) = store();
    let wrong = Node::user("ses_weir", Some("t0"), "cli", WORDS);
    let gone = Node::user("ses_weir", Some("t1"), "cli", "the weir was rebuilt");
    let fine = Node::user("ses_weir", Some("t2"), "cli", "the tide turns at four");
    let (pw, pg, pf) = (put(&s, &wrong), put(&s, &gone), put(&s, &fine));
    let f = crate::fact::retract::Retracted {
        node_id: &wrong.id,
        session_id: "ses_weir",
        gist: "the heron nests by the old weir",
        why: "it nests by the mill race",
        who: "owner",
        via: "cli",
        at_ms: AT_MS,
        exposures: &[],
        partial: false,
        card: false,
        private: true,
        labeled: &[],
    };
    let row = crate::fact::row(&f, Some("ses_weir"), None).unwrap();
    s.append(&[row.scoped(crate::retract::SCOPE)]).unwrap();
    put(&s, &gone.erased(AT_MS, "a test"));
    let parts = vec![
        quote(&wrong.id, pw, Field::Text, (0, 8), Form::Verbatim),
        Part::Code(" / ".into()),
        quote(&gone.id, pg, Field::Text, (0, 3), Form::Verbatim),
        Part::Code(" / ".into()),
        quote(&fine.id, pf, Field::Text, (0, 8), Form::Verbatim),
        Part::Code(" / ".into()),
        quote(&wrong.id, pw, Field::Text, (9, 14), Form::Verbatim),
    ];
    let marked = BTreeSet::from([wrong.id.clone(), gone.id]);
    let text = render(&s, &parts, &Since::new(&s, &marked)).unwrap();
    let day = crate::judge::spend::local_day(AT_MS);
    assert_eq!(
        text,
        format!(
            "the grey / {ERASED_QUOTE} / the tide / heron\n\
             [Since this was read: \u{201c}the heron nests by the old weir\u{201d} was marked wrong on {day}: \
             it nests by the mill race. The words above are as they were said.]"
        )
    );
    // Nothing marked: the rendering alone.
    assert_eq!(
        laid(&s, &parts),
        format!("the grey / {ERASED_QUOTE} / the tide / heron")
    );
}
