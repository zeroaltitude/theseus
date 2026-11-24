//! The bytes behind each head of a one-line rewrite.

use super::sample::Draw;
use super::*;

/// For every head of a text's line, at every character boundary and past its end, [`source`] gives bytes of
/// the text, on character boundaries, whose own line is exactly that head.
#[test]
fn a_source_is_the_bytes_of_each_head_of_its_line() {
    let swap = |c: char| if c == '\u{ab}' { '\u{2039}' } else { c };
    let mut draw = Draw::new(0x9e37_79b9_7f4a_7c15);
    for _ in 0..300 {
        let n = draw.below(40);
        let text = draw.text(n);
        let line = one_line(&text, swap);
        let line = line.trim();
        for p in (0..=line.len()).filter(|p| line.is_char_boundary(*p)) {
            let r = source(&text, p, swap);
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
            assert_eq!(one_line(&text[r], swap), &line[..p], "{text:?} at {p}");
        }
        assert_eq!(
            one_line(&text[source(&text, usize::MAX, swap)], swap),
            line,
            "{text:?}"
        );
    }
    assert_eq!(source(" \n ", 0, swap), 3..3);
    assert_eq!(source("", 5, swap), 0..0);
    assert_eq!(source("  ab \t cd ", 3, swap), 2..5);
    assert_eq!(source("  ab \t cd ", 2, swap), 2..4);
    assert_eq!(source("  ab \t cd ", 9, swap), 2..9);
}
