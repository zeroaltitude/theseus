use super::*;

fn plain(text: &str, width: usize) -> Vec<String> {
    Markdown::render(text, width, Mode::Plain)
        .iter()
        .map(StyledLine::plain)
        .collect()
}

fn ansi(text: &str, width: usize) -> Vec<String> {
    Markdown::render(text, width, Mode::Ansi)
        .iter()
        .map(StyledLine::ansi)
        .collect()
}

#[test]
fn a_heading_is_bold_and_underlined_and_a_paragraph_wraps_under_it() {
    let l = ansi("# Port\n\nsome words here", 12);
    assert_eq!(l[0], "\x1b[1;4mPort\x1b[0m");
    assert_eq!(l[1], "");
    assert_eq!(l[2], "some words");
    assert_eq!(l[3], "here");
    assert_eq!(plain("## Two", 40), ["## Two"]);
}

#[test]
fn lists_nest_by_their_indentation_and_wrap_under_their_text() {
    let l = plain("- one\n  - two is a longer item\n    1. deep\n- three", 16);
    assert_eq!(
        l,
        [
            "• one",
            "  ◦ two is a",
            "    longer item",
            "    1. deep",
            "• three"
        ]
    );
    let n = plain("9. nine\n10. ten\n    more", 30);
    assert_eq!(n, ["9. nine", "10. ten", "    more"]);
    assert_eq!(plain("- [x] done\n- [ ] open", 30), ["☑ done", "☐ open"]);
    assert_eq!(plain("-\n- a", 30), ["•", "• a"]);
}

#[test]
fn an_item_holds_paragraphs_and_a_fence() {
    let l = plain(
        "1. step\n\n   second para\n2. run:\n   ```sh\n   ls\n   ```\n3. end",
        40,
    );
    assert_eq!(
        l,
        [
            "1. step",
            "",
            "   second para",
            "2. run:",
            "     ls",
            "3. end"
        ]
    );
}

#[test]
fn a_quote_is_drawn_behind_a_bar_and_a_rule_is_a_line() {
    assert_eq!(
        plain("> quoted words that wrap\n> > inner\n\n---", 14),
        [
            "> quoted words",
            "> that wrap",
            ">",
            "> > inner",
            "",
            "──────────────"
        ]
    );
    assert_eq!(ansi("> q", 20)[0], "\x1b[90m▎ \x1b[0mq");
}

#[test]
fn a_fence_is_indented_highlighted_and_folded_behind_a_marker() {
    let l = ansi("```rust\nlet x = 1;\n```", 40);
    assert_eq!(l[0], "  \x1b[35mlet\x1b[0m x = \x1b[33m1\x1b[0m;");
    let wide = plain("```\nabcdefghijklmnopqrstuvwxyz\n```", 12);
    assert_eq!(wide, ["  abcdefghij", "  ↪ klmnopqr", "  ↪ stuvwxyz"]);
    // An unknown language and a missing one print plain.
    assert_eq!(ansi("```nolang\nfn x\n```", 40), ["  fn x"]);
    assert_eq!(plain("```\n\n  a\n```", 40), ["", "    a"]);
}

#[test]
fn blocks_are_a_blank_line_apart_and_a_reply_ends_without_one() {
    let l = plain("para\n\n```\ncode\n```\n\n| a |\n|---|\n| 1 |\n", 40);
    assert_eq!(l[0], "para");
    assert_eq!(l[1], "");
    assert_eq!(l[2], "  code");
    assert_eq!(l[3], "");
    assert!(l[4].starts_with('┌'));
    assert!(!l.last().unwrap().is_empty());
}

#[test]
fn finishing_starts_a_reply_over() {
    let mut md = Markdown::new(30, Mode::Plain);
    assert_eq!(md.push("one\n\n").len(), 1);
    assert_eq!(md.finish().len(), 0);
    // The next reply is a first block: no blank line before it.
    let mut out = md.push("two\n");
    out.extend(md.finish());
    assert_eq!(
        out.iter().map(StyledLine::plain).collect::<Vec<_>>(),
        ["two"]
    );
    // A line still arriving when the reply ends is a line.
    let mut md = Markdown::new(30, Mode::Plain);
    assert!(md.push("no newline at the end").is_empty());
    assert_eq!(md.finish()[0].plain(), "no newline at the end");
}

#[test]
fn carriage_returns_and_tabs_are_read_as_line_ends_and_indents() {
    assert_eq!(
        plain("a\r\nb\r\n\r\n- x\r\n\t- y", 30),
        ["a", "b", "", "• x", "  ◦ y"]
    );
}

#[test]
fn nesting_a_hundred_deep_ends_and_keeps_the_text() {
    let deep = format!("{}x{}", "**_".repeat(60), "_**".repeat(60));
    assert!(plain(&deep, 80).join("").contains('x'));
    let list: String = (0..60)
        .map(|i| format!("{}- l{i}\n", "  ".repeat(i)))
        .collect();
    assert!(plain(&list, 200).len() >= 60);
}
