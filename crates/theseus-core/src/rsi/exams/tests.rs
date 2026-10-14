//! Self-exams, pure of the core (theseus-pw1q.11): the split, the scrub, the
//! proposer rule, the review's gate on counting, the cap, and a sealed item's
//! bytes. The core's seeded day is `rpc/tests_self_exams.rs`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use futures_util::future::BoxFuture;

use super::pass::{run, Deps, Fate};
use super::*;

fn raw(family: Family, n: u32) -> Raw {
    Raw {
        family,
        source_rows: vec![format!("ledger:{n}")],
        proposer: None,
        task: format!("Where should message number {n} run?"),
        check: format!("reply has word \"profile{n}\""),
        note: "a note".into(),
        at_ms: u64::from(n),
    }
}

fn fair(reviewer: &str) -> Verdict {
    Verdict {
        tests_behaviour: true,
        answer_right: true,
        not_gamed: true,
        reason: "fine".into(),
        reviewer: reviewer.into(),
    }
}

/// A review as a function.
type ReviewFn = Box<dyn Fn(&Item) -> Result<Verdict> + Send + Sync>;

/// A reviewer from a function.
struct Stub(ReviewFn);

impl Reviewer for Stub {
    fn review<'a>(&'a self, item: &'a Item) -> BoxFuture<'a, Result<Verdict>> {
        Box::pin(async move { (self.0)(item) })
    }
}

/// A sealed store in memory: what it was handed.
#[derive(Default)]
struct Vault(Mutex<Vec<(String, String, Vec<u8>)>>);

impl SealedSink for Vault {
    fn put(&self, set: &str, family: &str, bytes: &[u8]) -> Result<ItemId> {
        let mut v = self.0.lock().unwrap();
        v.push((set.into(), family.into(), bytes.to_vec()));
        Ok(ItemId(format!("sealed-{}", v.len())))
    }
}

fn plain(s: &str) -> String {
    s.to_string()
}

struct Rig {
    dir: tempfile::TempDir,
    shelf: Shelf,
    key: SplitKey,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    Rig {
        shelf: Shelf::new(dir.path()),
        dir,
        key: SplitKey([7; 32]),
    }
}

async fn pass(
    r: &Rig,
    reviewer: &dyn Reviewer,
    sink: Option<&dyn SealedSink>,
    room: u32,
    raws: Vec<Raw>,
) -> pass::PassReport {
    let secrets = |s: &str| plain(s);
    let scrub = Scrub {
        secrets: &secrets,
        names: &[],
    };
    let seen = HashSet::new();
    let (allowed, record) = (|| true, |_: &pass::Outcome| {});
    let deps = Deps {
        reviewer,
        sink,
        shelf: &r.shelf,
        key: &r.key,
        scrub: &scrub,
        seen: &seen,
        room,
        set: "self-exams-test",
        allowed: &allowed,
        record: &record,
    };
    run(&deps, raws).await
}

fn files(dir: &Path) -> Vec<(std::path::PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let body = std::fs::read_to_string(&p).unwrap_or_default();
                out.push((p, body));
            }
        }
    }
    out
}

/// The split is a function of the key and the item: stable run to run,
/// about half and half, and another key splits another way.
#[test]
fn the_split_is_stable_balanced_and_keyed() {
    let (a, b) = (SplitKey([1; 32]), SplitKey([2; 32]));
    let ids: Vec<ItemId> = (0..400)
        .map(|n| id_of(Family::Correction, &format!("t{n}"), "c"))
        .collect();
    let first: Vec<Half> = ids
        .iter()
        .map(|i| half(&a, Family::Correction, i))
        .collect();
    let again: Vec<Half> = ids
        .iter()
        .map(|i| half(&a, Family::Correction, i))
        .collect();
    assert_eq!(first, again, "the same key splits the same way");
    let open = first.iter().filter(|h| **h == Half::Open).count();
    assert!(
        (140..=260).contains(&open),
        "about half open: {open} of 400"
    );
    let other: Vec<Half> = ids
        .iter()
        .map(|i| half(&b, Family::Correction, i))
        .collect();
    assert_ne!(first, other, "another key, another split");
    assert_eq!(SplitKey::from_hex(&a.hex()), Some(a));
}

/// An item's text is scrubbed of secrets, handles, emails and the names it is
/// given, and its id is a function of what it says.
#[test]
fn an_item_never_quotes_a_secret_or_a_name() {
    let secrets = |s: &str| s.replace("sk-test-ONLYFORTESTS", "[secret]");
    let names = vec!["Heron".to_string()];
    let scrub = Scrub {
        secrets: &secrets,
        names: &names,
    };
    let mut r = raw(Family::Correction, 1);
    r.task = "Heron said use sk-test-ONLYFORTESTS, ask @wren or discord:12345 or heron@example.org, not Heronry".into();
    r.note = "from person:wren".into();
    let item = Item::scrubbed(&r, &scrub);
    for gone in [
        "sk-test-ONLYFORTESTS",
        "Heron said",
        "@wren",
        "12345",
        "example.org",
        "wren",
    ] {
        assert!(
            !item.task.contains(gone) && !item.note.contains(gone),
            "{gone} in {item:?}"
        );
    }
    assert!(
        item.task.contains("Heronry"),
        "a name is a whole word: {}",
        item.task
    );
    assert!(item.task.contains("[secret]") && item.task.contains("[person]"));
    assert_eq!(item.id, Item::scrubbed(&r, &scrub).id);
    assert_eq!(item.id, id_of(item.family, &item.task, &item.check));
}

/// An item counts only after its review: a reviewer that does not answer, or
/// that says it is unfair, leaves the shelf empty and the count at zero.
#[tokio::test]
async fn an_unreviewed_item_does_not_count() {
    let r = rig();
    let down = Stub(Box::new(|_| {
        Err(anyhow::anyhow!("the reviewer is unreachable"))
    }));
    let raws: Vec<Raw> = (0..6).map(|n| raw(Family::FailedTask, n)).collect();
    let rep = pass(&r, &down, Some(&Vault::default()), 10, raws.clone()).await;
    assert_eq!(rep.counted(), 0);
    assert_eq!(rep.unreviewed(), 6);
    assert!(r.shelf.items().is_empty(), "nothing is written unreviewed");
    assert!(files(r.dir.path()).is_empty());

    let strict = Stub(Box::new(|_| {
        Ok(Verdict {
            not_gamed: false,
            reason: "any reply naming the word passes".into(),
            ..fair("opus")
        })
    }));
    let rep = pass(&r, &strict, Some(&Vault::default()), 10, raws.clone()).await;
    assert_eq!((rep.counted(), rep.rejected()), (0, 6));
    assert!(r.shelf.items().is_empty());
    let Fate::Rejected(why) = &rep.outcomes.iter().find(|o| !o.sealed).unwrap().fate else {
        panic!("an open item is rejected with its reason");
    };
    assert!(why.contains("trivially gamed") && why.contains("any reply naming the word passes"));

    let kind = Stub(Box::new(|_| Ok(fair("opus"))));
    let rep = pass(&r, &kind, Some(&Vault::default()), 10, raws).await;
    assert_eq!(rep.counted(), 6, "a fair review counts");
    assert_eq!(
        r.shelf.items().len(),
        rep.outcomes.iter().filter(|o| !o.sealed).count()
    );
}

/// A sealed item's bytes go to the sink alone: not the open dir, not an
/// outcome, and not a rejection's reason, whatever the reviewer quoted.
#[tokio::test]
async fn a_sealed_items_bytes_never_reach_the_open_dir_or_a_row() {
    let r = rig();
    let vault = Vault::default();
    let raws: Vec<Raw> = (0..40)
        .map(|n| Raw {
            task: format!("Where should the marmalade{n} message run?"),
            ..raw(Family::Correction, n)
        })
        .collect();
    let counting = Stub(Box::new(|_| Ok(fair("opus"))));
    let rep = pass(&r, &counting, Some(&vault), 100, raws.clone()).await;
    let sealed: Vec<_> = rep.outcomes.iter().filter(|o| o.sealed).collect();
    assert!(
        !sealed.is_empty() && sealed.len() < 40,
        "both halves are used"
    );
    assert_eq!(vault.0.lock().unwrap().len(), sealed.len());
    assert!(
        sealed.iter().all(|o| o.task.is_none()),
        "a sealed outcome names no text"
    );
    let open_text: String = files(r.dir.path()).into_iter().map(|(_, b)| b).collect();
    for (set, family, bytes) in vault.0.lock().unwrap().iter() {
        assert_eq!(
            (set.as_str(), family.as_str()),
            ("self-exams-test", "self_correction")
        );
        let item: Item = serde_json::from_slice(bytes).unwrap();
        assert!(
            !open_text.contains(&item.task),
            "sealed text in the open dir"
        );
        assert!(
            !format!("{rep:?}").contains(&item.task),
            "sealed text in the report"
        );
    }

    // A rejection of a sealed item gives the failed questions, not the
    // reviewer's words, which may quote the item.
    let quoting = Stub(Box::new(|i| {
        Ok(Verdict {
            answer_right: false,
            reason: format!("the answer to {:?} is wrong", i.task),
            ..fair("opus")
        })
    }));
    let rep = pass(&rig(), &quoting, Some(&vault), 100, raws).await;
    for o in &rep.outcomes {
        let Fate::Rejected(why) = &o.fate else {
            panic!("{o:?}")
        };
        assert_eq!(why.contains("marmalade"), !o.sealed, "{o:?}");
    }
}

/// With no sealed store, a sealed item is held and dropped with a count; it
/// costs no review and reaches no file.
#[tokio::test]
async fn a_sealed_item_without_a_store_is_dropped_and_counted() {
    let r = rig();
    let reviews = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = reviews.clone();
    let counted_reviews = Stub(Box::new(move |_| {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(fair("opus"))
    }));
    let raws: Vec<Raw> = (0..40).map(|n| raw(Family::OverBudget, n)).collect();
    let rep = pass(&r, &counted_reviews, None, 100, raws).await;
    let (open, sealed) = (
        rep.outcomes.iter().filter(|o| !o.sealed).count() as u64,
        rep.sealed_dropped(),
    );
    assert!(open > 0 && sealed > 0 && open + sealed == 40);
    assert_eq!(
        reviews.load(std::sync::atomic::Ordering::SeqCst),
        open as u32,
        "a dropped item is not reviewed"
    );
    assert_eq!(r.shelf.items().len() as u64, open);
}

/// The cap bounds the reviews a day: the rest wait, counted as capped.
#[tokio::test]
async fn the_day_cap_bounds_the_reviews() {
    let r = rig();
    let reviewer = Stub(Box::new(|_| Ok(fair("opus"))));
    let raws: Vec<Raw> = (0..30).map(|n| raw(Family::WrongLabel, n)).collect();
    let rep = pass(&r, &reviewer, Some(&Vault::default()), 3, raws).await;
    let reviewed = rep.counted() + rep.rejected() + rep.unreviewed();
    assert_eq!(reviewed, 3);
    assert_eq!(rep.capped, 30 - 3);
    assert_eq!(
        pass(&r, &reviewer, None, 0, vec![raw(Family::WrongLabel, 99)])
            .await
            .counted(),
        0
    );
}

/// A closed gate stops the pass before its next review.
#[tokio::test]
async fn a_closed_gate_stops_the_pass_between_reviews() {
    let r = rig();
    let calls = std::sync::atomic::AtomicU32::new(0);
    let reviewer = Stub(Box::new(|_| Ok(fair("opus"))));
    let secrets = |s: &str| plain(s);
    let scrub = Scrub {
        secrets: &secrets,
        names: &[],
    };
    let seen = HashSet::new();
    let allowed = || calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2;
    let record = |_: &pass::Outcome| {};
    let deps = Deps {
        reviewer: &reviewer,
        sink: Some(&Vault::default()),
        shelf: &r.shelf,
        key: &r.key,
        scrub: &scrub,
        seen: &seen,
        room: 50,
        set: "s",
        allowed: &allowed,
        record: &record,
    };
    let rep = run(&deps, (0..20).map(|n| raw(Family::Correction, n)).collect()).await;
    assert!(rep.stopped);
    assert_eq!(
        rep.counted() + rep.rejected(),
        2,
        "two reviews, then the gate said stop"
    );
}

/// The proposer rule: a self branch adds, changes and deletes nothing, and
/// the paths are named.
#[test]
fn a_self_branchs_edit_to_an_item_is_refused() {
    let r = rig();
    let item = Item::scrubbed(
        &raw(Family::Correction, 5),
        &Scrub {
            secrets: &|s| plain(s),
            names: &[],
        },
    );
    let branch = Writer::of_principal("self", "self/tidy-routes");
    assert_eq!(branch, Writer::SelfBranch("self/tidy-routes".into()));
    let e = r.shelf.put(&branch, &item).unwrap_err().to_string();
    assert!(
        e.contains("self/tidy-routes") && e.contains("never edits the exam"),
        "{e}"
    );
    assert!(!r.shelf.has(item.family, &item.id), "nothing was written");
    r.shelf.put(&Writer::Pass, &item).unwrap();
    assert!(r.shelf.has(item.family, &item.id));
    assert!(r
        .shelf
        .delete(&branch, item.family, &item.id)
        .unwrap_err()
        .to_string()
        .contains("may not"));
    assert!(
        r.shelf
            .delete(&Writer::Pass, item.family, &item.id)
            .is_err(),
        "nobody deletes"
    );
    let mut changed = item.clone();
    changed.check = "reply has \"anything\"".into();
    assert!(
        r.shelf.put(&Writer::Pass, &changed).is_err(),
        "an item is never changed"
    );
    assert!(
        r.shelf.put(&Writer::Pass, &item).is_ok(),
        "the same bytes again are fine"
    );
    assert_eq!(Writer::of_principal("operator", "x"), Writer::Pass);
    for p in [
        "self/exams/open/self_correction/ab.json",
        "/home/me/.theseus/self/exams/open/x.json",
        "crates/theseus-exam/exam/exam-v2.toml",
        "./crates/theseus-exam/exam/exam-v3.toml",
    ] {
        assert!(touches_exams(p), "{p}");
    }
    for p in [
        "crates/theseus-core/src/rsi/exams.rs",
        "docs/self/examples.md",
    ] {
        assert!(!touches_exams(p), "{p}");
    }
}
