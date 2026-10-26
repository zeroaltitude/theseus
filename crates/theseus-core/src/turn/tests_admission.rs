//! The admission queue (theseus-4g0l), on tokio's paused clock: class order,
//! first come first served inside a class, aging, the stuck window, a slot
//! lent and taken again, and no lost wake-up under a seeded stress.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Admission, Class, Notice, Slot, AGE_STEP};

const STUCK: Duration = Duration::from_secs(600);

/// A turn queued now, in `class`, that records its name when admitted and
/// keeps its slot until `release` says so.
fn queue_one(
    adm: &Arc<Admission>,
    class: Class,
    name: String,
    order: &Arc<Mutex<Vec<String>>>,
) -> tokio::task::JoinHandle<Slot> {
    let (adm, order) = (adm.clone(), order.clone());
    tokio::spawn(async move {
        let slot = adm.take(class, false).await.expect("admitted");
        order.lock().unwrap().push(name);
        slot
    })
}

/// Let every spawned task run until it waits.
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn a_typed_turn_is_admitted_before_queued_task_turns() {
    let adm = Admission::new(1, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let held = adm.try_take(Class::Task).expect("a free slot");
    let mut handles = Vec::new();
    for (class, name) in [
        (Class::Task, "task 1"),
        (Class::Task, "task 2"),
        (Class::Wake, "wake 1"),
        (Class::Task, "task 3"),
        (Class::Person, "typed"),
    ] {
        handles.push(queue_one(&adm, class, name.into(), &order));
        settle().await;
    }
    assert_eq!(adm.load(), (1, 5), "one slot held, five waiting");
    assert!(
        adm.try_take(Class::Person).is_none(),
        "a turn that cannot wait never passes the queue"
    );
    drop(held);
    // Each admitted turn ends in its turn: drop the slots in grant order.
    for _ in 0..5 {
        settle().await;
        let next = order.lock().unwrap().len();
        let i = handles
            .iter()
            .position(|h| h.is_finished())
            .unwrap_or_else(|| panic!("the slot went to no one after {next} grants"));
        drop(handles.remove(i).await.unwrap());
    }
    assert_eq!(
        *order.lock().unwrap(),
        ["typed", "wake 1", "task 1", "task 2", "task 3"],
        "a person's input first, then a wake, then the tasks in their arrival order"
    );
    assert_eq!(adm.load(), (0, 0));
}

#[tokio::test(start_paused = true)]
async fn turns_of_one_class_are_admitted_in_their_arrival_order() {
    let adm = Admission::new(3, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let held: Vec<Slot> = (0..3).map(|_| adm.try_take(Class::Wake).unwrap()).collect();
    let mut handles = std::collections::VecDeque::new();
    for i in 0..200 {
        let class = [Class::Task, Class::Wake][i % 2];
        handles.push_back((
            class,
            queue_one(&adm, class, format!("{} {i}", class.as_str()), &order),
        ));
    }
    settle().await;
    assert_eq!(adm.load(), (3, 200));
    drop(held);
    let mut running = Vec::new();
    while order.lock().unwrap().len() < 200 {
        settle().await;
        // The earliest admitted ends first.
        while let Some(i) = handles.iter().position(|(_, h)| h.is_finished()) {
            let (_, h) = handles.remove(i).unwrap();
            running.push(h.await.unwrap());
        }
        if !running.is_empty() {
            running.remove(0);
        }
    }
    let got = order.lock().unwrap().clone();
    for class in ["task", "wake"] {
        let seen: Vec<usize> = got
            .iter()
            .filter(|n| n.starts_with(class))
            .map(|n| n.split(' ').nth(1).unwrap().parse().unwrap())
            .collect();
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        assert_eq!(seen, sorted, "{class} turns out of their order: {seen:?}");
        assert_eq!(seen.len(), 100);
    }
    // No aging in play (the clock is paused): every wake before every task.
    let first_task = got.iter().position(|n| n.starts_with("task")).unwrap();
    assert_eq!(first_task, 100, "the wakes went first: {got:?}");
}

#[tokio::test(start_paused = true)]
async fn a_task_turn_waiting_past_the_step_is_admitted_ahead_of_new_wakes() {
    let adm = Admission::new(1, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let held = adm.try_take(Class::Wake).unwrap();
    let task = queue_one(&adm, Class::Task, "old task".into(), &order);
    settle().await;
    tokio::time::advance(AGE_STEP + Duration::from_secs(1)).await;
    let wake = queue_one(&adm, Class::Wake, "new wake".into(), &order);
    settle().await;
    drop(held);
    settle().await;
    assert_eq!(
        *order.lock().unwrap(),
        ["old task"],
        "a task that waited a step ranks with the wakes, and came first"
    );
    drop(task.await.unwrap());
    settle().await;
    drop(wake.await.unwrap());
    // Two steps: a task still ranks with the wakes only, so a new person's
    // input goes first (the rule that a task never ranks with a person).
    let held = adm.try_take(Class::Wake).unwrap();
    let task = queue_one(&adm, Class::Task, "older task".into(), &order);
    settle().await;
    tokio::time::advance(AGE_STEP * 2 + Duration::from_secs(1)).await;
    let typed = queue_one(&adm, Class::Person, "new typed".into(), &order);
    settle().await;
    drop(held);
    settle().await;
    assert_eq!(order.lock().unwrap()[2], "new typed");
    drop(typed.await.unwrap());
    settle().await;
    drop(task.await.unwrap());
    // A fresh task behind a fresh wake: no aging, the wake first.
    let held = adm.try_take(Class::Wake).unwrap();
    let task = queue_one(&adm, Class::Task, "fresh task".into(), &order);
    settle().await;
    let wake = queue_one(&adm, Class::Wake, "fresh wake".into(), &order);
    settle().await;
    drop(held);
    settle().await;
    assert_eq!(order.lock().unwrap()[4], "fresh wake");
    drop(wake.await.unwrap());
    settle().await;
    drop(task.await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn a_wait_past_the_window_while_the_queue_moves_does_not_fail() {
    let adm = Admission::new(1, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut held = adm.try_take(Class::Task).unwrap();
    // Ten turns ahead of the last, each holding its slot 100 s: the last
    // waits 1,000 s, past the 600 s window, with a turn admitted every 100.
    let ahead: Vec<_> = (0..10)
        .map(|i| queue_one(&adm, Class::Task, format!("ahead {i}"), &order))
        .collect();
    settle().await;
    let last = {
        let adm = adm.clone();
        tokio::spawn(async move { adm.take(Class::Task, false).await })
    };
    settle().await;
    for h in ahead {
        tokio::time::advance(Duration::from_secs(100)).await;
        drop(held);
        settle().await;
        held = h.await.unwrap();
    }
    tokio::time::advance(Duration::from_secs(100)).await;
    drop(held);
    settle().await;
    let slot = last.await.unwrap().expect("a moving queue fails no turn");
    assert_eq!(slot.class(), Class::Task);
}

#[tokio::test(start_paused = true)]
async fn a_stuck_queue_fails_its_waiter_with_words_that_say_so() {
    let adm = Admission::new(2, STUCK);
    let _a = adm.try_take(Class::Wake).unwrap();
    let _b = adm.try_take(Class::Wake).unwrap();
    let waiter = {
        let adm = adm.clone();
        tokio::spawn(async move { adm.take(Class::Wake, false).await })
    };
    settle().await;
    tokio::time::advance(STUCK - Duration::from_secs(1)).await;
    settle().await;
    assert!(!waiter.is_finished(), "not before the window");
    tokio::time::advance(Duration::from_secs(2)).await;
    let stuck = waiter.await.unwrap().expect_err("a queue that never moved");
    let words = stuck.to_string();
    assert!(
        words.starts_with("admission is stuck: no turn was admitted for 600 s"),
        "{words}"
    );
    assert!(words.contains("wake turn"), "{words}");
    assert!(words.contains("2 of 2 slots held"), "{words}");
    assert_eq!(adm.load(), (2, 0), "the failed waiter left the queue");
}

#[tokio::test(start_paused = true)]
async fn a_lent_slot_goes_to_the_next_turn_and_is_taken_back_at_the_head() {
    let adm = Admission::new(1, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut mine = adm.try_take(Class::Wake).unwrap();
    mine.bind("exe_job");
    let other = queue_one(&adm, Class::Task, "other".into(), &order);
    let later = queue_one(&adm, Class::Wake, "later wake".into(), &order);
    settle().await;
    assert!(adm.lend("exe_job"), "the job's wait lends its slot");
    assert!(!adm.lend("exe_job"), "once");
    settle().await;
    // The next by class: the later wake, not the task.
    assert_eq!(*order.lock().unwrap(), ["later wake"]);
    let back = {
        let adm = adm.clone();
        tokio::spawn(async move { adm.retake("exe_job").await })
    };
    settle().await;
    assert!(!back.is_finished(), "no slot is free: it waits");
    drop(later.await.unwrap());
    settle().await;
    assert!(back.is_finished(), "the job's turn is its class's head");
    assert_eq!(
        *order.lock().unwrap(),
        ["later wake"],
        "the task still waits"
    );
    drop(mine);
    settle().await;
    assert_eq!(*order.lock().unwrap(), ["later wake", "other"]);
    drop(other.await.unwrap());
    assert_eq!(adm.load(), (0, 0));
    assert!(!adm.lend("exe_job"), "a dropped slot is no one's to lend");
}

#[tokio::test(start_paused = true)]
async fn a_waiter_that_went_away_loses_no_slot() {
    let adm = Admission::new(1, STUCK);
    let held = adm.try_take(Class::Task).unwrap();
    let gone = {
        let adm = adm.clone();
        tokio::spawn(async move { adm.take(Class::Person, false).await })
    };
    settle().await;
    gone.abort();
    settle().await;
    drop(held);
    assert_eq!(adm.load(), (0, 0), "the slot is free, not held for no one");
    assert!(adm.try_take(Class::Task).is_some());
}

/// Releases racing arrivals on a runtime of four threads, seeded: every turn
/// is admitted, the ceiling never passed, and the queue ends empty. A lost
/// wake-up would leave a turn waiting with a slot free, and the run would
/// hang (the test's timeout) or end with a slot held for no one.
#[test]
fn no_wake_up_is_lost_when_releases_race_arrivals() {
    for seed in [1u64, 7, 42, 1_000_003] {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let adm = Admission::new(3, STUCK);
            let running = Arc::new(AtomicU32::new(0));
            let peak = Arc::new(AtomicU32::new(0));
            let mut rng = seed;
            let mut next = move || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng
            };
            let mut handles = Vec::new();
            // The queue's invariant, sampled all through the run.
            let sampler = {
                let adm = adm.clone();
                tokio::spawn(async move {
                    let mut lost = 0u32;
                    loop {
                        lost += u32::from(adm.waits_with_a_slot_free());
                        if adm.admitted() >= 400 && adm.load() == (0, 0) {
                            return lost;
                        }
                        tokio::task::yield_now().await;
                    }
                })
            };
            for i in 0..400u64 {
                let (adm, running, peak) = (adm.clone(), running.clone(), peak.clone());
                let class = Class::ALL[(next() % 3) as usize];
                let hold_us = next() % 300;
                let lend = next() % 4 == 0;
                let exec = format!("exe_{seed}_{i}");
                handles.push(tokio::spawn(async move {
                    let mut slot = adm.take(class, false).await.expect("never stuck");
                    slot.bind(&exec);
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    if lend {
                        running.fetch_sub(1, Ordering::SeqCst);
                        adm.lend(&exec);
                        tokio::task::yield_now().await;
                        adm.retake(&exec).await;
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                    }
                    tokio::time::sleep(Duration::from_micros(hold_us)).await;
                    running.fetch_sub(1, Ordering::SeqCst);
                    drop(slot);
                }));
                if next() % 3 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            let all = futures_util::future::join_all(handles);
            tokio::time::timeout(Duration::from_secs(30), all)
                .await
                .expect("every turn admitted: none waits with a slot free");
            let lost = sampler.await.unwrap();
            assert_eq!(lost, 0, "seed {seed}: a turn waited with a slot free");
            assert!(peak.load(Ordering::SeqCst) <= 3, "the ceiling held");
            assert_eq!(adm.load(), (0, 0), "seed {seed}: the queue ends empty");
            assert_eq!(adm.tasks_held(), 0, "seed {seed}: the task count leaked");
            assert!(
                adm.admitted() >= 400,
                "seed {seed}: every turn, and each retake"
            );
        });
    }
}

/// Wait for `fut` for a second of the paused clock; a hang is a failure.
async fn within<T>(what: &str, fut: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(1), fut)
        .await
        .unwrap_or_else(|_| panic!("{what} never returned"))
}

#[tokio::test(start_paused = true)]
async fn a_task_turn_never_takes_the_last_slot() {
    let adm = Admission::new(4, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for i in 0..4 {
        handles.push(queue_one(&adm, Class::Task, format!("task {i}"), &order));
        settle().await;
    }
    assert_eq!(order.lock().unwrap().len(), 3, "three tasks, not four");
    assert_eq!(adm.load(), (3, 1), "the fourth waits with a slot free");
    let person = within("a person's turn", adm.take(Class::Person, false))
        .await
        .expect("granted at once");
    assert_eq!(adm.load(), (4, 1));
    // A task ending frees a task slot: the waiting fourth is admitted.
    drop(person);
    settle().await;
    assert_eq!(order.lock().unwrap().len(), 3, "still at its limit");
    let first = handles.remove(0).await.unwrap();
    drop(first);
    settle().await;
    assert_eq!(order.lock().unwrap().len(), 4, "a task slot freed");
    assert!(adm.try_take(Class::Task).is_none());
    // The task count is the rule's own counter: drain every holder and say it
    // is back to zero, rather than inferring it from `load`.
    for h in handles {
        drop(h.await.unwrap());
    }
    settle().await;
    assert_eq!(adm.load(), (0, 0), "the queue ends empty");
    assert_eq!(adm.tasks_held(), 0, "the task count leaked");
}

#[tokio::test(start_paused = true)]
async fn a_free_slot_nobody_may_take_is_still_the_queue_moving() {
    let stuck = Duration::from_millis(200);
    let adm = Admission::new(2, stuck);
    let holder = adm.try_take(Class::Task).unwrap();
    let person = adm.try_take(Class::Person).unwrap();
    let waiter = {
        let adm = adm.clone();
        tokio::spawn(async move { adm.take(Class::Task, false).await })
    };
    settle().await;
    tokio::time::advance(Duration::from_millis(150)).await;
    drop(person); // hand_on finds only a declined task: the queue moved
    settle().await;
    tokio::time::advance(Duration::from_millis(150)).await;
    settle().await;
    assert!(
        !waiter.is_finished(),
        "300 ms in, but the queue moved at 150"
    );
    assert_eq!(adm.load(), (1, 1));
    drop(holder);
    settle().await;
    let slot = within("the waiter", waiter)
        .await
        .unwrap()
        .expect("admitted");
    assert_eq!(slot.class(), Class::Task);
}

#[tokio::test(start_paused = true)]
async fn a_retake_is_not_blocked_by_the_task_limit() {
    let adm = Admission::new(2, STUCK);
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut a = adm.try_take(Class::Task).unwrap();
    a.bind("exe_a");
    assert!(
        adm.try_take(Class::Task).is_none(),
        "limit is 1 at ceiling 2"
    );
    assert!(adm.lend("exe_a"));
    // While A lends, B takes the task slot and a third task waits.
    let b = adm.take(Class::Task, false).await.unwrap();
    let third = queue_one(&adm, Class::Task, "third".into(), &order);
    settle().await;
    assert_eq!(adm.load(), (1, 1));
    // A comes back: it held a slot, so the limit does not refuse it.
    within("the retake", adm.retake("exe_a")).await;
    assert_eq!(
        adm.load(),
        (2, 1),
        "the retake holds, the fresh waiter waits"
    );
    assert!(
        order.lock().unwrap().is_empty(),
        "the fresh waiter did not jump"
    );
    drop(b);
    drop(a);
    settle().await;
    assert_eq!(*order.lock().unwrap(), ["third"]);
    drop(third.await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn a_task_turn_ages_to_the_wakes_rank_and_no_higher() {
    let step = Duration::from_millis(100);
    let adm = Admission::with_step(1, STUCK, step);
    let order = Arc::new(Mutex::new(Vec::new()));
    let held = adm.try_take(Class::Wake).unwrap();
    let old = queue_one(&adm, Class::Task, "old task".into(), &order);
    settle().await;
    tokio::time::advance(step * 3).await;
    let fresh_task = queue_one(&adm, Class::Task, "fresh task".into(), &order);
    let person = queue_one(&adm, Class::Person, "person".into(), &order);
    settle().await;
    drop(held);
    settle().await;
    assert_eq!(
        *order.lock().unwrap(),
        ["person"],
        "an aged task is not a person"
    );
    drop(person.await.unwrap());
    settle().await;
    assert_eq!(
        order.lock().unwrap()[1],
        "old task",
        "aging still serves the oldest task first"
    );
    drop(old.await.unwrap());
    settle().await;
    drop(fresh_task.await.unwrap());
    // A wake still ages to a person's rank, ahead of a newer person.
    let held = adm.try_take(Class::Wake).unwrap();
    let wake = queue_one(&adm, Class::Wake, "old wake".into(), &order);
    settle().await;
    tokio::time::advance(step * 2).await;
    let person = queue_one(&adm, Class::Person, "new person".into(), &order);
    settle().await;
    drop(held);
    settle().await;
    assert_eq!(order.lock().unwrap()[3], "old wake");
    drop(wake.await.unwrap());
    settle().await;
    drop(person.await.unwrap());
}

#[test]
fn the_rule_lowers_a_limit_and_never_raises_the_ceiling() {
    // The slot rule lowers what task turns may hold; it never raises the
    // ceiling. A failing test is not a reason to raise either one.
    assert_eq!(Admission::new(8, STUCK).ceiling(), 8);
    assert_eq!(Admission::new(8, STUCK).task_limit(), 7);
    assert_eq!(
        Admission::new(1, STUCK).task_limit(),
        1,
        "ceiling 1 still runs tasks"
    );
    let cfg = crate::config::KernelSection::default();
    assert_eq!(cfg.admission_ceiling, 8, "no default moved");
    assert_eq!(cfg.admission_stuck_secs, 600);
}

/// A sink that keeps what it is told.
fn told(adm: &Arc<Admission>) -> Arc<Mutex<Vec<String>>> {
    let notes = Arc::new(Mutex::new(Vec::new()));
    let sink = notes.clone();
    adm.set_notice(Notice::new(move |text| sink.lock().unwrap().push(text)));
    notes
}

/// A ceiling-1 queue whose one slot is held, by `exe_held`, for good.
fn held_for_good(adm: &Arc<Admission>) -> Slot {
    let mut held = adm.try_take(Class::Wake).expect("a free slot");
    held.bind("exe_held");
    held
}

fn wait_as(
    adm: &Arc<Admission>,
    class: Class,
) -> tokio::task::JoinHandle<Result<Slot, super::Stuck>> {
    let adm = adm.clone();
    tokio::spawn(async move { adm.take(class, false).await })
}

#[tokio::test(start_paused = true)]
async fn a_person_waits_past_the_window_and_one_notice_names_the_holder() {
    let adm = Admission::new(1, STUCK);
    let notes = told(&adm);
    let _held = held_for_good(&adm);
    let waiter = wait_as(&adm, Class::Person);
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    settle().await;
    assert!(!waiter.is_finished(), "a person's wait is not failed");
    assert_eq!(adm.load(), (1, 1), "the waiter is still queued");
    let notes = notes.lock().unwrap();
    assert_eq!(notes.len(), 1, "one notice: {notes:?}");
    assert!(notes[0].contains("exe_held for 601 s"), "{}", notes[0]);
    assert!(notes[0].contains("1 of 1 slots held"), "{}", notes[0]);
}

#[tokio::test(start_paused = true)]
async fn a_task_waiter_still_fails_on_the_window() {
    let adm = Admission::new(1, STUCK);
    let notes = told(&adm);
    let _held = held_for_good(&adm);
    let waiter = wait_as(&adm, Class::Task);
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    let stuck = waiter.await.unwrap().expect_err("a queue that never moved");
    assert_eq!(
        stuck,
        super::Stuck {
            window_s: 600,
            class: "task",
            in_use: 1,
            ceiling: 1,
            waiting: 0,
        }
    );
    assert_eq!(adm.load(), (1, 0), "the failed waiter left the queue");
    assert!(
        notes.lock().unwrap().is_empty(),
        "a task's failure is told by its error"
    );
}

#[tokio::test(start_paused = true)]
async fn a_stuck_episode_is_told_once_however_long_it_lasts() {
    let adm = Admission::new(1, STUCK);
    let notes = told(&adm);
    let _held = held_for_good(&adm);
    let waiter = wait_as(&adm, Class::Person);
    settle().await;
    for _ in 0..4 {
        tokio::time::advance(STUCK + Duration::from_secs(1)).await;
        settle().await;
    }
    assert!(!waiter.is_finished());
    let notes = notes.lock().unwrap();
    assert_eq!(
        notes.len(),
        1,
        "past 3 windows, still one notice: {notes:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_person_who_kept_waiting_is_admitted_when_the_holder_ends() {
    let adm = Admission::new(1, STUCK);
    let notes = told(&adm);
    let held = held_for_good(&adm);
    let waiter = wait_as(&adm, Class::Person);
    settle().await;
    tokio::time::advance(STUCK * 2 + Duration::from_secs(5)).await;
    settle().await;
    assert!(!waiter.is_finished());
    assert_eq!(notes.lock().unwrap().len(), 1);
    drop(held);
    let slot = within("the person's grant", waiter)
        .await
        .unwrap()
        .expect("admitted once a slot came");
    assert_eq!(slot.class(), Class::Person);
    assert_eq!(adm.load(), (1, 0));
    // The queue moved, so the episode is over: a new one is told afresh.
    let mut again = slot;
    again.bind("exe_again");
    let waiter = wait_as(&adm, Class::Person);
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    settle().await;
    assert!(!waiter.is_finished());
    let notes = notes.lock().unwrap();
    assert_eq!(notes.len(), 2, "a later episode is told again: {notes:?}");
    assert!(notes[1].contains("exe_again"), "{}", notes[1]);
}

#[tokio::test(start_paused = true)]
async fn a_person_with_no_notice_sink_simply_keeps_waiting() {
    let adm = Admission::new(1, STUCK);
    let held = held_for_good(&adm);
    let waiter = wait_as(&adm, Class::Person);
    settle().await;
    tokio::time::advance(STUCK * 3).await;
    settle().await;
    assert!(!waiter.is_finished());
    drop(held);
    within("the person's grant", waiter)
        .await
        .unwrap()
        .expect("admitted");
}

#[tokio::test(start_paused = true)]
async fn a_persons_continue_path_sweeps_the_waiters_that_went_away() {
    let adm = Admission::new(1, STUCK);
    let _held = held_for_good(&adm);
    let gone = wait_as(&adm, Class::Person);
    settle().await;
    gone.abort();
    settle().await;
    assert_eq!(adm.load(), (1, 1), "the departed waiter's entry stays");
    let person = wait_as(&adm, Class::Person);
    settle().await;
    assert_eq!(adm.load(), (1, 2));
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    settle().await;
    assert!(!person.is_finished(), "the person keeps waiting");
    assert_eq!(
        adm.load(),
        (1, 1),
        "the window's pass clears the entry of the waiter that went away"
    );
}

/// The typed-input meaning steering and the never-failed wait share
/// (theseus-fzaep): a conversation's person, or the owner's input into a task.
#[test]
fn only_a_persons_typed_input_or_the_owners_into_a_task_is_typed() {
    for (class, owner, typed) in [
        (Class::Person, false, true),
        (Class::Person, true, true),
        (Class::Task, true, true),
        (Class::Task, false, false),
        (Class::Wake, false, false),
        (Class::Wake, true, false),
    ] {
        assert_eq!(class.is_typed(owner), typed, "{class:?}, owner {owner}");
    }
}

#[tokio::test(start_paused = true)]
async fn the_owners_input_into_a_task_waits_past_the_window_and_one_notice_names_the_holder() {
    let adm = Admission::new(1, STUCK);
    let notes = told(&adm);
    let _held = held_for_good(&adm);
    let waiter = wait_typed(&adm, Class::Task, true);
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    settle().await;
    assert!(
        !waiter.is_finished(),
        "the owner's typed input into a task is never failed at `take`'s window"
    );
    assert_eq!(adm.load(), (1, 1), "the waiter is still queued");
    let notes = notes.lock().unwrap();
    assert_eq!(notes.len(), 1, "one notice: {notes:?}");
    assert!(notes[0].contains("exe_held for 601 s"), "{}", notes[0]);
}

#[tokio::test(start_paused = true)]
async fn the_owners_input_into_a_task_is_admitted_as_a_task_when_the_holder_ends() {
    let adm = Admission::new(1, STUCK);
    let held = held_for_good(&adm);
    let waiter = wait_typed(&adm, Class::Task, true);
    settle().await;
    tokio::time::advance(STUCK * 2 + Duration::from_secs(5)).await;
    settle().await;
    assert!(!waiter.is_finished());
    drop(held);
    let slot = within("the owner's grant", waiter)
        .await
        .unwrap()
        .expect("admitted once a slot came");
    assert_eq!(slot.class(), Class::Task, "it queues and counts as a task");
}

#[tokio::test(start_paused = true)]
async fn a_task_turn_that_is_not_the_owners_input_still_fails_on_the_window() {
    let adm = Admission::new(1, STUCK);
    let _held = held_for_good(&adm);
    let waiter = wait_typed(&adm, Class::Task, false);
    settle().await;
    tokio::time::advance(STUCK + Duration::from_secs(1)).await;
    let stuck = waiter.await.unwrap().expect_err("a queue that never moved");
    assert_eq!(stuck.class, "task");
    assert_eq!(adm.load(), (1, 0), "the failed waiter left the queue");
}

fn wait_typed(
    adm: &Arc<Admission>,
    class: Class,
    typed: bool,
) -> tokio::task::JoinHandle<Result<Slot, super::Stuck>> {
    let adm = adm.clone();
    tokio::spawn(async move { adm.take_as(class, false, typed).await })
}

/// The runner's own admission of an input's turn, which waits for the same
/// execution's previous turn to end: `Admission::take` never fails a person,
/// and this wait must not either (it is the next typed message's wait behind
/// the person's own turn). The owner's typed input into a task is a person's
/// too (theseus-fzaep); any other task turn, and a wake, still fails on the
/// window.
mod behind_its_own_turn {
    use super::*;
    use crate::provider::FakeProvider;
    use crate::store::Store;
    use crate::{Config, Core};
    use theseus_protocol::SessionKind;

    fn core() -> (Arc<Core>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let core = Core::build(crate::rpc::Parts::for_tests(
            cfg,
            Arc::new(FakeProvider::scripted(vec![])),
            store,
        ))
        .unwrap();
        (core, dir)
    }

    /// An execution of `kind` whose turn is held and never ends.
    fn held_execution(
        core: &Arc<Core>,
        session: &str,
        kind: SessionKind,
    ) -> (String, theseus_kernel::TurnGuard) {
        let e = core
            .kernel
            .open_execution(
                session,
                kind,
                theseus_kernel::Authority {
                    principal: "test".into(),
                    delegated_by: None,
                    ceilings: Default::default(),
                },
                None,
                None,
            )
            .unwrap();
        let turn = core.kernel.admit_input(&e.id).unwrap();
        (e.id, turn)
    }

    /// An admit of `exec` in `class` with a wait (`owner`: the owner's input),
    /// on a task of its own.
    fn admit_as(
        core: &Arc<Core>,
        exec: &str,
        class: Class,
        owner: bool,
    ) -> tokio::task::JoinHandle<Result<(), String>> {
        let (core, exec) = (core.clone(), exec.to_string());
        tokio::spawn(async move {
            let mut frames = 0;
            match core
                .runner
                .admit(
                    &exec,
                    std::time::Instant::now(),
                    true,
                    class,
                    owner,
                    &mut frames,
                )
                .await
            {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{e:#}")),
            }
        })
    }

    const WORDS: &str = "did not end in 600 s";

    #[tokio::test(start_paused = true)]
    async fn a_persons_next_message_waits_for_its_own_turn_past_the_window() {
        let (core, _dir) = core();
        let (exec, _turn) = held_execution(&core, "ses_behind", SessionKind::Conversation);
        let person = admit_as(&core, &exec, Class::Person, false);
        // A unit guard on the predicate, not coverage of the wake surface:
        // `turn.rs` is the one caller, and it never asks for a wait in the
        // wake class.
        let wake = admit_as(&core, &exec, Class::Wake, false);
        settle().await;
        tokio::time::advance(STUCK * 2 + Duration::from_secs(1)).await;
        settle().await;
        let failed = within("the wake's failure", wake).await.unwrap();
        assert!(
            failed.is_err_and(|e| e.contains(WORDS)),
            "a wake's wait fails on the window"
        );
        assert!(
            !person.is_finished(),
            "a person's wait behind their own turn is not failed"
        );
        person.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn the_owners_message_into_a_task_waits_for_the_tasks_running_turn_past_the_window() {
        let (core, _dir) = core();
        let (exec, _turn) = held_execution(&core, "ses_task_owner", SessionKind::Task);
        let owner = admit_as(&core, &exec, Class::Task, true);
        settle().await;
        tokio::time::advance(STUCK * 2 + Duration::from_secs(1)).await;
        settle().await;
        assert!(
            !owner.is_finished(),
            "the owner's typed message into a task is not failed (it was: {WORDS:?})"
        );
        owner.abort();
    }

    /// Every slot held by turns that never end, on an execution of its own
    /// that is free: the wait is for a slot (`Admission::take`'s window), not
    /// for the execution's turn.
    fn all_slots_held(core: &Arc<Core>) -> Vec<Slot> {
        let adm = &core.runner.tools.admission;
        let mut held = Vec::new();
        while let Some(mut slot) = adm.try_take(Class::Wake) {
            slot.bind(&format!("exe_held_{}", held.len()));
            held.push(slot);
        }
        assert!(!held.is_empty());
        held
    }

    #[tokio::test(start_paused = true)]
    async fn the_owners_message_into_a_task_never_reaches_the_slot_queues_stuck_error() {
        let (core, _dir) = core();
        let _held = all_slots_held(&core);
        let e = core
            .kernel
            .open_execution(
                "ses_task_slots",
                SessionKind::Task,
                theseus_kernel::Authority {
                    principal: "test".into(),
                    delegated_by: None,
                    ceilings: Default::default(),
                },
                None,
                None,
            )
            .unwrap();
        let owner = admit_as(&core, &e.id, Class::Task, true);
        let other = admit_as(&core, &e.id, Class::Task, false);
        settle().await;
        tokio::time::advance(STUCK * 2 + Duration::from_secs(1)).await;
        settle().await;
        let failed = within("the other task turn's failure", other)
            .await
            .unwrap();
        assert!(
            failed.is_err_and(|e| e.contains("admission is stuck: no turn was admitted")),
            "a task turn that is not the owner's input fails at the slot queue's window"
        );
        assert!(
            !owner.is_finished(),
            "the owner's message waits for a slot and never takes `Err(Stuck)`"
        );
        owner.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_task_turn_that_is_not_the_owners_input_still_fails_on_the_window() {
        let (core, _dir) = core();
        let (exec, _turn) = held_execution(&core, "ses_task_other", SessionKind::Task);
        let other = admit_as(&core, &exec, Class::Task, false);
        settle().await;
        tokio::time::advance(STUCK * 2 + Duration::from_secs(1)).await;
        settle().await;
        let failed = within("the task turn's failure", other).await.unwrap();
        assert!(
            failed.is_err_and(|e| e.contains(WORDS)),
            "a task turn that is not the owner's typed input keeps today's bound"
        );
    }
}
