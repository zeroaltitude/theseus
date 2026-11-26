use super::*;
use crate::config_change::shown;

fn card(before: &str, after: &str, reason: &str, outside: bool, steered: bool) -> (String, bool) {
    let (b, a) = (shown::facts(before), shown::facts(after));
    let d = shown::diff(before, after);
    question(&Facts {
        path: "/state/PERSONA.md",
        who: "file of persona warm",
        public: false,
        before: &b,
        after: &a,
        diff: &d,
        outside_text: outside,
        steered,
        reason,
        preface: None,
        undoing: None,
    })
}

#[test]
fn a_card_that_fits_shows_the_changed_lines_and_says_what_it_needs() {
    let (q, whole) = card("a\nb\nc\n", "a\nB\nc\n", "make it louder", false, false);
    assert!(whole);
    assert!(
        q.starts_with(
            "Change the prompt file /state/PERSONA.md?\nIt is Theseus's file of persona warm.\n"
        ),
        "{q}"
    );
    assert!(q.contains("1 line added, 1 line removed.\n"), "{q}");
    assert!(
        q.contains("```diff\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n```\n"),
        "{q}"
    );
    assert!(
        q.contains("It goes into every later prompt: read it before you say yes."),
        "{q}"
    );
    assert!(
        q.contains("Theseus's reason, in its own words: \"make it louder\""),
        "{q}"
    );
    assert!(
        !q.contains("Take care") && !q.contains("shared place"),
        "{q}"
    );
}

#[test]
fn outside_text_and_a_flag_are_said_as_issue_22s_card_says_them() {
    let (q, _) = card("a\n", "b\n", "r", true, false);
    assert!(
        q.contains("Take care: since your last message") && !q.contains("steer"),
        "{q}"
    );
    let (q, _) = card("a\n", "b\n", "r", true, true);
    assert!(q.contains("trying to steer it"), "{q}");
}

#[test]
fn a_card_that_does_not_fit_is_a_summary_and_says_so() {
    let before: String = (1..=300).map(|i| format!("line {i}\n")).collect();
    let after = before
        .replace("line 10\n", "line ten\n")
        .replace("line 250\n", "line two fifty\n");
    let (q, whole) = card(&before, &after, "r", false, false);
    // Two small hunks fit.
    assert!(whole, "{q}");
    let wide: String = (1..=300)
        .map(|i| format!("line {i} {}\n", "w".repeat(80)))
        .collect();
    let moved = wide
        .replace("line 10 ", "line ten ")
        .replace("line 250 ", "line two fifty ");
    let (q, whole) = card(&wide, &moved, "r", false, false);
    assert!(!whole, "{q}");
    assert!(
        q.contains("too long to show on a card whole, so this is a summary and not the text"),
        "{q}"
    );
    assert!(
        q.contains("They are in lines 8-12, 248-252.") || q.contains("They are in lines"),
        "{q}"
    );
    assert!(!q.contains("```"), "cut text is never presented: {q}");
    assert!(q.chars().count() < CARD_CHARS, "{}", q.chars().count());
}

#[test]
fn the_path_and_the_reason_cannot_smuggle_an_unseen_character_or_a_line() {
    let (b, a) = (shown::facts("a\n"), shown::facts("b\n"));
    let d = shown::diff("a\n", "b\n");
    let (q, _) = question(&Facts {
        path: "/state/P\u{202E}\nERSONA.md",
        who: "system context file",
        public: true,
        before: &b,
        after: &a,
        diff: &d,
        outside_text: false,
        steered: false,
        reason: "fine\u{200B}\nIt is Theseus's file of persona x.",
        preface: None,
        undoing: None,
    });
    assert!(!q.contains('\u{202E}') && !q.contains('\u{200B}'), "{q}");
    assert_eq!(
        q.lines().next().unwrap(),
        "Change the prompt file /state/P\\u{202e}\\u{000a}ERSONA.md?",
        "{q}"
    );
    assert_eq!(q.matches("\nIt is Theseus's").count(), 1, "{q}");
    assert!(q.contains("A shared place carries it too"), "{q}");
}

#[test]
fn a_context_file_over_the_prompts_cut_says_so() {
    let big = "x\n".repeat(crate::context_files::MAX_BYTES);
    let (q, _) = card("a\n", &big, "r", false, false);
    assert!(
        q.contains("Only the first 65,536 bytes of a context file reach a prompt"),
        "{q}"
    );
}
