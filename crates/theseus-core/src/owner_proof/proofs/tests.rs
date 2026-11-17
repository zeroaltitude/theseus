//! The keeper's tests: the check and its limit, single use, what a restart
//! keeps, what the ledger says, and enrolment that keeps nothing until a
//! code comes from the app. The secrets here are RFC 6238's own published one
//! or freshly random ones.

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::ledger::LedgerRow;

/// The RFC 6238 test secret.
fn rfc() -> Secret {
    Secret::from_bytes(*b"12345678901234567890")
}

/// A moment a long way from any lock in the file: a minute-aligned time.
const T0: u64 = 1_700_000_040_000;

struct Rig {
    dir: tempfile::TempDir,
    store: Store,
    now: Arc<AtomicU64>,
    proofs: Proofs,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let proofs = Self::keeper(dir.path(), store.clone(), &now);
        Self {
            dir,
            store,
            now,
            proofs,
        }
    }

    fn keeper(state: &Path, store: Store, now: &Arc<AtomicU64>) -> Proofs {
        let now = Arc::clone(now);
        Proofs::open(
            state,
            store,
            Arc::new(move || now.load(Ordering::SeqCst)),
            24,
        )
    }

    /// The same state dir and store, a new keeper: a restart.
    fn restarted(&mut self) {
        self.proofs = Self::keeper(self.dir.path(), self.store.clone(), &self.now);
    }

    /// Enrol RFC 6238's secret as the file holds it after an enrolment at
    /// `T0`, with the step it was confirmed at already used.
    fn enrolled(&self) {
        let h = Held::new(&rfc(), T0, totp::step_at(T0 / 1000));
        file::write(&self.dir.path().join(file::FILE), &h).unwrap();
    }

    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    /// The code the app shows `steps` steps after `T0`.
    fn code(&self, steps: u64) -> String {
        totp::code_at_step(&rfc(), totp::step_at(T0 / 1000) + steps)
    }

    fn rows(&self, kind: &str) -> Vec<LedgerRow> {
        self.store
            .ledger_tail::<LedgerRow>(1000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == kind)
            .collect()
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join(file::FILE)
    }
}

#[test]
fn a_check_with_nothing_enrolled_says_so() {
    let r = Rig::new();
    assert_eq!(r.proofs.check("123456", "cli"), Err(Refused::NotEnrolled));
    let rows = r.rows("owner.proof.checked");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["outcome"], "not_enrolled");
}

/// The first good code goes; the same code again is `Used`; the next step's goes.
#[test]
fn a_good_code_goes_once() {
    let r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    let code = r.code(1);
    assert_eq!(r.proofs.check(&code, "cli"), Ok(()));
    assert_eq!(r.proofs.check(&code, "web"), Err(Refused::Used));
    r.advance(30_000);
    assert_eq!(r.proofs.check(&r.code(2), "cli"), Ok(()));
}

/// An earlier code of the window is refused once a later one was taken: the
/// window never lets a code be used twice by walking back to it.
#[test]
fn an_earlier_code_is_used_once_a_later_one_was_taken() {
    let r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    assert_eq!(
        r.proofs.check(&r.code(2), "cli"),
        Ok(()),
        "one step ahead of the clock"
    );
    assert_eq!(r.proofs.check(&r.code(1), "cli"), Err(Refused::Used));
    assert_eq!(r.proofs.check(&r.code(0), "cli"), Err(Refused::Used));
}

/// A used code is refused after a restart, too: the step is in the file.
#[test]
fn a_used_code_stays_used_across_a_restart() {
    let mut r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    let code = r.code(1);
    assert_eq!(r.proofs.check(&code, "cli"), Ok(()));
    r.restarted();
    assert_eq!(r.proofs.check(&code, "cli"), Err(Refused::Used));
}

/// Five wrong codes inside ten minutes lock checking for ten minutes, a good
/// code included, and the lock holds across a restart.
#[test]
fn five_wrong_codes_lock_checking_even_for_a_good_code_and_across_a_restart() {
    let mut r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    for n in 0..4 {
        assert_eq!(
            r.proofs.check("000000", "cli"),
            Err(Refused::Wrong),
            "try {n}"
        );
        r.advance(1_000);
    }
    let fifth = r.proofs.check("000000", "web");
    let Err(Refused::TooMany { until_ms }) = fifth else {
        panic!("the fifth wrong code locks: {fifth:?}");
    };
    assert_eq!(until_ms, r.now.load(Ordering::SeqCst) + LOCK_MS);
    let good = r.code(1);
    assert_eq!(
        r.proofs.check(&good, "cli"),
        Err(Refused::TooMany { until_ms })
    );
    r.restarted();
    assert_eq!(
        r.proofs.check(&good, "discord:dm"),
        Err(Refused::TooMany { until_ms }),
        "a restart clears nothing"
    );
    assert_eq!(r.proofs.status().locked_until_ms, Some(until_ms));
    // Ten minutes later it is checked again, with a clean count.
    r.advance(LOCK_MS);
    assert_eq!(r.proofs.check(&r.code(21), "cli"), Ok(()));
}

/// The tries are counted over the window: slow wrong codes never lock.
#[test]
fn wrong_codes_ten_minutes_apart_never_lock() {
    let r = Rig::new();
    r.enrolled();
    for n in 0..8 {
        r.advance(TRIES_WITHIN_MS);
        assert_eq!(
            r.proofs.check("000000", "cli"),
            Err(Refused::Wrong),
            "try {n}"
        );
    }
}

/// Every surface shares the one count: wrong codes from the CLI and from a
/// card are five between them.
#[test]
fn the_limit_is_one_count_across_surfaces() {
    let r = Rig::new();
    r.enrolled();
    for place in ["cli", "web", "discord:dm", "cli", "web"] {
        let _ = r.proofs.check("000000", place);
    }
    assert!(matches!(
        r.proofs.check(&r.code(0), "discord:dm"),
        Err(Refused::TooMany { .. })
    ));
}

/// A good code clears the count: four wrong and a good one start again.
#[test]
fn a_good_code_clears_the_count() {
    let r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    for _ in 0..4 {
        assert_eq!(r.proofs.check("000000", "cli"), Err(Refused::Wrong));
    }
    assert_eq!(r.proofs.check(&r.code(1), "cli"), Ok(()));
    for _ in 0..4 {
        assert_eq!(r.proofs.check("000000", "cli"), Err(Refused::Wrong));
    }
}

/// Every check, good or bad, is a row: the outcome and the place, and never
/// the code.
#[test]
fn every_check_is_a_row_that_never_holds_the_code() {
    let r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    let good = r.code(1);
    let _ = r.proofs.check("987654", "cli");
    let _ = r.proofs.check(&good, "web");
    let _ = r.proofs.check(&good, "discord:dm");
    let rows = r.rows("owner.proof.checked");
    let seen: Vec<(String, String)> = rows
        .iter()
        .map(|row| {
            (
                row.data["outcome"].as_str().unwrap().into(),
                row.data["place"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [("wrong", "cli"), ("ok", "web"), ("used", "discord:dm")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    for row in &rows {
        let text = serde_json::to_string(row).unwrap();
        assert!(!text.contains(&good) && !text.contains("987654"), "{text}");
    }
}

/// The file is the owner's alone, and one that others can read is not
/// trusted: the check is refused and says why it cannot run.
#[test]
fn the_file_is_mode_0600_and_a_looser_one_is_not_trusted() {
    let r = Rig::new();
    r.enrolled();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&r.path()), 0o600);
    assert!(
        !r.dir.path().join("owner-proof.json.new").exists(),
        "the write's own file is renamed away"
    );
    std::fs::set_permissions(r.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(r.proofs.check(&r.code(0), "cli"), Err(Refused::Unavailable));
    assert!(r.proofs.status().unavailable);
}

/// A file that does not parse is no proof: refused, never "not enrolled".
#[test]
fn a_file_that_does_not_parse_is_unavailable_and_not_a_way_in() {
    let r = Rig::new();
    std::fs::write(r.path(), b"not json").unwrap();
    std::fs::set_permissions(r.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(r.proofs.check("000000", "cli"), Err(Refused::Unavailable));
}

/// A check that cannot save its counters is refused, so no code is accepted
/// on a footing that would let it be used twice.
#[test]
fn a_check_that_cannot_save_is_refused() {
    let r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    // The write goes beside the file; a directory of that name makes it fail.
    std::fs::create_dir(r.dir.path().join("owner-proof.json.new")).unwrap();
    assert_eq!(r.proofs.check(&r.code(1), "cli"), Err(Refused::Unavailable));
    std::fs::remove_dir(r.dir.path().join("owner-proof.json.new")).unwrap();
    assert_eq!(
        r.proofs.check(&r.code(1), "cli"),
        Ok(()),
        "and nothing was used by the refusal"
    );
}

/// Enrolment keeps nothing until a code from the app comes: a begun one
/// leaves no file, a wrong code leaves none, and five wrong drop it.
#[test]
fn an_enrolment_that_never_confirmed_a_code_keeps_nothing() {
    let r = Rig::new();
    let offer = r.proofs.begin_enrol("cli").unwrap();
    assert!(offer
        .uri
        .starts_with("otpauth://totp/Theseus:owner?secret="));
    assert!(!r.path().exists(), "begun, and nothing kept");
    assert!(!r.proofs.status().enrolled);
    assert_eq!(
        r.proofs.confirm_enrol("000000", "cli"),
        Err(EnrolError::Code(Refused::Wrong))
    );
    assert!(!r.path().exists());
    for _ in 0..4 {
        let _ = r.proofs.confirm_enrol("000000", "cli");
    }
    assert!(!r.proofs.enrolling(), "five wrong codes drop the enrolment");
    assert_eq!(
        r.proofs.confirm_enrol("000000", "cli"),
        Err(EnrolError::NothingPending)
    );
    assert!(!r.path().exists());
}

#[test]
fn an_enrolment_that_waits_too_long_is_dropped() {
    let r = Rig::new();
    r.proofs.begin_enrol("cli").unwrap();
    r.advance(ENROL_WAIT_MS + 1);
    let secret = held(&r.proofs.pending)
        .as_ref()
        .map(|p| p.secret.clone())
        .unwrap();
    let code = totp::code_at_step(&secret, totp::step_at(r.now.load(Ordering::SeqCst) / 1000));
    assert_eq!(
        r.proofs.confirm_enrol(&code, "cli"),
        Err(EnrolError::NothingPending)
    );
    assert!(!r.path().exists());
}

/// A code from the app keeps the secret, once; the code that confirmed it is
/// used, and the app's next one goes.
#[test]
fn a_first_code_from_the_app_keeps_the_secret_and_is_itself_used() {
    let r = Rig::new();
    r.proofs.begin_enrol("cli").unwrap();
    let secret = held(&r.proofs.pending)
        .as_ref()
        .map(|p| p.secret.clone())
        .unwrap();
    let step = totp::step_at(T0 / 1000);
    let first = totp::code_at_step(&secret, step);
    assert_eq!(r.proofs.confirm_enrol(&first, "cli"), Ok(()));
    let status = r.proofs.status();
    assert!(status.enrolled);
    assert_eq!(status.enrolled_at_ms, Some(T0));
    assert_eq!(r.proofs.check(&first, "cli"), Err(Refused::Used));
    r.advance(30_000);
    let next = totp::code_at_step(&secret, step + 1);
    assert_eq!(r.proofs.check(&next, "cli"), Ok(()));
    let changed = r.rows("owner.proof.changed");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].data["what"], "enrolled");
}

/// An enrolment cannot be begun over an enrolled one: the secret is not
/// replaced by anyone who can reach this, and a reset takes a valid code.
#[test]
fn an_enrolment_is_refused_while_one_is_kept() {
    let r = Rig::new();
    r.enrolled();
    assert_eq!(
        r.proofs.begin_enrol("cli").unwrap_err(),
        EnrolError::Enrolled
    );
    assert!(!r.proofs.enrolling());
}

/// A reset needs a valid code, and takes the file away.
#[test]
fn a_reset_needs_a_valid_code() {
    let r = Rig::new();
    r.enrolled();
    r.advance(30_000);
    assert_eq!(r.proofs.reset("000000", "cli"), Err(Refused::Wrong));
    assert!(r.path().exists());
    assert_eq!(r.proofs.reset(&r.code(1), "cli"), Ok(()));
    assert!(!r.path().exists());
    assert!(!r.proofs.status().enrolled);
    let changed = r.rows("owner.proof.changed");
    assert_eq!(changed.last().unwrap().data["what"], "reset");
}

/// Nothing a person or a log can print holds the secret.
#[test]
fn nothing_printable_holds_the_secret() {
    let r = Rig::new();
    r.enrolled();
    let hex = hex::encode(rfc().bytes());
    let shown = format!("{:?} {:?} {:?}", r.proofs, r.proofs.status(), held_of(&r));
    assert!(
        !shown.contains(&hex) && !shown.contains(&rfc().base32()),
        "{shown}"
    );
    for why in [
        Refused::NotEnrolled,
        Refused::Wrong,
        Refused::Used,
        Refused::TooMany { until_ms: 1 },
        Refused::Unavailable,
    ] {
        let said = format!("{why} {why:?}");
        assert!(!said.contains(&hex), "{said}");
    }
}

fn held_of(r: &Rig) -> Held {
    file::read(&r.path()).unwrap().unwrap()
}
