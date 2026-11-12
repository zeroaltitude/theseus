//! The owner's yes, said in words (theseus-3i6d1; the owner, 2026-11-07
//! 10:24: *"I had just approved it verbally… this shouldn't have thrown up a
//! response dialog, but just notified me, further reinforcing that notify >>
//! approve"*).
//!
//! A question waiting for the owner is answered by the card's button, by
//! `work.answer`, or — since this — by the owner saying yes in the
//! conversation the question was asked in. The words are read before the turn
//! runs, as `seal_by_word` and `moved_by_words` are, and the answer goes
//! through [`Core::confirm_action`], the one place an answer becomes a
//! decision, so the place rule, the peer rule and `approval.refused` judge a
//! yes in words exactly as they judge a typed one. Nothing about what an
//! answer *means* is widened: only which inputs reach that call.
//!
//! **Which questions are in scope** is the place, not the session: the cards
//! all go where approvals go, and a task's own session asks there too (a
//! conversation holds at most one waiting question of its own, because the
//! question holds its turn). So a yes reads every question waiting in this
//! session or in a session that answers at this one's place, and a question
//! of another place — another DM, a channel — is not his to answer from
//! here.
//!
//! **Where the answer is said.** A yes Theseus cannot place is said where
//! approvals go, as a notice: a question that waits is a question a turn is
//! still running for, and a node written into a session mid-turn is refused
//! (`write_harness_note`), so the operator's own surface is the only one that
//! can be told while the question waits. A **bare** yes that finds nothing
//! waiting says nothing at all: the owner says "yes" in conversation often,
//! and a line in his DM for each of them is the opposite of what this row is
//! for. A yes that *names* a question (`tsk_…`, `act_…`) is addressed at one,
//! so when that one is gone he is told it is gone.
//!
//! **Which question a yes means is never guessed.** The same sitting recorded
//! Theseus abandoning the wrong task on a bare *"Yes, you may abandon it"*
//! while two were waiting. So a yes resolves a question only when there is
//! exactly one it can mean; with more than one it resolves nothing and the
//! conversation is told to ask which, naming them.
//!
//! The surfaces are `capture::owner_alone`'s — the CLI, the web UI and a DM
//! with the owner — and **not** a guild channel bound private, which
//! `places::owner_in_private` counts for the typed path. The difference is
//! the difference between an addressed answer and an overheard one: a typed
//! answer names its question, loose words do not, and in a room other people
//! write in a stranger's "yes" must never become the owner's.
//!
//! **And the turn is told what the yes did** (theseus-2yowd, row B6). The
//! notice above goes where approvals go; the turn the yes starts used to run
//! with the word in its input and nothing in it saying what the harness made
//! of it, which is how a bare "Yes, you may abandon it" could still be acted
//! on. Every outcome here now also stages one harness fact for that turn
//! (`crate::yes_fact`), naming what was approved and what was not, behind
//! `[standing] yes_fact`, which is off by default.

use serde_json::json;
use theseus_protocol::{ConfirmRequest, TurnSubmitParams};

use super::server::Conn;
use super::Core;
use crate::approval::Surface;
use crate::owner_note::Kind;
use crate::yes_fact;

/// The longest message read as a bare answer: past this it is a message that
/// happens to start with "yes", not an answer to a question.
const MOST_CHARS: usize = 120;

/// The shortest end of an id that names a question, as `work.answer`'s
/// `question_in` takes (`rpc/answer.rs`).
const SHORTEST_ID: usize = 4;

/// The note on an answer given in words, as the ledger and the card's settle
/// record it.
pub(crate) const SAID: &str = "said in words";

/// The id prefixes Theseus mints. A message holding one names a question,
/// so a yes carrying one is addressed at something rather than conversational.
const ID_PREFIXES: &[&str] = &["tsk_", "act_", "ses_", "exe_"];

/// The openings that are a yes, at a word boundary.
const YES: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "ok",
    "okay",
    "sure",
    "approved",
    "approve",
    "confirmed",
    "affirmative",
    "go ahead",
    "do it",
    "please do",
];

/// Words that take a yes back. A message holding one of these is not a bare
/// answer, whatever it opens with.
const NOT_YES: &[&str] = &[
    "no", "not", "n't", "never", "stop", "wait", "cancel", "decline", "instead", "don", "hold",
];

/// Whether `needle` (lower case) stands in `hay` (lower case) as a whole
/// token: what is around it is not a word character.
fn has_token(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let at = from + i;
        let before = hay[..at].chars().next_back();
        let after = hay[at + needle.len()..].chars().next();
        if !word(before) && !word(after) {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// Whether `text` is a bare yes: short, opening with an affirmative, and
/// taking nothing back.
pub fn says_yes(text: &str) -> bool {
    let t = text
        .trim()
        .trim_end_matches(['.', '!'])
        .trim()
        .to_lowercase();
    if t.is_empty() || t.chars().count() > MOST_CHARS {
        return false;
    }
    if NOT_YES.iter().any(|w| has_token(&t, w)) {
        return false;
    }
    YES.iter().any(|y| {
        t.strip_prefix(y)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
    })
}

/// The last `n` characters of `s`, lower case.
fn tail(s: &str, n: usize) -> String {
    let s = s.to_lowercase();
    let cut = s.char_indices().count().saturating_sub(n);
    s.chars().skip(cut).collect()
}

/// What a question is named by, besides a bare yes: its id (whole, or its
/// last 8 characters), its tool, the task whose session asks, and — for a
/// layer-1 task change — the task it changes, by id and by title.
fn keys_of(q: &ConfirmRequest) -> Vec<String> {
    let mut keys = vec![q.correlation_id.to_lowercase(), q.tool.to_lowercase()];
    if q.correlation_id.chars().count() > SHORTEST_ID {
        keys.push(tail(&q.correlation_id, 8));
    }
    if let Some(t) = &q.task {
        keys.push(t.task_id.to_lowercase());
        if t.short.chars().count() >= SHORTEST_ID {
            keys.push(t.short.to_lowercase());
        }
    }
    if let Some(ch) = &q.change {
        keys.push(ch.task.to_lowercase());
        if ch.task.chars().count() > SHORTEST_ID {
            keys.push(tail(&ch.task, 8));
        }
        if ch.title.trim().chars().count() >= SHORTEST_ID {
            keys.push(ch.title.trim().to_lowercase());
        }
    }
    keys
}

/// Whether `text` names a question, by holding one of the id prefixes
/// Theseus mints.
fn names_a_question(text: &str) -> bool {
    let t = text.to_lowercase();
    ID_PREFIXES.iter().any(|p| t.contains(p))
}

/// Whether a question has options rather than a yes (`ask::OWNER_ASK`): a
/// plain yes chooses nothing, and `confirm_action` refuses one.
fn needs_an_option(q: &ConfirmRequest) -> bool {
    q.tool == crate::ask::OWNER_ASK
}

/// What a message says about the questions waiting in its conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InWords {
    /// Not a bare yes: nothing happens here.
    NotAnAnswer,
    /// A yes, and nothing is waiting for it.
    NoneWaiting,
    /// A yes, and exactly one question it can mean, by its index in
    /// `waiting`.
    One(usize),
    /// A yes, and more than one question it could mean: their indices.
    /// Nothing is answered.
    Which(Vec<usize>),
}

/// Read `text` against the questions waiting in its conversation. Pure: no
/// store, no clock, so every case below is a unit test.
///
/// A yes names a question by holding its id, its task or its tool; when it
/// names exactly one, that one. When it names none and exactly one question
/// waits, that one — a single open ask takes a bare yes. Otherwise
/// [`InWords::Which`], and nothing is answered.
pub(crate) fn read(text: &str, waiting: &[ConfirmRequest]) -> InWords {
    if !says_yes(text) {
        return InWords::NotAnAnswer;
    }
    if waiting.is_empty() {
        return InWords::NoneWaiting;
    }
    let t = text.to_lowercase();
    let named: Vec<usize> = waiting
        .iter()
        .enumerate()
        .filter(|(_, q)| keys_of(q).iter().any(|k| has_token(&t, k)))
        .map(|(i, _)| i)
        .collect();
    match (named.len(), waiting.len()) {
        (1, _) => InWords::One(named[0]),
        (0, 1) => InWords::One(0),
        (0, _) => InWords::Which((0..waiting.len()).collect()),
        _ => InWords::Which(named),
    }
}

/// A yes the harness could not spend, and why: the one input both the
/// notice where approvals go and the starting turn's fact are worded from, so
/// the two can never say different things (theseus-2yowd).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Unplaced {
    /// The yes names a question that has already closed.
    Gone,
    /// More than one question here could be the one it meant: each, [`named`].
    Which(Vec<String>),
    /// The one question here has answers to choose from, which a bare yes
    /// does not pick.
    Choice(String),
}

/// The notice (its kind, title, why and body) and the starting turn's fact,
/// for one unplaced yes. Pure: its table is a unit test.
pub(crate) fn words(u: &Unplaced) -> (Kind, &'static str, String, String, yes_fact::Decided) {
    match u {
        Unplaced::Gone => (
            Kind::Fyi,
            "That question is no longer waiting",
            "your yes names a question that has already closed".into(),
            "It was answered, withdrawn or expired before your yes arrived, so nothing \
             ran. `theseus confirm` lists what is waiting now."
                .into(),
            yes_fact::Decided {
                why: "and the question it names had already closed".into(),
                approved: None,
                waiting: vec![],
                next: "Nothing ran and nothing is approved. Say that the question had \
                       already closed, and do not do its work by hand."
                    .into(),
            },
        ),
        Unplaced::Which(names) => (
            Kind::Decision,
            "Which one do you mean?",
            format!(
                "your yes could mean any of {} questions waiting here",
                names.len()
            ),
            format!(
                "Nothing has been approved. Name the one you mean and I will take it:\n{}",
                names
                    .iter()
                    .map(|n| format!("- {n}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            yes_fact::Decided {
                why: "and more than one question waiting here could be the one it meant, so \
                      the harness placed it at none of them"
                    .into(),
                approved: None,
                waiting: names.clone(),
                next: "Do not act as though any of them were approved, and do not do their \
                       work by hand. Ask the owner which one they mean."
                    .into(),
            },
        ),
        Unplaced::Choice(name) => (
            Kind::Decision,
            "That question has answers to choose from",
            "your yes meets a question with answers to choose from".into(),
            format!("Nothing has been approved. Which answer do you mean?\n- {name}"),
            yes_fact::Decided {
                why: "and the one question waiting here has answers to choose from, which a \
                      bare yes does not pick"
                    .into(),
                approved: None,
                waiting: vec![name.clone()],
                next: "Nothing has been approved. Ask the owner which of its answers they \
                       mean."
                    .into(),
            },
        ),
    }
}

/// One waiting question, as the note names it: by something the owner can
/// say back, never by the correlation id alone.
fn named(q: &ConfirmRequest) -> String {
    let id = tail(&q.correlation_id, 8);
    match (&q.change, &q.task) {
        (Some(ch), _) => format!("`{}` — {} «{}» ({})", q.tool, ch.field, ch.title, ch.task),
        (None, Some(t)) => format!("`{}` for task «{}» (…{id})", q.tool, t.short),
        (None, None) => format!("`{}` (…{id})", q.tool),
    }
}

impl Core {
    /// The owner's yes, said in the conversation a question waits in: the
    /// question is answered before the turn runs, as the owner's act from
    /// where they said it. A message from a job, a prompt, an MCP client or a
    /// shared place is no answer. A refusal leaves the turn to run as an
    /// ordinary message: `judge_act` has already said why and ledgered it.
    ///
    /// A yes that cannot be placed answers nothing and says so, naming what
    /// waits, so an ambiguous yes is never spent on the newest question.
    pub(super) fn answer_in_words(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &str) {
        if p.opened_from.is_some()
            || p.prompt.is_some()
            || !p.attachments.is_empty()
            || matches!(conn.surface, Surface::Mcp | Surface::Unnamed)
            || !says_yes(&p.input)
            || !self.owner_alone_in(session)
        {
            return;
        }
        let waiting = match self.waiting_here(session) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(session = %session, error = %format!("{e:#}"),
                    "the owner's yes: what waits here could not be read");
                return;
            }
        };
        match read(&p.input, &waiting) {
            InWords::NotAnAnswer => {}
            // A bare yes with nothing waiting is conversation, and says
            // nothing; one that names a question is addressed at it, and is
            // told that it is gone.
            InWords::NoneWaiting => {
                if names_a_question(&p.input) {
                    self.yes_unplaced(session, &Unplaced::Gone);
                }
            }
            InWords::Which(which) => {
                let names = which.iter().filter_map(|i| waiting.get(*i)).map(named);
                self.yes_unplaced(session, &Unplaced::Which(names.collect()));
            }
            InWords::One(i) => {
                let Some(q) = waiting.get(i) else { return };
                if needs_an_option(q) {
                    self.yes_unplaced(session, &Unplaced::Choice(named(q)));
                } else {
                    self.yes_taken(p, conn, session, q);
                }
            }
        }
    }

    /// The one question a yes can mean, taken through `confirm_action`, the
    /// one place an answer becomes a decision; then the starting turn is told
    /// that it was approved and that nothing else was, so the turn neither
    /// asks for it again nor does its work by hand.
    fn yes_taken(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &str, q: &ConfirmRequest) {
        let who = conn.answerer(
            p.author.clone(),
            p.speaking.as_ref().map(|s| s.discord.clone()),
        );
        if let Err(e) = self.confirm_action(&q.correlation_id, true, Some(SAID), who) {
            tracing::warn!(session = %session, question = %q.correlation_id,
                error = %format!("{e:#}"), "the owner's yes in words was not taken");
            return;
        }
        self.stage_yes_fact(
            session,
            &yes_fact::Decided {
                why: "and exactly one question waiting here could be the one it meant".into(),
                approved: Some(named(q)),
                waiting: vec![],
                next: "It is approved and runs on its own: do not ask for it again, and do \
                       not do its work by hand."
                    .into(),
            },
        );
    }

    /// A yes the harness could not spend: the notice where approvals go, and
    /// the same outcome staged for the turn that yes starts.
    fn yes_unplaced(&self, session: &str, u: &Unplaced) {
        let (kind, title, why, body, fact) = words(u);
        self.say_of_words(session, kind, title, &why, &body);
        self.stage_yes_fact(session, &fact);
    }

    /// Stage the fact for the turn about to start, when `[standing] yes_fact`
    /// is on. Off (the default) nothing is staged and nothing is rendered:
    /// the notice is all that is said, as before theseus-2yowd.
    fn stage_yes_fact(&self, session: &str, d: &yes_fact::Decided) {
        if self.cfg.standing.yes_fact {
            self.runner.yes_facts.stage(session, yes_fact::render(d));
        }
    }

    /// Every question waiting that the owner is being asked here: this
    /// session's, and those of a session that answers at this one's place
    /// (a task's own asks where its parent does). A place of its own and
    /// nothing else: a yes said in one conversation never answers another's.
    fn waiting_here(&self, session: &str) -> anyhow::Result<Vec<ConfirmRequest>> {
        let here = self.outbox.target(session);
        Ok(self
            .confirm_list()?
            .into_iter()
            .filter(|q| {
                q.session_id == session
                    || (here.is_some() && self.outbox.target(&q.session_id) == here)
            })
            .collect())
    }

    /// What a yes could not do, where approvals go. One literal kind, so the
    /// census of operator call sites reads it from the source.
    fn say_of_words(&self, session: &str, kind: Kind, title: &str, why: &str, body: &str) {
        let text = crate::owner_note::note(kind, title, why, body);
        if let Err(e) = self
            .outbox
            .to_operator(Some(session), json!({"kind": "notice", "text": text}))
        {
            tracing::warn!(session = %session, error = %format!("{e:#}"),
                "the notice about the owner's yes was not posted");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::TaskRef;

    fn q(id: &str, tool: &str) -> ConfirmRequest {
        ConfirmRequest {
            correlation_id: id.into(),
            session_id: "ses_1".into(),
            execution_id: "exe_1".into(),
            tool: tool.into(),
            reason: "because".into(),
            by: "test".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_bare_yes_is_read_and_a_yes_that_takes_itself_back_is_not() {
        for yes in [
            "yes",
            "Yes.",
            "yes!",
            "Yes, you may abandon it",
            "yep",
            "ok",
            "Okay, go ahead",
            "go ahead",
            "do it",
            "sure",
            "approved",
        ] {
            assert!(says_yes(yes), "{yes:?} is a yes");
        }
        for not in [
            "no",
            "yes, but not that one",
            "yes — wait",
            "yes, cancel the other",
            "yesterday I asked you to stop",
            "yessir, run the whole thing and then tell me what the second one did and why it \
             took so long, because I was not watching it at the time and I want the detail",
            "",
            "what did you decide?",
            "ok, don't",
        ] {
            assert!(!says_yes(not), "{not:?} is not a bare yes");
        }
    }

    #[test]
    fn one_waiting_question_takes_a_bare_yes_and_two_take_none() {
        let one = vec![q("call_aaaa1111", "task.close")];
        assert_eq!(read("yes", &one), InWords::One(0));
        assert_eq!(read("Yes, you may abandon it", &one), InWords::One(0));
        // The sitting's case: two waiting, a bare yes names neither.
        let two = vec![
            q("call_aaaa1111", "task.close"),
            q("call_bbbb2222", "task.close"),
        ];
        assert_eq!(
            read("Yes, you may abandon it", &two),
            InWords::Which(vec![0, 1])
        );
        assert_eq!(read("yes", &two), InWords::Which(vec![0, 1]));
    }

    #[test]
    fn a_yes_that_names_one_of_several_takes_that_one() {
        let two = vec![
            q("call_aaaa1111", "task.close"),
            q("call_bbbb2222", "proc.run"),
        ];
        assert_eq!(read("yes, bbbb2222", &two), InWords::One(1));
        assert_eq!(read("yes, the proc.run one", &two), InWords::One(1));
        // Naming them both is still ambiguous.
        assert_eq!(
            read("yes to aaaa1111 and bbbb2222", &two),
            InWords::Which(vec![0, 1])
        );
    }

    #[test]
    fn a_task_is_named_by_its_short_id() {
        let mut a = q("call_aaaa1111", "task.close");
        a.task = Some(TaskRef {
            task_id: "tsk_ea847b99".into(),
            short: "ea847b".into(),
            title: Some("the census".into()),
        });
        let b = q("call_bbbb2222", "task.close");
        let two = vec![a, b];
        assert_eq!(read("yes, abandon ea847b", &two), InWords::One(0));
    }

    #[test]
    fn nothing_waiting_and_not_an_answer_are_told_apart() {
        assert_eq!(read("yes", &[]), InWords::NoneWaiting);
        assert_eq!(read("what is waiting?", &[]), InWords::NotAnAnswer);
        let one = vec![q("call_aaaa1111", "task.close")];
        assert_eq!(read("what is waiting?", &one), InWords::NotAnAnswer);
    }
}
