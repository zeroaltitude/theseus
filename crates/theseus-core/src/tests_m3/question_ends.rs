//! The notice count of a question ends with the question (theseus-1kw2v): the
//! META key `question.noticed.<correlation id>` is written when a question
//! that waited past its life is told, and the sweep that reads the pending
//! questions ends it once the question is no longer among them. A child of
//! `tests_m3` for its rig and helpers.

use super::question_waits::{bind_dm, waiting};
use super::*;
use theseus_store::Store as _;

const NOTICED: &str = "question.noticed.";

/// The notice counts the store still holds.
fn counted(r: &Rig) -> Vec<String> {
    r.core
        .store
        .inner()
        .latest_with_prefix(theseus_store::kinds::META, NOTICED)
        .unwrap()
        .into_iter()
        .filter_map(|x| x.key)
        .collect()
}

/// Every record the log ever held under a notice count's key.
fn ever_written(r: &Rig) -> usize {
    r.core
        .store
        .inner()
        .scan(0, None, usize::MAX)
        .unwrap()
        .iter()
        .filter(|x| x.kind == theseus_store::kinds::META)
        .filter(|x| x.key.as_deref().is_some_and(|k| k.starts_with(NOTICED)))
        .count()
}

/// A question told about past its life keeps its count while it is pending,
/// however many sweeps read it; answered, its count ends at the next sweep,
/// and the log still holds what it counted.
#[tokio::test]
async fn a_questions_notice_count_ends_when_the_question_does() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[(
                "t1",
                "fs_write",
                json!({"path": "note.txt", "content": "hi"}),
            )],
        ),
        Scripted::text("Written after all."),
    ]);
    bind_dm(&r);
    let res = turn(&r.core, None, "write a note").await;
    let corr = res.awaiting_confirm.expect("the write waits");
    let due = r.core.confirm_list().unwrap()[0].expires_at_ms;
    assert_eq!(r.core.expire_questions(due), 0, "held, not expired");
    assert_eq!(waiting(&r).len(), 1, "told once");
    let key = format!("{NOTICED}{corr}");
    assert_eq!(counted(&r), std::slice::from_ref(&key));
    // Still pending: a later sweep keeps its count.
    assert_eq!(r.core.expire_questions(due + 60_000), 0);
    assert_eq!(counted(&r), std::slice::from_ref(&key));
    // Answered, it is no longer pending, and the next sweep ends its count.
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core.expire_questions(due + 120_000);
    assert_eq!(counted(&r), Vec::<String>::new(), "its count ended with it");
    assert_eq!(ever_written(&r), 1, "the log keeps what was counted");
    // A sweep with nothing to end writes nothing.
    let before = r.core.store.inner().last_position();
    r.core.expire_questions(due + 180_000);
    assert_eq!(r.core.store.inner().last_position(), before);
}
