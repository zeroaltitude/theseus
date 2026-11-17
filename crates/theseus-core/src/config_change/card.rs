//! The card's text (issue 22, line 1 step 3): built here, by the daemon, from
//! the two parsed configs, never from anything the model wrote except its
//! one-line reason, which is shown as the model's.

use super::diff::{words, Setting};
use crate::config::sensitivity;

/// The most settings the card lists before it says how many more there are.
pub const LISTED: usize = 12;
/// The longest the model's reason is shown.
pub const REASON_CHARS: usize = 200;

/// What the card says.
pub struct Facts<'a> {
    pub settings: &'a [Setting],
    pub outside_text: bool,
    pub reason: &'a str,
    /// How long a restart waits for a running turn to end, in seconds.
    pub wait_secs: u64,
}

/// The model's reason on one line, cut at [`REASON_CHARS`].
pub fn reason_line(r: &str) -> String {
    let one: String = r.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.chars().count() > REASON_CHARS {
        true => format!("{}…", one.chars().take(REASON_CHARS).collect::<String>()),
        false => one,
    }
}

/// The question the owner is asked.
pub fn question(f: &Facts<'_>) -> String {
    let n = f.settings.len();
    let heading = format!(
        "Change {}?\n",
        crate::narrative::count(n as u64, "setting", "settings")
    );
    heading + &lines(f)
}

/// An undo's question (issue 22): its own heading and its `preface`, then the
/// settings, the restart, the proof and the rest as every card says them.
pub fn question_undoing(number: u32, preface: &str, f: &Facts<'_>) -> String {
    format!("Undo change #{number}?\n{preface}{}", lines(f))
}

/// What a card says after its heading: each setting before and after, the
/// restart, the second proof, the injection label and the reason.
fn lines(f: &Facts<'_>) -> String {
    let n = f.settings.len();
    let mut out = String::new();
    for s in f.settings.iter().take(LISTED) {
        let when = match sensitivity::needs_restart(&s.path) {
            true => "after a restart",
            false => "at once",
        };
        out.push_str(&format!(
            "- {}: {} → {} ({when})\n",
            super::edit::dotted(&s.path),
            words(s.before.as_ref()),
            words(s.after.as_ref()),
        ));
    }
    if n > LISTED {
        out.push_str(&format!("- and {} more settings\n", n - LISTED));
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
    let sensitive: Vec<String> = f
        .settings
        .iter()
        .filter_map(|s| {
            sensitivity::reason(&s.path)
                .map(|why| format!("{} ({why})", super::edit::dotted(&s.path)))
        })
        .collect();
    if !sensitive.is_empty() {
        out.push_str(&format!(
            "These settings need a second proof as well as your yes: {}.\n",
            sensitive.join("; ")
        ));
    }
    if f.outside_text {
        out.push_str(
            "Take care: this conversation has read text from outside (a page, a search or a \
             server) since your last message, so the request may not be only yours.\n",
        );
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
            reason: "you said to turn the notes import off",
            wait_secs: 120,
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
            reason: "",
            wait_secs: 60,
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
    fn a_long_list_is_cut_and_the_reason_is_one_short_line() {
        let s: Vec<Setting> = (0..15)
            .map(|i| setting(&format!("kernel.k{i}"), None, Some(json!(i))))
            .collect();
        let q = question(&Facts {
            settings: &s,
            outside_text: false,
            reason: "a\nb  c",
            wait_secs: 1,
        });
        assert_eq!(q.matches("\n- ").count(), LISTED + 1, "{q}");
        assert!(q.contains("- and 3 more settings"), "{q}");
        assert!(q.contains("\"a b c\""), "{q}");
        assert_eq!(
            reason_line(&"x".repeat(500)).chars().count(),
            REASON_CHARS + 1
        );
    }
}
