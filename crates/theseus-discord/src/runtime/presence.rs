//! The bot's status line (theseus-2j2ka): its presence reads the counts of
//! what needs the owner and what runs, `🔴 1 needs you · ⚙️ 3 running`, and is
//! cleared when nothing works or waits. A child of `runtime`.
//!
//! - **Counts only.** Everyone who can see the bot in any guild it is in sees
//!   its status, so it carries a number and a word, never a title, a place or
//!   a question. The numbers are the whole task model's (`tasks::Counts`).
//! - **Never twice inside ten seconds.** Discord rate-limits presence updates
//!   and a status that flickers is noise: the first change goes at once, the
//!   next waits for `EVERY` after it and sends the latest, and a change that
//!   undoes itself sends nothing (`Pacer`, a pure state machine on explicit
//!   instants).
//! - **A fresh gateway session forgets it.** After READY the status is sent
//!   again; `[discord] status_line = false` never starts the task.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::time::Instant;
use twilight_gateway::Shard;

use super::tasks::Counts;
use super::Shared;

/// The shortest time between two presence updates.
pub(crate) const EVERY: Duration = Duration::from_secs(10);

#[cfg(test)]
thread_local! {
    static GAP: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

/// A test shortens the gap for its thread.
#[cfg(test)]
pub(crate) fn gap_for_tests(d: Duration) {
    GAP.with(|g| g.set(Some(d)));
}

fn every() -> Duration {
    #[cfg(test)]
    if let Some(d) = GAP.with(std::cell::Cell::get) {
        return d;
    }
    EVERY
}

/// What the status reads for these counts; none for a quiet bot.
pub(crate) fn line(c: Counts) -> Option<String> {
    let mut parts = Vec::new();
    if c.needs_you > 0 {
        parts.push(format!("🔴 {} needs you", c.needs_you));
    }
    if c.running > 0 {
        parts.push(format!("⚙️ {} running", c.running));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The frame that sets the status, or clears it.
pub(crate) fn frame(status: &Option<String>) -> String {
    let activities = match status {
        Some(t) => json!([{"name": "Custom Status", "type": 4, "state": t}]),
        None => json!([]),
    };
    json!({"op": 3, "d": {"since": null, "activities": activities, "status": "online", "afk": false}})
        .to_string()
}

/// What to do about the status now.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Nothing differs from what Discord has.
    Idle,
    /// Send it, now.
    Send,
    /// It differs, but the last update is too recent: look again then.
    Wait(Duration),
}

/// When the status was last sent and what it said, to pace the next.
#[derive(Default)]
pub(crate) struct Pacer {
    last: Option<Instant>,
    sent: Option<Option<String>>,
}

impl Pacer {
    pub fn step(&self, wanted: &Option<String>, now: Instant) -> Step {
        if self.sent.as_ref() == Some(wanted) {
            return Step::Idle;
        }
        match self.last {
            Some(at) if now < at + every() => Step::Wait(at + every() - now),
            _ => Step::Send,
        }
    }

    /// The status was sent.
    pub fn sent(&mut self, wanted: Option<String>, now: Instant) {
        self.last = Some(now);
        self.sent = Some(wanted);
    }

    /// Discord has none of it any more (a fresh gateway session): the status
    /// is due again, still paced by the last send.
    pub fn forget(&mut self) {
        self.sent = None;
    }
}

/// The gateway is ready: start the status task the first time, and have it
/// send the status again every time.
pub(super) fn on_ready(shared: &Arc<Shared>, shard: &Shard) {
    if !shared.core.cfg.discord.status_line {
        return;
    }
    shared.tasks.resend();
    if shared.tasks.presence_started.swap(true, Ordering::Relaxed) {
        return;
    }
    let (s, sender) = (shared.clone(), shard.sender());
    tokio::spawn(async move {
        let mut counts = s.tasks.counts();
        let mut pacer = Pacer::default();
        let stopped = s.core.outbox.stopped();
        tokio::pin!(stopped);
        loop {
            let wanted = line(*counts.borrow_and_update());
            let wait = match pacer.step(&wanted, Instant::now()) {
                Step::Send => {
                    if sender.send(frame(&wanted)).is_err() {
                        return;
                    }
                    pacer.sent(wanted, Instant::now());
                    continue;
                }
                Step::Wait(d) => Some(d),
                Step::Idle => None,
            };
            tokio::select! {
                () = async { match wait { Some(d) => tokio::time::sleep(d).await, None => std::future::pending().await } } => {}
                r = counts.changed() => if r.is_err() { return },
                () = s.tasks.resent() => pacer.forget(),
                () = &mut stopped => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(needs_you: u32, running: u32) -> Counts {
        Counts { needs_you, running }
    }

    #[test]
    fn the_line_is_counts_only_and_a_quiet_bot_has_none() {
        assert_eq!(
            line(counts(1, 3)).as_deref(),
            Some("🔴 1 needs you · ⚙️ 3 running")
        );
        assert_eq!(line(counts(0, 3)).as_deref(), Some("⚙️ 3 running"));
        assert_eq!(line(counts(2, 0)).as_deref(), Some("🔴 2 needs you"));
        assert_eq!(line(counts(0, 0)), None);
    }

    #[test]
    fn the_frame_sets_a_custom_status_or_clears_it() {
        let set: serde_json::Value =
            serde_json::from_str(&frame(&Some("⚙️ 3 running".into()))).unwrap();
        assert_eq!(set["op"], 3);
        assert_eq!(set["d"]["activities"][0]["state"], "⚙️ 3 running");
        assert_eq!(set["d"]["activities"][0]["type"], 4);
        let clear: serde_json::Value = serde_json::from_str(&frame(&None)).unwrap();
        assert_eq!(clear["d"]["activities"], json!([]));
    }

    #[test]
    fn two_updates_are_never_inside_ten_seconds_and_the_latest_is_the_one_sent() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        let a = Some("⚙️ 1 running".to_string());
        let b = Some("⚙️ 2 running".to_string());
        let c = Some("⚙️ 3 running".to_string());
        assert_eq!(p.step(&a, t0), Step::Send);
        p.sent(a.clone(), t0);
        assert_eq!(p.step(&a, t0 + Duration::from_secs(1)), Step::Idle);
        // A change inside the window waits out the rest of it...
        assert_eq!(
            p.step(&b, t0 + Duration::from_secs(1)),
            Step::Wait(Duration::from_secs(9))
        );
        // ...and a second change meanwhile only changes what is sent then.
        assert_eq!(
            p.step(&c, t0 + Duration::from_secs(9)),
            Step::Wait(Duration::from_secs(1))
        );
        assert_eq!(p.step(&c, t0 + EVERY), Step::Send);
        p.sent(c.clone(), t0 + EVERY);
        // A change that undoes itself inside the window sends nothing.
        assert_eq!(p.step(&c, t0 + EVERY + Duration::from_secs(3)), Step::Idle);
    }

    #[test]
    fn a_fresh_session_sends_again_but_still_paced() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        let a = Some("🔴 1 needs you".to_string());
        p.sent(a.clone(), t0);
        p.forget();
        assert_eq!(
            p.step(&a, t0 + Duration::from_secs(2)),
            Step::Wait(Duration::from_secs(8))
        );
        assert_eq!(p.step(&a, t0 + Duration::from_secs(10)), Step::Send);
    }
}
