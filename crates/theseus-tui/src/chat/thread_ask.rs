//! The router's question, and where it placed a message (theseus-wapgp,
//! theseus-0rfjx).
//!
//! A message that names no thread, sent from a place that has threads, finds
//! its own; when the router cannot tell it asks "which thread?" (`thread.asked`)
//! and the turn waits in the daemon for the answer, `[threads] ask_wait_secs`
//! (900 s) at most. The chat never showed the question, so a message of about
//! 150 characters or more (enough words to match two threads) was echoed and
//! then held for a quarter of an hour, with every later message behind it on
//! the connection's session lane. The chat now says the question as `ask`
//! does, and a line that is one of its keys answers it (`thread.answer`, on
//! this connection: it is not an ordered request, so the held turn does not
//! hold it). Anything else is a message, and the question stays until it is
//! answered, expires, or its turn begins.
//!
//! A turn the router placed in a thread says so: `→ thread <title>`, and how
//! to go there.

use serde_json::json;
use theseus_protocol::method;
use theseus_protocol::thread_route::{ThreadAsk, ThreadTag};
use theseus_protocol::TurnSubmitResult;

use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::Sty;

/// An option's key from what was typed: its key, `n` or `h`, or its label.
fn choice_of(a: &ThreadAsk, typed: &str) -> Option<usize> {
    let t = typed.trim().to_lowercase();
    let t = match t.as_str() {
        "n" => "new",
        "h" => "home",
        other => other,
    };
    a.options
        .iter()
        .position(|o| o.key == t || o.label.to_lowercase() == t)
}

/// The options as the CLI words them: `[1] pr round   [n]ew …   [h]ome`.
fn keys_line(a: &ThreadAsk) -> String {
    let opts: Vec<String> = a
        .options
        .iter()
        .map(|o| match o.key.as_str() {
            "new" => format!("[n]ew {}", o.label),
            "home" => "[h]ome".to_string(),
            k => format!("[{k}] {}", o.label),
        })
        .collect();
    opts.join("   ")
}

/// How long the wait has left, in words.
fn left(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 90 {
        format!("{} min", s.div_ceil(60))
    } else {
        format!("{s} s")
    }
}

impl Chat {
    /// `thread.asked`: the question, in the transcript. Heard again (the
    /// connection and the session's watch both carry it), it is said once.
    pub(super) fn thread_asked(&mut self, a: ThreadAsk) {
        if !self.ours(&a.session_id) || self.now_ms >= a.expires_ms {
            return;
        }
        if self
            .thread_ask
            .as_ref()
            .is_some_and(|t| t.ask_id == a.ask_id)
        {
            return;
        }
        self.gap();
        self.say_words(Sty::Yellow, &format!("? {}", a.question));
        if !a.message.is_empty() {
            self.say_words(Sty::Dim, &format!("  > {}", a.message));
        }
        self.say_words(Sty::Plain, &format!("  {}", keys_line(&a)));
        self.say_words(
            Sty::Dim,
            &format!(
                "  type a key and Enter · unanswered, it runs at home in {}",
                left(a.expires_ms - self.now_ms)
            ),
        );
        self.thread_ask = Some(a);
    }

    /// A line typed while the question waits: a key answers it (the
    /// `thread.answer` to send); anything else is a message, which the
    /// question's keys are said to once.
    pub(super) fn thread_answered_by(&mut self, typed: &str) -> Option<Vec<Effect>> {
        let a = self.thread_ask.as_ref()?;
        if self.now_ms >= a.expires_ms {
            self.thread_ask = None;
            return None;
        }
        let Some(at) = choice_of(a, typed) else {
            let keys = keys_line(a);
            self.flash(format!("the router still asks which thread · {keys}"));
            return None;
        };
        let (ask_id, option) = (a.ask_id.clone(), a.options[at].clone());
        self.thread_ask = None;
        self.say_plain(Sty::Dim, &format!("  → {}", option.label));
        Some(vec![call(
            method::THREAD_ANSWER,
            json!({ "ask_id": ask_id, "choice": option.key, "author": AUTHOR }),
            Purpose::Route,
        )])
    }

    /// The `thread.answer` was refused (the question ended meanwhile).
    pub(super) fn thread_answer_refused(&mut self, why: &str) {
        self.thread_ask = None;
        self.say_words(Sty::Red, &format!("✗ the answer was not taken: {why}"));
    }

    /// The turn began, or its submit failed: the question it waited on is over.
    pub(super) fn thread_ask_over(&mut self) {
        self.thread_ask = None;
    }

    /// The turn ran in a thread the router chose: say so, and how to go there.
    pub(super) fn placed_line(&mut self, r: &TurnSubmitResult) {
        let Some(ThreadTag {
            slug: Some(_),
            title,
            movable,
            ..
        }) = r.thread.as_ref()
        else {
            return;
        };
        self.say_plain(Sty::Cyan, &format!("→ thread {title}"));
        self.say_words(
            Sty::Dim,
            &format!("  go there: theseus chat {}", r.session_id),
        );
        if *movable {
            self.say_words(
                Sty::Dim,
                "  a guess: say \"that was for <thread>\" to move it",
            );
        }
    }
}
