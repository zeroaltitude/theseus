//! A retitle made before the mark existed (`task.title_by:<id>`) is read from the ledger (issue 28): the
//! newest `task.updated` or `task.change_accepted` row whose `fields` holds `title` names the session. The
//! stores here are built in the old shape by hand: the record retitled and its version moved, the row
//! written, and no mark.

use std::collections::BTreeSet;

use serde_json::json;
use theseus_store::{kinds, NewRecord};

use super::tests::{rig, rig_with, Rig};
use super::tests_work_global::{conversation, mark, said};
use crate::content::{Flag, Hand};
use crate::ledger::LedgerRow;
use crate::task_graph::retitle::{retitlers, ROWS_MAX, ROWS_READ};

fn restricted_rig() -> Rig {
    rig_with(|cfg, _| cfg.serve_default_from_cn())
}

fn flags(r: &Rig, session: &str) -> Option<crate::content::ContentFlags> {
    crate::content_wait::flags_of(&r.core.store, session).filter(|f| !f.labels.is_empty())
}

/// The owner's project and its milestone, from the CLI: the project's id.
fn owners(r: &Rig) -> String {
    r.core
        .project_create(
            &serde_json::from_value(json!({
                "title": "Harbour survey", "objective": "Chart every mooring",
                "acceptance": ["every mooring has a depth"],
                "milestones": [{"title": "Outer harbour", "due": "in 30d"}],
                "review": "none"
            }))
            .unwrap(),
            "the CLI",
        )
        .unwrap()
        .project
        .id
}

fn task(r: &Rig, id: &str) -> crate::task_graph::TaskRecord {
    crate::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

/// A task-graph ledger row, as `task.update` writes one: `kind`, the session, and the fields it touched.
fn row(kind: &str, task: &str, session: &str, fields: &str) -> NewRecord {
    let row = LedgerRow {
        at_unix_ms: theseus_protocol::now_unix_ms(),
        kind: kind.into(),
        session_id: Some(session.into()),
        turn_id: None,
        data: json!({"task": task, "title": "x", "version": 2, "state": "accepted", "fields": fields}),
    };
    NewRecord::json(kinds::LEDGER, None, &row).unwrap()
}

/// A retitle as it was made before the mark: the record carries the new title at the next version, the
/// row names the session, and no mark is written.
fn retitle_the_old_way(r: &Rig, id: &str, title: &str, by: &str) {
    let mut rec = task(r, id);
    rec.title = title.into();
    rec.version += 1;
    r.core
        .store
        .append(&[
            crate::task_graph::record(&rec).unwrap(),
            row("task.updated", id, by, "title"),
        ])
        .unwrap();
}

/// An owner's project that a model retitled before the mark existed is that model's session's words: held
/// from a restricted turn, its flags taken by an unrestricted one. The owner's milestone beside it is theirs.
#[tokio::test]
async fn an_old_retitle_is_the_retitling_sessions_words() {
    for restricted in [true, false] {
        let r = match restricted {
            true => restricted_rig(),
            false => rig(),
        };
        let id = owners(&r);
        let renamer = conversation(&r).await;
        retitle_the_old_way(&r, &id, "Reef survey", &renamer);
        assert!(r
            .core
            .store
            .get_meta::<String>(&crate::task_graph::title_by_key(&id))
            .unwrap()
            .is_none());
        mark(&r, &renamer);
        let here = conversation(&r).await;
        let text = said(&r, &here, json!({"what": ["projects"]})).await;
        assert!(text.contains("next Outer harbour"), "{text}");
        match restricted {
            true => {
                assert!(!text.contains("Reef survey"), "{text}");
                assert!(text.contains("held back"), "{text}");
                assert!(flags(&r, &here).is_none());
            }
            false => {
                assert!(text.contains("Reef survey"), "{text}");
                let f = flags(&r, &here).expect("the renamer's flags are taken");
                assert!(f.has(Flag::Confidential));
            }
        }
    }
}

/// The owner's project nobody retitled is untouched: given as theirs, nothing held, nothing taken; and it
/// costs no read of the ledger (its version is 1). One edited otherwise (version above 1, a row that touched
/// `deps`) finds no title row and is answered as it was before: the owner's.
#[tokio::test]
async fn an_owners_project_nobody_retitled_is_untouched() {
    let r = restricted_rig();
    let id = owners(&r);
    let renamer = conversation(&r).await;
    mark(&r, &renamer);
    // Rows of other tasks' retitles by the marked session are in the ledger: they must not matter.
    r.core
        .store
        .append(&[row("task.updated", "tsk_other", &renamer, "title")])
        .unwrap();
    ROWS_READ.with(|c| c.set(0));
    let here = conversation(&r).await;
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(
        text.contains("Harbour survey") && text.contains("next Outer harbour"),
        "{text}"
    );
    assert!(
        !text.contains("held back") && flags(&r, &here).is_none(),
        "{text}"
    );
    assert_eq!(
        ROWS_READ.with(std::cell::Cell::get),
        0,
        "a never-edited project reads no ledger"
    );
    // Edited otherwise: its version moves, a row says `deps`, and it is still the owner's.
    let mut rec = task(&r, &id);
    rec.version += 1;
    r.core
        .store
        .append(&[
            crate::task_graph::record(&rec).unwrap(),
            row("task.updated", &id, &renamer, "deps"),
        ])
        .unwrap();
    let text = said(&r, &here, json!({"what": ["projects"]})).await;
    assert!(
        text.contains("Harbour survey") && !text.contains("held back"),
        "{text}"
    );
    assert!(flags(&r, &here).is_none());
}

/// The mark beats the ledger; the newest row beats an older one; a `change_accepted` row counts; a pruned
/// row leaves the title unproven, answered as before (a model's record is that model's, an owner's the owner's).
#[test]
fn the_mark_then_the_newest_row_then_what_the_record_says() {
    let r = rig();
    let id = owners(&r);
    let mut rec = task(&r, &id);
    rec.version = 3;
    let by_model = {
        let mut m = rec.clone();
        m.origin.by_model = true;
        m.origin.session = "ses_maker".into();
        m
    };
    let hand = |rec: &crate::task_graph::TaskRecord| {
        crate::task_graph::title_hand(&r.core.store, rec).unwrap()
    };
    // No mark and no row: unproven.
    assert_eq!(hand(&rec), Hand::Owner);
    assert_eq!(hand(&by_model), Hand::Model("ses_maker".into()));
    // An older row by one session, a newer by another: the newest, and `change_accepted` counts.
    r.core
        .store
        .append(&[
            row("task.updated", &id, "ses_first", "title,deps"),
            row(
                "task.change_accepted",
                &id,
                "ses_second",
                "objective, title",
            ),
            row("task.updated", &id, "ses_third", "deps"),
        ])
        .unwrap();
    assert_eq!(hand(&rec), Hand::Model("ses_second".into()));
    assert_eq!(hand(&by_model), Hand::Model("ses_second".into()));
    // A mark stands over every row.
    r.core
        .store
        .put_meta(&crate::task_graph::title_by_key(&id), &"ses_marked")
        .unwrap();
    assert_eq!(hand(&rec), Hand::Model("ses_marked".into()));
}

/// The read stops at the cap: a retitle row among the newest `ROWS_MAX` is found, and one just past them
/// is not (unproven, as before), and a look over 24 titles that were edited and never retitled reads
/// the cap once, not once each.
#[test]
fn the_read_is_bounded_and_shared() {
    let r = rig();
    let other = |n: usize| -> Vec<NewRecord> {
        (0..n)
            .map(|_| row("task.updated", "tsk_other", "ses_x", "deps"))
            .collect()
    };
    let wanted: BTreeSet<&str> = ["tsk_wanted"].into();
    // The wanted row, then exactly ROWS_MAX - 1 newer ones: it is the last read, and found.
    let mut recs = vec![row("task.updated", "tsk_wanted", "ses_renamer", "title")];
    recs.extend(other(ROWS_MAX - 1));
    r.core.store.append(&recs).unwrap();
    let got = retitlers(&r.core.store, &wanted, ROWS_MAX).unwrap();
    assert_eq!(
        got.found.get("tsk_wanted").map(String::as_str),
        Some("ses_renamer")
    );
    // One more newer row pushes it past the cap: not found.
    r.core.store.append(&other(1)).unwrap();
    ROWS_READ.with(|c| c.set(0));
    let got = retitlers(&r.core.store, &wanted, ROWS_MAX).unwrap();
    assert!(got.found.is_empty() && got.cut, "{got:?}");
    assert_eq!(
        ROWS_READ.with(std::cell::Cell::get),
        ROWS_MAX,
        "it read the cap and stopped"
    );
    // 24 titles edited and never retitled: one walk of the cap, not 24.
    let ids: Vec<String> = (0..24).map(|n| format!("tsk_edited{n}")).collect();
    let all: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    ROWS_READ.with(|c| c.set(0));
    assert!(retitlers(&r.core.store, &all, ROWS_MAX)
        .unwrap()
        .found
        .is_empty());
    assert_eq!(ROWS_READ.with(std::cell::Cell::get), ROWS_MAX);
}

/// Through `title_hands`, as a look asks: 24 records past version 1 with no mark share the one walk.
#[test]
fn a_look_over_many_edited_titles_reads_the_cap_once() {
    let r = rig();
    let id = owners(&r);
    let mut rec = task(&r, &id);
    rec.version = 2;
    let many: Vec<crate::task_graph::TaskRecord> = (0..24)
        .map(|n| {
            let mut m = rec.clone();
            m.id = format!("tsk_edited{n}");
            m
        })
        .collect();
    let refs: Vec<&crate::task_graph::TaskRecord> = many.iter().collect();
    r.core
        .store
        .append(
            &(0..ROWS_MAX + 100)
                .map(|_| row("task.updated", "tsk_other", "ses_x", "deps"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    ROWS_READ.with(|c| c.set(0));
    let hands = crate::task_graph::title_hands(&r.core.store, &refs).unwrap();
    // The walk was cut at the cap with no title row: unproven, not the owner's.
    assert!(
        hands.iter().all(|h| *h == Hand::Unproven(None)),
        "{hands:?}"
    );
    assert_eq!(ROWS_READ.with(std::cell::Cell::get), ROWS_MAX);
}

/// A look at an owner's project edited otherwise (version 2) whose title's row is out of the walk's reach:
/// at the cap (2,001 task rows, the title's the oldest) it is unproven and held from a restricted turn, in
/// plain words that name no conversation; on an unrestricted turn it is given and nothing is taken. The
/// owner's milestone (never edited) is theirs in both.
#[tokio::test]
async fn a_title_out_of_the_walks_reach_is_held_as_unproven() {
    for restricted in [true, false] {
        let r = match restricted {
            true => restricted_rig(),
            false => rig(),
        };
        let id = owners(&r);
        let renamer = conversation(&r).await;
        mark(&r, &renamer);
        // The title's row first, then 2,000 newer rows of another task: it is the 2,001st newest.
        let mut rec = task(&r, &id);
        rec.title = "Reef survey".into();
        rec.version += 1;
        let mut recs = vec![
            crate::task_graph::record(&rec).unwrap(),
            row("task.updated", &id, &renamer, "title"),
        ];
        recs.extend((0..ROWS_MAX).map(|_| row("task.updated", "tsk_other", "ses_x", "deps")));
        r.core.store.append(&recs).unwrap();
        let here = conversation(&r).await;
        let text = said(&r, &here, json!({"what": ["projects"]})).await;
        assert!(text.contains("next Outer harbour"), "{text}");
        match restricted {
            true => {
                assert!(!text.contains("Reef survey"), "{text}");
                assert!(
                    text.contains("(held back: its writer could not be established)"),
                    "{text}"
                );
                assert!(
                    !text.contains("that conversation"),
                    "names no conversation: {text}"
                );
            }
            false => {
                assert!(
                    text.contains("Reef survey") && !text.contains("held back"),
                    "{text}"
                );
                assert!(flags(&r, &here).is_none(), "no session to take flags from");
            }
        }
    }
}

/// A walk that ran to the end of the rows without a title row is a different answer: the record's own word
/// stands, given. And a ledger that begins after the record was created has lost a stretch of its life, so
/// a title with no row there is unproven.
#[test]
fn the_end_of_the_rows_is_the_records_word_and_a_ledger_that_begins_late_is_unproven() {
    let r = rig();
    let id = owners(&r);
    let mut edited = task(&r, &id);
    edited.version = 3;
    let hand = |rec: &crate::task_graph::TaskRecord| {
        crate::task_graph::title_hand(&r.core.store, rec).unwrap()
    };
    // The rows run out (a few of other tasks'), the ledger reaches back past the record: the owner's.
    r.core
        .store
        .append(&[row("task.updated", "tsk_other", "ses_x", "deps")])
        .unwrap();
    assert_eq!(hand(&edited), Hand::Owner);
    let mut model = edited.clone();
    model.origin.by_model = true;
    model.origin.session = "ses_maker".into();
    assert_eq!(hand(&model), Hand::Model("ses_maker".into()));
    // The record says it was created before the ledger's first row: a retitle in that stretch is gone.
    let mut old = edited;
    old.created_at_ms = 1;
    assert_eq!(hand(&old), Hand::Unproven(None));
    // A version-1 record has nothing to find, whenever it was created.
    old.version = 1;
    assert_eq!(hand(&old), Hand::Owner);
}

/// The shape-building window after a start (`ledger_page` answers none): the newest rows of every kind are
/// read. A ledger shorter than the cap is read whole, so an old retitle is found and a title with no row is
/// the owner's; a longer one may hold an older retitle than the tail reaches, and the title is unproven.
#[test]
fn in_the_shape_building_window_a_short_ledger_is_read_whole_and_a_long_one_is_cut() {
    let r = rig();
    let id = owners(&r);
    let mut edited = task(&r, &id);
    edited.version = 3;
    r.core.store.shape_window(true);
    let hand = |rec: &crate::task_graph::TaskRecord| {
        crate::task_graph::title_hand(&r.core.store, rec).unwrap()
    };
    assert_eq!(hand(&edited), Hand::Owner, "the whole short ledger, no row");
    r.core
        .store
        .append(&[row("task.updated", &id, "ses_renamer", "title")])
        .unwrap();
    assert_eq!(hand(&edited), Hand::Model("ses_renamer".into()));
    // More rows than the cap: the tail may not reach it.
    r.core
        .store
        .append(
            &(0..ROWS_MAX)
                .map(|_| row("task.updated", "tsk_other", "ses_x", "deps"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(hand(&edited), Hand::Unproven(None));
    r.core.store.shape_window(false);
    assert_eq!(
        hand(&edited),
        Hand::Unproven(None),
        "the index walk is cut at the cap too"
    );
}

/// A record a model made, edited (version above 1), with its title's row out of the walk's reach: the original
/// words are still its maker's conversation's, so a restricted turn is held from them with the plain line
/// ("its writer could not be established"), and an unrestricted turn is given them AND takes that
/// conversation's flags on, as it did before the walk was cut safe.
#[tokio::test]
async fn an_unproven_title_a_model_made_still_has_its_makers_flags_taken_on() {
    for restricted in [true, false] {
        let r = match restricted {
            true => restricted_rig(),
            false => rig(),
        };
        // A project with no milestone, so the title is the only words of the maker's in the look.
        let maker = conversation(&r).await;
        r.turn_with(
            "task_create",
            &maker,
            json!({"kind": "project", "title": "Harbour survey", "review": "none"}),
        )
        .await;
        mark(&r, &maker);
        let mut rec = crate::task_graph::all(&r.core.store)
            .unwrap()
            .into_iter()
            .find(|t| t.kind == theseus_protocol::tasks::TaskKind::Project)
            .unwrap();
        rec.version += 1;
        // One more row than the cap, so the walk is cut and does not run to the end.
        let mut recs = vec![crate::task_graph::record(&rec).unwrap()];
        recs.extend((0..=ROWS_MAX).map(|_| row("task.updated", "tsk_other", "ses_x", "deps")));
        r.core.store.append(&recs).unwrap();
        assert_eq!(
            crate::task_graph::title_hand(&r.core.store, &rec).unwrap(),
            Hand::Unproven(Some(maker.clone()))
        );
        let here = conversation(&r).await;
        let text = said(&r, &here, json!({"what": ["projects"]})).await;
        match restricted {
            true => {
                assert!(!text.contains("Harbour survey"), "{text}");
                assert!(
                    text.contains("(held back: its writer could not be established)"),
                    "{text}"
                );
                assert!(flags(&r, &here).is_none());
            }
            false => {
                assert!(text.contains("Harbour survey"), "{text}");
                assert!(flags(&r, &here).is_some(), "the maker's flags are taken on");
            }
        }
    }
}
