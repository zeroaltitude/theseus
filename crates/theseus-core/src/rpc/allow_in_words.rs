//! The owner's override of a held call, said in the conversation it was held
//! in (theseus-p5kln). The chat route, and it is half the card and not a
//! nicety: the owner stated the principle twice in five minutes on the
//! afternoon of 2026-11-12 (S17-D5b, S17-D7) — **minimise the cases where a
//! person must drop to a command line** — so `theseus undo allow <id>` alone
//! would not close it.
//!
//! The shape is `rpc/answer_in_words.rs`'s, clause for clause, because the
//! two routes are the same act in two different vocabularies: the words are
//! read at `turn.submit` **before the turn runs**, and the override goes
//! through [`Core::undo_allow`], the one place a hold's allowance is written,
//! so `judge_act(Act::UndoAllow)` — `places::owner_in_person` — judges a
//! spoken override exactly as it judges a typed one. **Nothing about what an
//! override means is widened here: only which inputs reach that call.**
//!
//! **Which holds are in scope** is the place, not the session, as a yes in
//! words is: a task's own calls are held in its own session and the owner is
//! asked about them where its parent answers.
//!
//! **Which hold is meant is never guessed.** The same sitting that produced
//! this route produced the guard: Theseus abandoned the wrong task on a bare
//! *"Yes, you may abandon it"* while two were waiting. So a spoken override
//! takes a hold only when there is exactly one it can mean; with more than
//! one it allows **nothing** and says so where approvals go, naming each. A
//! bare override phrase with nothing held says nothing at all.
//!
//! **A bare yes is not an override.** `answer_in_words` reads "yes" against
//! the questions waiting here, and a held call is deliberately not one of
//! those — it made no card. So this route has its own words
//! ([`says_allow`]), all of which say *run it as it was*, and a message that
//! merely agrees with something reaches neither route.

use theseus_protocol::work::undo::{allowed_words, pick, HeldCall};
use theseus_protocol::TurnSubmitParams;

use super::server::Conn;
use super::Core;
use crate::approval::Surface;
use crate::owner_note::Kind;
use crate::toolrun::holds::Holds;

/// The longest message read as an override: past this it is a message that
/// happens to mention the original, not an instruction about one.
const MOST_CHARS: usize = 120;

/// The phrases that say "run the call as it was first proposed". Each is the
/// owner's own idiom for it, his recorded sentence included (*"it's OK to run
/// what was originally suggested"*). Matched as a run of words, anywhere in
/// the message, because an override is usually said after naming the call.
const ALLOW: &[&str] = &[
    "run what was originally suggested",
    "run what was original",
    "run the original",
    "run it as it was",
    "run it as proposed",
    "run it as originally proposed",
    "run the call as proposed",
    "as originally suggested",
    "the original was right",
    "the original is right",
    "allow the original",
    "allow the held call",
    "allow the hold",
    "undo allow",
];

/// Words that take an override back. A message holding one is not an
/// override, whatever else it says: the same rule `answer_in_words` applies
/// to a yes, and for the same reason.
const NOT_ALLOW: &[&str] = &[
    "not", "n't", "never", "stop", "wait", "cancel", "decline", "instead", "don", "hold off",
    "rather",
];

/// Whether `text` says to run a held call as it was first proposed.
pub fn says_allow(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() || t.chars().count() > MOST_CHARS {
        return false;
    }
    // "re-plan it instead" and "don't run the original" are refusals.
    if NOT_ALLOW
        .iter()
        .any(|w| super::answer_in_words::has_token(&t, w))
    {
        return false;
    }
    ALLOW.iter().any(|a| t.contains(a))
}

/// What a message says about the calls held in its conversation. Pure, so
/// every case below is a unit test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InWords {
    /// Not an override: nothing happens here.
    NotAnOverride,
    /// An override, and nothing is held for it.
    NoneHeld,
    /// An override, and exactly one hold it can mean, by its index.
    One(usize),
    /// An override, and more than one hold it could mean. Nothing is allowed.
    Which(Vec<usize>),
}

/// Read `text` against the calls held in its conversation.
///
/// An override names a hold by its id, whole or by an end of four characters
/// or more, or by the tool it called; when it names exactly one, that one.
/// When it names none and exactly one call is held, that one. Otherwise
/// [`InWords::Which`], and nothing is allowed.
pub(crate) fn read(text: &str, held: &[HeldCall]) -> InWords {
    if !says_allow(text) {
        return InWords::NotAnOverride;
    }
    if held.is_empty() {
        return InWords::NoneHeld;
    }
    let t = text.to_lowercase();
    let mut named: Vec<usize> = Vec::new();
    let mut note = |i: usize| {
        if !named.contains(&i) {
            named.push(i);
        }
    };
    for (i, h) in held.iter().enumerate() {
        // The tool, as a whole token: `run the original fs.write`.
        if super::answer_in_words::has_token(&t, &h.tool.to_lowercase()) {
            note(i);
        }
    }
    // The id, whole or by an end of four characters or more, by exactly the
    // rule `undo.allow` takes it by (`work::undo::pick`), so the two
    // vocabularies cannot drift: a word that names two holds names both, and
    // the caller refuses.
    for word in t.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        for found in pick(held, word) {
            if let Some(i) = held.iter().position(|h| h.id == found.id) {
                note(i);
            }
        }
    }
    named.sort_unstable();
    match (named.len(), held.len()) {
        (1, _) => InWords::One(named[0]),
        (0, 1) => InWords::One(0),
        (0, _) => InWords::Which((0..held.len()).collect()),
        _ => InWords::Which(named),
    }
}

/// One held call, as a note names it: by something the owner can say back.
fn named(h: &HeldCall) -> String {
    format!("`{}` — {} ({})", h.tool, h.summary, h.id)
}

impl Core {
    /// The owner's override, said in the conversation a call was held in: the
    /// allowance is written before the turn runs, as his act from where he
    /// said it. A message from a job, a prompt, an MCP client or a shared
    /// place is no override. A refusal leaves the turn to run as an ordinary
    /// message: `judge_act` has already said why and ledgered it.
    pub(super) fn allow_in_words(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &str) {
        if p.opened_from.is_some()
            || p.prompt.is_some()
            || !p.attachments.is_empty()
            || matches!(conn.surface, Surface::Mcp | Surface::Unnamed)
            || !says_allow(&p.input)
            || !self.owner_alone_in(session)
        {
            return;
        }
        let held = match self.held_here(session) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(session = %session, error = %format!("{e:#}"),
                    "the owner's override: what is held here could not be read");
                return;
            }
        };
        match read(&p.input, &held) {
            InWords::NotAnOverride => {}
            // An override with nothing held is said where approvals go: the
            // owner meant something by it, so silence would read as a yes.
            InWords::NoneHeld => self.say_of_override(
                session,
                Kind::Fyi,
                "Nothing is held here",
                "your words would let a held call run, and no call is held in this conversation",
                "Nothing ran and nothing is allowed. `theseus undo list` says what is held.",
            ),
            InWords::Which(which) => {
                let names: Vec<String> = which
                    .iter()
                    .filter_map(|i| held.get(*i))
                    .map(named)
                    .collect();
                self.say_of_override(
                    session,
                    Kind::Decision,
                    "Which held call do you mean?",
                    &format!(
                        "your words could mean any of {} calls held here",
                        names.len()
                    ),
                    &format!(
                        "Nothing has been allowed. Name the one you mean and I will let it run \
                         as it was first proposed:\n{}",
                        names
                            .iter()
                            .map(|n| format!("- {n}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                );
            }
            InWords::One(i) => {
                let Some(h) = held.get(i) else { return };
                self.override_taken(p, conn, session, h);
            }
        }
    }

    /// The one hold an override can mean, taken through [`Core::undo_allow`],
    /// the one place a hold's allowance is written — so the place rule and
    /// the peer rule judge a spoken override exactly as a typed one.
    fn override_taken(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &str, h: &HeldCall) {
        let who = conn.answerer(
            p.author.clone(),
            p.speaking.as_ref().map(|s| s.discord.clone()),
        );
        match self.undo_allow(&h.id, who) {
            Ok(allowed) => self.say_of_override(
                session,
                Kind::Fyi,
                "The original call is allowed",
                "you said the original was right",
                &allowed_words(&allowed),
            ),
            Err(e) => tracing::warn!(session = %session, hold = %h.id,
                error = %format!("{e:#}"), "the owner's override in words was not taken"),
        }
    }

    /// Every call held that the owner is being offered here: this session's,
    /// and those of a session that answers at this one's place (a task's own
    /// calls are held in its own session). Waiting for an allowance only: a
    /// hold he has already allowed is not offered again.
    fn held_here(&self, session: &str) -> anyhow::Result<Vec<HeldCall>> {
        let here = self.outbox.target(session);
        Ok(Holds::read(&self.store)?
            .list(None, false)
            .into_iter()
            .filter(|h| h.allowed.is_none())
            .filter(|h| {
                h.session_id == session
                    || (here.is_some() && self.outbox.target(&h.session_id) == here)
            })
            .collect())
    }

    /// What an override did or could not do, where approvals go. One literal
    /// kind, so the census of operator call sites reads it from the source.
    fn say_of_override(&self, session: &str, kind: Kind, title: &str, why: &str, body: &str) {
        let text = crate::owner_note::note(kind, title, why, body);
        if let Err(e) = self.outbox.to_operator(
            Some(session),
            serde_json::json!({"kind": "notice", "text": text}),
        ) {
            tracing::warn!(session = %session, error = %format!("{e:#}"),
                "the notice about the owner's override was not posted");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn held(id: &str, tool: &str) -> HeldCall {
        HeldCall {
            id: id.into(),
            session_id: "ses_1".into(),
            turn_id: None,
            tool: tool.into(),
            input: json!({"path": "/w/notes.txt"}),
            summary: format!("{tool} notes.txt"),
            judgment: "jdg_1".into(),
            shape: Some("move_aside".into()),
            held_ms: 1,
            allowed: None,
            ran_ms: None,
        }
    }

    #[test]
    fn the_owners_own_sentence_is_an_override_and_a_refusal_is_not() {
        for yes in [
            "it's OK to run what was originally suggested",
            "Run the original.",
            "run it as proposed",
            "the original was right",
            "allow the held call",
            "undo allow hld_1a2b3c4d",
        ] {
            assert!(says_allow(yes), "{yes}");
        }
        for no in [
            "yes",
            "ok",
            "don't run the original",
            "do not run the original",
            "re-plan it instead",
            "never run the original",
            "run the original later, but wait",
            "",
            // Long enough to be a message about the original, not an order.
            "I was thinking about the original approach we discussed and whether we should \
             run the original or reshape it, which depends on the backup situation",
        ] {
            assert!(!says_allow(no), "{no}");
        }
    }

    #[test]
    fn an_override_takes_one_hold_and_never_guesses_between_two() {
        let one = [held("hld_1a2b3c4d", "fs.write")];
        assert_eq!(read("run the original", &one), InWords::One(0));
        assert_eq!(read("yes", &one), InWords::NotAnOverride);
        assert_eq!(read("run the original", &[]), InWords::NoneHeld);

        let two = [
            held("hld_1a2b3c4d", "fs.write"),
            held("hld_5e6f7a8b", "fs.patch"),
        ];
        assert_eq!(
            read("run the original", &two),
            InWords::Which(vec![0, 1]),
            "two held calls and nothing to tell them apart: nothing is allowed"
        );
        assert_eq!(
            read("run the original, hld_5e6f7a8b", &two),
            InWords::One(1),
            "named by its id"
        );
        assert_eq!(
            read("run the original 3c4d", &two),
            InWords::One(0),
            "named by an end of four"
        );
        assert_eq!(
            read("run the original fs.patch", &two),
            InWords::One(1),
            "named by its tool"
        );
        // Two holds of ONE tool, named by that tool: ambiguous, not guessed.
        let twins = [
            held("hld_aaaa1111", "fs.write"),
            held("hld_bbbb2222", "fs.write"),
        ];
        assert_eq!(
            read("run the original fs.write", &twins),
            InWords::Which(vec![0, 1])
        );
    }
}
