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
/// rate limit; `some` fails an injection and the read of `gone`, and answers
/// every other read.
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
  limited) echo "[ERROR] 2026/11/22 12:00:00 Too many requests" >&2; exit 1 ;;
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
        let start = crate::learning::local_midnight(theseus_protocol::now_unix_ms()) + 3_600_000;
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
        assert!(f.error.contains("Too many requests"), "{f:?}");
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
