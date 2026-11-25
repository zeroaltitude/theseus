//! A filing's Undo (issue 57, `Undo::SelfDrop` through `told.undo`) drops the
//! item only while it is still as it was filed: open, never started, no
//! claim, no session, and only the filing's own evidence. Started, or failed
//! and retried by the owner, it has moved on: the Undo refuses, names the
//! ordinary command, and changes nothing.

use theseus_protocol::rsi::SelfFileParams;
use theseus_protocol::tasks::TaskState;

use super::tests::{rig_with, Rig};
use crate::approval::Answerer;
use crate::owner_note::told::{self, Undo};

fn owner() -> Answerer {
    Answerer::from("the CLI")
}

/// A finding filed from the owner's terminal, and its told note: the item's
/// id and the note's post id.
fn filed(r: &Rig) -> (String, String) {
    let p = SelfFileParams {
        finding: "The tide table reads in feet; it should read in metres.".into(),
        ..Default::default()
    };
    let item = r.core.self_file(&p, &owner()).unwrap().item;
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus filed a finding",
        "In ses_0000aa1b, without asking you",
        "",
        &Undo::SelfDrop(item.clone()),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    (item, a.correlation_id)
}

fn version(r: &Rig, item: &str) -> u64 {
    crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap()
        .version
}

fn drops(r: &Rig, item: &str) -> usize {
    let t = crate::task_graph::get(&r.core.store, item).unwrap().unwrap();
    t.evidence
        .iter()
        .filter(|e| e.identity.starts_with("dropped:"))
        .count()
}

/// The refusal a moved-on item gets: fixed words naming the ordinary
/// command, nothing changed, and the note's Undo line gone for that line.
async fn refused_as_moved(r: &Rig, item: &str, post: &str) {
    let before = version(r, item);
    let e = r.core.told_undo(post, &owner()).await.unwrap_err();
    assert_eq!(e.data["moved_on"], true, "{e:?}");
    assert!(e.message.contains("has moved on since it was filed"), "{e:?}");
    assert!(
        e.message
            .contains(&format!("`theseus self drop {item}`")),
        "{e:?}"
    );
    let text = e.data["text"].as_str().unwrap();
    assert!(!text.contains("Undo: `"), "{text}");
    assert!(text.ends_with(&told::moved_line(&format!("theseus self drop {item}"))));
    assert_eq!(version(r, item), before, "the item is untouched");
    assert_eq!(drops(r, item), 0);
    let again = r.core.told_undo(post, &owner()).await.unwrap_err();
    assert_eq!(again.data["ran"], false, "{again:?}");
    assert_eq!(version(r, item), before);
}

/// Filed, then Undo: dropped, once.
#[tokio::test]
async fn a_filing_still_as_filed_is_dropped_once() {
    let r = rig_with(1, 30.0);
    let (item, post) = filed(&r);
    let done = r.core.told_undo(&post, &owner()).await.unwrap();
    assert!(done.text.ends_with("↩️ Undone by the CLI."), "{}", done.text);
    assert_eq!(r.state(&item), TaskState::Abandoned);
    assert_eq!(drops(&r, &item), 1);
    let again = r.core.told_undo(&post, &owner()).await.unwrap_err();
    assert_eq!(again.data["undone_by"], "the CLI");
    assert_eq!(drops(&r, &item), 1, "dropped once");
}

/// Filed, started by the pipeline, then Undo: refused, the build untouched.
#[tokio::test]
async fn a_started_filing_refuses_its_undo_and_is_untouched() {
    let r = rig_with(1, 30.0);
    let (item, post) = filed(&r);
    assert!(r.core.backlog_start_item(&item).unwrap().is_some(), "it started");
    assert_eq!(r.state(&item), TaskState::InProgress);
    refused_as_moved(&r, &item, &post).await;
    assert_eq!(r.state(&item), TaskState::InProgress);
}

/// Filed, failed, retried by the owner (back to proposed), then Undo:
/// refused, the retried item untouched.
#[tokio::test]
async fn a_failed_and_retried_filing_refuses_its_undo() {
    let r = rig_with(1, 30.0);
    let (item, post) = filed(&r);
    r.core
        .backlog_mint_settle(
            &item,
            crate::rsi::backlog::Settled::Failed("a tier failed".into()),
        )
        .unwrap();
    assert_eq!(r.state(&item), TaskState::Failed);
    let p = theseus_protocol::rsi::SelfRetryParams {
        item: item.clone(),
        ..Default::default()
    };
    r.core.self_retry(&p, &owner()).unwrap();
    assert_eq!(r.state(&item), TaskState::Proposed, "retried");
    refused_as_moved(&r, &item, &post).await;
    assert_eq!(r.state(&item), TaskState::Proposed);
}
