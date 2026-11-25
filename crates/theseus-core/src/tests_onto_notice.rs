//! What the owner reads about the map of people, places and things
//! (theseus-nzwa5): the notice that an entry was added and the question for
//! one Theseus is not sure of, in plain words, and a message Theseus wrote to
//! itself, which is never filed.

use theseus_judge::fake::FakeJev;

use crate::judge::onto::words::{self, Asked};
use crate::owner_note::is_headed;
use crate::tests_judge::kinds;
use crate::tests_onto::{bound, cards, rig, said};

const WORDS_NEVER_SAID: [&str; 6] = ["Jev", "tagged", "filed", "categor", "resource", "kind:"];

fn plain(text: &str) {
    assert!(is_headed(text), "{text}");
    for w in WORDS_NEVER_SAID {
        assert!(!text.contains(w), "{w:?} in {text}");
    }
}

/// The owner's wording, to the letter: Theseus acts, the conversation is
/// named, the kind is a folder here, the whole name is shown, and the undo is
/// kept.
#[test]
fn the_notice_of_a_new_entry_is_the_agreed_words() {
    let got = words::minted_text(
        Some("Harbor planning"),
        "resource",
        &["harbor-bench".into()],
        0.56,
        "jdg_9",
    );
    assert_eq!(
        got,
        "ℹ️ FYI · Theseus started keeping track of something new\n\
         In \"Harbor planning\", this folder came up and Theseus had no record of it:\n\
         harbor-bench\n\
         It added the folder to the things it knows about. Conversations where it comes up \
         will now share memory more readily.\n\
         How sure it was: 56%\n\
         Undo: `theseus ontology undo jdg_9`"
    );
    plain(&got);
}

/// The kind is said as a person would, never as `kind: name`, and a name is
/// shown whole, its parents first.
#[test]
fn the_kind_is_a_plain_word_and_the_name_is_whole() {
    let said = |kind: &str, path: &[&str]| {
        let path: Vec<String> = path.iter().map(|p| p.to_string()).collect();
        let text = words::minted_text(Some("Notes"), kind, &path, 0.9, "jdg_1");
        plain(&text);
        text
    };
    for (kind, name, word) in [
        ("topic", "tides", "topic"),
        ("resource", "https://example.org/a/b", "link"),
        ("resource", "db1.acme-labs.io", "website"),
        ("resource", "~/notes/plan.md", "file"),
        ("resource", "~/projects/harbor-bench", "folder"),
        ("capability", "tide tables", "skill"),
    ] {
        let t = said(kind, &[name]);
        assert!(t.contains(&format!("this {word} came up")), "{t}");
        assert!(t.contains(&format!("It added the {word} to")), "{t}");
        assert!(t.contains(&format!("\n{name}\n")), "{t}");
    }
    let t = said("topic", &["theseus", "ontology"]);
    assert!(t.contains("\ntheseus › ontology\n"), "{t}");
    // A conversation with no title is said so, and a long title is cut at an
    // ellipsis, never in the middle of a sentence's meaning.
    assert!(words::in_conversation(None).starts_with("In a conversation with no title"));
    let long = "w ".repeat(120);
    let cut = words::in_conversation(Some(&long));
    assert!(cut.ends_with("…\"") && cut.chars().count() < 100, "{cut}");
    // The limit falls inside a long word: the notice stops at the word before.
    let inside = format!("{}extraordinarily long", "a ".repeat(37));
    assert_eq!(
        words::in_conversation(Some(&inside)),
        format!("In \"{}…\"", "a ".repeat(37).trim_end())
    );
}

/// The question is the same words about the same thing, and says nothing was
/// written; the answer's command is kept when there is one.
#[test]
fn the_question_says_nothing_changed_and_how_sure() {
    let ask = |name: Option<&str>, known: bool, how: Option<&str>| {
        let text = words::ask_text(&Asked {
            title: Some("Harbor planning"),
            kind: "resource",
            name,
            known,
            how,
            confidence: 0.56,
        });
        assert!(text.starts_with("❓ Decision · "), "{text}");
        plain(&text);
        assert!(text.contains("How sure it was: 56%"), "{text}");
        text
    };
    let new = ask(Some("harbor-bench"), false, None);
    assert!(new.contains("this folder came up and Theseus has no record of it:\nharbor-bench"));
    assert!(new.contains("not sure enough to add it to the things it knows about, so it has not"));
    let known = ask(
        Some("harbor-bench"),
        true,
        Some("theseus ontology member ses_1 +cat_harbor"),
    );
    assert!(known.contains("already knows it:\nharbor-bench"), "{known}");
    assert!(known.contains("so nothing has changed"), "{known}");
    assert!(known.ends_with("`theseus ontology member ses_1 +cat_harbor` links it."));
    let unnamed = ask(None, false, None);
    assert!(unnamed.contains("no name for yet"), "{unnamed}");
}

/// Through a whole core: the card names the session by its title.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_card_of_a_mint_names_the_conversation_by_its_title() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    r.core
        .store
        .update_session(&sid, |s| {
            s.label = Some("Ledger migration".into());
            Ok(vec![])
        })
        .unwrap();
    said(&r, &sid, "let's plan the Lantern Ledger migration").await;
    let cards = cards(&r.core);
    let text = cards[0]["text"].as_str().unwrap();
    assert!(
        text.contains("In \"Ledger migration\", this topic came up"),
        "{text}"
    );
    assert!(text.contains("\nLantern Ledger\n"), "{text}");
    plain(text);
}

/// A message Theseus writes to itself (the self loop's instruction to its
/// builder) is never filed: the same message, from a person, is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_theseus_wrote_to_itself_is_never_filed() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    let own = bound(&r.core);
    crate::tests_route::turn_by(
        &r.core,
        Some(&own),
        "let's plan the Lantern Ledger migration",
        "self:backlog",
    )
    .await;
    let judge = r.core.runner.judge.clone();
    tokio::task::spawn_blocking(move || judge.flush_sink())
        .await
        .unwrap();
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.member_list(&own, "topic").is_none(), "nothing was filed");
    assert!(cards(&r.core).is_empty(), "nothing was said");
    assert!(kinds(&r.core.store, "onto.filed").is_empty());

    let person = bound(&r.core);
    said(&r, &person, "let's plan the Lantern Ledger migration").await;
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.member_list(&person, "topic").is_some(), "a person's is");
}

/// A long title is cut before a known value it would split, even where the
/// value follows the title's first word with no space between: the notice
/// holds the value whole or not at all. The value is invented.
#[test]
fn a_long_title_is_cut_before_a_known_value() {
    use crate::secrets::{Secret, SecretBoard};
    use crate::tests_cut_sites::{whole_or_absent, V};
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(V.into()));
    let before = "x".repeat(70);
    let said = words::in_conversation(Some(&format!("{before}{V} and after")));
    assert_eq!(said, format!("In \"{before}…\""));
    whole_or_absent("the notice's title", &said, V);
}
