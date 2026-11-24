//! `undo.v2` (issue 46; finding 08-judge-13): `undo.v1` with its advice for overwriting a file corrected so the
//! original is kept, and the rule no code fed taken out. Every option id, bar, `no_match`, the state, the
//! action and the description are v1's; v1 is retired, its file kept as it was.

use crate::learn::RollbackRule;
use crate::pack::by_name;

/// The four places undo.v1 named the write-beside shape, and what undo.v2 says instead: the shape's own
/// sentence, the `irreversible_reshapeable` option, `reversible`'s step two, and `shape`'s instructions.
const WORDS: [(&str, &str); 4] = [
    (
        "Write the new content to a new name beside the target and rename it into place, so the old bytes are there until the moment of the rename and no reader ever sees a half-written file.",
        "Write the new content to a new name beside the target, move the original to a kept name beside it, then rename the new file into place, so the old bytes survive the rename and no reader ever sees a half-written file. Report the kept name; remove it later on the operator's word.",
    ),
    (
        "moving the targets aside instead of removing them, writing beside a file and renaming into place, copying",
        "moving the targets aside instead of removing them, writing beside a file and renaming it into place with the original kept under another name, copying",
    ),
    (
        "Then ask whether moving the targets aside, writing beside a file and renaming into place, copying or exporting first,",
        "Then ask whether moving the targets aside, writing beside a file and renaming it into place with the original kept under another name, copying or exporting first,",
    ),
    (
        "a file whose current bytes nothing else holds can be written beside and renamed, and",
        "a file whose current bytes nothing else holds can be written beside, its original kept under another name, and the new one renamed into place, and",
    ),
];

fn swap(text: &str) -> (String, usize) {
    let mut out = text.to_string();
    let mut n = 0;
    for (old, new) in WORDS {
        if out.contains(old) {
            assert_eq!(out.matches(old).count(), 1, "{old}");
            out = out.replace(old, new);
            n += 1;
        }
    }
    (out, n)
}

/// v2 is v1 parsed with exactly [`WORDS`] changed and `on_path_p95` gone; everything else is equal.
#[test]
fn undo_v2_is_undo_v1_with_the_kept_copy_advice_and_without_the_unfed_rule() {
    let v1 = by_name("undo.v1")
        .or_else(|| crate::pack::retired("undo.v1"))
        .unwrap();
    let v2 = by_name("undo.v2").expect("undo.v2 is embedded");
    assert_eq!((v1.version, v2.version), (1, 2));
    let mut planted = (*v1).clone();
    planted.version = 2;
    planted.sha256 = v2.sha256.clone();
    planted
        .rollback
        .retain(|r| !matches!(r, RollbackRule::OnPathP95 { .. }));
    assert_eq!(planted.rollback.len(), 1, "labels_per_day stays");
    let mut swapped = 0;
    for q in planted.questions.iter_mut() {
        let (text, n) = swap(&q.instructions);
        q.instructions = text;
        swapped += n;
        for o in q.options.iter_mut() {
            let (text, n) = swap(o.means.as_deref().unwrap());
            o.means = Some(text);
            swapped += n;
        }
    }
    assert_eq!(swapped, WORDS.len(), "each change is made once");
    assert_eq!(planted, *v2);
}
