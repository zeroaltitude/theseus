//! The trial's clock (theseus-p1mv9): no whole-trial timer, a bound on
//! silence and a ceiling, on tokio's paused clock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use theseus_tools::Tool;
use tokio::time::Instant;

use super::{
    wait_trial, Beat, Propose, CALL_TIMEOUT_SECS, MAX_TESTS, START_TIMEOUT_SECS, STOPPED,
    TRIAL_CEILING, TRIAL_SILENCE,
};

struct Flag(Arc<AtomicBool>);

impl Drop for Flag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn a_trial_that_keeps_finishing_steps_is_not_cut_at_a_hundred_seconds() {
    let beat = Arc::new(Beat::default());
    let b = beat.clone();
    let t0 = Instant::now();
    let mut task = tokio::spawn(async move {
        for _ in 0..8 {
            tokio::time::sleep(Duration::from_secs(15)).await;
            b.stamp();
        }
        "done"
    });
    let got = wait_trial(&mut task, &beat, TRIAL_SILENCE, TRIAL_CEILING).await;
    assert_eq!(got, Ok("done"));
    assert_eq!(beat.stamps(), 8);
    assert!(
        t0.elapsed() >= Duration::from_secs(120),
        "{:?}",
        t0.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn a_trial_that_goes_quiet_is_aborted_at_the_silence_bound() {
    let beat = Arc::new(Beat::default());
    let b = beat.clone();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Flag(dropped.clone());
    let t0 = Instant::now();
    let mut task = tokio::spawn(async move {
        let _guard = guard;
        for _ in 0..2 {
            tokio::time::sleep(Duration::from_secs(10)).await;
            b.stamp();
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    });
    let err = wait_trial(&mut task, &beat, TRIAL_SILENCE, TRIAL_CEILING)
        .await
        .unwrap_err();
    assert!(err.contains("without finishing a step"), "{err}");
    let quiet = t0.elapsed() - Duration::from_secs(20);
    assert!(
        quiet >= TRIAL_SILENCE && quiet <= TRIAL_SILENCE + Duration::from_secs(1),
        "{quiet:?}"
    );
    assert_eq!(beat.stamps(), 2);
    let _ = task.await;
    assert!(dropped.load(Ordering::SeqCst), "the task was not aborted");
}

#[tokio::test(start_paused = true)]
async fn a_trial_past_its_ceiling_is_aborted_though_it_keeps_stamping() {
    let beat = Arc::new(Beat::default());
    let b = beat.clone();
    let mut task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            b.stamp();
        }
    });
    let err = wait_trial(&mut task, &beat, TRIAL_SILENCE, Duration::from_secs(60))
        .await
        .unwrap_err();
    assert!(err.contains("ceiling"), "{err}");
    assert!(beat.stamps() >= 10);
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_trial_answers_stopped() {
    let beat = Arc::new(Beat::default());
    let mut task = tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    });
    let handle = task.abort_handle();
    let waiting = wait_trial(&mut task, &beat, TRIAL_SILENCE, TRIAL_CEILING);
    tokio::pin!(waiting);
    tokio::select! {
        _ = &mut waiting => panic!("returned before the abort"),
        () = tokio::time::sleep(Duration::from_secs(1)) => handle.abort(),
    }
    assert_eq!(waiting.await.unwrap_err(), STOPPED);
}

#[test]
fn the_trial_ceiling_is_the_sum_of_its_step_bounds_and_the_call_outlasts_it() {
    assert_eq!(
        TRIAL_CEILING,
        Duration::from_secs(START_TIMEOUT_SECS + MAX_TESTS as u64 * CALL_TIMEOUT_SECS)
    );
    assert!(TRIAL_SILENCE > Duration::from_secs(START_TIMEOUT_SECS.max(CALL_TIMEOUT_SECS)));
    assert!(Propose.deadline().unwrap() > TRIAL_CEILING);
}
