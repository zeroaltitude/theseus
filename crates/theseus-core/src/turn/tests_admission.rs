//! The admission queue (theseus-4g0l), on tokio's paused clock: class order,
//! first come first served inside a class, aging, the stuck window, a slot
//! lent and taken again, and no lost wake-up under a seeded stress.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Admission, Class, Slot, AGE_STEP};

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
    let held: Vec<Slot> = (0..3).map(|_| adm.try_take(Class::Task).unwrap()).collect();
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
    // Two steps: it ranks with a person's input, and passes a new one.
    let held = adm.try_take(Class::Wake).unwrap();
    let task = queue_one(&adm, Class::Task, "older task".into(), &order);
    settle().await;
    tokio::time::advance(AGE_STEP * 2 + Duration::from_secs(1)).await;
    let typed = queue_one(&adm, Class::Person, "new typed".into(), &order);
    settle().await;
    drop(held);
    settle().await;
    assert_eq!(order.lock().unwrap()[2], "older task");
    drop(task.await.unwrap());
    settle().await;
    drop(typed.await.unwrap());
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
    let _a = adm.try_take(Class::Task).unwrap();
    let _b = adm.try_take(Class::Task).unwrap();
    let waiter = {
        let adm = adm.clone();
        tokio::spawn(async move { adm.take(Class::Person, false).await })
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
    assert!(words.contains("person turn"), "{words}");
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
            assert!(
                adm.admitted() >= 400,
                "seed {seed}: every turn, and each retake"
            );
        });
    }
}
