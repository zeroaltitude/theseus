//! What the harness did with the owner's yes, told to the turn that yes
//! starts (theseus-2yowd, row B6; the residue theseus-3i6d1 left open).
//!
//! Its producer changed with chat-acts-are-tools (S43-1). It used to be
//! `rpc/answer_in_words.rs`, which read a yes in the owner's words before the
//! turn ran and could not guess which question an ambiguous one meant; there
//! is no such matcher now. It is `chat.answer_question`
//! (`rpc/chat_answer.rs::stage_answer_fact`): the model answers the one
//! question the owner's reply answers, and the block tells the rest of that
//! turn what was answered and — the half that matters — what was NOT, so the
//! turn cannot read a silence as either.
//!
//! **Why a staged block and not a node.** The obvious carrier is a harness
//! note in the session (`rpc/thread_seal.rs`'s `write_harness_note`), and it
//! is refused here twice over: it bails while a turn runs in the session, and
//! a stored node would be in every later request too, where this fact is
//! stale the moment the owner answers. So the fact is staged in memory at
//! `turn.submit` and claimed by the first turn that compiles in that session
//! ([`Staged::text_for`]): every loop of that turn renders the same bytes, and
//! the next turn clears it and renders nothing.
//!
//! **Where in the request.** The request's tail, beside the task view, the
//! commitments and the day's observations
//! (`task_graph::view::attach_text`), never inside the three cached system
//! layers: nothing per request goes in those, so no prefix byte moves and
//! this is never a cache write. The head says the harness wrote it.
//!
//! **Off by default.** `[standing] yes_fact` is false: this is a new way for
//! the harness to be told into a turn, and a channel that did not exist
//! before lands switched off until the golden system blocks have been
//! re-walked with it on.

use std::collections::HashMap;
use std::sync::Mutex;

/// How the block begins, as the commitments' and the day's observations' do
/// (`card::noted::HEAD`): a reader of a request can tell it from the
/// conversation's own words.
pub const HEAD: &str = "[What the harness did with the owner's last message";

/// The most questions it names, and its bound in tokens (estimated as the
/// task view's are, `task_graph::view::BYTES_PER_TOKEN` to a token). A yes
/// with more waiting than this says how many it left out.
pub const MAX_LINES: usize = 6;
pub const MAX_TOKENS: u64 = 400;

fn tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN)
}

/// What the harness made of the owner's yes: the clause that says why, what
/// was approved, what was not and still waits, and what this turn should do
/// about it. `rpc/chat_answer.rs` fills one in, so the
/// words the notice uses and the words the turn reads come from one place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Decided {
    /// Why it came out this way, in one clause, after "the harness read it as
    /// a yes": "and more than one question was waiting here".
    pub why: String,
    /// What the yes approved, named as [`named`] names it; `None` when it
    /// approved nothing.
    pub approved: Option<String>,
    /// What it did **not** approve and that is still waiting.
    pub waiting: Vec<String>,
    /// What this turn is to do about it, in plain words.
    pub next: String,
}

/// The block's text. Always says both halves — what was approved and what was
/// not — so the model cannot read a silence as either.
pub(crate) fn render(d: &Decided) -> String {
    let mut text = format!(
        "{HEAD}: the owner's words were read as a yes, {}. Approved: {}.",
        d.why.trim_end_matches(['.', ' ']),
        d.approved.as_deref().unwrap_or("nothing")
    );
    if d.waiting.is_empty() {
        text.push_str(" Nothing else was waiting.");
    } else {
        text.push_str(&format!(
            " NOT approved, and still waiting ({}):",
            d.waiting.len()
        ));
        let mut shown = 0;
        for w in &d.waiting {
            let line = format!("\n- {}", one_line(w));
            if shown >= MAX_LINES || tokens(&text) + tokens(&line) + tokens(&d.next) > MAX_TOKENS {
                break;
            }
            text.push_str(&line);
            shown += 1;
        }
        if shown < d.waiting.len() {
            text.push_str(&format!("\n- and {} more", d.waiting.len() - shown));
        }
    }
    // What to do goes on a line of its own after a list, so it cannot read as
    // another of the questions.
    text.push(if d.waiting.is_empty() { ' ' } else { '\n' });
    text.push_str(d.next.trim());
    text.push(']');
    text
}

/// A text on one line: a question's name is a line of the block, so a newline
/// in it could pass for another.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One staged fact, and the turn that claimed it.
#[derive(Debug, Clone)]
struct Pending {
    text: String,
    turn: Option<String>,
}

/// The facts staged at `turn.submit` and waiting for the turn they belong to,
/// one per session. In memory only: the fact is true of exactly one turn, and
/// a restart has no turn to tell.
#[derive(Default)]
pub struct Staged {
    by_session: Mutex<HashMap<String, Pending>>,
}

impl Staged {
    /// Stage `text` for the next turn in `session`. An unclaimed fact is
    /// replaced: the newer message is the one the turn runs on.
    pub(crate) fn stage(&self, session: &str, text: String) {
        self.by_session
            .lock()
            .unwrap()
            .insert(session.to_string(), Pending { text, turn: None });
    }

    /// The fact for `turn`, if it is the turn the staged fact belongs to. The
    /// first turn to ask claims it, so every loop of that turn renders the
    /// same bytes; any later turn clears it and is given nothing, which is
    /// what keeps the fact out of the next request.
    pub(crate) fn text_for(&self, session: &str, turn: &str) -> Option<String> {
        let mut held = self.by_session.lock().unwrap();
        let p = held.get_mut(session)?;
        match p.turn.as_deref() {
            None => {
                p.turn = Some(turn.to_string());
                Some(p.text.clone())
            }
            Some(t) if t == turn => Some(p.text.clone()),
            Some(_) => {
                held.remove(session);
                None
            }
        }
    }

    /// How many sessions hold a staged fact (the tests').
    #[cfg(test)]
    fn len(&self) -> usize {
        self.by_session.lock().unwrap().len()
    }
}

/// The turn's side (`compile_step::tail_blocks`): the fact staged for this
/// turn, attached to the compiled request after the cached prefix, its tokens
/// counted in the estimate. Returns the text it attached; `None`, and the
/// request untouched, where the owner is not alone, for a model of restricted
/// origin, or when no fact was staged for this turn.
pub(crate) fn attach(
    staged: &Staged,
    session: &str,
    turn: &str,
    class: theseus_protocol::PlaceClass,
    place: Option<&str>,
    restricted: bool,
    compiled: &mut crate::compiler::Compiled,
) -> Option<String> {
    if restricted || !crate::capture::owner_alone(class, place) {
        return None;
    }
    let text = staged.text_for(session, turn)?;
    crate::task_graph::view::attach_text(&mut compiled.request, &text);
    let t = tokens(&text);
    compiled.est_tokens += t;
    compiled.estimate.tokens += t;
    compiled.estimate.estimated += t;
    compiled.estimate.upper += t + t * crate::compiler::MARGIN_PERCENT / 100;
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decided(waiting: &[&str]) -> Decided {
        Decided {
            why: "and more than one question was waiting here".into(),
            approved: None,
            waiting: waiting.iter().map(|w| (*w).to_string()).collect(),
            next: "Do not act as though any of them were approved. Ask which one the owner \
                   means."
                .into(),
        }
    }

    /// The block says both halves, names what waits, and ends with what the
    /// turn is to do. A yes that approved nothing says so in those words, so
    /// a silence cannot read as an approval.
    #[test]
    fn the_block_says_what_was_approved_and_what_was_not() {
        let text = render(&decided(&[
            "`task.close` «the outer harbour»",
            "`proc.run` (…aa11)",
        ]));
        assert!(text.starts_with(HEAD), "{text}");
        assert!(text.contains("Approved: nothing."), "{text}");
        assert!(
            text.contains("NOT approved, and still waiting (2):"),
            "{text}"
        );
        assert!(
            text.contains("- `task.close` «the outer harbour»"),
            "{text}"
        );
        assert!(text.contains("- `proc.run` (…aa11)"), "{text}");
        assert!(text.contains("Ask which one the owner means."), "{text}");
        assert!(text.ends_with(']'), "{text}");
    }

    /// A yes that did approve one names it, and says nothing else waited, so
    /// the model does not ask for the approved call a second time.
    #[test]
    fn a_yes_that_approved_one_names_it_and_says_nothing_else_waited() {
        let d = Decided {
            why: "and exactly one question was waiting here".into(),
            approved: Some("`task.close` for task «the outer harbour»".into()),
            waiting: vec![],
            next: "It is approved and runs on its own; do not ask for it again.".into(),
        };
        let text = render(&d);
        assert!(
            text.contains("Approved: `task.close` for task «the outer harbour»."),
            "{text}"
        );
        assert!(text.contains("Nothing else was waiting."), "{text}");
        assert!(!text.contains("NOT approved"), "{text}");
    }

    /// Past [`MAX_LINES`] the block says how many it left out rather than
    /// growing without bound, and a newline inside a name is folded.
    #[test]
    fn a_long_list_is_cut_and_says_how_many_it_left_out() {
        let names: Vec<String> = (0..MAX_LINES + 3)
            .map(|i| format!("`q{i}`\nmore"))
            .collect();
        let d = Decided {
            waiting: names,
            ..decided(&[])
        };
        let text = render(&d);
        assert!(text.contains("still waiting (9):"), "{text}");
        assert_eq!(text.matches("\n- `q").count(), MAX_LINES, "{text}");
        assert!(text.contains("\n- and 3 more"), "{text}");
        assert!(
            !text.contains("`q0`\nmore"),
            "a name kept its newline: {text}"
        );
        assert!(text.contains("`q0` more"), "{text}");
    }

    /// One turn claims the staged fact and reads the same bytes at every
    /// loop; the next turn clears it and reads nothing. This is what "for
    /// that turn, not the next one" means in code.
    #[test]
    fn one_turn_claims_the_fact_and_the_next_turn_clears_it() {
        let s = Staged::default();
        assert_eq!(s.text_for("ses_1", "trn_1"), None);
        s.stage("ses_1", "the fact".into());
        assert_eq!(s.len(), 1);
        assert_eq!(s.text_for("ses_1", "trn_1").as_deref(), Some("the fact"));
        // Every later loop of the same turn.
        assert_eq!(s.text_for("ses_1", "trn_1").as_deref(), Some("the fact"));
        // Another session's turn reads nothing of it.
        assert_eq!(s.text_for("ses_2", "trn_1"), None);
        // The next turn in the session clears it.
        assert_eq!(s.text_for("ses_1", "trn_2"), None);
        assert_eq!(s.len(), 0);
        assert_eq!(s.text_for("ses_1", "trn_1"), None);
        // A second message before the turn compiles replaces the first.
        s.stage("ses_1", "one".into());
        s.stage("ses_1", "two".into());
        assert_eq!(s.text_for("ses_1", "trn_3").as_deref(), Some("two"));
    }
}
