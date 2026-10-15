//! `/answer` and the answers beyond buttons (theseus-a5uds). A child of
//! `runtime`.
//!
//! - **`/answer [question:]`**, with autocomplete over every open question
//!   the presser may answer (`confirm.list` as the presser: the CLI's and the
//!   web UI's sessions' too), shows the card's controls ephemerally: buttons,
//!   or a string select for more than five answers, and "Decline with a note…".
//! - **The list is kept** per presser and channel for [`TTL`], so a keystroke
//!   asks the daemon nothing: the first autocomplete reads it, and the next
//!   ones within three seconds read memory.
//! - **A modal** takes words (a decline's note, an ask's own answer) and, for
//!   an ask's own answer, one file, read as any file sent here is. Its submit
//!   is `work.answer` as the presser with the words (and the file's text) as
//!   the note; the core judges it, and a refusal is told to the presser alone.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::answer::{ConfirmListParams, WorkAnswerParams, WorkAnswerResult};
use theseus_protocol::work::ask::AskInput;
use theseus_protocol::work::{answer_options, AnswerOption, QuestionKind};
use theseus_protocol::{ConfirmListResult, ConfirmRequest, LedgerKind};
use tokio::time::Instant;
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};
use twilight_model::application::interaction::Interaction;

use super::{origin_of, Shared};
use crate::files::{self, FileMeta, Plan};
use crate::render::answer_v2::{self as av2, FILE_FIELD, NOTE_FIELD, OTHER};
use crate::render::board_v2::{ago, short};
use crate::render::v2::{container, text};
use crate::render::{card_v2, clip};

/// How long a presser's list of questions is read from memory.
pub(super) const TTL: Duration = Duration::from_secs(3);
/// The most of a file's text that goes into a note.
const FILE_CHARS: usize = 8_000;

/// One presser's open questions, and when they were read.
type Read = (Instant, Vec<ConfirmRequest>);

/// A presser's open questions, as last asked.
#[derive(Default)]
pub(crate) struct Questions {
    cache: Mutex<HashMap<(u64, u64), Read>>,
    /// How many times the daemon was asked (a test's seam, and the answer to
    /// "does a keystroke ask the daemon").
    pub(crate) fetches: AtomicU64,
}

/// The options of a request, as a card draws them.
fn options_of(r: &ConfirmRequest) -> Vec<AnswerOption> {
    if !r.options.is_empty() {
        return r.options.clone();
    }
    let kind = if r.budget.is_some() {
        QuestionKind::Budget
    } else if r.tool == theseus_protocol::work::ask::OWNER_ASK {
        QuestionKind::Ask
    } else {
        QuestionKind::Approval
    };
    answer_options(&r.question(kind, true))
}

/// The card's words for a request: who asks and what, an ask's own question
/// with its options numbered, and when.
fn words_of(r: &ConfirmRequest) -> String {
    let who = r
        .task
        .as_ref()
        .and_then(|t| t.title.clone())
        .unwrap_or_else(|| "A conversation".into());
    let mut t = match AskInput::parse(&r.input)
        .ok()
        .filter(|_| r.tool == theseus_protocol::work::ask::OWNER_ASK)
    {
        Some(a) => {
            let mut s = format!(
                "❓ **{}** asks: **{}**",
                clip(&who, 60),
                clip(&a.question, 400)
            );
            for (n, o) in a.options.iter().enumerate() {
                s.push_str(&format!("\n**{}.** {}", n + 1, clip(&o.label, 80)));
            }
            s
        }
        None => format!("🔔 **{}** · {}", clip(&who, 60), clip(&r.reason, 400)),
    };
    t.push_str(&format!(
        "\n-# `…{}` · asked {}",
        short(&r.correlation_id),
        ago(r.requested_at_ms)
    ));
    t
}

/// A choice's name in the autocomplete.
fn name_of(r: &ConfirmRequest) -> String {
    let who = r
        .task
        .as_ref()
        .and_then(|t| t.title.clone())
        .unwrap_or_else(|| "conversation".into());
    let mark = if r.tool == theseus_protocol::work::ask::OWNER_ASK {
        "❓"
    } else {
        "🔴"
    };
    format!(
        "{mark} {} · {} …{}",
        clip(&who, 30),
        clip(&r.reason, 40),
        short(&r.correlation_id)
    )
}

/// The choices for `typed` among the open questions, at most 25.
pub(super) fn choices(open: &[ConfirmRequest], typed: &str) -> Vec<(String, String)> {
    let typed = typed.trim().to_lowercase();
    open.iter()
        .filter(|r| {
            typed.is_empty()
                || name_of(r).to_lowercase().contains(&typed)
                || r.correlation_id.to_lowercase().ends_with(&typed)
        })
        .take(crate::render::snapshot::MAX_CHOICES)
        .map(|r| (name_of(r), r.correlation_id.clone()))
        .collect()
}

/// The card `/answer` shows for one question: its words, its controls, and
/// "Decline with a note…" where the question takes one.
pub(super) fn view_of(r: &ConfirmRequest) -> Vec<Value> {
    let options = options_of(r);
    let mut inner = vec![text(words_of(r))];
    inner.extend(av2::controls(&r.correlation_id, &options));
    inner.extend(av2::note_row(&r.correlation_id, &options));
    vec![container(Some(0xE5_48_4D), inner)]
}

impl Shared {
    /// The open questions the presser may answer, from memory when asked
    /// within [`TTL`], else from `confirm.list` as the presser.
    pub(super) async fn answerable(&self, i: &Interaction) -> Vec<ConfirmRequest> {
        let user = i.author().map(|u| u.id.get()).unwrap_or(0);
        let channel = i.channel.as_ref().map_or(0, |c| c.id.get());
        let key = (user, channel);
        {
            let c = self.cmds.questions.cache.lock().unwrap();
            if let Some((at, list)) = c.get(&key).filter(|(at, _)| at.elapsed() < TTL) {
                let _ = at;
                return list.clone();
            }
        }
        self.cmds.questions.fetches.fetch_add(1, Ordering::Relaxed);
        let discord = origin_of(Some(user), Some(channel), i.guild_id);
        let list = match self
            .rpc
            .call::<_, ConfirmListResult>(
                theseus_protocol::method::CONFIRM_LIST,
                ConfirmListParams { discord },
            )
            .await
        {
            Ok(r) => r
                .confirms
                .into_iter()
                .filter(|c| c.may_answer.unwrap_or(false))
                .collect(),
            Err(e) => {
                self.board.error("confirm.list", None, e);
                Vec::new()
            }
        };
        let mut c = self.cmds.questions.cache.lock().unwrap();
        c.insert(key, (Instant::now(), list.clone()));
        if c.len() > 256 {
            c.retain(|_, (at, _)| at.elapsed() < TTL);
        }
        list
    }

    /// `/answer question:` as it is typed.
    pub(super) async fn answer_choices(&self, i: &Interaction, typed: &str) {
        let open = self.answerable(i).await;
        self.autocomplete(i, choices(&open, typed)).await;
    }

    /// `/answer`: the card for the question named, or the one waiting; a list
    /// to pick from when several wait.
    pub(super) async fn answer_view(&self, i: &Interaction, question: Option<&str>) {
        // A command reads the daemon afresh: what it shows is what it answers.
        self.cmds.questions.cache.lock().unwrap().clear();
        let open = self.answerable(i).await;
        let named = question.map(str::trim).filter(|q| !q.is_empty());
        let pick = match named {
            Some(q) => open
                .iter()
                .find(|r| r.correlation_id == q || r.correlation_id.ends_with(q)),
            None if open.len() == 1 => open.first(),
            None => None,
        };
        match (pick, open.len()) {
            (Some(r), _) => self.respond_v2(i, 4, &view_of(r), true).await,
            (None, 0) => {
                self.tell(
                    i,
                    "Nothing is waiting for you that you can answer here.".into(),
                )
                .await
            }
            (None, n) if named.is_none() => {
                let mut lines = format!("🔴 **{n} questions are waiting for you.** Pick one:\n");
                for r in open.iter().take(10) {
                    lines.push_str(&format!("• {}\n", name_of(r)));
                }
                lines.push_str("\nUse `/answer question:` and choose one as you type.");
                self.tell(i, lines).await;
            }
            (None, _) => {
                self.tell(
                    i,
                    format!(
                        "No question you can answer matches `{}`.",
                        named.unwrap_or_default()
                    ),
                )
                .await
            }
        }
    }

    /// A press that needs words: the modal for `option`.
    pub(super) async fn open_answer_modal(&self, i: &Interaction, question: &str, option: &str) {
        let title = if option == OTHER {
            "Something else"
        } else {
            "Decline with a note"
        };
        let with_file = option == OTHER;
        self.respond_modal(i, &av2::modal(question, option, title, with_file))
            .await;
    }

    /// A submitted answer modal: `work.answer` as the presser with the words,
    /// and the file's text after them.
    pub(super) async fn answer_modal(&self, i: &Interaction, m: &ModalInteractionData, who: &str) {
        let Some((question, option)) = av2::parse_pair(av2::MODAL, &m.custom_id) else {
            return;
        };
        let (mut note, files) = fields_of(m);
        if let Some(file) = files.first() {
            note.push_str(&self.file_note(m, file).await);
        }
        let note = note.trim().to_string();
        let discord = origin_of(
            i.author().map(|u| u.id.get()),
            i.channel.as_ref().map(|c| c.id.get()),
            i.guild_id,
        );
        let r = self
            .rpc
            .call::<_, WorkAnswerResult>(
                theseus_protocol::method::WORK_ANSWER,
                WorkAnswerParams {
                    question_id: question.to_string(),
                    option: option.to_string(),
                    note: (!note.is_empty()).then_some(note),
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                },
            )
            .await;
        self.core.binding_ledger(
            LedgerKind::DiscordConfirm,
            None,
            json!({"correlation_id": question, "approve": theseus_protocol::answer::approves(option),
                   "option": option, "by": who, "ok": r.is_ok(), "modal": true,
                   "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        let line = match r {
            Ok(done) => format!("✅ **Answered** by {who} · `…{}`", short(&done.question_id)),
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => {
                let why = e.data.get("why").and_then(Value::as_str);
                format!(
                    "🔐 Your answer did not count: {}. It keeps waiting for an answer that does.",
                    why.unwrap_or(&e.message)
                )
            }
            Err(e) => format!("⚠️ Could not answer: {e}"),
        };
        // From a button on a message the modal edits that message; from a
        // command it is a line to the presser alone.
        if i.message.is_some() && line.starts_with('✅') {
            self.respond_v2(i, 7, &card_v2::closed(&line), true).await;
        } else {
            self.tell(i, line).await;
        }
    }

    /// The words a modal's file adds to the note: its text (a text file) as
    /// the binding reads any attachment, else what was left unread.
    async fn file_note(&self, m: &ModalInteractionData, id: &str) -> String {
        let Some(att) = m.resolved.as_ref().and_then(|r| {
            r.attachments
                .iter()
                .find(|(k, _)| k.get().to_string() == id)
                .map(|(_, a)| a.clone())
        }) else {
            return String::new();
        };
        let meta = FileMeta::of(&att);
        let plan = files::plan(&meta, self.caps);
        if !matches!(plan, Plan::Text) {
            let why = match plan {
                Plan::Skip(r) => r,
                _ => "it is not a text file, and an answer carries text only".into(),
            };
            return format!("\n\n[file {} not read: {why}]", meta.name);
        }
        let got = files::fetch_all(self.files_http.clone(), vec![meta.clone()], self.caps).await;
        match got.first() {
            Some(a) if a.text.is_some() => format!(
                "\n\n[file {}]\n{}",
                meta.name,
                clip(a.text.as_deref().unwrap_or(""), FILE_CHARS)
            ),
            Some(a) => format!(
                "\n\n[file {} not read: {}]",
                meta.name,
                a.not_read.as_deref().unwrap_or("it did not download")
            ),
            None => String::new(),
        }
    }
}

/// A modal's words (the note input) and the ids of the files it carries.
fn fields_of(m: &ModalInteractionData) -> (String, Vec<String>) {
    fn walk(c: &ModalInteractionComponent, note: &mut String, files: &mut Vec<String>) {
        match c {
            ModalInteractionComponent::Label(l) => walk(&l.component, note, files),
            ModalInteractionComponent::ActionRow(r) => {
                r.components.iter().for_each(|c| walk(c, note, files))
            }
            ModalInteractionComponent::TextInput(t) if t.custom_id == NOTE_FIELD => {
                note.push_str(&t.value)
            }
            ModalInteractionComponent::FileUpload(f) if f.custom_id == FILE_FIELD => {
                files.extend(f.values.iter().map(|v| v.get().to_string()))
            }
            _ => {}
        }
    }
    let (mut note, mut files) = (String::new(), Vec::new());
    m.components
        .iter()
        .for_each(|c| walk(c, &mut note, &mut files));
    (note, files)
}
