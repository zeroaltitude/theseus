//! The vault's reads, counted (issue 44). Every test here runs a fake `op`, a
//! shell script that writes its first argument to a `runs` file each time it
//! starts and answers by the word in its `mode` file: the count of its runs is
//! the assertion. No real vault is read, and every value is a made-up literal.
//! A day is a virtual one: the rounds sleep on a clock the test moves, while
//! each `op` runs for real.

use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

use super::pace::{Clock, SystemClock, LADDER};
use super::*;

/// The fake `op`: `ok` answers every read and injection; `missing` refuses
/// every reference as an item that is not there; `limited` says the vault's
/// rate limit, naming an item and a vault in its words; `limited2h` says it
/// with a reset two hours away; `twowaits` fails an injection as a missing
/// item, then refuses the read of `short` for a minute and every other read
/// for two hours; `some` fails an injection and the read of
/// `gone`, and answers every other read.
const FAKE_OP: &str = r#"#!/bin/sh
D="$(dirname "$0")"
printf '%s\n' "$1" >> "$D/runs"
case "$(cat "$D/mode")" in
  ok)
    case "$1" in
      read) printf 'made-up-read-value\n' ;;
      inject) awk '/^\{\{ .* \}\}$/ { n++; printf "made-up-inject-%d\n\n", n; next } { print }' ;;
    esac ;;
  missing) echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1 ;;
  limited) echo "[ERROR] 2026/11/22 12:00:00 Too many requests reading \"made-up-item-words\" in \"made-up-vault-words\"" >&2; exit 1 ;;
  limited2h) echo "[ERROR] 2026/11/22 12:00:00 Too many requests. Retry after 2 hours" >&2; exit 1 ;;
  twowaits)
    if [ "$1" = inject ]; then echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1; fi
    case "$3" in
      *short*) echo "[ERROR] 2026/11/22 12:00:00 Too many requests. Retry after 60 seconds" >&2; exit 1 ;;
      *) echo "[ERROR] 2026/11/22 12:00:00 Too many requests. Retry after 2 hours" >&2; exit 1 ;;
    esac ;;
  some)
    if [ "$1" = inject ]; then echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1; fi
    case "$3" in
      *gone*) echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1 ;;
      *) printf 'made-up-read-value\n' ;;
    esac ;;
esac
"#;

/// A fake `op` in `dir`, answering by `mode`, counting into `reads`.
fn fake_op(dir: &std::path::Path, mode: &str, reads: Arc<VaultReads>) -> Arc<OpReader> {
    use std::os::unix::fs::PermissionsExt;
    let op_bin = dir.join("op");
    std::fs::write(&op_bin, FAKE_OP).unwrap();
    std::fs::set_permissions(&op_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("mode"), mode).unwrap();
    Arc::new(OpReader::with_bin("test-not-a-token", op_bin).counting_into(reads))
}

/// What the fake `op` was started as, each run its first argument.
fn runs(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("runs"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn refs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A clock the test moves: each sleep moves it on at once, and the first
/// sleep that would pass `end` never ends and says so.
struct Virtual {
    now: AtomicU64,
    end: u64,
    past_end: tokio::sync::Notify,
}

impl Virtual {
    /// From an hour after a local midnight, for `secs` and a second more.
    fn span(secs: u64) -> Arc<Self> {
        Self::at(3600, secs)
    }

    /// From `after` seconds after a local midnight, for `secs` and a second
    /// more.
    fn at(after: u64, secs: u64) -> Arc<Self> {
        let start = crate::learning::local_midnight(theseus_protocol::now_unix_ms()) + after * 1000;
        Arc::new(Self {
            now: AtomicU64::new(start),
            end: start + secs * 1000 + 1000,
            past_end: tokio::sync::Notify::new(),
        })
    }
}

impl Clock for Virtual {
    fn now_ms(&self) -> u64 {
        self.now.load(SeqCst)
    }
    fn sleep(&self, d: Duration) -> BoxFuture<'static, ()> {
        let to = self.now.load(SeqCst) + d.as_millis() as u64;
        if to > self.end {
            self.past_end.notify_one();
            return Box::pin(std::future::pending());
        }
        self.now.store(to, SeqCst);
        Box::pin(tokio::task::yield_now())
    }
}

/// The daemon's ladder without its jitter, so a count is exact, and
/// `ceiling`.
fn pace(clock: Arc<Virtual>, ceiling: u32) -> Pace {
    Pace {
        ladder: LADDER.to_vec(),
        jitter: false,
        ceiling,
        clock,
    }
}

/// A board over `refs` counting into `reads`.
fn board(refs: &BTreeMap<String, String>, reads: Arc<VaultReads>) -> Arc<SecretBoard> {
    let b = SecretBoard::for_config(refs, Instant::now());
    b.count_from(reads);
    b
}

/// Run the rounds on `clock` until it passes its end.
async fn run_until_the_end(
    b: &Arc<SecretBoard>,
    refs: BTreeMap<String, String>,
    op: Arc<OpReader>,
    clock: &Arc<Virtual>,
    ceiling: u32,
) {
    tokio::spawn(resolve_with(
        b.clone(),
        refs,
        op,
        pace(clock.clone(), ceiling),
    ));
    tokio::time::timeout(Duration::from_secs(60), clock.past_end.notified())
        .await
        .expect("the rounds reached the clock's end");
}

/// Commit 1: each `op` the daemon starts is counted with the entries it asks
/// for, and health's `vault_today` says the day's count; a board with no
/// count says nothing.
#[tokio::test]
async fn every_op_the_daemon_starts_is_counted_and_health_says_the_days_reads() {
    let dir = tempfile::tempdir().unwrap();
    let reads = VaultReads::new(Arc::new(SystemClock));
    let op = fake_op(dir.path(), "ok", reads.clone());
    let refs = refs(&[
        ("a", "op://V/a/f"),
        ("b", "op://V/b/f"),
        ("c", "op://V/c/f"),
    ]);
    let b = board(&refs, reads.clone());
    tokio::spawn(resolve_into(b.clone(), refs.clone(), op.clone()));
    b.wait_settled(Duration::from_secs(10)).await.unwrap();
    assert_eq!(runs(dir.path()), ["inject"], "one op for three entries");
    let today = b.status().vault_today.expect("counted");
    assert_eq!((today.processes, today.references), (1, 3));
    assert_eq!(today.ceiling, pace::default_vault_reads_per_day());
    assert_eq!(today.stopped, None);
    op.read(&SecretRef::parse("op://V/d/f").unwrap())
        .await
        .unwrap();
    assert_eq!(runs(dir.path()), ["inject", "read"]);
    assert_eq!(reads.today(), (2, 4));
    let uncounted = SecretBoard::for_config(&refs, Instant::now());
    assert_eq!(uncounted.status().vault_today, None, "no count, no line");
}

/// Commit 2: one reference is one `op read`; two are one `op inject`, and,
/// when it fails, one `op read` each to name the bad one.
#[tokio::test]
async fn one_reference_is_one_op_read_and_two_are_one_inject() {
    let dir = tempfile::tempdir().unwrap();
    let op = fake_op(dir.path(), "some", VaultReads::new(Arc::new(SystemClock)));
    let one = op.fetch(&["op://V/gone/f".to_string()]).await;
    assert_eq!(runs(dir.path()), ["read"]);
    assert_eq!(one.method, "read");
    assert!(one.values[0].is_err());
    let two = op
        .fetch(&["op://V/good/f".to_string(), "op://V/gone/f".to_string()])
        .await;
    assert_eq!(runs(dir.path()), ["read", "inject", "read", "read"]);
    assert_eq!(two.method, "inject, then read");
    assert_eq!(
        two.values[0].as_ref().unwrap().as_str(),
        "made-up-read-value"
    );
    assert!(two.values[1].is_err());
}

/// Commits 2 and 3: a retry of one failing entry is one `op read`, where it
/// was an `op inject` and then an `op read`. The first round, with two entries, is
/// the injection and, since it failed, one read of each.
#[tokio::test]
async fn a_single_pending_retry_is_one_op_read() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(6);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "some", reads.clone());
    let refs = refs(&[("good", "op://V/good/f"), ("gone", "op://V/gone/f")]);
    let b = board(&refs, reads);
    run_until_the_end(&b, refs, op, &clock, u32::MAX).await;
    assert_eq!(
        runs(dir.path()),
        ["inject", "read", "read", "read"],
        "the first round, then one retry of `gone`: one read"
    );
    assert_eq!(b.get("good").unwrap().expose(), "made-up-read-value");
    assert_eq!(b.status().failed[0].name, "gone");
    assert_eq!(b.status().method.as_deref(), Some("read"));
}

/// Commit 3, the ladder: a virtual day of one entry that never resolves is
/// about thirty `op` runs (5 s, 10 s, 20 s, 40 s, 60 s, 10 min, then hourly).
/// The old pace, every minute after the first minute, was about 1,440 rounds
/// and two runs each: put the ladder's last rung back to 60 s and this count
/// fails.
#[tokio::test]
async fn a_failing_entry_is_fetched_about_thirty_times_a_day() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(24 * 3600);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "missing", reads.clone());
    let refs = refs(&[("gone", "op://V/gone/f")]);
    let b = board(&refs, reads);
    run_until_the_end(&b, refs, op, &clock, u32::MAX).await;
    let runs = runs(dir.path());
    assert_eq!(runs.len(), 30, "{} runs in a day", runs.len());
    assert!(runs.iter().all(|r| r == "read"), "{runs:?}");
    assert_eq!(b.status().state, "failed");
}

/// Commit 3, the vault's refusal: a rate limit names no bad entry, so no
/// entry is read one by one after the injection, and the next try waits an
/// hour. Three virtual hours: four injections and no read.
#[tokio::test]
async fn the_vaults_rate_limit_reads_no_entry_alone_and_waits_an_hour() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(3 * 3600);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "limited", reads.clone());
    let refs = refs(&[("a", "op://V/a/f"), ("b", "op://V/b/f")]);
    let b = board(&refs, reads.clone());
    run_until_the_end(&b, refs, op, &clock, u32::MAX).await;
    assert_eq!(runs(dir.path()), ["inject"; 4]);
    assert_eq!(reads.today(), (4, 8));
    for f in &b.status().failed {
        assert_eq!(
            f.error,
            "the vault refused the read: its rate limit; it is asked again in 1 h"
        );
    }
}

/// Commit 3, the ceiling: past the day's ceiling a failing entry is not
/// fetched again, and health says why in words.
#[tokio::test]
async fn past_the_days_ceiling_the_retries_stop() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(20 * 3600);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "missing", reads.clone());
    let refs = refs(&[("gone", "op://V/gone/f")]);
    let b = board(&refs, reads.clone());
    run_until_the_end(&b, refs, op, &clock, 5).await;
    assert_eq!(
        runs(dir.path()).len(),
        5,
        "rounds at 0, 5, 15, 35 and 75 s, then none"
    );
    let today = b.status().vault_today.unwrap();
    assert_eq!((today.references, today.ceiling), (5, 5));
    let why = today.stopped.expect("health says why");
    assert!(
        why.starts_with("retries stopped: the daemon read 5 vault entries today"),
        "{why}"
    );
    assert!(
        why.ends_with("after midnight, or when Theseus is next started"),
        "{why}"
    );
}

/// Round 2: the day's ceiling is kept before every round, for the round's
/// worst case: three failing entries are one injection and three reads (six
/// entries), so at a ceiling of 7 the second round, which could read six
/// more, never starts. Health says why.
#[tokio::test]
async fn a_batch_never_crosses_the_days_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(20 * 3600);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "missing", reads.clone());
    let refs = refs(&[
        ("a", "op://V/a/f"),
        ("b", "op://V/b/f"),
        ("c", "op://V/c/f"),
    ]);
    let b = board(&refs, reads.clone());
    run_until_the_end(&b, refs, op, &clock, 7).await;
    assert_eq!(runs(dir.path()), ["inject", "read", "read", "read"]);
    assert_eq!(reads.today(), (4, 6), "never past 7");
    let why = b.status().vault_today.unwrap().stopped.expect("stopped");
    assert!(
        why.contains("the next round could read 6 more, past the day's 7"),
        "{why}"
    );
}

/// Round 2: a refusal and the day's boundary are two deadlines, and the later
/// wins. A two-hour refusal at 23:00 with the day's ceiling reached waits its
/// two hours: no round at midnight, one at 01:00 on the new day.
#[tokio::test]
async fn a_refusal_spanning_midnight_waits_out_the_refusal() {
    for (secs, want) in [(90 * 60, 1), (150 * 60, 2)] {
        let dir = tempfile::tempdir().unwrap();
        let clock = Virtual::at(23 * 3600, secs);
        let reads = VaultReads::new(clock.clone());
        let op = fake_op(dir.path(), "limited2h", reads.clone());
        let refs = refs(&[("gone", "op://V/gone/f")]);
        let b = board(&refs, reads.clone());
        run_until_the_end(&b, refs, op, &clock, 1).await;
        assert_eq!(runs(dir.path()).len(), want, "{secs} s after 23:00");
    }
}

/// A log of everything the rounds write while it is held, as text.
#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

/// Round 2: on the refusal path the log and health carry the fixed sentence
/// and the reset, never `op`'s own words: the fake's stderr names an item and
/// a vault, and neither is anywhere, for an injection or a single read.
#[tokio::test]
async fn the_refusal_path_says_fixed_words_and_none_of_ops() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    for pairs in [
        &[("a", "op://V/a/f"), ("b", "op://V/b/f")][..],
        &[("a", "op://V/a/f")][..],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let clock = Virtual::span(60);
        let reads = VaultReads::new(clock.clone());
        let op = fake_op(dir.path(), "limited", reads.clone());
        let refs = refs(pairs);
        let b = board(&refs, reads);
        run_until_the_end(&b, refs, op, &clock, u32::MAX).await;
        assert_eq!(runs(dir.path()).len(), 1, "{pairs:?}");
        let health = serde_json::to_string(&b.status()).unwrap();
        assert!(
            health.contains("its rate limit; it is asked again in 1 h"),
            "{health}"
        );
        for words in ["made-up-item-words", "made-up-vault-words"] {
            assert!(!health.contains(words), "{health}");
        }
    }
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(log.contains("the vault's rate limit"), "{log}");
    for words in [
        "made-up-item-words",
        "made-up-vault-words",
        "Too many requests",
    ] {
        assert!(!log.contains(words), "{log}");
    }
}

/// Round 2: the refresh verb is out of this change, and its method with it.
#[test]
fn no_method_refreshes_the_vaults_entries() {
    assert!(!theseus_protocol::method::ALL.contains(&"secrets.refresh"));
}

/// Round 3: an ordinary failed injection, then fallback reads the vault refuses
/// with different waits (a minute, two hours): the round waits the LONGER, and
/// each refused entry says its own fixed sentence with no reference before it.
#[tokio::test]
async fn differing_fallback_refusals_wait_the_longest() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(90 * 60);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "twowaits", reads.clone());
    let refs = refs(&[("a", "op://V/short/f"), ("b", "op://V/long/f")]);
    let b = board(&refs, reads);
    run_until_the_end(&b, refs, op, &clock, u32::MAX).await;
    assert_eq!(
        runs(dir.path()),
        ["inject", "read", "read"],
        "no round before the two hours"
    );
    let failed: BTreeMap<String, String> = b
        .status()
        .failed
        .into_iter()
        .map(|f| (f.name, f.error))
        .collect();
    assert_eq!(
        failed["a"],
        "the vault refused the read: its rate limit; it is asked again in 1 min"
    );
    assert_eq!(
        failed["b"],
        "the vault refused the read: its rate limit; it is asked again in 2 h"
    );
}

/// Round 3: a first round that can never fit the day's ceiling (two vault
/// references, worst four entries, at a ceiling of 3) is not slept on for
/// ever: its vault entries fail at once in fixed words, no `op` runs, a local
/// entry beside them still resolves, and the board settles (`theseusd check`
/// waits on that).
#[tokio::test]
async fn a_batch_that_can_never_fit_fails_plainly_and_settles() {
    std::env::set_var("THESEUS_VAULT_BACKOFF_FITS", "made-up-local-value");
    let dir = tempfile::tempdir().unwrap();
    let clock = Virtual::span(3600);
    let reads = VaultReads::new(clock.clone());
    let op = fake_op(dir.path(), "ok", reads.clone());
    let refs = refs(&[
        ("a", "op://V/a/f"),
        ("b", "op://V/b/f"),
        ("local", "env:THESEUS_VAULT_BACKOFF_FITS"),
    ]);
    let b = board(&refs, reads);
    tokio::spawn(resolve_with(b.clone(), refs, op, pace(clock.clone(), 3)));
    b.wait_settled(Duration::from_secs(10))
        .await
        .expect("the board settles");
    assert!(runs(dir.path()).is_empty(), "no op ran");
    let st = b.status();
    assert_eq!(st.ready, ["local"]);
    for f in &st.failed {
        assert_eq!(
            f.error,
            "more vault entries than the daemon reads in a day: 2 distinct references, the day's ceiling \
             is 3"
        );
    }
    assert_eq!(st.failed.len(), 2);
}

