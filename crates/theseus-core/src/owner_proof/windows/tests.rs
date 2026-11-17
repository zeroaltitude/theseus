//! The window and the seam: what a good code covers, where, for how long, and
//! what it never covers. The clock is passed in; nothing here sleeps.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use theseus_protocol::work::{Question, QuestionKind, WorkKind, WorkRef};

use super::*;
use crate::ledger::LedgerRow;
use crate::owner_proof::totp::{self, Secret};
use crate::owner_proof::Proofs;
use crate::store::Store;

const HOUR: u64 = 3_600_000;
/// A minute-aligned moment.
const T0: u64 = 1_700_000_040_000;

fn rfc() -> Secret {
    Secret::from_bytes(*b"12345678901234567890")
}

struct Rig {
    dir: tempfile::TempDir,
    store: Store,
    now: Arc<AtomicU64>,
    proofs: Proofs,
    hours: u32,
}

impl Rig {
    fn new(hours: u32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let now = Arc::new(AtomicU64::new(T0));
        let proofs = Self::keeper(dir.path(), store.clone(), &now, hours);
        let h = file::Held::new(&rfc(), T0, totp::step_at(T0 / 1000));
        file::write(&dir.path().join(file::FILE), &h).unwrap();
        Self {
            dir,
            store,
            now,
            proofs,
            hours,
        }
    }

    fn keeper(state: &Path, store: Store, now: &Arc<AtomicU64>, hours: u32) -> Proofs {
        let now = Arc::clone(now);
        Proofs::open(
            state,
            store,
            Arc::new(move || now.load(Ordering::SeqCst)),
            hours,
        )
    }

    fn restarted(&mut self) {
        self.proofs = Self::keeper(self.dir.path(), self.store.clone(), &self.now, self.hours);
    }

    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    /// The code the app shows now.
    fn code(&self) -> String {
        totp::code_at_step(
            &rfc(),
            totp::step_at(self.now.load(Ordering::SeqCst) / 1000),
        )
    }

    /// The code of the step after the one `code()` is for, usable once the
    /// clock has not moved: single use means the same step never goes twice.
    fn code_ahead(&self, steps: u64) -> String {
        totp::code_at_step(
            &rfc(),
            totp::step_at(self.now.load(Ordering::SeqCst) / 1000) + steps,
        )
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
}

fn question(id: &str, needs: bool, fresh: bool) -> Question {
    Question {
        id: id.into(),
        kind: QuestionKind::Change,
        asked_by: WorkRef {
            id: "exe_1".into(),
            kind: WorkKind::Conversation,
            session_id: Some("ses_1".into()),
            title: String::new(),
        },
        prompt: "a change to named settings".into(),
        detail: None,
        input: None,
        options: Vec::new(),
        on_expiry: None,
        expires_at_ms: None,
        floor: false,
        may_answer: true,
        tool: Some("config.change".into()),
        asked_at_ms: 0,
        choices: Vec::new(),
        needs_proof: needs,
        needs_fresh_proof: fresh,
    }
}

fn answer<'a>(code: Option<&'a str>, place: &'a str) -> Answer<'a> {
    Answer { code, place }
}

#[test]
fn a_question_that_asks_for_no_proof_is_not_required_and_leaves_no_row() {
    let r = Rig::new(24);
    let q = question("q1", false, false);
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::NotRequired
    );
    assert!(r.rows("owner.proof.checked").is_empty());
}

/// A yes with no code, and no window: nothing is given, and the answer is recorded.
#[test]
fn a_yes_with_no_code_is_missing_and_recorded() {
    let r = Rig::new(24);
    let q = question("q1", true, false);
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Missing
    );
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(Some("  "), "cli")),
        Proof::Missing,
        "a form left blank is no code"
    );
    let rows = r.rows("owner.proof.checked");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.data["outcome"] == "missing"));
}

#[test]
fn a_wrong_code_is_refused_and_recorded_and_opens_nothing() {
    let r = Rig::new(24);
    let q = question("q1", true, false);
    let got = r
        .proofs
        .required_and_given(&q, &answer(Some("000000"), "cli"));
    assert_eq!(got, Proof::Refused(Refused::Wrong));
    assert!(r.proofs.status().windows.is_empty());
    assert_eq!(r.rows("owner.proof.checked")[0].data["outcome"], "wrong");
}

#[test]
fn a_question_that_needs_proof_with_none_enrolled_cannot_be_approved() {
    let r = Rig::new(24);
    std::fs::remove_file(r.dir.path().join(file::FILE)).unwrap();
    let q = question("q1", true, false);
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Refused(Refused::NotEnrolled)
    );
    assert_eq!(
        r.proofs
            .required_and_given(&q, &answer(Some("123456"), "cli")),
        Proof::Refused(Refused::NotEnrolled)
    );
    let line = r.proofs.standing("cli", false);
    assert!(line.not_enrolled && line.code_needed);
    assert!(line.line.contains("theseus owner proof enroll"), "{line:?}");
}

/// A good code is given once; the same code again is used; and it opens a
/// window over its own place.
#[test]
fn a_good_code_is_given_and_opens_a_window_over_its_place() {
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(Some(&code), "cli")),
        Proof::Given
    );
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(Some(&code), "cli")),
        Proof::Refused(Refused::Used),
        "the code is single use even inside its own window"
    );
    let status = r.proofs.status();
    assert_eq!(status.windows.len(), 1);
    assert_eq!(status.windows[0].place, "cli");
    assert_eq!(
        status.windows[0].until_ms,
        r.now.load(Ordering::SeqCst) + 24 * HOUR
    );
    let opened = r.rows("owner.proof.window_opened");
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].data["place"], "cli");
}

/// Inside the window a second question from the same place needs no code, and
/// its card says so; from another place it needs one.
#[test]
fn inside_the_window_the_same_place_needs_no_code_and_another_does() {
    let r = Rig::new(24);
    r.advance(30_000);
    let first = question("q1", true, false);
    let code = r.code();
    assert_eq!(
        r.proofs
            .required_and_given(&first, &answer(Some(&code), "discord:dm")),
        Proof::Given
    );
    r.advance(3 * HOUR);
    let second = question("q2", true, false);
    let opened_at = T0 + 30_000;
    assert_eq!(
        r.proofs
            .required_and_given(&second, &answer(None, "discord:dm")),
        Proof::GivenEarlier {
            at_ms: opened_at,
            until_ms: opened_at + 24 * HOUR
        }
    );
    assert_eq!(
        r.proofs.required_and_given(&second, &answer(None, "cli")),
        Proof::Missing,
        "another place needs a code of its own"
    );
    assert_eq!(
        r.proofs.required_and_given(&second, &answer(None, "web")),
        Proof::Missing
    );
    // The card says which it is, from each place.
    let here = r.proofs.standing("discord:dm", false);
    assert!(!here.code_needed);
    assert_eq!(
        here.line,
        "second proof: given here 3 hours ago, good for 21 hours more"
    );
    let there = r.proofs.standing("cli", false);
    assert!(there.code_needed);
    assert_eq!(
        there.line,
        "second proof: needed, enter the code from your authenticator app"
    );
    // And the use is a row naming the question, not a code.
    let used = r.rows("owner.proof.window_used");
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].data["question"], "q2");
    assert_eq!(used[0].data["place"], "discord:dm");
}

/// 24 hours from the code the window is over, with the clock passed in.
#[test]
fn after_the_window_a_code_is_needed_again() {
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(Some(&code), "cli")),
        Proof::Given
    );
    r.advance(24 * HOUR - 1);
    assert!(matches!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::GivenEarlier { .. }
    ));
    r.advance(1);
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Missing
    );
    assert!(r.proofs.status().windows.is_empty());
    assert!(r.proofs.standing("cli", false).code_needed);
}

/// A use does not extend the window: it ends 24 hours from the code.
#[test]
fn a_use_does_not_extend_the_window() {
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&code), "cli"));
    let until = r.proofs.status().windows[0].until_ms;
    for _ in 0..5 {
        r.advance(4 * HOUR);
        let got = r.proofs.required_and_given(&q, &answer(None, "cli"));
        assert!(matches!(got, Proof::GivenEarlier { .. }), "{got:?}");
        assert_eq!(r.proofs.status().windows[0].until_ms, until);
    }
    r.advance(4 * HOUR + 1);
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Missing,
        "24 hours from the code, not from the last use"
    );
}

#[test]
fn a_new_good_code_replaces_the_window_it_does_not_stack() {
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let c1 = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&c1), "cli"));
    r.advance(2 * HOUR);
    let c2 = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&c2), "cli"));
    let status = r.proofs.status();
    assert_eq!(status.windows.len(), 1);
    assert_eq!(
        status.windows[0].until_ms,
        r.now.load(Ordering::SeqCst) + 24 * HOUR
    );
}

#[test]
fn a_window_of_zero_hours_asks_every_time() {
    let r = Rig::new(0);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(Some(&code), "cli")),
        Proof::Given
    );
    assert!(r.proofs.status().windows.is_empty());
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Missing
    );
    assert!(r.rows("owner.proof.window_opened").is_empty());
}

#[test]
fn the_window_is_held_to_a_day() {
    let r = Rig::new(1_000);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&code), "cli"));
    assert_eq!(
        r.proofs.status().windows[0].until_ms,
        r.now.load(Ordering::SeqCst) + 24 * HOUR
    );
}

/// A question marked `needs_fresh_proof` asks inside a window, and a good code
/// given for it opens no window of its own.
#[test]
fn a_fresh_question_asks_inside_a_window_and_opens_none() {
    let r = Rig::new(24);
    r.advance(30_000);
    let ordinary = question("q1", true, false);
    let code = r.code();
    let _ = r
        .proofs
        .required_and_given(&ordinary, &answer(Some(&code), "cli"));
    let fresh = question("q2", false, true);
    assert_eq!(
        r.proofs.required_and_given(&fresh, &answer(None, "cli")),
        Proof::Missing,
        "an open window does not cover it"
    );
    assert!(r.proofs.standing("cli", true).code_needed);
    // A fresh code for it goes, and opens nothing: from another place, none.
    r.advance(30_000);
    let code = r.code();
    assert_eq!(
        r.proofs
            .required_and_given(&fresh, &answer(Some(&code), "web")),
        Proof::Given
    );
    assert_eq!(r.proofs.status().windows.len(), 1, "only the first window");
    assert_eq!(
        r.proofs.required_and_given(&ordinary, &answer(None, "web")),
        Proof::Missing
    );
}

#[test]
fn lock_ends_every_window_at_once_and_says_who() {
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    for (place, ahead) in [("cli", 0), ("web", 1)] {
        let code = r.code_ahead(ahead);
        assert_eq!(
            r.proofs.required_and_given(&q, &answer(Some(&code), place)),
            Proof::Given
        );
    }
    assert_eq!(r.proofs.status().windows.len(), 2);
    assert_eq!(r.proofs.lock("cli", "cli"), Ok(2));
    assert!(r.proofs.status().windows.is_empty());
    for place in ["cli", "web"] {
        assert_eq!(
            r.proofs.required_and_given(&q, &answer(None, place)),
            Proof::Missing
        );
    }
    let ended = r.rows("owner.proof.locked");
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].data["closed"], 2);
    assert_eq!(ended[0].data["by"], "cli");
    assert_eq!(r.proofs.lock("cli", "cli"), Ok(0), "nothing left to close");
}

/// A restart keeps a window; the daemon's own end of every window (a config
/// rollback's restart) ends them.
#[test]
fn a_restart_keeps_a_window_and_a_rollbacks_end_closes_it() {
    let mut r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&code), "cli"));
    r.restarted();
    assert!(matches!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::GivenEarlier { .. }
    ));
    assert_eq!(r.proofs.end_windows("a config rollback"), Ok(1));
    r.restarted();
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Missing
    );
    let ended = r.rows("owner.proof.locked");
    assert_eq!(ended[0].data["by"], "daemon");
    assert_eq!(ended[0].data["place"], "a config rollback");
}

/// A reset takes a fresh code, whatever window is open, and removes the
/// windows with the secret.
#[test]
fn a_reset_is_never_covered_by_a_window() {
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&code), "cli"));
    assert_eq!(r.proofs.reset("", "cli"), Err(Refused::Wrong));
    assert_eq!(r.proofs.reset("000000", "cli"), Err(Refused::Wrong));
    assert!(r.proofs.status().enrolled);
    r.advance(30_000);
    let fresh = r.code();
    assert_eq!(r.proofs.reset(&fresh, "cli"), Ok(()));
    assert!(r.proofs.status().windows.is_empty());
}

/// Enrolling pairs the app and trusts nothing: no window opens.
#[test]
fn enrolment_opens_no_window() {
    let r = Rig::new(24);
    std::fs::remove_file(r.dir.path().join(file::FILE)).unwrap();
    r.proofs.begin_enrol("cli").unwrap();
    let secret = held(&r.proofs.pending)
        .as_ref()
        .map(|p| p.secret.clone())
        .unwrap();
    let code = totp::code_at_step(&secret, totp::step_at(T0 / 1000));
    r.proofs.confirm_enrol(&code, "cli").unwrap();
    assert!(r.proofs.status().enrolled);
    assert!(r.proofs.status().windows.is_empty());
    assert!(r.rows("owner.proof.window_opened").is_empty());
    let q = question("q1", true, false);
    assert_eq!(
        r.proofs.required_and_given(&q, &answer(None, "cli")),
        Proof::Missing
    );
}

/// The window is in a file only the daemon writes, at mode 0600, and its rows
/// never hold a code.
#[test]
fn the_window_lives_in_the_daemons_file_and_no_row_holds_a_code() {
    use std::os::unix::fs::PermissionsExt;
    let r = Rig::new(24);
    r.advance(30_000);
    let q = question("q1", true, false);
    let code = r.code();
    let _ = r.proofs.required_and_given(&q, &answer(Some(&code), "cli"));
    let path = r.dir.path().join(file::FILE);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"windows\""), "{text}");
    for kind in [
        "owner.proof.checked",
        "owner.proof.window_opened",
        "owner.proof.window_used",
        "owner.proof.locked",
    ] {
        for row in r.rows(kind) {
            let s = serde_json::to_string(&row).unwrap();
            assert!(!s.contains(&code), "{kind}: {s}");
        }
    }
}

#[test]
fn spans_are_said_in_words() {
    assert_eq!(span(0), "under a minute");
    assert_eq!(span(59_999), "under a minute");
    assert_eq!(span(60_000), "1 minute");
    assert_eq!(span(5 * 60_000), "5 minutes");
    assert_eq!(span(HOUR), "1 hour");
    assert_eq!(span(21 * HOUR + 30 * 60_000), "21 hours");
}
