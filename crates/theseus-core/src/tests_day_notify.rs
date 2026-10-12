//! The daily ceiling under `daily_spend_ceiling_mode = "notify"` and the
//! burn-rate brake, through the core (theseus-9e02; the owner, 2026-10-11:
//! tokens are less cautious, so the ceiling warns and only a runaway stops):
//! a turn past the ceiling is made and the owner hears once a level, in a row,
//! a DM post and a narrative line; the runaway line stops the day with its own
//! words and class; health's first line and the Money view carry it; a session
//! that burns its line in ten minutes is asked to resume; the stop mode keeps
//! its refusal (`tests_day_ceiling.rs`), and so does AWS (`aws/tests_money.rs`).

use theseus_kernel::{usd_to_micros, PROVIDER_TOOL};

use crate::config::DailyCeilingMode;
use crate::tests_day_ceiling::{billed, bind_dm, posts, rig, rows, turn, turn_in};
use crate::turn::TurnError;

fn notify(c: &mut crate::Config) {
    c.kernel.daily_spend_ceiling_mode = DailyCeilingMode::Notify;
}

/// Past the ceiling the call is made, the turn ends well, and the day's
/// warning is a `spend.day_warning` row, one post to the owner's DM and a
/// narrative line, once; a second turn in the same level says nothing more.
#[tokio::test]
async fn a_turn_past_the_ceiling_is_made_and_the_owner_is_warned_once() {
    let r = rig(vec![billed("one"), billed("two")], |c| {
        notify(c);
        c.kernel.daily_spend_ceiling_usd = 0.01;
        c.kernel.daily_notice_step_usd = 1_000.0;
    });
    bind_dm(&r.core);
    let (sid, res) = turn(&r.core, "hello").await;
    res.expect("the ceiling refuses nothing");
    assert_eq!(r.fake.requests().len(), 1, "the call was made");
    let w = rows(&r.core, "spend.day_warning");
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(
        (w[0].data["level"].as_u64(), w[0].data["posted"].as_bool()),
        (Some(1), Some(true))
    );
    assert_eq!(w[0].data["ceiling_usd"], 0.01);
    assert!(rows(&r.core, "spend.ceiling").is_empty(), "no refusal row");
    let p = posts(&r.core, "spend_ceiling");
    assert_eq!(p.len(), 1, "{p:?}");
    let text = p[0]["text"].as_str().unwrap();
    for want in [
        "⚠️ Today's model spend is",
        "the $0.01 daily ceiling",
        "the work goes on",
        "$2000 runaway line",
        "`theseus stop",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    // The same level a second time says nothing.
    turn_in(&r.core, &sid, "again").await.unwrap();
    assert_eq!(r.fake.requests().len(), 2);
    assert_eq!(rows(&r.core, "spend.day_warning").len(), 1);
    assert_eq!(posts(&r.core, "spend_ceiling").len(), 1);
    // The Money view and health say it, and no call is refused.
    let day = r.core.budget_list().unwrap().day_ceiling.expect("the day");
    assert!(day.past && !day.reached, "{day:?}");
    assert_eq!((day.mode.as_str(), day.runaway_usd), ("notify", 2000.0));
    let health = r.core.health().day_spend.expect("health's first line");
    assert!(health.past);
}

/// A warning at each step: the level the day's spend is at after a turn is the
/// last row's, levels only grow, and none is said twice.
#[tokio::test]
async fn each_step_past_the_ceiling_warns_once() {
    let r = rig(vec![billed("a"), billed("b"), billed("c")], |c| {
        notify(c);
        c.kernel.daily_spend_ceiling_usd = 0.01;
        c.kernel.daily_notice_step_usd = 0.01;
    });
    bind_dm(&r.core);
    let (sid, res) = turn(&r.core, "one").await;
    res.unwrap();
    turn_in(&r.core, &sid, "two").await.unwrap();
    turn_in(&r.core, &sid, "three").await.unwrap();
    let c = r.core.kernel.day_ceiling();
    let spent = c.today(c.now()).spent;
    let levels: Vec<u64> = rows(&r.core, "spend.day_warning")
        .iter()
        .map(|x| x.data["level"].as_u64().unwrap())
        .collect();
    assert!(levels.len() >= 2, "a step a turn crosses: {levels:?}");
    assert!(levels.windows(2).all(|w| w[0] < w[1]), "{levels:?}");
    assert_eq!(
        *levels.last().unwrap(),
        theseus_kernel::day_ceiling::level_of(spent, usd_to_micros(0.01), usd_to_micros(0.01))
    );
    assert_eq!(posts(&r.core, "spend_ceiling").len(), levels.len());
}

/// The runaway line stops the day under notify: a turn whose call would pass
/// it ends `daily_runaway` with its words, the owner hears the stop once (a
/// refusal row and a post of its own), a smaller call is refused too, and the
/// ceiling's warning was never a refusal.
#[tokio::test]
async fn the_runaway_line_stops_a_turn_with_its_own_words_and_class() {
    let r = rig(vec![billed("never")], |c| {
        notify(c);
        c.kernel.daily_spend_ceiling_usd = 1.0;
        c.kernel.daily_runaway_usd = 5.0;
    });
    bind_dm(&r.core);
    let k = r.core.kernel.day_ceiling();
    k.book(k.now(), usd_to_micros(4.9999));
    let (_, res) = turn(&r.core, "hello").await;
    let err = res.unwrap_err();
    let te = err.downcast_ref::<TurnError>().expect("a turn's failure");
    assert_eq!(te.class, "daily_runaway");
    let words = format!("{:#}", te.source);
    assert!(
        words.starts_with("today's spend reached the $5 daily runaway line")
            && words.contains("`[kernel] daily_runaway_usd` applies at the daemon's next start"),
        "{words}"
    );
    assert!(r.fake.requests().is_empty(), "no call was made");
    let row = rows(&r.core, "spend.ceiling");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].data["limit_usd"], 5.0);
    // The warning for the spend booked past the ceiling, and the stop's own post.
    let p = posts(&r.core, "spend_ceiling");
    assert_eq!(p.len(), 2, "{p:?}");
    assert!(p[0]["text"].as_str().unwrap().starts_with("⚠️"));
    assert!(p[1]["text"]
        .as_str()
        .unwrap()
        .starts_with("⛔ Today's model spend reached the $5 daily runaway line"));
    // Sticky: however small, and the Money view says the day is stopped.
    assert!(k.fits(k.now(), 1).is_err());
    assert!(r.core.budget_list().unwrap().day_ceiling.unwrap().reached);
    // A higher line lifts it (the next start, here the same counter).
    k.set_rules(true, usd_to_micros(50.0), usd_to_micros(200.0), 0);
    // Still stopped today: `since` holds until the line is no higher.
    assert!(k.fits(k.now(), 1).is_ok(), "a higher line lifts the stop");
}

/// A session that burns its line in ten minutes is asked to resume: its next
/// call is not made, its question names the rate, the window and the key, and
/// an approved resume lets the next call through.
#[tokio::test]
async fn a_burning_session_is_paused_and_asks_to_resume() {
    let r = rig(vec![billed("one"), billed("two")], |c| {
        notify(c);
        c.kernel.spend_limit_usd = 1_000.0;
        c.kernel.runaway_usd_per_10min = 0.001;
    });
    let (sid, res) = turn(&r.core, "hello").await;
    res.unwrap();
    assert_eq!(r.fake.requests().len(), 1);
    // The next call is held back and the owner is asked.
    let _ = turn_in(&r.core, &sid, "more").await;
    assert_eq!(
        r.fake.requests().len(),
        1,
        "the paused session made no call"
    );
    let asked = r.core.kernel.pending_confirms().unwrap();
    let q = asked
        .iter()
        .find(|a| a.tool == theseus_kernel::BUDGET_TOOL)
        .expect("a budget question");
    let call = &q.proposal.as_ref().unwrap().args["call"];
    assert_eq!(call["brake"]["window_minutes"], 10);
    let words = crate::turn::budget_question("This session", 0, 0, 0, Default::default(), call);
    for want in [
        "in the last 10 minutes",
        "burn-rate line",
        "runaway_usd_per_10min",
        "paused",
    ] {
        assert!(words.contains(want), "{want}: {words}");
    }
    // Approving resumes it, with its window empty.
    let (e, _) = r
        .core
        .kernel
        .reset_budget(&q.correlation_id, "cli")
        .unwrap();
    assert_eq!(e.budget.resets, 0, "a resume is no reset");
    turn_in(&r.core, &sid, "resumed").await.unwrap();
    assert_eq!(r.fake.requests().len(), 2);
    let _ = PROVIDER_TOOL;
}
