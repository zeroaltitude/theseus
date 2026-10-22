//! What one check comes to (theseus-f2r2b): pure, so the rule "an unchanged
//! value writes nothing" is a test of this table and nothing else.
//!
//! `Entry` is a watch in memory: its spec, what it has seen, when it checks
//! next. `apply` takes a check's result and says whether it is news
//! (`ProbeNews`, which the kernel turns into one frame) or quiet (nothing is
//! written, and the next check is simply later).
//!
//! - **The first value** is news (the baseline `changes` compares with), then
//!   each different value. The same value again is quiet, however often.
//! - **A met condition** is news, and ends the watch. So is its expiry.
//! - **Failures back off** ×2 from the interval to an hour (or the interval,
//!   when that is longer), and the third in a row is news once: the same
//!   failure again informs nobody.
//! - **After a restart** the entry starts from the state its last frame left
//!   (`seed`), and its first check is at once: a read, safe to repeat.

use theseus_kernel::{ProbeNews, ProbeVerdict, WatchState};
use theseus_protocol::term::words::cut_marked;

use super::cond::Observed;
use super::quiet::Quiet;
use super::spec::{Check, Spec, Then};

/// The most a failing watch waits between checks, when its interval is
/// shorter than this.
pub const BACKOFF_CAP_MS: u64 = 3_600_000;
/// How many failures in a row are news.
pub const FAILURES_NEWS: u32 = 3;
/// How long a mail watch's cursor may move in memory before a look writes it:
/// a restart then reads at most this much history again, and a long watch's
/// cursor never grows older than Gmail keeps.
pub const CURSOR_KEEP_MS: u64 = 6 * 3_600_000;

/// A watch as the daemon holds it in memory.
#[derive(Debug, Clone)]
pub struct Entry {
    pub execution_id: String,
    pub wake_id: String,
    pub spec: Spec,
    pub set_ms: u64,
    pub first: Option<String>,
    pub last: Option<String>,
    pub checks: u64,
    pub checked_ms: u64,
    pub failures: u32,
    /// The failure the owner was told of, until a check succeeds.
    pub failing: Option<String>,
    pub next_ms: u64,
    pub running: bool,
    /// A mail watch's history cursor: where its next look starts.
    pub cursor: Option<String>,
    /// When the cursor was last written to the store (memory only).
    pub kept_ms: u64,
    /// The mail looks that found nothing since the last row was written
    /// (memory only; `quiet.rs`).
    pub quiet: Quiet,
    /// The task a mail match or a follow-up started (its short id), so a
    /// news frame that failed and is tried again starts none a second time.
    pub started: Option<String>,
}

/// What `apply` made of a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Nothing to write.
    Quiet,
    News(ProbeNews),
}

impl Entry {
    /// An entry that checks at `next_ms`, with the state its last frame left.
    pub fn seed(
        execution_id: String,
        wake_id: String,
        spec: Spec,
        set_ms: u64,
        state: Option<WatchState>,
        next_ms: u64,
    ) -> Entry {
        let s = state.unwrap_or_default();
        // A reply is never due: its one look is its expiry.
        let next_ms = match spec.check {
            Check::Reply { .. } => spec.expires_ms,
            _ => next_ms,
        };
        Entry {
            execution_id,
            wake_id,
            spec,
            set_ms,
            first: s.first,
            last: s.last,
            checks: s.checks,
            checked_ms: s.checked_ms,
            failures: 0,
            failing: s.failing,
            next_ms,
            running: false,
            cursor: None,
            kept_ms: 0,
            quiet: Quiet::default(),
            started: None,
        }
    }

    pub(super) fn state(&self) -> WatchState {
        WatchState {
            first: self.first.clone(),
            last: self.last.clone(),
            checks: self.checks,
            checked_ms: self.checked_ms,
            failing: self.failing.clone(),
        }
    }

    /// The delay to the next check after `failures` failures in a row.
    pub fn delay_ms(&self) -> u64 {
        let every = self.spec.every_ms.max(1);
        if self.failures == 0 {
            return every;
        }
        let cap = BACKOFF_CAP_MS.max(every);
        every
            .checked_shl(self.failures.min(20))
            .map_or(cap, |d| d.min(cap))
    }

    /// Take one check's result at `now`.
    pub fn apply(&mut self, now: u64, result: Result<Observed, String>) -> Step {
        self.checks += 1;
        self.checked_ms = now;
        let step = match result {
            Err(e) => self.failed(e),
            Ok(seen) => self.seen(now, seen),
        };
        let step = match step {
            // Out of time, unmet: it gives up.
            Step::Quiet
            | Step::News(ProbeNews {
                verdict: ProbeVerdict::Changed | ProbeVerdict::Failing,
                ..
            }) if now >= self.spec.expires_ms => {
                self.news(ProbeVerdict::Expired, self.expired_text(now))
            }
            s => s,
        };
        self.next_ms = if matches!(
            step,
            Step::News(ProbeNews {
                verdict: ProbeVerdict::Met { .. } | ProbeVerdict::Expired,
                ..
            })
        ) {
            u64::MAX
        } else {
            // The expiry is a check of its own, never overshot by an interval.
            (now.saturating_add(self.delay_ms())).min(self.spec.expires_ms.max(now + 1))
        };
        step
    }

    fn failed(&mut self, e: String) -> Step {
        self.failures += 1;
        if self.failures == FAILURES_NEWS && self.failing.is_none() {
            let text = format!(
                "👁 \"{}\" keeps failing: {} ({} checks in a row). I will keep trying, further apart.",
                self.spec.title,
                first_line(&e),
                self.failures
            );
            self.failing = Some(first_line(&e));
            return self.news(ProbeVerdict::Failing, text);
        }
        Step::Quiet
    }

    fn seen(&mut self, now: u64, seen: Observed) -> Step {
        self.failures = 0;
        self.failing = None;
        if matches!(self.spec.check, Check::Reply { .. }) {
            return Step::Quiet;
        }
        let first_before = self.first.clone();
        if self.first.is_none() {
            self.first = Some(seen.value.clone());
        }
        // A mail look moves the cursor; it is written with a frame the watch
        // writes anyway, or once `CURSOR_KEEP_MS` has passed.
        let mut write_cursor = None;
        if let Some(m) = &seen.mail {
            let moved = self.cursor.as_deref() != Some(m.cursor.as_str());
            write_cursor = m.rebaselined.then(|| {
                "mail history was older than Gmail keeps: started again from now, and mail \
                 that arrived between may have been missed"
                    .to_string()
            });
            if moved && first_before.is_some() && now.saturating_sub(self.kept_ms) >= CURSOR_KEEP_MS
            {
                write_cursor.get_or_insert_with(String::new);
            }
            self.cursor = Some(m.cursor.clone());
        }
        if self.spec.cond.met(&seen, first_before.as_deref()) {
            self.last = Some(seen.value.clone());
            if let Some(m) = &seen.mail {
                let text = self.mail_text(now, &seen.value);
                let detail = serde_json::json!({"external": true, "trusted": m.trusted,
                    "unverified": !m.unverified.is_empty()});
                return self.news_with(ProbeVerdict::Met { turn: false }, text, detail);
            }
            let turn = !matches!(self.spec.then, Then::Notice);
            let text = self.met_text(now, &seen.value, turn);
            return self.news(ProbeVerdict::Met { turn }, text);
        }
        if self.last.as_deref() != Some(seen.value.as_str()) || write_cursor.is_some() {
            self.last = Some(seen.value);
            return self.news(ProbeVerdict::Changed, write_cursor.unwrap_or_default());
        }
        Step::Quiet
    }

    fn news(&mut self, verdict: ProbeVerdict, text: String) -> Step {
        self.news_with(verdict, text, serde_json::Value::Null)
    }

    fn news_with(
        &mut self,
        verdict: ProbeVerdict,
        text: String,
        detail: serde_json::Value,
    ) -> Step {
        let keep = self.keep(&verdict);
        if keep.as_ref().is_some_and(|(_, v)| !v.is_empty()) {
            self.kept_ms = self.checked_ms;
        }
        Step::News(ProbeNews {
            verdict,
            state: self.state(),
            text,
            detail,
            keep,
        })
    }

    /// A mail watch's cursor, for the frame: written with each news, and
    /// cleared when the watch ends.
    fn keep(&self, verdict: &ProbeVerdict) -> Option<(String, String)> {
        if !matches!(self.spec.check, Check::Mail { .. }) {
            return None;
        }
        let key = format!("{}{}", super::mail::CURSOR_PREFIX, self.wake_id);
        match verdict {
            ProbeVerdict::Met { .. } | ProbeVerdict::Expired => Some((key, String::new())),
            _ => self.cursor.clone().map(|c| (key, c)),
        }
    }

    /// What a mail match says: the watch's question and each message's
    /// sender, subject and snippet. It is outside text.
    fn mail_text(&self, now: u64, value: &str) -> String {
        let (waited, checks) = self.after(now);
        let title = self.spec.title.trim();
        format!("👁 new mail for \"{title}\" (after {waited}, {checks} checks, $0):\n{value}")
    }

    /// How long it waited, in words: `after 2 d 4 h and 611 checks`.
    fn after(&self, now: u64) -> (String, u64) {
        (
            crate::wake::span(now.saturating_sub(self.set_ms)),
            self.checks,
        )
    }

    fn met_text(&self, now: u64, value: &str, turn: bool) -> String {
        let (waited, checks) = self.after(now);
        let title = self.spec.title.trim();
        if turn {
            let mut t = format!("👁 watch met: {title}");
            if !value.is_empty() {
                t.push_str(&format!(" → {}", cut_marked(value, 120)));
            }
            t.push_str(&format!(", after {waited} and {checks} checks"));
            if let Then::Task(brief) = &self.spec.then {
                t.push_str(&format!("\nYou asked for this when it held: {brief}"));
            }
            t
        } else {
            let said = title.trim_end_matches('?').trim_end();
            let v = if value.is_empty() || value.contains('\n') {
                String::new()
            } else {
                format!(" → {}", cut_marked(value, 120))
            };
            format!("👁 {said}{v} (after {waited}, {checks} checks, $0)")
        }
    }

    fn expired_text(&self, now: u64) -> String {
        let (waited, checks) = self.after(now);
        let last = match self.last.as_deref() {
            Some(v) if !v.is_empty() => format!("; the last value was {}", cut_marked(v, 120)),
            _ => String::new(),
        };
        format!(
            "👁 gave up on \"{}\" after {waited} and {checks} checks{last}",
            self.spec.title
        )
    }
}

fn first_line(s: &str) -> String {
    cut_marked(
        s.lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim(),
        120,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::cond;
    use crate::watch::spec::Check;

    const MIN: u64 = 60_000;

    fn entry(until: &str, then: Then) -> Entry {
        let spec = Spec {
            title: "PR 123 merged?".into(),
            check: Check::File { path: "/x".into() },
            then,
            until: until.into(),
            cond: cond::parse(until).unwrap(),
            every_ms: 5 * MIN,
            expires_ms: 10_000 * MIN,
            follow_up: None,
        };
        Entry::seed("exe_1".into(), "wak_1".into(), spec, 0, None, 0)
    }

    fn val(v: &str) -> Result<Observed, String> {
        Ok(Observed {
            value: v.into(),
            code: None,
            exists: true,
            mail: None,
        })
    }

    fn verdict(s: &Step) -> Option<&ProbeVerdict> {
        match s {
            Step::News(n) => Some(&n.verdict),
            Step::Quiet => None,
        }
    }

    /// The point of the row: the same value, checked again and again, is
    /// news once.
    #[test]
    fn twenty_unchanged_checks_are_news_once() {
        let mut e = entry("equals MERGED", Then::Notice);
        let mut news = 0;
        for i in 0..20 {
            if matches!(e.apply(i * 5 * MIN, val("OPEN")), Step::News(_)) {
                news += 1;
            }
        }
        assert_eq!(news, 1, "only the first value is news");
        assert_eq!(e.checks, 20);
        assert!(
            matches!(e.apply(21 * 5 * MIN, val("DRAFT")), Step::News(_)),
            "a change is news"
        );
        assert_eq!(e.apply(22 * 5 * MIN, val("DRAFT")), Step::Quiet);
    }

    #[test]
    fn a_met_condition_ends_the_watch_with_its_message() {
        let mut e = entry("equals MERGED", Then::Notice);
        e.apply(0, val("OPEN"));
        let s = e.apply(2 * 86_400_000 + 4 * 3_600_000, val("MERGED"));
        let Step::News(n) = s else { panic!("{s:?}") };
        assert_eq!(n.verdict, ProbeVerdict::Met { turn: false });
        assert_eq!(
            n.text,
            "👁 PR 123 merged → MERGED (after 2 d 4 h, 2 checks, $0)"
        );
        assert_eq!(e.next_ms, u64::MAX, "it does not check again");
        let mut t = entry("equals MERGED", Then::Turn);
        for i in 0..610 {
            t.apply(i, val("OPEN"));
        }
        let Step::News(n) = t.apply(2 * 86_400_000 + 4 * 3_600_000, val("MERGED")) else {
            panic!()
        };
        assert_eq!(n.verdict, ProbeVerdict::Met { turn: true });
        assert_eq!(
            n.text,
            "👁 watch met: PR 123 merged? → MERGED, after 2 d 4 h and 611 checks"
        );
        let mut k = entry("equals MERGED", Then::Task("ship it".into()));
        let Step::News(n) = k.apply(1, val("MERGED")) else {
            panic!()
        };
        assert!(
            n.text.ends_with("You asked for this when it held: ship it"),
            "{}",
            n.text
        );
    }

    #[test]
    fn changes_waits_for_a_second_value() {
        let mut e = entry("changes", Then::Notice);
        assert_eq!(
            verdict(&e.apply(0, val("a"))),
            Some(&ProbeVerdict::Changed),
            "the baseline"
        );
        assert_eq!(e.apply(1, val("a")), Step::Quiet);
        assert_eq!(
            verdict(&e.apply(2, val("b"))),
            Some(&ProbeVerdict::Met { turn: false })
        );
    }

    #[test]
    fn three_failures_inform_once_and_back_off() {
        let mut e = entry("equals MERGED", Then::Notice);
        e.apply(0, val("OPEN"));
        let err = || Err("gh: not logged in\nrun gh auth login".to_string());
        let mut now = 0;
        assert_eq!(
            e.apply(
                {
                    now += 1;
                    now
                },
                err()
            ),
            Step::Quiet
        );
        assert_eq!(e.next_ms, 1 + 10 * MIN, "x2");
        assert_eq!(
            e.apply(
                {
                    now += 1;
                    now
                },
                err()
            ),
            Step::Quiet
        );
        assert_eq!(e.next_ms, 2 + 20 * MIN, "x4");
        let s = e.apply(
            {
                now += 1;
                now
            },
            err(),
        );
        let Step::News(n) = s else { panic!("{s:?}") };
        assert_eq!(n.verdict, ProbeVerdict::Failing);
        assert!(
            n.text.contains("gh: not logged in") && !n.text.contains("auth login"),
            "{}",
            n.text
        );
        assert_eq!(n.state.failing.as_deref(), Some("gh: not logged in"));
        for _ in 0..5 {
            assert_eq!(
                e.apply(
                    {
                        now += 1;
                        now
                    },
                    err()
                ),
                Step::Quiet,
                "told once"
            );
        }
        assert_eq!(e.next_ms, now + 60 * MIN, "an hour at the most");
        // It recovers quietly, and may be told again if it fails again.
        assert_eq!(
            e.apply(
                {
                    now += 1;
                    now
                },
                val("OPEN")
            ),
            Step::Quiet
        );
        assert_eq!(e.failing, None);
        for _ in 0..2 {
            e.apply(
                {
                    now += 1;
                    now
                },
                err(),
            );
        }
        assert!(matches!(
            e.apply(
                {
                    now += 1;
                    now
                },
                err()
            ),
            Step::News(_)
        ));
    }

    #[test]
    fn the_expiry_gives_up_unless_it_is_met() {
        let mut e = entry("equals MERGED", Then::Notice);
        e.spec.expires_ms = 100 * MIN;
        e.apply(0, val("OPEN"));
        assert_eq!(e.apply(97 * MIN, val("OPEN")), Step::Quiet);
        assert_eq!(e.next_ms, 100 * MIN, "the expiry is a check of its own");
        let Step::News(n) = e.apply(100 * MIN, val("OPEN")) else {
            panic!()
        };
        assert_eq!(n.verdict, ProbeVerdict::Expired);
        assert!(
            n.text
                .contains("gave up on \"PR 123 merged?\" after 1 h 40 min and 3 checks"),
            "{}",
            n.text
        );
        assert!(n.text.contains("the last value was OPEN"));
        let mut m = entry("equals MERGED", Then::Notice);
        m.spec.expires_ms = 100 * MIN;
        let s = m.apply(100 * MIN, val("MERGED"));
        assert_eq!(
            verdict(&s),
            Some(&ProbeVerdict::Met { turn: false }),
            "met at the last moment wins"
        );
        let mut f = entry("equals MERGED", Then::Notice);
        f.spec.expires_ms = 100 * MIN;
        let s = f.apply(100 * MIN, Err("down".into()));
        assert_eq!(
            verdict(&s),
            Some(&ProbeVerdict::Expired),
            "a failing watch gives up too"
        );
    }

    #[test]
    fn a_restart_resumes_from_the_last_frame() {
        let state = WatchState {
            first: Some("OPEN".into()),
            last: Some("OPEN".into()),
            checks: 611,
            checked_ms: 5,
            failing: None,
        };
        let spec = entry("changes", Then::Notice).spec;
        let mut e = Entry::seed("e".into(), "w".into(), spec, 0, Some(state), 0);
        assert_eq!(
            e.apply(10, val("OPEN")),
            Step::Quiet,
            "the same value is still quiet after a restart"
        );
        assert_eq!(e.checks, 612, "the count goes on");
        assert!(matches!(e.apply(11, val("MERGED")), Step::News(_)));
    }
}
