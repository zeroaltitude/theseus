//! The commitments block and the model's `wake.cancel` and `wake.move`
//! (theseus-lherl), through the whole core: the block is the last block of
//! every request while a wake is pending and absent when none is; a stand-in
//! model that reads the block's ids moves and cancels by them; a wake keeps
//! its id and its series through a move; another conversation's wake is not
//! found; a shared place is refused the tools.

use serde_json::{json, Value};

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{rig_scripted, session, turn, ALICE, OWNER};

/// The text blocks of the request's last user message.
fn blocks_of(req: &ProviderRequest) -> Vec<String> {
    let last = req.messages.iter().rev().find(|m| m["role"] == "user");
    match last.map(|m| &m["content"]) {
        Some(Value::Array(b)) => b
            .iter()
            .filter_map(|b| b["text"].as_str().map(String::from))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

/// What the person said: the message's blocks but the harness's.
fn said(req: &ProviderRequest) -> String {
    blocks_of(req)
        .into_iter()
        .filter(|t| !crate::task_graph::view::is_head(t))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The commitments block, when the request's last block is one.
fn block_of(req: &ProviderRequest) -> Option<String> {
    blocks_of(req)
        .pop()
        .filter(|t| t.starts_with(crate::commitments::HEAD))
}

/// The short ids the block names, in its order: what the model reads.
fn ids_in(req: &ProviderRequest) -> Vec<String> {
    block_of(req)
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(1).map(String::from))
        .collect()
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// `SET <after>: <note>` sets a wake; `SERIES <every>[ days a,b]: <note>` a
/// repeating one; `MOVE <n> <after>[ series]` and `CANCEL <n>` change the
/// n-th of the ids the block shows (`RAW <call> <tool> <id> <json>` names an
/// id itself, under the call id `<call>`); anything else is answered.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let last = said(req);
    let nth = |n: &str| {
        let ids = ids_in(req);
        ids.get(n.parse::<usize>().unwrap() - 1)
            .cloned()
            .unwrap_or_else(|| format!("none-{n}-in-{ids:?}"))
    };
    if let Some(rest) = last.strip_prefix("SET ") {
        let (after, note) = rest.split_once(": ").unwrap();
        return Scripted::tools(
            "Setting it.",
            &[("w1", "wake_at", json!({"after": after, "note": note}))],
        );
    }
    if let Some(rest) = last.strip_prefix("SERIES ") {
        let (head, note) = rest.split_once(": ").unwrap();
        let mut input = json!({"note": note, "after": "1h"});
        let mut it = head.split(' ');
        input["every"] = json!(it.next().unwrap());
        if let Some(days) = it.next() {
            input["days"] = json!(days.split(',').collect::<Vec<_>>());
        }
        return Scripted::tools("Setting it.", &[("w1", "wake_at", input)]);
    }
    if let Some(rest) = last.strip_prefix("MOVE ") {
        let mut it = rest.split(' ');
        let id = nth(it.next().unwrap());
        let after = it.next().unwrap();
        let mut input = json!({"id": id, "after": after});
        if it.next() == Some("series") {
            input["series"] = json!(true);
        }
        return Scripted::tools("Moving it.", &[("m1", "wake_move", input)]);
    }
    if let Some(n) = last.strip_prefix("CANCEL ") {
        return Scripted::tools(
            "Cancelling.",
            &[("c1", "wake_cancel", json!({"id": nth(n)}))],
        );
    }
    if let Some(rest) = last.strip_prefix("RAW ") {
        let mut it = rest.splitn(4, ' ');
        let (call, tool, id) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
        let mut input: Value = serde_json::from_str(it.next().unwrap_or("{}")).unwrap();
        input["id"] = json!(id);
        return Scripted::tools("Trying.", &[(call, tool, input)]);
    }
    Scripted::text("Hello.")
}

fn private_dm() -> String {
    format!("dm:{OWNER}")
}

fn exec_of(core: &crate::Core, sid: &str) -> theseus_kernel::Execution {
    let rec: crate::session::SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

fn result_of(core: &crate::Core, sid: &str, id: &str) -> String {
    crate::tests_places::result_of(core, sid, id)
}

fn rows(core: &crate::Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

fn model() -> crate::tests_places::Rig {
    rig_scripted(|_| {}, script)
}

/// The block is the last block of the request's last message, the loop after
/// a wake is set already shows it, and none is there while none is pending.
#[tokio::test]
async fn the_block_is_the_requests_last_block_while_a_wake_is_pending() {
    let r = model();
    let sid = session(&r.core, Some(&private_dm()));
    turn(&r.core, &sid, "hello").await;
    assert!(
        block_of(r.requests().last().unwrap()).is_none(),
        "no wake, no block"
    );
    turn(&r.core, &sid, "SET 1h: check PR 123").await;
    // The loop that answers the call is the first to show it.
    let reqs = r.requests();
    let after_set = reqs.iter().rev().find(|q| answers_a_call(q)).unwrap();
    let block = block_of(after_set).expect("the block after the call");
    assert!(block.contains("check PR 123"), "{block}");
    assert!(block.lines().nth(1).unwrap().starts_with("⏰ "), "{block}");
    // The next turn's request carries it too, after the message's words.
    turn(&r.core, &sid, "SERIES 1d mon,fri: evening haiku").await;
    turn(&r.core, &sid, "what do I have?").await;
    let q = r.requests().pop().unwrap();
    let blocks = blocks_of(&q);
    assert_eq!(blocks.first().unwrap(), "what do I have?");
    let block = block_of(&q).unwrap();
    assert_eq!(ids_in(&q).len(), 2, "{block}");
    assert!(block.contains("🔁 "), "{block}");
    assert!(block.contains("every 1d on mon,fri"), "{block}");
    // No breakpoint of its own: at most the one the view's move leaves on
    // the block before it, never on the block itself.
    let last = q.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    assert!(last.get("cache_control").is_none(), "{last}");
    // Cancel both: the block goes.
    turn(&r.core, &sid, "CANCEL 1").await;
    turn(&r.core, &sid, "CANCEL 1").await;
    turn(&r.core, &sid, "and now?").await;
    assert!(
        block_of(r.requests().last().unwrap()).is_none(),
        "nothing pending, no block"
    );
    assert!(exec_of(&r.core, &sid).wakes.is_empty());
}

/// `wake.move` keeps the id, the note and a series' `every` and `days` (every
/// weekday, so the first occurrence does not depend on the day the test runs), and is one row, `wake.moved`, with the times it moved between.
#[tokio::test]
async fn a_move_keeps_the_id_and_the_series_and_says_from_and_to() {
    let r = model();
    let sid = session(&r.core, Some(&private_dm()));
    turn(&r.core, &sid, "SET 1h: check PR 123").await;
    turn(
        &r.core,
        &sid,
        "SERIES 1d mon,tue,wed,thu,fri,sat,sun: evening haiku",
    )
    .await;
    let before = exec_of(&r.core, &sid).wakes;
    assert_eq!(before.len(), 2);
    let one = before.iter().find(|w| w.repeat.is_none()).unwrap().clone();
    let series = before.iter().find(|w| w.repeat.is_some()).unwrap().clone();
    // The one-shot, to 3 h from now.
    let ids = {
        turn(&r.core, &sid, "hello").await;
        ids_in(r.requests().last().unwrap())
    };
    let n = ids
        .iter()
        .position(|s| *s == crate::task::short(&one.id))
        .unwrap()
        + 1;
    turn(&r.core, &sid, &format!("MOVE {n} 3h")).await;
    let after = exec_of(&r.core, &sid).wakes;
    let moved = after.iter().find(|w| w.id == one.id).expect("the same id");
    assert_eq!(after.len(), 2, "moved, never set a second");
    assert_eq!(moved.note, one.note);
    assert!(
        moved.due_at_ms > one.due_at_ms + 100 * 60_000,
        "later: {moved:?}"
    );
    let row = rows(&r.core, "wake.moved");
    assert_eq!(row.len(), 1, "{row:?}");
    assert_eq!(row[0]["wake_id"], json!(one.id));
    assert_eq!(row[0]["from"], json!(one.due_at_ms));
    assert_eq!(row[0]["to"], json!(moved.due_at_ms));
    assert_eq!(row[0]["by"], json!("model"));
    let text = result_of(&r.core, &sid, "m1");
    assert!(text.starts_with("Moved wake "), "{text}");
    // The series: its next occurrence alone, then its time.
    let m = ids_in(&{
        turn(&r.core, &sid, "hello").await;
        r.requests().pop().unwrap()
    });
    let n = m
        .iter()
        .position(|s| *s == crate::task::short(&series.id))
        .unwrap()
        + 1;
    turn(&r.core, &sid, &format!("MOVE {n} 5h")).await;
    let e = exec_of(&r.core, &sid);
    let s = e.wakes.iter().find(|w| w.id == series.id).unwrap();
    assert_eq!(
        s.repeat, series.repeat,
        "a move of the next one leaves the series"
    );
    assert!(s.due_at_ms > series.due_at_ms + 200 * 60_000, "{s:?}");
    assert_eq!(s.occurrence, series.occurrence);
    // The series' time: its occurrences move, its rule does not.
    let before = s.clone();
    let n = ids_in(&{
        turn(&r.core, &sid, "hello").await;
        r.requests().pop().unwrap()
    })
    .iter()
    .position(|x| *x == crate::task::short(&series.id))
    .unwrap()
        + 1;
    turn(&r.core, &sid, &format!("MOVE {n} 7h series")).await;
    let s = exec_of(&r.core, &sid)
        .wakes
        .into_iter()
        .find(|w| w.id == series.id)
        .unwrap();
    let (old, new) = (before.repeat.unwrap(), s.repeat.unwrap());
    assert_eq!(
        (old.every, &old.days, old.until_ms),
        (new.every, &new.days, new.until_ms)
    );
    assert_ne!(old.first_ms, new.first_ms, "the series' own time moved");
    assert_eq!(rows(&r.core, "wake.moved").len(), 3);
}

/// A cancel by the model ends the wake with a row that says who.
#[tokio::test]
async fn a_cancel_by_the_model_ends_it_and_the_row_says_so() {
    let r = model();
    let sid = session(&r.core, Some(&private_dm()));
    turn(&r.core, &sid, "SERIES 1d: pulse").await;
    turn(&r.core, &sid, "CANCEL 1").await;
    assert!(
        exec_of(&r.core, &sid).wakes.is_empty(),
        "the series ended with it"
    );
    let row = rows(&r.core, "wake.cancelled");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0]["by"], json!("model"));
    let text = result_of(&r.core, &sid, "c1");
    assert!(text.contains("its series is ended"), "{text}");
}

/// Another conversation's wake is not found, by its whole id or the end of
/// it; and a name that fits none says which are the conversation's own.
#[tokio::test]
async fn another_conversations_wake_is_refused() {
    let r = model();
    let mine = session(&r.core, Some(&private_dm()));
    let other = session(&r.core, None);
    turn(&r.core, &other, "SET 2h: the other's errand").await;
    turn(&r.core, &mine, "SET 1h: my errand").await;
    let theirs = exec_of(&r.core, &other).wakes[0].clone();
    for name in [theirs.id.clone(), crate::task::short(&theirs.id)] {
        for tool in ["wake_cancel", "wake_move"] {
            let call = format!("r_{tool}_{}", name.len());
            let args = if tool == "wake_move" {
                "{\"after\": \"3h\"}"
            } else {
                "{}"
            };
            turn(&r.core, &mine, &format!("RAW {call} {tool} {name} {args}")).await;
            let text = result_of(&r.core, &mine, &call);
            assert!(
                text.contains("no pending wake of this conversation is named"),
                "{tool}: {text}"
            );
            assert!(text.contains("pending wakes are"), "{text}");
        }
    }
    assert_eq!(exec_of(&r.core, &other).wakes, vec![theirs], "untouched");
    assert_eq!(exec_of(&r.core, &mine).wakes.len(), 1);
    assert!(rows(&r.core, "wake.cancelled").is_empty());
    assert!(rows(&r.core, "wake.moved").is_empty());
}

/// A shared place may set a wake and sees it in the block, and is refused
/// the model's cancel and move with the place rule's words.
#[tokio::test]
async fn a_shared_place_is_refused_the_cancel_and_the_move() {
    let r = model();
    let sid = session(&r.core, Some(&format!("dm:{ALICE}")));
    assert_eq!(
        r.core.runner.class_of(&sid),
        crate::places::PlaceClass::Shared
    );
    turn(&r.core, &sid, "SET 1h: a reminder for the room").await;
    let id = exec_of(&r.core, &sid).wakes[0].id.clone();
    for tool in ["wake_cancel", "wake_move"] {
        let args = if tool == "wake_move" {
            "{\"after\": \"2h\"}"
        } else {
            "{}"
        };
        turn(&r.core, &sid, &format!("RAW r_{tool} {tool} {id} {args}")).await;
        let text = result_of(&r.core, &sid, &format!("r_{tool}"));
        assert!(
            text.starts_with("Not run: wake.") && text.contains("is not offered in a shared place"),
            "{tool}: {text}"
        );
    }
    assert_eq!(exec_of(&r.core, &sid).wakes.len(), 1, "still pending");
    assert!(rows(&r.core, "wake.cancelled").is_empty());
    assert!(rows(&r.core, "wake.moved").is_empty());
}

/// A clock at a fixed offset east of UTC, so the goldens read no zone.
fn fixed(offset_secs: i64) -> impl Fn(u64) -> crate::wake::Local {
    move |ms| {
        let secs = (ms / 1000) as i64 + offset_secs;
        let (y, m, d) = crate::wake::civil_from_days(secs.div_euclid(86_400));
        let s = secs.rem_euclid(86_400);
        crate::wake::Local {
            year: y as i32,
            month: m,
            day: d,
            hour: (s / 3600) as u32,
            minute: (s / 60 % 60) as u32,
            second: (s % 60) as u32,
            offset_secs,
        }
    }
}

fn wake(
    id: &str,
    due_ms: u64,
    note: &str,
    repeat: Option<theseus_kernel::Repeat>,
) -> theseus_kernel::PendingWake {
    theseus_kernel::PendingWake {
        id: format!("wak_01{id}"),
        due_at_ms: due_ms,
        note: note.into(),
        set_at_ms: 0,
        by: "act_x".into(),
        target: None,
        occurrence: u32::from(repeat.is_some()),
        repeat,
        ..Default::default()
    }
}

/// 2026-10-12 00:12 UTC: Sunday 17:12 at UTC−7.
const NOW: u64 = 1_791_763_920_000;
/// Friday 2026-10-16 09:00, Sunday the 18th 21:00 and Sunday the 25th 09:00,
/// at UTC−7; and Friday the 30th 18:00.
const FRI: u64 = 1_792_166_400_000;
const SUN_EVENING: u64 = 1_792_382_400_000;
const NEXT_SUN: u64 = 1_792_944_000_000;
const END: u64 = 1_793_408_400_000;

const HEAD_ONE: &str = "[Your pending wakes in this conversation, ";
const HEAD_REST: &str = ": to change one, move it with wake.move or end it with wake.cancel, \
                         never set a second.]";

fn series(days: Vec<theseus_kernel::Day>, until_ms: Option<u64>) -> theseus_kernel::Repeat {
    theseus_kernel::Repeat {
        every: theseus_kernel::Every::parse("1d").unwrap(),
        first_ms: SUN_EVENING,
        days,
        until_ms,
        cron: None,
    }
}

/// A forced compaction drops the "Set wake" results from the request, and
/// the block is still its last block, with both wakes: the model knows its
/// commitments after a compaction, and can move one by words.
#[tokio::test]
async fn after_a_forced_compaction_the_request_still_carries_the_block() {
    use crate::tests_compaction as c;
    let r = c::rig(
        vec![
            Scripted::tools(
                "Setting both.",
                &[
                    (
                        "w1",
                        "wake_at",
                        json!({"after": "1h", "note": "check PR 123"}),
                    ),
                    (
                        "w2",
                        "wake_at",
                        json!({"every": "1d", "note": "evening haiku"}),
                    ),
                ],
            ),
            Scripted::text("Set both."),
        ],
        vec![c::summary(
            "The keeper set two wakes and logged six tides.",
            9_000,
            120,
        )],
    );
    let sid = c::session(&r.core);
    c::turn(&r.core, &sid, "set a wake for the PR and a daily haiku")
        .await
        .unwrap();
    assert_eq!(exec_of(&r.core, &sid).wakes.len(), 2);
    let turns = c::until_recompiled(&r, &sid, 6_000).await;
    assert!(turns >= 3, "rang after {turns} turns");
    let compiled = c::rows(&r.core, "context.compiled");
    assert_eq!(
        compiled.last().unwrap()["strategy"],
        "compaction",
        "{compiled:?}"
    );
    c::turn(&r.core, &sid, "what reminders do I have?")
        .await
        .unwrap();
    let q = r.model.requests().pop().unwrap();
    let all = serde_json::to_string(&q.messages).unwrap();
    assert!(
        !all.contains("Set wake "),
        "the set results are gone with the summarized range"
    );
    let block = block_of(&q).expect("the block, after the compaction");
    assert!(
        block.contains("check PR 123") && block.contains("evening haiku"),
        "{block}"
    );
    assert_eq!(ids_in(&q).len(), 2);
}

/// The block's golden: none, one, a series, a far one-shot, and ten-plus cut
/// at ten with how many were left out.
#[test]
fn the_blocks_golden_none_one_a_series_and_a_long_list() {
    use theseus_kernel::Day;
    let at = fixed(-7 * 3600);
    assert_eq!(crate::commitments::render(&[], NOW, &at), None);

    let one = [wake("a8cebb", FRI, "check PR 123", None)];
    let shown = crate::commitments::render(&one, NOW, &at).unwrap();
    assert_eq!(
        shown.text,
        format!("{HEAD_ONE}1{HEAD_REST}\n⏰ a8cebb Fri 09:00 check PR 123")
    );
    assert_eq!((shown.lines, shown.left_out), (1, 0));

    // A series: its span, its clock time, the weekday of its next; and one a
    // week out names its date.
    let many = [
        wake("a8cebb", FRI, "check PR 123", None),
        wake(
            "a41d04",
            SUN_EVENING,
            "evening haiku\nand post it",
            Some(series(vec![], None)),
        ),
        wake("b4f566", NEXT_SUN, "water the ferns", None),
        wake(
            "c0ffee",
            NEXT_SUN + 3_600_000,
            "standup notes",
            Some(series(vec![Day::Mon, Day::Fri], Some(END))),
        ),
    ];
    assert_eq!(
        crate::commitments::render(&many, NOW, &at).unwrap().text,
        format!(
            "{HEAD_ONE}4{HEAD_REST}\n\
             ⏰ a8cebb Fri 09:00 check PR 123\n\
             🔁 a41d04 every 1d 21:00 (next Sun) evening haiku\n\
             ⏰ b4f566 Sun Oct 25 09:00 water the ferns\n\
             🔁 c0ffee every 1d on mon,fri 10:00 until Oct 30 18:00 (next Sun) standup notes"
        )
    );

    // Thirteen: ten lines, and how many more.
    let long: Vec<_> = (0..13)
        .map(|i| {
            wake(
                &format!("{i:06}"),
                FRI + i * 60_000,
                &format!("note {i}"),
                None,
            )
        })
        .collect();
    let shown = crate::commitments::render(&long, NOW, &at).unwrap();
    let want: Vec<String> = std::iter::once(format!("{HEAD_ONE}13{HEAD_REST}"))
        .chain((0..10).map(|i| format!("⏰ {i:06} Fri 09:{i:02} note {i}")))
        .chain(std::iter::once(
            "(and 3 more: wake.list, or `theseus wakes`, shows them all.)".to_string(),
        ))
        .collect();
    assert_eq!(shown.text, want.join("\n"));
    assert_eq!((shown.lines, shown.left_out), (10, 3));
}

/// A reminder says so (`remind:`), and a paused wake or series says it is held
/// (theseus-8ux6x): the model reads which of its commitments are the owner's, and
/// which will not fire until they are resumed.
#[test]
fn a_reminder_says_remind_and_a_paused_one_says_paused() {
    use theseus_kernel::{Purpose, WakeRun};
    let at = fixed(-7 * 3600);
    let remind = theseus_kernel::PendingWake {
        purpose: Purpose::Remind,
        run: WakeRun::Notice,
        ..wake("a8cebb", FRI, "check PR 123", None)
    };
    let held = theseus_kernel::PendingWake {
        paused_at_ms: Some(NOW),
        ..wake("b4f566", NEXT_SUN, "water the ferns", None)
    };
    let held_series = theseus_kernel::PendingWake {
        purpose: Purpose::Remind,
        paused_at_ms: Some(NOW),
        ..wake(
            "a41d04",
            SUN_EVENING,
            "evening haiku",
            Some(series(vec![], None)),
        )
    };
    let shown = crate::commitments::render(&[remind, held_series, held], NOW, &at).unwrap();
    assert_eq!(
        shown.text,
        format!(
            "{HEAD_ONE}3{HEAD_REST}\n\
             ⏰ a8cebb Fri 09:00 remind: check PR 123\n\
             ⏸ a41d04 every 1d 21:00 (paused) remind: evening haiku\n\
             ⏸ b4f566 Sun Oct 25 09:00 (paused) water the ferns"
        )
    );
}

/// Long titles cannot take the block past its 300 tokens: it shows what fits
/// and counts the rest.
#[test]
fn the_block_stays_inside_its_token_bound() {
    let at = fixed(-7 * 3600);
    let note = "n".repeat(200);
    let long: Vec<_> = (0..12)
        .map(|i| wake(&format!("{i:06}"), FRI + i * 60_000, &note, None))
        .collect();
    let shown = crate::commitments::render(&long, NOW, &at).unwrap();
    assert!(
        shown.tokens <= crate::commitments::MAX_TOKENS,
        "{}",
        shown.tokens
    );
    assert!(shown.left_out > 0 && shown.lines < 10, "{shown:?}");
    assert!(shown
        .text
        .contains(&format!("(and {} more:", shown.left_out)));
}

/// Building the block costs at most 50 µs (the row's bound), from its best
/// of many runs so a loaded machine's noise is not the block's.
#[test]
fn the_block_builds_in_under_fifty_microseconds() {
    let long: Vec<_> = (0..10)
        .map(|i| {
            wake(
                &format!("{i:06}"),
                FRI + i * 60_000,
                "check the build",
                None,
            )
        })
        .collect();
    let best = (0..300)
        .map(|_| {
            let t0 = std::time::Instant::now();
            std::hint::black_box(crate::commitments::render(
                std::hint::black_box(&long),
                NOW,
                &crate::wake::local,
            ));
            t0.elapsed()
        })
        .min()
        .unwrap();
    assert!(best.as_micros() < 50, "{best:?}");
}
