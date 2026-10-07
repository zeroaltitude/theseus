//! `blocking` and the runtime's end (theseus-fy0i).

use std::future::Future;
use std::sync::mpsc;
use std::task::{Context, Waker};
use std::time::Duration;

use super::blocking;

/// A runtime of one worker, with tokio's timer: the daemon's runtime, small.
fn one_worker() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap()
}

/// The time driver's own word that it has shut down: a timer an hour away,
/// registered now, which fires early only when the driver shuts down, as the
/// driver fires every timer it holds then.
fn canary(rt: &tokio::runtime::Runtime) -> std::pin::Pin<Box<tokio::time::Sleep>> {
    let _in = rt.enter();
    let mut s = Box::pin(tokio::time::sleep(Duration::from_secs(3600)));
    let polled = s.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(
        polled.is_pending(),
        "the canary is registered, an hour away"
    );
    s
}

/// A task whose `blocking` section outlives the start of its runtime's drop
/// polls no timer after the drop has shut the time driver down. The section
/// handed its worker's role to another thread, so the drop waits for no poll
/// of the task before the driver shuts down. The task used to poll its next
/// sleep on the way out of the section, and tokio panicked there ("A Tokio
/// 1.x context was found, but it is being shutdown"), which aborts a release
/// daemon: a stop soon after serving did, while a tender's read waited. The
/// section here is held until the driver fires the canary, so no sleep in the
/// test decides the order.
#[test]
fn a_section_that_outlives_the_runtimes_drop_polls_no_timer_after_it() {
    let rt = one_worker();
    let canary = canary(&rt);
    let (entered, held) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let task = rt.spawn(async move {
        blocking(|| {
            entered.send(()).unwrap();
            let _ = released.recv();
        });
        tokio::time::sleep(Duration::from_millis(1)).await;
    });
    held.recv().unwrap();
    // The worker's role went to another thread: a task spawned now runs while
    // the section still holds the worker's first thread.
    rt.block_on(rt.spawn(async {})).unwrap();
    let releaser = std::thread::spawn(move || {
        let t = std::time::Instant::now();
        while !canary.is_elapsed() && t.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(1));
        }
        let fired = canary.is_elapsed();
        release.send(()).unwrap();
        fired
    });
    drop(rt);
    assert!(
        releaser.join().unwrap(),
        "the runtime's drop never shut its time driver down"
    );
    let ended = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(task);
    let Err(e) = ended else {
        panic!("the task finished, though its runtime ended before its sleep could");
    };
    assert!(
        e.is_cancelled(),
        "the task polled a timer after its runtime's drop shut the driver down: {e}"
    );
}

/// The yield is only a yield: after a section the task has no budget left,
/// so its next await on a tokio resource gives way first, and then it runs
/// on as before, its sleep and all. On a current-thread runtime nothing is
/// handed over, and nothing is spent.
#[test]
fn after_a_section_the_task_gives_way_once_and_runs_on() {
    let rt = one_worker();
    let (spent, slept) = rt.block_on(async {
        tokio::spawn(async {
            blocking(|| ());
            let spent = !tokio::task::coop::has_budget_remaining();
            tokio::time::sleep(Duration::from_millis(1)).await;
            (spent, tokio::task::coop::has_budget_remaining())
        })
        .await
        .unwrap()
    });
    assert!(spent, "a section on a worker spends the task's budget");
    assert!(slept, "the task's next poll has a budget again");
    let current = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(current.block_on(async {
        blocking(|| ());
        tokio::task::coop::has_budget_remaining()
    }));
}
