//! The work board's side of the TUI (theseus-k2vca): what `work.watch` says of
//! each session, kept by the position rule beside the board's executions, and
//! the words a sidebar row is made of from it. A row says what its task is
//! doing and for how long (`job 2:14 of usual 2:40`), and a finished task's
//! first report line shows under it until it is seen.
//!
//! The executions stay the board's own (`executions.watch`: the questions,
//! reminders and `confirm.resolved` ride that stream, which `work.watch` does
//! not carry); this is the overlay that words the rows. A daemon that does not
//! know `work.watch` leaves `live` false, and a row says its attention label as
//! it always did.
//!
//! Pure: no clock, no I/O. The caller passes the time and the clock's format.

use std::collections::{HashMap, HashSet};

use theseus_protocol::forecast::WorkForecast;
use theseus_protocol::work::progress_line::{aged_said, forecast_cost, step_words, time_left};
use theseus_protocol::work::{DoingKind, WorkKind, WorkState, WorkView};
use unicode_width::UnicodeWidthStr;

/// What `work.watch` said of the sessions, and of the jobs under them.
#[derive(Debug, Default)]
pub struct Work {
    /// `work.watch` answered at this connect: rows read it.
    pub live: bool,
    /// Every view, by its id.
    views: HashMap<String, WorkView>,
    /// A conversation's or a task's view id, by its session.
    sessions: HashMap<String, String>,
    /// Since a connect, until its first snapshot: the ids an event has
    /// updated on the new connection (the board's rule for executions).
    touched: Option<HashSet<String>>,
}

/// How a row says what its session is doing. The times are the daemon's
/// (`since`), so the row repaints from the clock alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// The model is thinking since this time.
    Model { since_ms: u64 },
    /// A tool runs since this time.
    Tool { label: String, since_ms: u64 },
    /// A job runs since this time, and usually takes `usual_ms`; `said` is the
    /// model's last word, said at `said_at_ms`, and the task has done this since
    /// `doing_ms` (the words show their age when they are older than that).
    Job {
        since_ms: u64,
        usual_ms: Option<u64>,
        said: Option<String>,
        said_at_ms: u64,
        doing_ms: u64,
    },
    /// It failed at this time, and why.
    Failed { at_ms: u64, reason: String },
    /// It finished at this time.
    Done { at_ms: u64 },
    /// Words already: a question (`asks: …`), a wait, `queued`, `ready`.
    Text(String),
}

/// What a row knows besides its session's name.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// Plan steps done and in all, when the task has a plan.
    pub steps: Option<(u32, u32)>,
    pub paused: bool,
    /// The forecast, the start and the spend so far of a task that has not
    /// ended: what its cost against the usual and its time left are made from
    /// (theseus-ypnxt.2).
    pub timing: Option<(WorkForecast, u64, f64)>,
    pub form: Form,
    /// A finished task's first report line.
    pub report: Option<String>,
}

impl Work {
    /// A new connection: its first snapshot replaces what is held, but for
    /// the ids an event updates before it arrives. Not live until it answers.
    pub fn reconnected(&mut self) {
        self.touched = Some(HashSet::new());
        self.live = false;
    }

    /// Apply a view by the position rule: only if its position, then its
    /// `at_ms`, is greater than the last applied for its id (the daemon keeps
    /// a job's position and takes a later `at_ms` for a later sample). Whether
    /// it was applied.
    pub fn apply(&mut self, v: WorkView) -> bool {
        if self
            .views
            .get(&v.id)
            .is_some_and(|o| (v.position, v.at_ms) <= (o.position, o.at_ms))
        {
            return false;
        }
        if let Some(t) = self.touched.as_mut() {
            t.insert(v.id.clone());
        }
        self.put(v);
        true
    }

    fn put(&mut self, v: WorkView) {
        if let (Some(sid), WorkKind::Conversation | WorkKind::Task) = (&v.session_id, v.kind) {
            self.sessions.insert(sid.clone(), v.id.clone());
        }
        self.views.insert(v.id.clone(), v);
    }

    /// An id left the board (an ended child past its root's last event). A
    /// session's own view stays: the board's row for it stays too (executions
    /// are never dropped within a connection), and its report with it.
    pub fn gone(&mut self, id: &str) {
        if self
            .views
            .get(id)
            .is_some_and(|v| matches!(v.kind, WorkKind::Conversation | WorkKind::Task))
        {
            return;
        }
        self.views.remove(id);
    }

    /// `work.watch`'s answer. The first after a connect is the truth: the ids
    /// it lacks go, unless an event on the new connection brought them.
    pub fn snapshot(&mut self, views: Vec<WorkView>) {
        self.live = true;
        let Some(touched) = self.touched.take() else {
            for v in views {
                self.apply(v);
            }
            return;
        };
        let listed: HashSet<&str> = views.iter().map(|v| v.id.as_str()).collect();
        self.views
            .retain(|id, _| listed.contains(id.as_str()) || touched.contains(id));
        self.sessions.retain(|_, id| self.views.contains_key(id));
        for v in views {
            if touched.contains(&v.id) {
                self.apply(v);
            } else {
                self.put(v);
            }
        }
    }

    /// A session's view.
    pub fn of_session(&self, sid: &str) -> Option<&WorkView> {
        self.views.get(self.sessions.get(sid)?)
    }

    /// A session's title as the daemon names it, if it does.
    pub fn title(&self, sid: &str) -> Option<&str> {
        self.of_session(sid)
            .map(|v| v.title.as_str())
            .filter(|t| !t.trim().is_empty())
    }

    /// The job each task or conversation is waiting on, by its view's id: the
    /// one that began last among those that work.
    pub fn jobs(&self) -> HashMap<&str, &WorkView> {
        let mut out: HashMap<&str, &WorkView> = HashMap::new();
        for v in self.views.values() {
            let (WorkKind::Job, Some(parent), true) =
                (v.kind, v.parent.as_deref(), v.state.working())
            else {
                continue;
            };
            if out
                .get(parent)
                .is_none_or(|old| (v.started_at_ms, &v.id) > (old.started_at_ms, &old.id))
            {
                out.insert(parent, v);
            }
        }
        out
    }

    /// Whether any session shows a time that runs (a model, a tool or a job),
    /// so the loop ticks on the minute.
    pub fn ticking(&self) -> bool {
        self.live
            && self.views.values().any(|v| {
                matches!(v.kind, WorkKind::Conversation | WorkKind::Task)
                    && v.state.working()
                    && v.doing.as_ref().is_some_and(|d| {
                        matches!(
                            d.what,
                            DoingKind::Thinking | DoingKind::Tool | DoingKind::Job
                        )
                    })
            })
    }

    /// What a session's row says, if the daemon's view of it agrees with the
    /// board's level (`level`): the two streams may be a frame apart, and a row
    /// never words a state the glyph beside it does not show.
    pub fn facts(
        &self,
        sid: &str,
        level: theseus_protocol::Level,
        jobs: &HashMap<&str, &WorkView>,
    ) -> Option<Facts> {
        let v = self
            .of_session(sid)
            .filter(|v| v.attention.level == level)?;
        Some(Facts {
            steps: v
                .progress
                .as_ref()
                .filter(|p| p.steps_total > 0)
                .map(|p| (p.steps_done, p.steps_total)),
            paused: v.paused,
            timing: v
                .forecast
                .clone()
                .filter(|_| !v.state.ended())
                .map(|f| (f, v.started_at_ms, v.cost.map_or(0.0, |c| c.spent_usd))),
            form: form_of(v, jobs.get(v.id.as_str()).copied()),
            report: (v.state == WorkState::Done && v.kind == WorkKind::Task)
                .then(|| {
                    v.report
                        .as_ref()
                        .and_then(|r| r.first_line.as_deref())
                        .map(first_line)
                        .filter(|l| !l.is_empty())
                })
                .flatten(),
        })
    }
}

fn form_of(v: &WorkView, job: Option<&WorkView>) -> Form {
    if let Some(q) = v.question.as_ref().filter(|_| !v.state.ended()) {
        return Form::Text(match q.kind {
            theseus_protocol::work::QuestionKind::Budget => v.attention.label.clone(),
            _ => format!("asks: {}", one_line(&q.prompt)),
        });
    }
    match v.state {
        WorkState::Failed => Form::Failed {
            at_ms: v.ended_at_ms.unwrap_or(v.state_since_ms),
            reason: v
                .report
                .as_ref()
                .and_then(|r| r.reason.as_deref())
                .map(one_line)
                .unwrap_or_default(),
        },
        WorkState::Done => Form::Done {
            at_ms: v.ended_at_ms.unwrap_or(v.state_since_ms),
        },
        WorkState::Running | WorkState::Waiting | WorkState::Queued => match &v.doing {
            Some(d) => match d.what {
                DoingKind::Thinking => Form::Model {
                    since_ms: d.since_ms,
                },
                DoingKind::Tool => Form::Tool {
                    label: d.label.clone(),
                    since_ms: d.since_ms,
                },
                DoingKind::Job => Form::Job {
                    since_ms: job
                        .map(|j| j.started_at_ms)
                        .filter(|t| *t > 0)
                        .unwrap_or(d.since_ms),
                    usual_ms: job.and_then(|j| j.eta).map(|e| e.usual_ms),
                    said: v
                        .progress
                        .as_ref()
                        .and_then(|p| p.said.as_deref())
                        .map(one_line)
                        .filter(|s| !s.is_empty()),
                    said_at_ms: v.progress.as_ref().map_or(0, |p| p.at_ms),
                    doing_ms: d.since_ms,
                },
                _ => Form::Text(v.attention.label.clone()),
            },
            None => Form::Text(v.attention.label.clone()),
        },
        _ => Form::Text(v.attention.label.clone()),
    }
}

/// `m:ss`, or `h:mm:ss` past an hour.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// What a row says its session is doing, at `now_ms`; `hm` writes a time of
/// day. `paused` and the plan's `step 3 of 5` come first, the spend against
/// the usual and the time left last.
pub fn doing(f: &Facts, now_ms: u64, hm: &dyn Fn(u64) -> String) -> String {
    let since = |t: u64| clock(now_ms.saturating_sub(t));
    let body = match &f.form {
        Form::Model { since_ms } => format!("model {}", since(*since_ms)),
        Form::Tool { label, since_ms } => format!("{label} {}", since(*since_ms)),
        Form::Job {
            since_ms,
            usual_ms,
            said,
            said_at_ms,
            doing_ms,
        } => {
            let mut s = format!("job {}", since(*since_ms));
            if let Some(u) = usual_ms {
                s.push_str(&format!(" of usual {}", clock(*u)));
            }
            if let Some(w) = said {
                let w = aged_said(w, *said_at_ms, Some(*doing_ms), now_ms);
                s.push_str(&format!(" · {w}"));
            }
            s
        }
        Form::Failed { at_ms, reason } if reason.is_empty() => format!("failed {}", hm(*at_ms)),
        Form::Failed { at_ms, reason } => format!("failed {}: {reason}", hm(*at_ms)),
        Form::Done { at_ms } => format!("done {}", hm(*at_ms)),
        Form::Text(t) => t.clone(),
    };
    // `paused` first, so a cut row keeps it; then the step, what it does, and
    // the time left.
    let parts = [
        f.paused.then(|| "paused".to_string()),
        f.steps.and_then(|(done, total)| step_words(done, total)),
        Some(body),
        f.timing
            .as_ref()
            .and_then(|(fc, _, spent)| forecast_cost(fc, *spent)),
        f.timing
            .as_ref()
            .and_then(|(fc, started, spent)| time_left(fc, *started, *spent, now_ms)),
    ];
    parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// A row's title and doing cut to `room` columns, two apart: the title first,
/// down to `MIN_TITLE` columns; then the doing.
pub fn fit(title: &str, suffix: &str, doing: &str, room: usize) -> (String, String) {
    let (t, s, d) = (title.width(), suffix.width(), doing.width());
    if t + s + 2 + d <= room {
        return (format!("{title}{suffix}"), doing.to_string());
    }
    let min = (MIN_TITLE + s).min(t + s);
    let title_room = room.saturating_sub(2 + d);
    if title_room >= min {
        return (
            format!("{}{suffix}", cut(title, title_room - s)),
            doing.to_string(),
        );
    }
    let doing_room = room.saturating_sub(min + 2);
    (
        format!("{}{suffix}", cut(title, min - s)),
        cut(doing, doing_room),
    )
}

/// The least of a title a cut doing leaves.
const MIN_TITLE: usize = 8;

/// `text` in at most `room` columns, `…` where it was cut.
pub fn cut(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    if room == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w + 1 > room {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

fn one_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default();
    let line: String = line.chars().filter(|c| !c.is_control()).collect();
    crate::glyph::tidy(line.trim()).into_owned()
}

fn first_line(s: &str) -> String {
    one_line(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row words only a state its glyph shows: the work view's level must be
    /// the board's (the two streams may be a frame apart). Reviewer A's planted
    /// revert of this rule was caught by no test.
    #[test]
    fn a_row_takes_the_work_views_words_only_at_the_boards_level() {
        let v: WorkView = serde_json::from_value(serde_json::json!({
            "id": "exe_1", "kind": "task", "position": 3, "at_ms": 1, "root": "exe_1",
            "session_id": "ses_task", "title": "t", "state": "running",
            "attention": {"level": "working", "label": "turn 1", "since_ms": 1},
            "started_at_ms": 1, "state_since_ms": 1, "flagged": false,
            "progress": {"steps_done": 1, "steps_total": 3, "said": "Laying out.", "at_ms": 1}
        }))
        .unwrap();
        let mut w = Work::default();
        assert!(w.apply(v));
        let jobs = HashMap::new();
        assert!(w
            .facts("ses_task", theseus_protocol::Level::Working, &jobs)
            .is_some());
        assert!(w
            .facts("ses_task", theseus_protocol::Level::NeedsYou, &jobs)
            .is_none());
    }

    /// A job row's last words carry their age when they are over a minute
    /// older than what the task does now, and none when they are fresh
    /// (theseus-ztkhg.1).
    #[test]
    fn a_job_rows_old_words_carry_their_age() {
        let row = |said_at_ms: u64| {
            let v: WorkView = serde_json::from_value(serde_json::json!({
                "id": "exe_1", "kind": "task", "position": 3, "at_ms": 1, "root": "exe_1",
                "session_id": "ses_task", "title": "t", "state": "running",
                "attention": {"level": "working", "label": "job", "since_ms": 1},
                "doing": {"what": "job", "label": "job", "since_ms": 200_000, "turn": 1, "loop_index": 0},
                "started_at_ms": 1, "state_since_ms": 1, "flagged": false,
                "progress": {"steps_done": 0, "steps_total": 0, "said": "Round 1 done. Continuing.", "at_ms": said_at_ms}
            }))
            .unwrap();
            let mut w = Work::default();
            assert!(w.apply(v));
            let f = w
                .facts(
                    "ses_task",
                    theseus_protocol::Level::Working,
                    &HashMap::new(),
                )
                .unwrap();
            doing(&f, 230_000, &|t| t.to_string())
        };
        assert_eq!(
            row(100_000),
            "job 0:30 · Round 1 done. Continuing. (2 min ago)"
        );
        assert_eq!(row(190_000), "job 0:30 · Round 1 done. Continuing.");
    }

    /// The daemon keeps a job's position and takes a later `at_ms` for a later
    /// sample: the row's last line must follow both samples.
    #[test]
    fn a_job_rows_last_line_advances_at_one_position() {
        let sample = |at_ms: u64, said: &str| -> WorkView {
            serde_json::from_value(serde_json::json!({
                "id": "exe_1", "kind": "task", "position": 3, "at_ms": at_ms, "root": "exe_1",
                "session_id": "ses_task", "title": "t", "state": "running",
                "attention": {"level": "working", "label": "job", "since_ms": 1},
                "doing": {"what": "job", "label": "job", "since_ms": 1, "turn": 1, "loop_index": 0},
                "started_at_ms": 1, "state_since_ms": 1, "flagged": false,
                "progress": {"steps_done": 0, "steps_total": 0, "said": said, "at_ms": at_ms}
            }))
            .unwrap()
        };
        let line = |w: &Work| {
            let f = w
                .facts(
                    "ses_task",
                    theseus_protocol::Level::Working,
                    &HashMap::new(),
                )
                .unwrap();
            doing(&f, 10_000, &|t| t.to_string())
        };
        let mut w = Work::default();
        assert!(w.apply(sample(5_000, "Round 1 done.")));
        assert!(w.apply(sample(6_000, "Round 2 done.")));
        assert!(line(&w).contains("Round 2 done."), "{}", line(&w));
        assert!(
            !w.apply(sample(6_000, "Round 2 done.")),
            "a repeat is not news"
        );
        assert!(
            !w.apply(sample(5_500, "stale")),
            "an older sample is dropped"
        );
    }

    #[test]
    fn a_clock_reads_minutes_and_seconds_and_hours() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(31_000), "0:31");
        assert_eq!(clock(134_000), "2:14");
        assert_eq!(clock(3_725_000), "1:02:05");
    }

    #[test]
    fn a_title_is_cut_before_the_doing_and_the_doing_after_it() {
        let (t, d) = fit("Sound the north channel", "", "job 2:14 of usual 2:40", 48);
        assert_eq!(
            (t.as_str(), d.as_str()),
            ("Sound the north channel", "job 2:14 of usual 2:40")
        );
        let (t, d) = fit("Sound the north channel", "", "job 2:14 of usual 2:40", 40);
        assert_eq!(
            (t.as_str(), d.as_str()),
            ("Sound the north…", "job 2:14 of usual 2:40")
        );
        let (t, d) = fit("Sound the north channel", "", "job 2:14 of usual 2:40", 34);
        assert_eq!(t, "Sound the…");
        assert_eq!(d, "job 2:14 of usual 2:40");
        let (t, d) = fit("Sound the north channel", "", "job 2:14 of usual 2:40", 20);
        assert_eq!(t, "Sound t…");
        assert_eq!(d, "job 2:14 …");
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(cut("abcd", 3), "ab…");
    }
}
