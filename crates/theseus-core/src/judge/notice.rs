//! `security.v3`'s live notices (step 24's notices, theseus-0j2.13; design
//! §2.8b with v3 in `security.v1`'s place, and §2.7's brake). The owner's
//! decision of 2026-10-04: when v3, live, answers `risky` in its act band
//! for a call the gate let run without asking, the owner hears of it after
//! the call, which never waits on Jev.
//!
//! - **Which calls** ([`flagged`]): posture `open` (no notice of its own),
//!   not made to wait by the external-text hold, and v3's judgment answered
//!   by its pinned model, `risky` act-true (at least the pack's `act`, 0.90).
//!   Only `risky` decides; a skipped, failed, or shed judgment posts none.
//! - **Off the call's path.** The gate's spawned task posts it once the
//!   judgments land, beside the running call: one frame holding the post to
//!   the owner (`Outbox::stage_to_operator`, kind `jev_notice`, which the
//!   Discord binding posts to the owner's DM alone, never a shared place)
//!   and its `tool.notified` row (`by: judge`, keyed `notice_<judgment>`,
//!   scoped `judge:security`); then `judge.noticed` to the turn's clients.
//! - **The brake** ([`Brake`]): `security.v1`'s `[[rollback]]` rules (more
//!   than 30 notices in a day, or 3 labeled `noise` in a day), run by
//!   `theseus_judge::learn::check_all` over the local day's notices and the
//!   owner's `noise` labels on v3's judgments, from any surface, as each
//!   notice is about to post and as each such label lands. When one fires,
//!   that notice is not posted and notices stop until the next local day: one
//!   `judge.paused` row (`what: "notices"`), one post that says so, and a
//!   META mark health reads. v3 is still asked and recorded meanwhile.
//! - **A restart** keeps it: the first use in a run reads today's rows in
//!   `judge:security` (off every call's path: the gate's task, or a label).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_judge::band::{Band, Top};
use theseus_judge::learn::{check_all, CanaryEvent, Fired, RollbackRule};
use theseus_judge::{Judgment, Mode};
use theseus_protocol::judge::{percent, JudgeNoticed};
use theseus_protocol::{Notice, PolicyNotified};
use theseus_store::{kinds, NewRecord};

use super::gate::{GateCall, RISKY, SECURITY_CANDIDATE, SECURITY_PACK};
use super::{spend, JudgeService};
use crate::bus::EventSink;
use crate::config::PackMode;
use crate::fact::judge::{JudgeNoticesPaused, JudgeNotified};
use crate::fact::{Rec, To};
use crate::ledger::LedgerRow;

/// Where the notices' rows are, with the security packs' judgments and labels.
pub const SCOPE: &str = "judge:security";

/// The META mark of a pause, which health reads without a scan.
pub const META_KEY: &str = "judge.notices";

/// v3's other yes-or-no questions a notice gives as its reasons, in short words.
const REASONS: [(&str, &str); 5] = [
    ("destructive", "destructive"),
    ("exfiltrates", "sends data out"),
    ("beyond_ask", "beyond the ask"),
    ("steered", "steered by what it read"),
    ("touches_credentials", "touches credentials"),
];

/// A notice's `tool.notified` row's key.
pub fn notice_key(judgment: &str) -> String {
    format!("notice_{judgment}")
}

/// The notices' pause's `judge.paused` row's key, by its day.
pub fn paused_key(day: &str) -> String {
    format!("notices_paused_{day}")
}

/// The notice an open call's judgments earn, if any: `security.v3` live,
/// answered by its pinned model, `risky` act-true, for a call that ran
/// without anyone's say (posture `open`, the hold not raising it, no notice
/// of its own).
pub fn flagged(call: &GateCall, judgments: &[Judgment]) -> Option<JudgeNoticed> {
    if call.posture != "open" || call.waited_on_hold() || call.notified {
        return None;
    }
    let j = judgments
        .iter()
        // Live, or canary in a canary's arm: the ladder records an acting
        // canary so (26a).
        .find(|j| {
            j.pack == SECURITY_CANDIDATE
                && matches!(j.mode, Mode::Live | Mode::Canary)
                && j.actionable()
        })?;
    let risky = j.answer(RISKY)?;
    if risky.band.band != Band::Act || risky.band.top != Top::Noul(true) {
        return None;
    }
    let reasons = REASONS
        .iter()
        .filter_map(|(q, words)| {
            let a = j.answer(q)?;
            let leans = a.band.top == Top::Noul(true) && a.band.band != Band::Escalate;
            leans.then(|| format!("{words} {}%", percent(a.band.value)))
        })
        .collect();
    Some(JudgeNoticed {
        session_id: call.session_id.clone(),
        turn_id: call.turn_id.clone(),
        tool_use_id: call.tool_use_id.clone(),
        correlation_id: call.correlation_id.clone(),
        tool: call.tool.clone(),
        summary: call.plan.summary.clone(),
        pack: j.pack.clone(),
        judgment: j.id.clone(),
        mode: "live".into(),
        risky: risky.band.value,
        percent: percent(risky.band.value),
        reasons,
    })
}

/// A pause of the notices, as its row and META mark keep it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Paused {
    /// The local day it pauses, and the day they come back.
    pub day: String,
    pub until: String,
    pub rule: String,
    pub why: String,
    /// In short: `3 labeled noise today`.
    pub short: String,
}

/// The brake's day: what today holds, read once per run.
#[derive(Debug, Default)]
struct Day {
    day: String,
    loaded: bool,
    notices: usize,
    noise: usize,
    paused: Option<Paused>,
}

/// The notices' brake: today's count of each, and a pause. Tests move its
/// clock (`skew_ms`) to cross a local midnight.
#[derive(Debug, Default)]
pub struct Brake {
    day: Mutex<Day>,
    skew_ms: AtomicU64,
}

/// What the brake says of a notice about to post.
enum Verdict {
    Post,
    Paused,
    Trips(Paused),
}

impl Brake {
    fn lock(&self) -> MutexGuard<'_, Day> {
        self.day.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Now, as the brake reads the day.
    pub fn now_ms(&self) -> u64 {
        theseus_protocol::now_unix_ms() + self.skew_ms.load(Ordering::Relaxed)
    }

    /// Move the brake's clock forward (tests: the next local day).
    #[cfg(test)]
    pub fn skew(&self, ms: u64) {
        self.skew_ms.fetch_add(ms, Ordering::Relaxed);
    }
}

/// `security.v1`'s rollback rules, the design's brake on these notices.
fn rules() -> Vec<RollbackRule> {
    theseus_judge::pack::by_name(SECURITY_PACK)
        .map(|p| p.rollback.clone())
        .unwrap_or_default()
}

/// The day's events as the rules read them, with one more notice when a
/// notice is about to post.
fn events(d: &Day, one_more: bool) -> Vec<CanaryEvent> {
    let notice = CanaryEvent::Notice { day: d.day.clone() };
    let noise = CanaryEvent::Label {
        day: d.day.clone(),
        label: "noise".into(),
    };
    let n = d.notices + usize::from(one_more);
    std::iter::repeat_n(notice, n)
        .chain(std::iter::repeat_n(noise, d.noise))
        .collect()
}

/// The local day after `now_ms`'s.
fn next_day(now_ms: u64) -> String {
    spend::local_day(crate::learning::local_midnight(now_ms) + 30 * 3600 * 1000)
}

/// A fired rule as a pause of `today`, in short words.
fn paused_of(f: &Fired, d: &Day, now_ms: u64) -> Paused {
    let short = match f.rule.as_str() {
        "labels_per_day" => format!("{} labeled noise today", d.noise),
        "notices_per_day" => format!("{} notices today", d.notices + 1),
        _ => f.why.clone(),
    };
    Paused {
        day: d.day.clone(),
        until: next_day(now_ms),
        rule: f.rule.clone(),
        why: f.why.clone(),
        short,
    }
}

/// Today's notices, the owner's noise labels on v3's judgments, and a pause,
/// from the rows in `judge:security`.
fn read_today(store: &crate::store::Store, today: &str) -> Day {
    let mut d = Day {
        day: today.into(),
        loaded: true,
        ..Day::default()
    };
    let records = theseus_store::blocking(|| store.scope_after(SCOPE, 0));
    let records = match records {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "judge: today's notices were not read; counting from none");
            return d;
        }
    };
    for r in records {
        if r.kind != kinds::LEDGER {
            continue;
        }
        let Ok(row) = r.decode::<LedgerRow>() else {
            continue;
        };
        let x = &row.data;
        match row.kind.as_str() {
            "judge.paused" if x["what"] == "notices" && x["day"] == today => {
                d.paused = serde_json::from_value(x.clone()).ok();
            }
            "tool.notified" | "judge.label" if spend::local_day(row.at_unix_ms) != today => {}
            "tool.notified" if x["by"] == "judge" => d.notices += 1,
            "judge.label"
                if x["pack"] == SECURITY_CANDIDATE
                    && x["label"] == "noise"
                    && x["source"] == "operator" =>
            {
                d.noise += 1
            }
            _ => {}
        }
    }
    d
}

impl JudgeService {
    /// The brake's day as of `today`: read from the store at the first use
    /// of a run, and begun afresh at a new local day.
    fn brake_day(&self, today: &str) -> MutexGuard<'_, Day> {
        let mut d = self.brake.lock();
        if !d.loaded {
            *d = read_today(&self.store, today);
        } else if d.day != today {
            *d = Day {
                day: today.into(),
                loaded: true,
                ..Day::default()
            };
        }
        d
    }

    /// Post `n`'s notice to the owner, unless the brake holds it: the post
    /// and its `tool.notified` row in one frame, then `judge.noticed` to the
    /// turn's clients. Blocking: the gate's task runs it off the runtime.
    pub(crate) fn post_notice(&self, call: &GateCall, n: &JudgeNoticed, sink: Option<&EventSink>) {
        let Some(core) = self.core() else { return };
        let now = self.brake.now_ms();
        let today = spend::local_day(now);
        let verdict = {
            let mut d = self.brake_day(&today);
            match &d.paused {
                Some(_) => Verdict::Paused,
                None => match check_all(&rules(), &events(&d, true)).first() {
                    Some(f) => {
                        let p = paused_of(f, &d, now);
                        d.paused = Some(p.clone());
                        Verdict::Trips(p)
                    }
                    None => {
                        d.notices += 1;
                        Verdict::Post
                    }
                },
            }
        };
        match verdict {
            Verdict::Post => {}
            Verdict::Paused => {
                tracing::debug!(judgment = %n.judgment, "judge: notices are paused today; not posted");
                return;
            }
            Verdict::Trips(p) => {
                self.pause_notices(&core, Some(&call.session_id), &p);
                return;
            }
        }
        let notice = PolicyNotified {
            session_id: n.session_id.clone(),
            turn_id: n.turn_id.clone(),
            tool_use_id: n.tool_use_id.clone(),
            correlation_id: n.correlation_id.clone(),
            tool: n.tool.clone(),
            input: call.input.clone(),
            summary: n.summary.clone(),
            notice: Notice {
                kind: "judge".into(),
                setting: format!("[judge.packs.\"{}\"] notices = true", n.pack),
                rule: n.line(),
            },
            granted: None,
            task: call.task.then(|| crate::task::short(&call.session_id)),
            by: Some("judge".into()),
            judgment: Some(n.judgment.clone()),
        };
        let body = json!({"kind": "jev_notice", "judgment": n.judgment, "tool": n.tool,
            "summary": n.summary, "percent": n.percent, "reasons": n.reasons, "line": n.line(),
            "call": n.correlation_id, "task": notice.task});
        let written = (|| -> anyhow::Result<()> {
            let (a, mut frame) = core
                .outbox
                .stage_to_operator(Some(&call.session_id), body)?;
            let f = JudgeNotified {
                notice: &notice,
                noticed: n,
                post: &a.correlation_id,
            };
            let mut r = crate::fact::row(&f, Some(&n.session_id), Some(&n.turn_id))?;
            r.key = Some(notice_key(&n.judgment));
            frame.push(r.scoped(SCOPE));
            self.store.append(&frame)?;
            core.outbox.posted(&a);
            Ok(())
        })();
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "judge: a notice's frame was not written");
            self.brake.lock().notices -= 1;
            return;
        }
        // Each notice posted counts toward security's day brake on the
        // ladder (26a: more than 30 in a day).
        let day = self.today();
        self.land(SECURITY_CANDIDATE, CanaryEvent::Notice { day });
        let f = JudgeNotified {
            notice: &notice,
            noticed: n,
            post: "",
        };
        let to = match sink {
            Some(s) => To::Sink(s),
            None => To::Session(&core.bus, &n.session_id),
        };
        if let Some(narrator) = self.narrator.get() {
            Rec {
                narrator,
                session: Some(&n.session_id),
                turn: Some(&n.turn_id),
                to,
                store: &self.store,
            }
            .announce(&f);
        }
    }

    /// The brake fired: one frame with the `judge.paused` row, the post that
    /// says so, and the META mark.
    fn pause_notices(&self, core: &crate::rpc::Core, session: Option<&str>, p: &Paused) {
        let f = JudgeNoticesPaused {
            pack: SECURITY_CANDIDATE,
            day: &p.day,
            until: &p.until,
            rule: &p.rule,
            why: &p.why,
            short: &p.short,
        };
        let text = format!("🔕 Jev's notices are paused until tomorrow: {}.", p.short);
        let written = (|| -> anyhow::Result<()> {
            let body = json!({"kind": "jev_paused", "text": text, "rule": p.rule, "day": p.day,
                "until": p.until});
            let (a, mut frame) = core.outbox.stage_to_operator(session, body)?;
            let mut r = crate::fact::row(&f, None, None)?;
            r.key = Some(paused_key(&p.day));
            frame.push(r.scoped(SCOPE));
            frame.push(NewRecord::json(kinds::META, Some(META_KEY), p)?);
            self.store.append(&frame)?;
            core.outbox.posted(&a);
            Ok(())
        })();
        match written {
            Ok(()) => {
                self.announce(None, None, &f);
                // The ladder reads this pause as security's day brake (26a,
                // by its key): read it again now, so it says so today.
                self.ladder().reload();
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the notices' pause was not written")
            }
        }
    }

    /// A label written on a judgment (`judge.label`, any surface): an
    /// owner's `noise` on a v3 judgment counts toward the brake, and a label
    /// on a noticed judgment shows on its post, whose buttons go.
    pub(crate) fn after_label(&self, core: &crate::rpc::Core, label: &Labeled<'_>) {
        let session = label.session.unwrap_or_default();
        if label.pack != SECURITY_CANDIDATE
            || self.mode_for(SECURITY_CANDIDATE, session).mode != PackMode::Live
        {
            return;
        }
        if *label.label == "noise" {
            let now = self.brake.now_ms();
            let today = spend::local_day(now);
            let tripped = {
                // A run's first use reads the day from the store, which
                // holds this label's row already: it counts once.
                let read = !self.brake.lock().loaded;
                let mut d = self.brake_day(&today);
                if !read {
                    d.noise += 1;
                }
                match (&d.paused, check_all(&rules(), &events(&d, false)).first()) {
                    (None, Some(f)) => {
                        let p = paused_of(f, &d, now);
                        d.paused = Some(p.clone());
                        Some(p)
                    }
                    _ => None,
                }
            };
            if let Some(p) = tripped {
                self.pause_notices(core, label.session, &p);
            }
        }
        let noticed = self
            .store
            .ledger_by_key(&notice_key(label.judgment))
            .ok()
            .flatten()
            .and_then(|r| r.decode::<LedgerRow>().ok());
        let Some(row) = noticed else { return };
        let body = json!({"kind": "jev_labeled", "notice": row.data["post"], "judgment": label.judgment,
            "label": label.label, "who": label.who, "via": label.via});
        if let Err(e) = core.outbox.to_operator(None, body) {
            tracing::warn!(error = %format!("{e:#}"), "judge: a notice's label was not posted");
        }
    }

    /// Health's word on the notices: `on`, `paused until <day>: <why>`, or
    /// `off`. A read at most: the brake's day when this run has one, else
    /// the META mark.
    pub(crate) fn notices_state(&self) -> String {
        // The config's switches first (`mode_of`: the judge, `max_mode`, the
        // pack's line, `notices`), then the ladder, read only once loaded:
        // health never reads it itself. A pause today reads as paused,
        // though the ladder takes it as a day's brake; any other move down
        // as off (26a).
        if self.cfg.mode_of(SECURITY_CANDIDATE, PackMode::Live) != PackMode::Live {
            return "off".into();
        }
        let today = spend::local_day(self.brake.now_ms());
        let today = today.as_str();
        let paused = {
            let d = self.brake.lock();
            match d.loaded && d.day == today {
                true => d.paused.clone(),
                false => self
                    .store
                    .get_meta::<Paused>(META_KEY)
                    .ok()
                    .flatten()
                    .filter(|p| p.day == today),
            }
        };
        let acts =
            !self.ladder().is_loaded() || self.ladder().standing(SECURITY_CANDIDATE).rung.acts();
        match paused {
            Some(p) => format!("paused until {}: {}", p.until, p.short),
            None if acts => "on".into(),
            None => "off".into(),
        }
    }

    /// The brake's own clock (tests move it).
    pub fn brake(&self) -> &Brake {
        &self.brake
    }
}

/// A label as the brake and a notice's post read it.
pub struct Labeled<'a> {
    pub pack: &'a str,
    pub judgment: &'a str,
    pub label: &'a Value,
    pub who: &'a str,
    pub via: &'a str,
    pub session: Option<&'a str>,
}
