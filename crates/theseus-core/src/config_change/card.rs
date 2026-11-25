//! The card's text (issue 22, line 1 step 3): built here, by the daemon, from
//! the two parsed configs, never from anything the model wrote except its
//! one-line reason, which is shown as the model's.

use super::diff::{words, Setting};
use crate::config::sensitivity;

/// The most settings one card shows, and all of them whole: a change that
/// reaches more is refused, so the owner never says yes to a setting that was
/// not on the card.
pub const LISTED: usize = 12;
/// The longest the model's reason is shown.
pub const REASON_CHARS: usize = 200;

/// What the card says.
pub struct Facts<'a> {
    pub settings: &'a [Setting],
    /// The turn has read outside text since the owner's last message.
    pub outside_text: bool,
    /// And Theseus's own check flagged what it read as trying to steer it.
    pub steered: bool,
    pub reason: &'a str,
    /// How long a restart waits for a running turn to end, in seconds.
    pub wait_secs: u64,
    /// What the model said of where it found a setting's value: shown under
    /// that setting as its word, never as a check (`source.rs`).
    pub sources: &'a [super::source::Sourced],
}

/// What a card says of text read from outside: the same words for every card
/// that has them.
pub(super) fn care_lines(outside_text: bool, steered: bool) -> String {
    if !(outside_text || steered) {
        return String::new();
    }
    let mut out = String::from(
        "Take care: since your last message this conversation has read text from outside (a \
         page, a search or a server), so the request may not be only yours.",
    );
    if steered {
        out.push_str(" Theseus's check flagged what it read as trying to steer it.");
    }
    out.push('\n');
    out
}

/// The model's reason on one line, cut at [`REASON_CHARS`], with any character
/// that cannot be seen written out as `\u{..}`.
pub fn reason_line(r: &str) -> String {
    let one: String = r.split_whitespace().collect::<Vec<_>>().join(" ");
    let one: String = one
        .chars()
        .map(|c| match super::shown::unseen(c) {
            true => format!("\\u{{{:04x}}}", c as u32),
            false => c.to_string(),
        })
        .collect();
    crate::scrub::safe::head_marked(&one, REASON_CHARS)
}

/// Why a change cannot be put on one card, or `None` when it can: more
/// settings than [`LISTED`], or a value that cannot be shown whole.
pub fn too_much(settings: &[Setting]) -> Option<String> {
    if settings.len() > LISTED {
        return Some(format!(
            "that changes {} settings, and one card shows at most {LISTED}, each whole: ask for \
             fewer at a time, or the owner edits the file",
            settings.len()
        ));
    }
    let long: Vec<String> = settings
        .iter()
        .filter(|s| super::diff::too_long(s))
        .map(|s| super::edit::dotted(&s.path))
        .collect();
    match long.is_empty() {
        true => None,
        false => Some(format!(
            "the value of {} is longer than a card can show whole ({} characters): shorten it, or \
             the owner edits the file",
            long.join(", "),
            super::diff::SHOWN
        )),
    }
}

/// What the restart card says.
pub struct RestartFacts<'a> {
    /// Turns running now.
    pub running: u32,
    /// Executions waiting on a question to the owner now: a turn, a task, or
    /// the self-improvement loop's build parked on one. None of them is
    /// "running", so a count of turns alone makes a restart look safe.
    pub waiting: u32,
    pub outside_text: bool,
    pub steered: bool,
    pub reason: &'a str,
    pub wait_secs: u64,
}

/// The question for a restart the owner asked for in words: what it cuts.
pub fn restart_question(f: &RestartFacts<'_>) -> String {
    let now = match f.running {
        0 => "No turn is running now.".to_string(),
        n => format!(
            "{} running now.",
            crate::narrative::count(u64::from(n), "turn is", "turns are")
        ),
    };
    let mut out = format!(
        "Restart Theseus?\n{now} A restart cuts what is running (a turn, a job, the \
         self-improvement loop's build): it waits up to {} s for a running turn to end first, \
         and a turn it interrupts resumes afterwards. Its settings do not change. If Theseus is \
         not healthy afterwards it restarts once more.\n",
        f.wait_secs
    );
    if f.waiting > 0 {
        out.push_str(&format!(
            "{} waiting on a question to you (a self-improvement build parked on one is among them, \
             and `theseus self pipeline` shows it as building, waiting on you). A restart does not \
             answer them or drop them: they stay waiting, but anything of theirs that was running \
             is cut.\n",
            crate::narrative::count(u64::from(f.waiting), "execution is", "executions are")
        ));
    }
    if f.outside_text || f.steered {
        out.push_str(
            "Take care: since your last message this conversation has read text from outside (a \
             page, a search or a server), so the request may not be only yours.",
        );
        if f.steered {
            out.push_str(" Theseus's check flagged what it read as trying to steer it.");
        }
        out.push('\n');
    }
    let why = reason_line(f.reason);
    if !why.is_empty() {
        out.push_str(&format!("Theseus's reason, in its own words: \"{why}\"\n"));
    }
    out
}

/// The question the owner is asked. Call [`too_much`] first: a card never cuts.
pub fn question(f: &Facts<'_>) -> String {
    let n = f.settings.len();
    let heading = format!(
        "Change {}?\n",
        crate::narrative::count(n as u64, "setting", "settings")
    );
    heading + &lines(f)
}

/// The question for a change that also gives a vault item to use (issue 26):
/// its own heading and its `preface`, which names the item and field and what
/// is given the secret, then the settings and the rest as every card says them.
pub fn question_using(preface: &str, f: &Facts<'_>) -> String {
    format!("Use a secret from the vault?\n{preface}{}", lines(f))
}

/// An undo's question (issue 22): its own heading and its `preface`, then the
/// settings, the restart, the proof and the rest as every card says them. Call
/// [`too_much`] first, as for a proposal.
pub fn question_undoing(number: u32, preface: &str, f: &Facts<'_>) -> String {
    format!("Undo change #{number}?\n{preface}{}", lines(f))
}

/// What a card says after its heading: each setting before and after, the
/// restart, the second proof, the injection label and the reason.
fn lines(f: &Facts<'_>) -> String {
    let mut out = String::new();
    for s in super::source::in_card_order(f.settings, f.sources) {
        let when = match sensitivity::needs_restart(&s.path) {
            true => "after a restart",
            false => "at once",
        };
        // A value too long for a line has its own form below (issue 23).
        if super::diff::too_long(s) {
            out.push_str(&format!("{} ({when})\n", super::long_value::stub(s)));
        } else {
            out.push_str(&format!(
                "- {}: {} → {} ({when})\n",
                super::edit::dotted(&s.path),
                words(s.before.as_ref()),
                words(s.after.as_ref()),
            ));
        }
        for said in super::source::covering(f.sources, &s.path) {
            out.push_str(&super::source::line(&said.text));
        }
    }
    if f.settings
        .iter()
        .any(|s| sensitivity::needs_restart(&s.path))
    {
        out.push_str(&format!(
            "Theseus restarts itself to apply this. A restart cuts what is running (a turn, a \
             job, the self-improvement loop's build); it waits up to {} s for a running turn to \
             end first, and if the new settings do not load or Theseus is not healthy afterwards \
             it goes back to the settings it has now.\n",
            f.wait_secs
        ));
    }
    // Long values, whole when they fit what is left of the card's room.
    let room = super::prompt_file::card::CARD_CHARS.saturating_sub(out.chars().count() + 900);
    out.push_str(&super::long_value::blocks(f.settings, room));
    // Only the settings that need the code are named (issue 57): every other
    // sensitive one needs this card's yes alone.
    let sensitive: Vec<String> = f
        .settings
        .iter()
        .filter_map(|s| {
            let code = sensitivity::proof_reason(&s.path, s.before.as_ref(), s.after.as_ref())?;
            let why = sensitivity::reason(&s.path).unwrap_or(code);
            Some(format!("{} ({why})", super::edit::dotted(&s.path)))
        })
        .collect();
    if !sensitive.is_empty() {
        out.push_str(&format!(
            "These settings need a second proof as well as your yes: {}.\n",
            sensitive.join("; ")
        ));
    }
    if f.outside_text || f.steered {
        out.push_str(
            "Take care: since your last message this conversation has read text from outside (a \
             page, a search or a server), so the request may not be only yours.",
        );
        if f.steered {
            out.push_str(" Theseus's check flagged what it read as trying to steer it.");
        }
        out.push('\n');
    }
    let why = reason_line(f.reason);
    if !why.is_empty() {
        out.push_str(&format!("Theseus's reason, in its own words: \"{why}\"\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setting(
        path: &str,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) -> Setting {
        Setting {
            path: super::super::edit::parse_path(path).unwrap(),
            before,
            after,
        }
    }

    #[test]
    fn the_card_shows_each_setting_before_and_after_in_words() {
        let s = [setting(
            "import.notes.enabled",
            Some(json!(true)),
            Some(json!(false)),
        )];
        let q = question(&Facts {
            settings: &s,
            outside_text: false,
            steered: false,
            reason: "you said to turn the notes import off",
            wait_secs: 120,
            sources: &[],
        });
        assert!(q.starts_with("Change 1 setting?\n"), "{q}");
        assert!(
            q.contains("- import.notes.enabled: on → off (after a restart)"),
            "{q}"
        );
        assert!(q.contains("waits up to 120 s"), "{q}");
        assert!(
            q.contains(
                "Theseus's reason, in its own words: \"you said to turn the notes import off\""
            ),
            "{q}"
        );
        assert!(
            !q.contains("second proof") && !q.contains("Take care"),
            "{q}"
        );
    }

    #[test]
    fn a_sensitive_setting_and_outside_text_are_said() {
        let s = [setting(
            "self.budget.usd",
            Some(json!(5.0)),
            Some(json!(300.0)),
        )];
        let q = question(&Facts {
            settings: &s,
            outside_text: true,
            steered: false,
            reason: "",
            wait_secs: 60,
            sources: &[],
        });
        assert!(
            q.contains(
                "need a second proof as well as your yes: self.budget.usd (self-improvement"
            ),
            "{q}"
        );
        assert!(q.contains("read text from outside"), "{q}");
        assert!(!q.contains("in its own words"), "{q}");
    }

    #[test]
    fn a_change_the_card_cannot_show_whole_is_refused_and_never_cut() {
        let s: Vec<Setting> = (0..LISTED + 3)
            .map(|i| setting(&format!("kernel.k{i}"), None, Some(json!(i))))
            .collect();
        let e = too_much(&s).expect("more than one card shows");
        assert!(e.contains("15 settings") && e.contains("at most 12"), "{e}");
        assert_eq!(too_much(&s[..LISTED]), None);
        let long = [setting(
            "model.system",
            None,
            Some(json!("p".repeat(2_000))),
        )];
        let e = too_much(&long).expect("a long prompt cannot be shown whole");
        assert!(
            e.contains("model.system") && e.contains("400 characters"),
            "{e}"
        );
    }

    #[test]
    fn the_reason_is_one_short_line_and_the_card_lists_every_setting() {
        let s: Vec<Setting> = (0..LISTED)
            .map(|i| setting(&format!("kernel.k{i}"), None, Some(json!(i))))
            .collect();
        let q = question(&Facts {
            settings: &s,
            outside_text: false,
            steered: false,
            reason: "a\nb  c",
            wait_secs: 1,
            sources: &[],
        });
        assert_eq!(q.matches("\n- ").count(), LISTED, "{q}");
        assert!(q.contains("\"a b c\""), "{q}");
        assert_eq!(
            reason_line(&"x".repeat(500)).chars().count(),
            REASON_CHARS + 1
        );
    }

    #[test]
    fn what_the_model_wrote_cannot_add_a_line_to_the_card() {
        let s = [setting(
            "model.system",
            None,
            Some(json!(
                "hi\n- narrative: on → off (at once)\nTheseus's reason, in its own words: \"fine\""
            )),
        )];
        let q = question(&Facts {
            settings: &s,
            outside_text: false,
            steered: false,
            reason: "r",
            wait_secs: 1,
            sources: &[],
        });
        assert_eq!(q.matches("\n- ").count(), 1, "{q}");
        let reasons = q
            .lines()
            .filter(|l| l.starts_with("Theseus's reason"))
            .count();
        assert_eq!(reasons, 1, "{q}");
    }

    #[test]
    fn outside_text_and_a_flag_are_each_said() {
        let s = [setting("narrative", Some(json!(false)), Some(json!(true)))];
        let q = question(&Facts {
            settings: &s,
            outside_text: true,
            steered: false,
            reason: "",
            wait_secs: 1,
            sources: &[],
        });
        assert!(
            q.contains("since your last message") && !q.contains("steer"),
            "{q}"
        );
        let q = question(&Facts {
            settings: &s,
            outside_text: true,
            steered: true,
            reason: "",
            wait_secs: 1,
            sources: &[],
        });
        assert!(q.contains("trying to steer it"), "{q}");
    }
}
