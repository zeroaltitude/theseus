//! The future in Discord (theseus-j90w2; horizon HZ9): what the board's
//! **Coming up**, **Routines** and **Projects** are drawn from, kept in
//! memory, and the buttons of a post that speaks for a routine or a watch.
//! A child of `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - **The snapshot.** One loop reads the routines (`routine.list`, and the
//!   30-day record of the few the board shows from `routine.runs`), the
//!   watches (`watch.list`) and the next 30 days (`work.agenda`, once, naming
//!   the places' sessions as the Discord surface must) whenever something
//!   says the future may have changed: the work engine's changes, a wake
//!   noticed, a post the courier made for a routine or a watch, or a command
//!   here. A burst is one read (`DEBOUNCE`), reads are at least `MIN_GAP`
//!   apart, and a read that changed nothing redraws nothing. The commands'
//!   autocomplete reads this snapshot and no daemon: it answers from memory.
//! - **The board** gets the snapshot for its conversations. `PlaceMsg::Tasks`
//!   redraws it, so the lane's own pace (`BOARD_EVERY`) bounds the edits.
//! - **A post's buttons** (`hz:<answer>:<wake id>`): a failed or blocked
//!   routine's Run now / Pause / Details, a met watch's Done. A press is
//!   `work.answer` as the presser, which the core judges as it judges the
//!   CLI's; the message becomes the answer, its buttons gone.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::answer::{WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::routines::{
    RoutineInfo, RoutineListResult, RoutineRunsParams, RoutineRunsResult,
};
use theseus_protocol::wakes::WakeAnswer;
use theseus_protocol::watches::{WatchInfo, WatchListParams, WatchListResult};
use theseus_protocol::work::horizon::{AgendaLine, AgendaParams, AgendaResult, AgendaWhat};
use theseus_protocol::work::WorkKind;
use theseus_protocol::{method, now_unix_ms, PlaceClass};
use tokio::sync::Notify;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::{AllowedMentions, Component, MessageFlags};
use twilight_model::http::interaction::{InteractionResponseData, InteractionResponseType};

use super::{PlaceMsg, Shared};
use crate::render::horizon::{self as words, FutureParts};

/// A burst of changes is one read: wait this long after the first.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// With nothing said, the future is read again after this long.
#[cfg(not(test))]
const IDLE: Duration = Duration::from_secs(60);
#[cfg(test)]
const IDLE: Duration = Duration::from_millis(700);
/// Reads are at least this far apart.
const MIN_GAP: Duration = Duration::from_secs(2);
/// A post button's `custom_id` prefix: `hz:<answer id>:<wake id>`.
pub(crate) const POST_PREFIX: &str = "hz";
/// How many routines have their 30-day record read.
const RECORDS: usize = 8;

/// What the last read found.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Snapshot {
    pub routines: Vec<RoutineInfo>,
    pub watches: Vec<WatchInfo>,
    pub lines: Vec<AgendaLine>,
    pub projects: Vec<theseus_protocol::work::WorkView>,
    /// `(ok, failed)` of each routine's runs in the last 30 days.
    pub records: HashMap<String, (u32, u32)>,
    pub at_ms: u64,
}

/// The future's memory: the snapshot, and the wake that asks for a read.
#[derive(Default)]
pub(crate) struct Future {
    snap: Mutex<Snapshot>,
    dirty: Notify,
    /// The same word for the scheduled cards' worker (`sched.rs`): a
    /// `Notify` wakes one waiter, so each reader has its own.
    pub(super) sched: Notify,
    /// What the records were read for: `(runs, last started)` of a routine.
    seen: Mutex<HashMap<String, (u32, u64, bool)>>,
    /// Command watches waiting for the owner's yes, by token: when they were
    /// made, and what they will create (`horizon_cmds.rs`).
    pub(super) pending: Mutex<HashMap<String, (u64, theseus_protocol::watches::WatchCreateParams)>>,
}

impl Future {
    #[cfg(test)]
    pub(super) fn seed_for_test(&self, s: Snapshot) {
        self.set(s);
    }

    /// The future may have changed: read it again, once for a burst.
    pub fn dirty(&self) {
        self.dirty.notify_one();
        self.sched.notify_one();
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().unwrap().clone()
    }

    fn set(&self, s: Snapshot) -> bool {
        let mut cur = self.snap.lock().unwrap();
        let (mut a, mut b) = (s.clone(), cur.clone());
        a.at_ms = 0;
        b.at_ms = 0;
        let changed = a != b;
        *cur = s;
        if changed {
            // The scheduled cards follow what the snapshot holds.
            self.sched.notify_one();
        }
        changed
    }

    /// The board's parts: the next five lines, the routines by their next
    /// run, the projects. None when there is nothing to show. With `own`, a
    /// conversation's reminder or check-in is a line only when that
    /// conversation is one of `own` (a routine's run, a task's start and a
    /// watch are not a conversation's).
    pub fn parts(
        &self,
        own: Option<&HashSet<String>>,
        names: HashMap<String, String>,
    ) -> Option<FutureParts> {
        let s = self.snapshot();
        let lines: Vec<AgendaLine> = s
            .lines
            .iter()
            .filter(|l| {
                own.is_none_or(|own| {
                    !is_wake_of_conversation(l)
                        || l.session_id.as_ref().is_some_and(|s| own.contains(s))
                })
            })
            .cloned()
            .collect();
        let mut routines = s.routines.clone();
        routines.sort_by_key(|r| (r.paused, r.next_ms.unwrap_or(u64::MAX), r.id.clone()));
        let p = FutureParts {
            coming: words::next_lines(&lines, 50),
            routines,
            projects: s.projects,
            records: s.records,
            names,
        };
        (!p.is_empty()).then_some(p)
    }

    /// The pending wakes by id, soonest first, reminders before the rest:
    /// what `/snooze` and `/cancel` offer. A series is listed once.
    pub fn wakes(&self) -> Vec<AgendaLine> {
        let mut lines = self.snapshot().lines;
        lines.sort_by_key(|l| l.at_ms);
        let mut seen = std::collections::HashSet::new();
        let mut v: Vec<AgendaLine> = lines
            .into_iter()
            .filter(|l| l.kind == WorkKind::Wake && l.id.starts_with("wak_"))
            .filter(|l| seen.insert(l.id.clone()))
            .collect();
        v.sort_by_key(|l| {
            (
                l.purpose.as_deref() != Some("remind"),
                l.at_ms,
                l.id.clone(),
            )
        });
        v
    }
}

/// A reminder or a check-in: a wake that belongs to the conversation that set
/// it.
fn is_wake_of_conversation(l: &AgendaLine) -> bool {
    l.what == AgendaWhat::Due && !matches!(l.purpose.as_deref(), Some("watch" | "routine"))
}

/// The loop that keeps the snapshot. It ends with the daemon's stop.
pub(super) fn start(shared: &Arc<Shared>) {
    let s = shared.clone();
    tokio::spawn(async move {
        let stopped = s.core.outbox.stopped();
        tokio::pin!(stopped);
        s.tasks.future.dirty();
        loop {
            // Something said the future moved, or the safety read comes:
            // a routine made from the CLI, a wake set by the model, and a
            // new day move it with no event this binding hears.
            tokio::select! {
                () = s.tasks.future.dirty.notified() => {}
                () = tokio::time::sleep(IDLE) => {}
                () = &mut stopped => return,
            }
            tokio::time::sleep(DEBOUNCE).await;
            if s.read_future().await {
                s.redraw_private_boards();
            }
            tokio::time::sleep(MIN_GAP).await;
        }
    });
}

impl Shared {
    /// Whether the place `target` is private: the future is shown there alone.
    pub(crate) fn is_private(&self, target: &str) -> bool {
        self.core
            .runner
            .place_rule
            .class(&self.core.cfg, Some(target))
            == PlaceClass::Private
    }

    /// Every private place draws its board again.
    pub(super) fn redraw_private_boards(&self) {
        self.broadcast(|| PlaceMsg::Tasks);
    }

    /// Read the routines, the watches and the agenda; true when the snapshot
    /// changed. A read that fails keeps the last snapshot and says so once in
    /// health.
    pub(crate) async fn read_future(&self) -> bool {
        let now = now_unix_ms();
        let routines = match self
            .rpc
            .call::<_, RoutineListResult>(method::ROUTINE_LIST, json!({}))
            .await
        {
            Ok(r) => r.routines,
            Err(e) => {
                self.board.error("routine.list", None, e);
                return false;
            }
        };
        // A read that fails keeps the last snapshot: an empty answer made of an
        // error would be stamped as read and take the watches off the board.
        let watches = match self
            .rpc
            .call::<_, WatchListResult>(method::WATCH_LIST, WatchListParams::default())
            .await
        {
            Ok(r) => r.watches,
            Err(e) => {
                self.board.error("watch.list", None, e);
                return false;
            }
        };
        let roots: Vec<String> = {
            let r = self.routes.lock().unwrap();
            r.by_session
                .keys()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect()
        };
        let lines = if roots.is_empty() {
            vec![]
        } else {
            match self
                .rpc
                .call::<_, AgendaResult>(
                    method::WORK_AGENDA,
                    AgendaParams {
                        from_ms: now,
                        to_ms: now + 30 * 86_400_000,
                        roots: Some(roots),
                    },
                )
                .await
            {
                Ok(r) => r.lines,
                Err(e) => {
                    self.board.error("work.agenda", None, e);
                    return false;
                }
            }
        };
        let records = self.read_records(&routines, now).await;
        let projects = self.tasks.projects();
        self.tasks.set_reminder(self.reminder_today(&lines, now));
        self.tasks.future.set(Snapshot {
            routines,
            watches,
            lines,
            projects,
            records,
            at_ms: now,
        })
    }

    /// The next reminder (a person's, not a check-in) due before the local
    /// midnight, as minutes into the local day.
    fn reminder_today(&self, lines: &[AgendaLine], now: u64) -> Option<u16> {
        let zone = &self.core.kernel.config().zone;
        let day = |ms| theseus_kernel::cron::local(zone, ms, "%Y-%m-%d");
        let today = day(now);
        let at = lines
            .iter()
            .filter(|l| l.purpose.as_deref() == Some("remind") && l.at_ms > now)
            .map(|l| l.at_ms)
            .filter(|&ms| day(ms) == today)
            .min()?;
        let hm = theseus_kernel::cron::local(zone, at, "%H:%M");
        let (h, m) = hm.split_once(':')?;
        Some(h.parse::<u16>().ok()? * 60 + m.parse::<u16>().ok()?)
    }

    /// The 30-day record of the first few routines, read again only when the
    /// routine's run count, its last run or its running changed.
    async fn read_records(
        &self,
        routines: &[RoutineInfo],
        now: u64,
    ) -> HashMap<String, (u32, u32)> {
        let mut out = self.tasks.future.snapshot().records;
        out.retain(|id, _| routines.iter().any(|r| &r.id == id));
        for r in routines.iter().take(RECORDS) {
            let key = (
                r.runs,
                r.last.as_ref().map_or(0, |l| l.started_ms),
                r.running,
            );
            if out.contains_key(&r.id)
                && self.tasks.future.seen.lock().unwrap().get(&r.id) == Some(&key)
            {
                continue;
            }
            let got = self
                .rpc
                .call::<_, RoutineRunsResult>(
                    method::ROUTINE_RUNS,
                    RoutineRunsParams {
                        id: r.id.clone(),
                        limit: Some(60),
                    },
                )
                .await;
            if let Ok(g) = got {
                let since = now.saturating_sub(30 * 86_400_000);
                let (mut ok, mut failed) = (0, 0);
                for run in g.runs.iter().filter(|x| x.started_ms >= since) {
                    match run.state.as_str() {
                        "complete" => ok += 1,
                        "failed" => failed += 1,
                        _ => {}
                    }
                }
                out.insert(r.id.clone(), (ok, failed));
                self.tasks
                    .future
                    .seen
                    .lock()
                    .unwrap()
                    .insert(r.id.clone(), key);
            }
        }
        out
    }
}

// ---- a post's buttons -----------------------------------------------------

/// The buttons of a post that speaks for a routine or a watch: one per
/// answer, `hz:<answer id>:<wake id>`.
pub(crate) fn buttons(wake: &str, answers: &[(String, String)]) -> Vec<Component> {
    let row: Vec<Component> = answers
        .iter()
        .take(5)
        .map(|(id, label)| {
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{POST_PREFIX}:{id}:{wake}")),
                disabled: false,
                emoji: None,
                label: Some(match id.as_str() {
                    "run_now" => format!("▶ {label}"),
                    "pause" => format!("⏸ {label}"),
                    _ => label.clone(),
                }),
                style: if id == "run_now" || id == "done" {
                    ButtonStyle::Success
                } else {
                    ButtonStyle::Secondary
                },
                url: None,
                sku_id: None,
            })
        })
        .collect();
    if row.is_empty() {
        return vec![];
    }
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: row,
    })]
}

/// The answers a post's body carries, as `(id, label)`: only those a surface
/// can send through `work.answer` (a method the daemon answers).
pub(crate) fn answers_of(body: &Value) -> Vec<(String, String)> {
    let all: Vec<WakeAnswer> = serde_json::from_value(body["answers"].clone()).unwrap_or_default();
    all.into_iter().map(|a| (a.id, a.label)).collect()
}

/// A press: its answer's id and the wake's.
pub(crate) fn parse_post(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(POST_PREFIX)?.strip_prefix(':')?;
    let (answer, wake) = rest.split_once(':')?;
    (!answer.is_empty() && wake.starts_with("wak_")).then(|| (answer.to_string(), wake.to_string()))
}

impl Shared {
    /// A button of a routine's or a watch's post: `work.answer` as the
    /// presser. The message becomes the answer (its buttons go); a refusal is
    /// told to the presser alone and the buttons stay.
    pub(super) async fn post_press(
        &self,
        i: &Interaction,
        (answer, wake): (String, String),
        who: &str,
        discord: Option<theseus_protocol::DiscordOrigin>,
    ) {
        let r = self
            .rpc
            .call::<_, WorkAnswerResult>(
                method::WORK_ANSWER,
                WorkAnswerParams {
                    question_id: wake.clone(),
                    option: answer.clone(),
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                },
            )
            .await;
        self.tasks.future.dirty();
        let was = i
            .message
            .as_ref()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let data = match r {
            Ok(r) if answer == "details" => {
                // Details are for the presser alone, and the buttons stay.
                self.reply_ephemeral(i, r.detail.unwrap_or_else(|| "no details".into()))
                    .await;
                return;
            }
            Ok(_) => InteractionResponseData {
                content: Some(format!("{was}\n-# {}", settled(&answer, who))),
                components: Some(vec![]),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            },
            Err(e) => {
                self.reply_ephemeral(i, format!("⚠️ That did not count: {}", e.message))
                    .await;
                return;
            }
        };
        self.answer(i, InteractionResponseType::UpdateMessage, Some(data))
            .await;
    }

    /// An ephemeral line for the presser.
    pub(super) async fn reply_ephemeral(&self, i: &Interaction, text: String) {
        let data = InteractionResponseData {
            content: Some(crate::render::clip(&text, 1_900)),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            ..Default::default()
        };
        self.answer(
            i,
            InteractionResponseType::ChannelMessageWithSource,
            Some(data),
        )
        .await;
    }
}

/// What a settled post says of its answer.
fn settled(answer: &str, _who: &str) -> String {
    match answer {
        "run_now" => "▶ Running now".into(),
        "pause" => "⏸ Paused".into(),
        "done" => "✅ Done".into(),
        other => format!("answered: {other}"),
    }
}

impl Shared {
    /// The future may have changed (a post the courier made for a routine or
    /// a watch): read it again, once for a burst.
    pub(crate) fn tasks_future_dirty(&self) {
        self.tasks.future.dirty();
    }

    /// A routine run's report with the routine's own head (theseus-j90w2):
    /// `🔁 Morning digest · <time> · run 22 · 52 s · $0.04`, then its words.
    /// None for a report that is not a finished routine run's.
    pub(crate) fn routine_report(&self, body: &Value, said: Option<&str>) -> Option<String> {
        if body["outcome"].as_str() != Some("complete") {
            return None;
        }
        let task = body["task"].as_str()?;
        let mark: theseus_protocol::routines::RunMark = self
            .core
            .store
            .get_meta(&format!("{}{task}", theseus_protocol::routines::RUN_PREFIX))
            .ok()
            .flatten()?;
        let spec = self
            .core
            .runner
            .tools
            .routines
            .find(&self.core.store, &mark.routine)
            .ok()?;
        let took_ms = body["elapsed_ms"].as_u64().unwrap_or(0);
        let started = now_unix_ms().saturating_sub(took_ms) / 1000;
        let took = if took_ms < 60_000 {
            format!("{} s", took_ms / 1000)
        } else {
            format!("{} min {} s", took_ms / 60_000, took_ms % 60_000 / 1000)
        };
        let head = format!(
            "🔁 **{}** · <t:{started}:f> · run {} · {took} · {}",
            crate::render::clip(&spec.title, 80),
            mark.run,
            theseus_protocol::usd(body["spent_usd"].as_f64().unwrap_or(0.0)),
        );
        Some(match said.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => format!("{head}\n{}", crate::render::clip(s, 1_800)),
            None => head,
        })
    }
}

impl Shared {
    /// The name of each conversation by its session, as health lists the
    /// places: a place's own, Home's by the place that adopts it, a thread's
    /// by the place it answers at.
    pub(super) fn conversation_names(&self) -> HashMap<String, String> {
        let mut names: HashMap<String, String> = self
            .board
            .st
            .lock()
            .unwrap()
            .places
            .iter()
            .filter_map(|p| {
                Some((
                    p.session_id.clone().filter(|s| !s.is_empty())?,
                    p.label.clone(),
                ))
            })
            .collect();
        for (session, place) in self.tasks.adopted() {
            if let Some(name) = names.get(&place).cloned() {
                names.insert(session, name);
            }
        }
        names
    }
}

/// What a place's board shows of the future (issue 30; `docs/design/board-place.md`).
/// While the task channel stands, or is being looked for, and there is an
/// owner's DM place to draw it from, the whole future is on the board that
/// place draws into the task channel, and on no other place's. Otherwise (no
/// task channel, or no DM bound: the task channel then has no place to be
/// drawn by) a place shows the reminders and check-ins of its own
/// conversations, and the routines and projects as before.
pub(super) fn parts_for(shared: &Shared, target: &str, session: &str) -> Option<FutureParts> {
    let names = shared.conversation_names();
    if shared.task_posts.home.expected() && shared.owner_dm_place().is_some() {
        let mut parts = (shared.owner_dm_place().as_deref() == Some(target))
            .then(|| shared.tasks.future.parts(None, names))
            .flatten()?;
        // Where the scheduled items have cards of their own the cards are the
        // list: a reminder, a check-in, a routine's run and a task's start are
        // off the board's Coming up, and the Routines with them. What has no
        // card (a watch's give-up, a milestone) stays, and so do the tasks and
        // the projects.
        if shared.sched_target().is_some() {
            parts.coming.retain(|l| {
                !(is_wake_of_conversation(l)
                    || matches!(l.what, AgendaWhat::Run | AgendaWhat::Starts))
            });
            parts.routines.clear();
        }
        return (!parts.is_empty()).then_some(parts);
    }
    let own = shared.tasks.sessions_shown(session);
    shared.tasks.future.parts(Some(&own), names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_shared_board_shows_its_own_upcoming_reminder() {
        let d = tempfile::tempdir().unwrap();
        let core = crate::runtime::tests::core_scripted(d.path(), vec![]);
        let shared = crate::runtime::shared_for_tests(&core);
        let target = "discord:channel:123";
        assert!(!shared.is_private(target));
        shared.tasks.future.set(Snapshot {
            lines: vec![AgendaLine {
                at_ms: 1000,
                id: "wak_remind".into(),
                kind: WorkKind::Wake,
                what: AgendaWhat::Due,
                title: "stretch".into(),
                session_id: Some("ses_example".into()),
                purpose: Some("remind".into()),
                occurrence: None,
            }],
            ..Default::default()
        });
        let parts =
            parts_for(&shared, target, "ses_example").expect("shared is not a withholding rule");
        assert_eq!(parts.coming[0].title, "stretch");
    }

    #[test]
    fn reminders_come_first_in_the_pending_wakes_a_command_offers() {
        use theseus_protocol::work::horizon::AgendaWhat;
        let line = |id: &str, at: u64, purpose: &str| AgendaLine {
            at_ms: at,
            id: id.into(),
            kind: WorkKind::Wake,
            what: AgendaWhat::Due,
            title: id.into(),
            session_id: None,
            purpose: Some(purpose.into()),
            occurrence: None,
        };
        let f = Future::default();
        f.set(Snapshot {
            lines: vec![
                line("wak_check_early", 1_000, "check"),
                line("wak_remind_late", 9_000, "remind"),
                line("wak_remind_late", 19_000, "remind"),
                line("wak_remind_soon", 5_000, "remind"),
                // A routine's run is not a wake to snooze.
                AgendaLine {
                    id: "ses_routine".into(),
                    what: AgendaWhat::Run,
                    ..line("x", 500, "routine")
                },
            ],
            ..Default::default()
        });
        let ids: Vec<String> = f.wakes().into_iter().map(|l| l.id).collect();
        assert_eq!(
            ids,
            ["wak_remind_soon", "wak_remind_late", "wak_check_early"]
        );
    }

    #[test]
    fn a_posts_buttons_carry_the_wake_and_parse_back() {
        let b = buttons(
            "wak_01J0aaaa",
            &[
                ("run_now".into(), "Run now".into()),
                ("pause".into(), "Pause".into()),
                ("details".into(), "Details".into()),
            ],
        );
        let Component::ActionRow(r) = &b[0] else {
            panic!()
        };
        let ids: Vec<String> = r
            .components
            .iter()
            .map(|c| match c {
                Component::Button(b) => b.custom_id.clone().unwrap(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            ids,
            [
                "hz:run_now:wak_01J0aaaa",
                "hz:pause:wak_01J0aaaa",
                "hz:details:wak_01J0aaaa"
            ]
        );
        assert_eq!(
            parse_post(&ids[1]),
            Some(("pause".into(), "wak_01J0aaaa".into()))
        );
        assert_eq!(parse_post("hz:pause:ses_1"), None);
        assert_eq!(parse_post("wake:done:wak_1"), None);
        assert!(buttons("wak_1", &[]).is_empty());
    }
}
