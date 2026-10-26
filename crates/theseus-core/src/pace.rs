//! The wait between two chunks of background work: while the machine is busy,
//! up to [`theseus_store::pressure::BOUND`] a chunk (theseus-63tfk, theseus-tood).
//! A test never waits: the host's pressure is no input of its own. A run of the
//! onto sweep asks up to 20 pairs, and 19 waits of 10 s on a loaded host passed
//! nextest's 120 s kill (theseus-75t4q; `docs/design/onto-sweep-pace.md`).
//!
//! Every async caller in this crate goes through here, never to
//! `theseus_store::pressure::quiet` itself: [`pace`] for a loop that only waits
//! (the onto sweep, the people backfill and sweep), [`quiet`] for one that adds up
//! how long it yielded (the index's terms and shape stretches).
//!
//! Three tests hold the three halves. The **sites** go through here, by
//! `no_async_caller_in_core_waits_on_the_raw_pressure`, which reads this crate's
//! sources. The **skip** is held by `a_test_never_waits_out_a_busy_machine`, which fakes
//! the pressure, and for the onto sweep end to end by
//! `tests_onto_names::a_chain_clusters_on_a_busy_machine`. The **wait** is held in
//! `theseus-store`: its async loop by `pressure`'s own
//! `a_chunk_waits_while_busy_and_goes_when_quiet_within_the_bound` (a paused clock
//! over `quiet_with`), and its real `/proc/pressure` reading by `tests/pressure.rs`
//! over `quiet_blocking`. A daemon's own tests are not this crate's tests, so they
//! wait as in production.

use std::time::Duration;

/// The wait between two questions or chunks of one run, for a loop that has no use
/// for how long it took.
pub(crate) async fn pace() {
    quiet().await;
}

/// [`pace`], and how long it waited: zero in a test, and where the first look found
/// the machine quiet.
#[cfg(not(test))]
pub(crate) async fn quiet() -> Duration {
    theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await
}

/// A test's wait: none, and zero said so.
#[cfg(test)]
#[allow(clippy::unused_async, reason = "the daemon's version awaits")]
pub(crate) async fn quiet() -> Duration {
    Duration::ZERO
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use theseus_store::pressure::LOOK_EVERY;

    use super::{pace, quiet};
    use crate::tests_activation_pace::{namespaced, INNER};

    /// In a user and mount namespace of its own, with fake pressure files bound
    /// over `/proc/pressure` saying IO is busy (as `tests_activation_pace` does),
    /// the test's wait is none: [`quiet`] says zero where the production wait
    /// would have spent its whole bound, and [`pace`] goes at once. The control
    /// comes first: the store's own wait, given one look's bound, does wait in
    /// these namespaces, so the fake is busy and the zero is the skip, not a quiet
    /// machine. With the skip planted off (the production `quiet` in a test build),
    /// the test fails on `BOUND` where it wants zero, after one 10 s wait: no
    /// timeout to wait for. Where namespaces can't be made, it says so and passes.
    #[tokio::test(flavor = "current_thread")]
    async fn a_test_never_waits_out_a_busy_machine() {
        if std::env::var_os(INNER).is_some() {
            println!("PACE-INNER started");
            assert!(theseus_store::pressure::busy().is_some(), "the fake pressure files say busy");
            let control = theseus_store::pressure::quiet(LOOK_EVERY).await;
            assert!(control >= LOOK_EVERY, "the real wait waited {control:?}");
            assert_eq!(quiet().await, Duration::ZERO, "a test's wait is none");
            pace().await;
            println!("PACE-INNER ok");
            return;
        }
        let Some(text) = namespaced(
            "pace::tests::a_test_never_waits_out_a_busy_machine",
            "PACE-INNER",
        ) else {
            return;
        };
        assert!(text.contains("PACE-INNER ok"), "{text}");
    }

    /// Every `.rs` file under `dir` but `pace.rs` itself, as the lines of it that
    /// call the store's wait directly.
    fn raw_waits(dir: &Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                raw_waits(&path, hits);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path != Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pace.rs")
            {
                for (i, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    if line.contains("pressure::quiet(") {
                        hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }

    /// An async caller of `theseus_store::pressure::quiet` has a wait no test can
    /// skip: it goes through `pace::pace` or `pace::quiet`, the one place a test
    /// build leaves the wait out (theseus-qc5r6: the people backfill and sweep and the
    /// index's terms and shape stretches each had their own). `quiet_blocking`, for a
    /// thread that may sleep, is another name and is not matched.
    #[test]
    fn no_async_caller_in_core_waits_on_the_raw_pressure() {
        let mut hits = Vec::new();
        raw_waits(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut hits);
        assert!(
            hits.is_empty(),
            "call `crate::pace::pace` or `crate::pace::quiet`, not the store's wait:\n{}",
            hits.join("\n")
        );
    }
}
