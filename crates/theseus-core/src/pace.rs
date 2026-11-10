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
//! So does a blocking one that a test of this crate reaches: [`quiet_blocking`] and
//! [`quiet_blocking_unless`] are the store's own two waits, with the bound the caller
//! gives, and in a test build they answer zero at once, unless the test faked the
//! machine's pressure itself (below). Four threads keep the store's wait, on purpose:
//! the nightly consolidation's plan, the learning tender's packs, the ledger pin and
//! the WAL's history check, which only `theseusd`'s start runs to a wait (the one test
//! that runs the check has a log under its 4 MiB stretch, which never waits). A daemon's
//! own tests link this crate without `cfg(test)`, so nothing here could skip theirs.
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

/// A test's wait: none, and zero said so. Written as a function returning a
/// ready future rather than an `async fn` that never awaits: the two are the
/// same to every caller (`quiet().await`), and this way the skip needs no
/// `#[allow(clippy::unused_async)]` — an added allow is a keel erosion, and a
/// test's convenience is not a reason to spend the owner's yes on one.
#[cfg(test)]
pub(crate) fn quiet() -> std::future::Ready<Duration> {
    std::future::ready(Duration::ZERO)
}

/// The store's blocking waits, as they are: a production build calls them, with the
/// bound it names (`theseus_store::pressure::BOUND` at every caller).
#[cfg(not(test))]
pub(crate) use theseus_store::pressure::{quiet_blocking, quiet_blocking_unless};

/// A test's blocking wait: none, unless the test has made the machine's pressure
/// an input of its own. The host's pressure is no input of a test (a test under
/// load waited its whole `bound` at every call); a `/proc/pressure` the test faked
/// is, and `tests_activation_pace::namespaced` says so by setting `INNER`, as the
/// tests that hold a stop ending the warm build's waits need: they wait as in
/// production there. Only whether the wait happens differs from production: the
/// bound and the stop are the caller's, as ever.
#[cfg(test)]
pub(crate) fn quiet_blocking_unless(bound: Duration, stopped: impl Fn() -> bool) -> Duration {
    if std::env::var_os(crate::tests_activation_pace::INNER).is_none() {
        return Duration::ZERO;
    }
    theseus_store::pressure::quiet_blocking_unless(bound, stopped)
}

/// [`quiet_blocking_unless`] with no stop to end it.
#[cfg(test)]
pub(crate) fn quiet_blocking(bound: Duration) -> Duration {
    quiet_blocking_unless(bound, || false)
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
            assert!(
                theseus_store::pressure::busy().is_some(),
                "the fake pressure files say busy"
            );
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
    /// `calls` says call the store's wait directly.
    fn raw_waits(dir: &Path, calls: fn(&str) -> bool, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                raw_waits(&path, calls, hits);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path != Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pace.rs")
            {
                for (i, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    if calls(line) {
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
    /// thread that may sleep, is another name, held by the next test.
    #[test]
    fn no_async_caller_in_core_waits_on_the_raw_pressure() {
        let mut hits = Vec::new();
        raw_waits(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            |line| line.contains("pressure::quiet("),
            &mut hits,
        );
        assert!(
            hits.is_empty(),
            "call `crate::pace::pace` or `crate::pace::quiet`, not the store's wait:\n{}",
            hits.join("\n")
        );
    }

    /// The files whose threads keep the store's blocking wait (the module's docs say
    /// why): the nightly consolidation's plan, the learning tender's packs, the ledger
    /// pin and the WAL's history check.
    const DAEMON_THREADS: [&str; 4] = [
        "consolidate/run.rs",
        "learning/tender.rs",
        "ledger_pin.rs",
        "rpc/mod.rs",
    ];

    /// A blocking caller in this crate waits through `pace::quiet_blocking` or
    /// `pace::quiet_blocking_unless`, which a test build skips (theseus-la5oc: six of
    /// them, in the import's frame boundaries, the sampler, the warm build's pages and
    /// the night's mining, each ran its whole bound under load in a test), except the
    /// four daemon threads. The four are asserted too, so the exception is no list kept
    /// by hand and checked by nothing: a thread that stops waiting on the store leaves
    /// it, and the scan that finds none of them has found nothing.
    #[test]
    fn no_blocking_caller_in_core_waits_on_the_raw_pressure_but_the_daemons_four() {
        let mut hits = Vec::new();
        raw_waits(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            |line| {
                line.contains("pressure")
                    && line.contains("quiet_blocking")
                    && !line.contains("pace::quiet_blocking")
            },
            &mut hits,
        );
        let kept = |h: &String, f: &str| h.contains(&format!("/src/{f}:"));
        for f in DAEMON_THREADS {
            assert!(
                hits.iter().any(|h| kept(h, f)),
                "{f} no longer calls the store's blocking wait: take it off DAEMON_THREADS\n{}",
                hits.join("\n")
            );
        }
        hits.retain(|h| !DAEMON_THREADS.iter().any(|f| kept(h, f)));
        assert!(
            hits.is_empty(),
            "call `crate::pace::quiet_blocking` or `crate::pace::quiet_blocking_unless`, not the store's wait:\n{}",
            hits.join("\n")
        );
    }
}
