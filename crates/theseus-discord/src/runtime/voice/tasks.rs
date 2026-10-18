//! A task's question and report in the owner's call, and the owner's spoken
//! answer (theseus-bh3t.3).
//!
//! - **Said once, short**: from the voice status's one work watch
//!   (`voice_status`), a task that asks the owner something is told as
//!   `<title> needs you: <what>. Say approve or deny.`, and one that ends as
//!   `<title> is done: <its first sentences>`. Each is a `Command::Report`, so
//!   the engine waits for the floor and says it at the next pause. Never tool
//!   output: code, links, paths and tool lines are cut, and the words are at
//!   most `SPOKEN_MAX` characters. A question is said once (a bounded set),
//!   a task's end once, and what was there when the call began is the past.
//! - **Only the owner's, only in private**: a call in a shared voice place
//!   says nothing, nor does one about a task that reports to a shared place.
//!   A task that reports to the voice place's own session is not told here
//!   at its end: its report wakes that session, and `voice_heard` speaks that
//!   turn's reply.
//! - **Held by voice** (theseus-ypnxt.5): a whole utterance of the owner that is
//!   "pause it" or "go on" (`held_words`) is `task.pause` or `task.resume` as the
//!   speaker, for the task the last question named while it is pending, else the
//!   private place's only running task (only paused one, to go on); with two to
//!   choose from it is an ordinary voice turn. The RPC's answer is said back once.
//! - **Answered by voice**: the question last said is pending for
//!   `PENDING_FOR`. While it is, a whole utterance of the owner that is
//!   "approve", "the second one" or "stop it" (`matched`, a table) is
//!   `work.answer` or `task.cancel` as the speaker, with the voice channel's
//!   ids, so the core judges it as it judges a press; it is no voice turn.
//!   Anything else is an ordinary voice turn. A bare "yes" is a backchannel
//!   (`theseus_voice::heard`), so the spoken question asks for "approve".

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_core::Core;
use theseus_protocol::answer::{approves, WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::task_pause::{TaskPauseParams, TaskPauseResult, PAUSED_AT_MS};
use theseus_protocol::work::{answer_options, AnswerOption, Question, QuestionKind, WorkKind};
use theseus_protocol::work::{WorkState, WorkView};
use theseus_protocol::{method, DiscordOrigin, PlaceClass, TaskCancelParams, TaskCancelResult};
use theseus_voice::Command;
use tokio::sync::mpsc;

use super::super::Place;
use super::VoiceTurn;

/// What is said about one task, in characters.
const SPOKEN_MAX: usize = 220;
/// Questions and ends remembered as said.
const SEEN_MAX: usize = 256;
/// How long the last question can be answered by voice.
const PENDING_FOR: Duration = Duration::from_secs(300);
/// Said when the core does not count an answer.
const REFUSED: &str = "I can't take that here.";

/// What the call has told, and the question it waits on.
pub(in crate::runtime) struct Tasks {
    commands: mpsc::UnboundedSender<Command>,
    /// The voice place's target, `discord:channel:<id>`.
    target: String,
    /// When the call began: an end before it is the past.
    pub(super) since_ms: u64,
    seen: HashSet<String>,
    order: VecDeque<String>,
    pub(super) pending: Option<Pending>,
    /// The tasks of this private place that have not ended, by execution id.
    live: HashMap<String, Live>,
}

/// A task the call could pause or resume.
struct Live {
    /// What `task.pause` takes: its session, else its execution.
    task: String,
    running: bool,
    held: bool,
}

/// The question last said.
pub(super) struct Pending {
    pub(super) question: String,
    /// The asking task's execution.
    pub(super) task: String,
    pub(super) options: Vec<AnswerOption>,
    pub(super) at: Instant,
}

/// What a spoken utterance asks for.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Said {
    /// `work.answer` with this option, and the words to answer aloud.
    Answer { option: String, ack: String },
    /// `task.cancel` of the asking task.
    Stop,
}

impl Tasks {
    pub(in crate::runtime) fn new(
        commands: mpsc::UnboundedSender<Command>,
        channel: u64,
        since_ms: u64,
    ) -> Self {
        Self {
            commands,
            target: format!("discord:channel:{channel}"),
            since_ms,
            seen: HashSet::new(),
            order: VecDeque::new(),
            pending: None,
            live: HashMap::new(),
        }
    }

    /// The board when the watch began: all of it is the past.
    pub(in crate::runtime) fn seed(&mut self, views: &[WorkView]) {
        for v in views {
            if let Some(key) = key_of(v) {
                self.mark(key);
            }
        }
    }

    /// True the first time `key` is marked.
    fn mark(&mut self, key: String) -> bool {
        if !self.seen.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        if self.order.len() > SEEN_MAX {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        true
    }

    /// A view from the watch: a new question or a task's end is said.
    pub(in crate::runtime) fn see(&mut self, core: &Core, v: &WorkView) {
        self.track(core, v);
        let Some(key) = key_of(v) else {
            // A question settled elsewhere is no longer the one to answer.
            self.settled(v);
            return;
        };
        let ended = v.kind == WorkKind::Task;
        if ended && v.ended_at_ms.unwrap_or(v.state_since_ms) < self.since_ms {
            return;
        }
        if !self.mark(key) {
            return;
        }
        let session = match (&v.question, ended) {
            (Some(q), false) => q.asked_by.session_id.as_deref(),
            _ => v.session_id.as_deref(),
        };
        let reports_to = session.and_then(|s| core.outbox.target(s));
        let private =
            |t: Option<&str>| core.runner.place_rule.class(&core.cfg, t) == PlaceClass::Private;
        if !private(Some(&self.target)) || !private(reports_to.as_deref()) {
            return;
        }
        // Its report wakes the voice place's session, whose reply is spoken.
        if ended && reports_to.as_deref() == Some(&self.target) {
            return;
        }
        match (&v.question, ended) {
            (Some(q), false) => self.ask(v, q),
            _ => {
                if let Some(text) = ending(v) {
                    let _ = self.commands.send(Command::Report { text });
                }
            }
        }
    }

    /// Keep the private place's open tasks, for "pause it" and "go on".
    fn track(&mut self, core: &Core, v: &WorkView) {
        if v.kind != WorkKind::Task {
            return;
        }
        let to = v.session_id.as_deref().and_then(|s| core.outbox.target(s));
        let private =
            |t: Option<&str>| core.runner.place_rule.class(&core.cfg, t) == PlaceClass::Private;
        if v.state.ended() || !private(Some(&self.target)) || !private(to.as_deref()) {
            self.live.remove(&v.id);
            return;
        }
        self.live.insert(
            v.id.clone(),
            Live {
                task: v.session_id.clone().unwrap_or_else(|| v.id.clone()),
                running: v.state.working(),
                held: v.paused || v.next_at_ms == Some(PAUSED_AT_MS),
            },
        );
    }

    /// (running, held) among the open tasks, for tests.
    #[cfg(test)]
    pub(super) fn open_counts(&self) -> (usize, usize) {
        let held = self.live.values().filter(|l| l.held).count();
        (
            self.live.values().filter(|l| l.running && !l.held).count(),
            held,
        )
    }

    /// The task "pause it" (`pause`) or "go on" means, if `words` say one and
    /// the speaker is the owner: the task the last question named while it is
    /// pending, else the only one running (to pause) or paused (to go on).
    pub(super) fn hold(&self, words: &str, owner: bool, now: Instant) -> Option<(bool, String)> {
        let pause = held_words(words)?;
        if !owner {
            return None;
        }
        if let Some(p) = self
            .pending
            .as_ref()
            .filter(|p| now.duration_since(p.at) <= PENDING_FOR)
        {
            return Some((pause, p.task.clone()));
        }
        let mut open = self
            .live
            .values()
            .filter(|l| if pause { l.running && !l.held } else { l.held });
        let only = open.next()?;
        open.next().is_none().then(|| (pause, only.task.clone()))
    }

    /// Say a task's question, and wait for the answer to it.
    fn ask(&mut self, v: &WorkView, q: &Question) {
        let who = clean(&q.asked_by.title, 80);
        let who = if who.is_empty() { "A task".into() } else { who };
        let (what, how) = match q.kind {
            QuestionKind::Approval => (
                match q.tool.as_deref().filter(|t| !t.is_empty()) {
                    Some(t) => format!("wants to run {}", t.replace(['.', '_'], " ")),
                    None => "wants your approval".into(),
                },
                "Say approve or deny.",
            ),
            QuestionKind::Budget => ("is over its budget".into(), "Say approve or deny."),
            QuestionKind::Change => ("proposes a change".into(), "Say approve or deny."),
            QuestionKind::Extension => {
                ("wants to load an extension".into(), "Say approve or deny.")
            }
            QuestionKind::Ask => (
                sentences(&q.prompt, 1, SPOKEN_MAX),
                "Say the option's number.",
            ),
        };
        let options = match q.options.is_empty() {
            true => answer_options(q),
            false => q.options.clone(),
        };
        let listed = match q.kind {
            QuestionKind::Ask => numbered(&q.choices),
            _ => String::new(),
        };
        let text = format!("{who} needs you: {what}. {listed}{how}");
        self.pending = Some(Pending {
            question: v.id.clone(),
            task: q.asked_by.id.clone(),
            options,
            at: Instant::now(),
        });
        let _ = self.commands.send(Command::Report { text });
    }

    /// A question that has ended is not pending.
    fn settled(&mut self, v: &WorkView) {
        if v.kind == WorkKind::Question
            && v.state.ended()
            && self.pending.as_ref().is_some_and(|p| p.question == v.id)
        {
            self.pending = None;
        }
    }

    /// What `words` asks of the pending question, from the owner: taken, so
    /// it is asked once. Nothing when none is pending (or it is stale), the
    /// speaker is not an owner, or the words are no answer.
    pub(super) fn take(
        &mut self,
        words: &str,
        owner: bool,
        now: Instant,
    ) -> Option<(Said, Pending)> {
        let p = self.pending.as_ref()?;
        if now.duration_since(p.at) > PENDING_FOR {
            self.pending = None;
            return None;
        }
        if !owner {
            return None;
        }
        let said = matched(words, &p.options)?;
        let p = self.pending.take()?;
        if said == Said::Stop {
            // "Stopped." is its end; the board's word of it would repeat it.
            self.mark(format!("end:{}", p.task));
        }
        Some((said, p))
    }
}

/// `<task>:<question>` for a question open, `end:<task>` for a task that ended.
fn key_of(v: &WorkView) -> Option<String> {
    match (v.kind, &v.question) {
        (WorkKind::Question, Some(q)) if !v.state.ended() && q.asked_by.kind == WorkKind::Task => {
            Some(format!("{}:{}", q.asked_by.id, v.id))
        }
        (WorkKind::Task, _) if v.state.ended() => Some(format!("end:{}", v.id)),
        _ => None,
    }
}

/// `<title> is done: <its report's first sentences>`, or `failed:`.
fn ending(v: &WorkView) -> Option<String> {
    let title = clean(&v.title, 80);
    let title = if title.is_empty() {
        "A task".into()
    } else {
        title
    };
    let report = v.report.as_ref()?;
    let reason = report.reason.as_deref().filter(|r| !r.is_empty());
    let line = report.first_line.as_deref().filter(|r| !r.is_empty());
    Some(match v.state {
        WorkState::Done => match line {
            Some(l) => format!("{title} is done: {}", sentences(l, 2, SPOKEN_MAX)),
            None => format!("{title} is done."),
        },
        WorkState::Failed => match reason.or(line) {
            Some(r) => format!("{title} failed: {}", sentences(r, 2, SPOKEN_MAX)),
            None => format!("{title} failed."),
        },
        WorkState::Cancelled => format!("{title} was stopped."),
        WorkState::Expired => format!("{title} expired."),
        _ => return None,
    })
}

/// The choices as speech: `Options: one, A. two, B. `.
fn numbered(choices: &[String]) -> String {
    if choices.is_empty() {
        return String::new();
    }
    let n = ["one", "two", "three", "four", "five", "six"];
    let list: Vec<String> = choices
        .iter()
        .zip(n)
        .map(|(c, n)| format!("{n}, {}.", clean(c, 40).trim_end_matches('.')))
        .collect();
    format!("{} ", list.join(" "))
}

/// `text` as speech: no code, links, paths or tool lines, on one line.
fn clean(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.trim_start().starts_with('$') {
            continue;
        }
        let mut ticked = false;
        for word in line.split_whitespace() {
            // Inline code: from one backtick to the next.
            let opens = word.matches('`').count() % 2 == 1;
            let skip = ticked
                || word.contains('`')
                || word.contains('/')
                || word.contains('\\')
                || is_dotted(word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']));
            if opens {
                ticked = !ticked;
            }
            if !skip {
                out.push_str(word);
                out.push(' ');
            } else if let Some(end) = word.chars().last().filter(|c| matches!(c, '.' | '!' | '?')) {
                // What was cut still ended its sentence.
                out.truncate(out.trim_end().len());
                if !out.is_empty() && !out.ends_with(['.', '!', '?']) {
                    out.push(end);
                    out.push(' ');
                }
            }
        }
    }
    let flat = out.trim().to_string();
    match flat.chars().count() > max {
        true => {
            let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
            let at = cut.rfind(' ').unwrap_or(cut.len());
            format!("{}…", cut[..at].trim_end())
        }
        false => flat,
    }
}

/// A file or a tool: `notes.txt`, `fs.write`.
fn is_dotted(word: &str) -> bool {
    let Some((name, ext)) = word.split_once('.') else {
        return false;
    };
    name.chars().next().is_some_and(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        && !ext.is_empty()
        && ext
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// The first `n` sentences of `text` cleaned, within `max` characters, cut at
/// a sentence when the next would not fit.
fn sentences(text: &str, n: usize, max: usize) -> String {
    let flat = clean(text, usize::MAX);
    let mut out = String::new();
    let mut taken = 0;
    let mut rest = flat.as_str();
    while taken < n && !rest.is_empty() {
        let end = rest
            .char_indices()
            .find(|&(i, c)| {
                matches!(c, '.' | '!' | '?')
                    && rest[i + c.len_utf8()..].starts_with(char::is_whitespace)
            })
            .map_or(rest.len(), |(i, c)| i + c.len_utf8());
        let sentence = rest[..end].trim();
        let joined = match out.is_empty() {
            true => sentence.to_string(),
            false => format!("{out} {sentence}"),
        };
        if joined.chars().count() > max {
            if out.is_empty() {
                return clean(sentence, max);
            }
            break;
        }
        out = joined;
        taken += 1;
        rest = rest[end..].trim_start();
    }
    out.trim_end_matches(['.', '!', '?']).to_string()
}

/// The words as lowercase alphanumerics (an apostrophe stays), spaced singly.
fn normal(words: &str) -> String {
    words
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const APPROVE: &[&str] = &[
    "approve",
    "approved",
    "approve it",
    "approve that",
    "go ahead",
    "do it",
];
const DENY: &[&str] = &[
    "deny", "denied", "no", "don't", "reject", "decline", "deny it",
];
const STOP: &[&str] = &[
    "stop it",
    "cancel it",
    "stop the task",
    "cancel the task",
    "stop that",
    "cancel that",
];
const ORDINALS: &[(&str, usize)] = &[
    ("first", 1),
    ("one", 1),
    ("1", 1),
    ("second", 2),
    ("two", 2),
    ("2", 2),
    ("third", 3),
    ("three", 3),
    ("3", 3),
    ("fourth", 4),
    ("four", 4),
    ("4", 4),
    ("fifth", 5),
    ("five", 5),
    ("5", 5),
    ("sixth", 6),
    ("six", 6),
    ("6", 6),
];
/// Words before an ordinal that change nothing: "the second one", "option two".
const BEFORE: &[&str] = &["the", "option", "number", "choice", "please"];

const PAUSE: &[&str] = &["pause it", "pause that", "pause the task"];
const RESUME: &[&str] = &["go on", "resume it", "resume that", "resume the task"];

/// Whether `words`, a whole utterance, pause a task (true) or let it go on.
pub(super) fn held_words(words: &str) -> Option<bool> {
    let heard = normal(words);
    let heard = heard.as_str();
    match (PAUSE.contains(&heard), RESUME.contains(&heard)) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

/// What `words`, a whole utterance, asks of a question with `options`.
pub(super) fn matched(words: &str, options: &[AnswerOption]) -> Option<Said> {
    let heard = normal(words);
    let heard = heard.as_str();
    if STOP.contains(&heard) {
        return Some(Said::Stop);
    }
    let pick = |id: &str, ack: &str| Said::Answer {
        option: id.into(),
        ack: ack.into(),
    };
    if APPROVE.contains(&heard) {
        let o = options.iter().find(|o| approves(&o.id))?;
        return Some(pick(&o.id, "Approved."));
    }
    if DENY.contains(&heard) {
        let o = options
            .iter()
            .find(|o| matches!(o.id.as_str(), "decline" | "deny" | "reject"))?;
        return Some(pick(&o.id, "Declined."));
    }
    // An ordinal alone, with or without the words around it.
    let ordinal = |w: &str| ORDINALS.iter().find(|(o, _)| *o == w).map(|&(_, n)| n);
    let mut w: Vec<&str> = heard
        .split(' ')
        .skip_while(|w| BEFORE.contains(w))
        .collect();
    if w.len() == 2 && w[1] == "one" && ordinal(w[0]).is_some() {
        w.pop();
    }
    let [word] = w[..] else {
        return None;
    };
    let n = ordinal(word)?;
    let o = options.get(n - 1)?;
    Some(pick(
        &o.id,
        &format!(
            "Option {}.",
            ["one", "two", "three", "four", "five", "six"][n - 1]
        ),
    ))
}

impl Place {
    /// The call's words, if they answer the question it last asked of the
    /// owner: `work.answer` or `task.cancel` as the speaker, the transcript
    /// posted as any voice turn's is, and a short word back. True when they
    /// did, and no voice turn is made of them.
    /// Whether every utterance of the turn is one owner's.
    fn spoke_alone_as_owner(&self, t: &VoiceTurn) -> bool {
        let Some(speaker) = t.utterances.first().map(|u| u.speaker.0) else {
            return false;
        };
        let core = &self.shared.core;
        t.utterances.iter().all(|u| u.speaker.0 == speaker)
            && core
                .runner
                .place_rule
                .owners(&core.cfg)
                .contains(&format!("discord:{speaker}"))
    }

    pub(super) fn voice_answered(&mut self, t: &VoiceTurn) -> bool {
        let Some(first) = t.utterances.first() else {
            return false;
        };
        let speaker = first.speaker.0;
        let owner = self.spoke_alone_as_owner(t);
        let words = t
            .utterances
            .iter()
            .map(|u| u.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let (tasks, commands, guild, channel) = {
            let call = self.shared.voice.call.lock().unwrap();
            let Some(c) = call
                .as_ref()
                .filter(|c| c.serial == t.serial && c.key == self.key)
            else {
                return false;
            };
            let Some(tasks) = c.status.as_ref().and_then(|s| s.tasks.clone()) else {
                return false;
            };
            (tasks, c.commands.clone(), c.guild, c.channel)
        };
        let now = Instant::now();
        let (taken, held) = {
            let mut tasks = tasks.lock().unwrap();
            match tasks.take(&words, owner, now) {
                Some(t) => (Some(t), None),
                None => (None, tasks.hold(&words, owner, now)),
            }
        };
        if taken.is_none() && held.is_none() {
            return false;
        }
        let name = self.shared.voice.name(speaker);
        for u in &t.utterances {
            self.say(&format!("🎙️ **{name}**: {}", u.text), None);
        }
        let held_by = held;
        let origin = DiscordOrigin {
            user_id: speaker.to_string(),
            channel_id: channel.to_string(),
            guild_id: Some(guild.to_string()),
        };
        let (rpc, shared, sid, target) = (
            self.shared.rpc.clone(),
            Arc::clone(&self.shared),
            self.session_id.clone(),
            self.target.clone(),
        );
        let author = Some(format!("discord:{name}"));
        tokio::spawn(async move {
            let Some((said, pending)) = taken else {
                if let Some((pause, task)) = held_by {
                    let (rpc, shared, sid, target) = (rpc, shared, sid, target);
                    let text =
                        hold(&rpc, pause, task, author, origin, (&shared, &sid, &target)).await;
                    let _ = commands.send(Command::Report { text });
                }
                return;
            };
            let (said, refused) = match said {
                Said::Answer { option, ack } => {
                    let params = WorkAnswerParams {
                        question_id: pending.question,
                        option,
                        author,
                        discord: Some(origin),
                        ..Default::default()
                    };
                    match rpc
                        .call::<_, WorkAnswerResult>(method::WORK_ANSWER, params)
                        .await
                    {
                        Ok(_) => (ack, None),
                        Err(e) => (REFUSED.into(), Some(why(&e))),
                    }
                }
                Said::Stop => {
                    let params = TaskCancelParams {
                        task: pending.task,
                        author,
                    };
                    match rpc
                        .call::<_, TaskCancelResult>(method::TASK_CANCEL, params)
                        .await
                    {
                        Ok(_) => ("Stopped.".into(), None),
                        Err(e) => (REFUSED.into(), Some(why(&e))),
                    }
                }
            };
            if let Some(why) = refused {
                refuse(&shared, &sid, &target, &why);
            }
            let _ = commands.send(Command::Report { text: said });
        });
        true
    }
}

/// `task.pause` or `task.resume` as the speaker; the words to say back. A
/// refusal is told in the place, as an answer's is.
async fn hold(
    rpc: &crate::rpc_client::RpcClient,
    pause: bool,
    task: String,
    author: Option<String>,
    origin: DiscordOrigin,
    (shared, sid, target): (&crate::runtime::Shared, &str, &str),
) -> String {
    let params = TaskPauseParams {
        task,
        author,
        discord: Some(origin),
    };
    let m = if pause {
        method::TASK_PAUSE
    } else {
        method::TASK_RESUME
    };
    match rpc.call::<_, TaskPauseResult>(m, params).await {
        Ok(r) => hold_words(pause, r.changed).to_string(),
        Err(e) => {
            refuse(shared, sid, target, &why(&e));
            REFUSED.to_string()
        }
    }
}

/// What is said back for `task.pause` (`pause`) or `task.resume`.
fn hold_words(pause: bool, changed: bool) -> &'static str {
    match (pause, changed) {
        (true, true) => "Paused.",
        (true, false) => "It was already paused.",
        (false, true) => "Going on.",
        (false, false) => "It was not paused.",
    }
}

/// The owner's DM hears why an answer did not count.
fn refuse(shared: &crate::runtime::Shared, sid: &str, target: &str, why: &str) {
    let body = serde_json::json!({"kind": "notice", "text": format!("🔐 Your answer did not count: {why}")});
    if let Err(e) = shared.core.outbox.post(sid, "", target, body) {
        shared
            .board
            .error("post notice", Some(sid), format!("{e:#}"));
    }
}

/// The core's words for a refusal, else the error's.
fn why(e: &crate::rpc_client::CallError) -> String {
    e.data
        .get("why")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&e.message)
        .to_string()
}
