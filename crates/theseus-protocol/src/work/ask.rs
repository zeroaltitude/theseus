//! `owner_ask` (theseus-j7cje): a task's multiple-choice question for the
//! owner, with 2 to 6 options and a default it expires to. Pure: the tool's
//! input, its limits, the answer's id forms, and the words the model reads
//! back. The tool, the answer and the expiry are the core's (`ask.rs`).
//!
//! The options are numbered from 1 for people (`theseus answer <id> 2`, the
//! TUI's keys 1 to 6, `default`, the words the model reads); their answer
//! ids are the work view's `choice:<index from 0>`, and `choice:other` is
//! "something else", which takes a note.

use serde::Deserialize;
use serde_json::Value;

/// The tool's name.
pub const OWNER_ASK: &str = "owner_ask";
/// The tools whose question still expires at its life: an `owner_ask` applies
/// its default, and an extension's ack and a pack's promotion are declined.
/// A call's question, and a budget question, are held past it (theseus-v2dnc);
/// the core's `held_past_life` and the clients' cards all read this one list
/// (theseus-lrc3m), and the cockpit's `heldquestion.ts` keeps a copy.
pub const EXPIRING_TOOLS: [&str; 3] = [OWNER_ASK, "extend.ack", "pack.promote"];

/// Whether `tool`'s question is held until it is answered, never expired. A
/// budget question holds too, and says so itself.
pub fn held_tool(tool: &str) -> bool {
    !EXPIRING_TOOLS.contains(&tool)
}
/// The fewest and the most options.
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 6;
/// The longest question, option label and option detail, in characters.
pub const QUESTION_CHARS: usize = 500;
pub const LABEL_CHARS: usize = 60;
pub const DETAIL_CHARS: usize = 300;
/// A question's life when it names none, and its longest, in minutes.
pub const DEFAULT_MINUTES: u64 = 30;
pub const MAX_MINUTES: u64 = 24 * 60;

/// One option: the words on its button, and a line that says more.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskOption {
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// `owner_ask`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskInput {
    pub question: String,
    pub options: Vec<AskOption>,
    /// The option that applies if no one answers: its number, from 1.
    #[serde(default)]
    pub default: Option<usize>,
    /// How long the question waits, in minutes.
    #[serde(default)]
    pub expires_in: Option<u64>,
}

impl AskInput {
    /// The input read and checked: a refusal says which limit it broke.
    pub fn parse(input: &Value) -> Result<Self, String> {
        let a: AskInput = serde_json::from_value(input.clone())
            .map_err(|e| format!("invalid input for {OWNER_ASK}: {e}"))?;
        let chars = |s: &str| s.trim().chars().count();
        if chars(&a.question) == 0 || chars(&a.question) > QUESTION_CHARS {
            return Err(format!(
                "`question` is {} characters; it takes 1 to {QUESTION_CHARS}",
                chars(&a.question)
            ));
        }
        let n = a.options.len();
        if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&n) {
            return Err(format!(
                "`options` has {n}; a question takes {MIN_OPTIONS} to {MAX_OPTIONS} options"
            ));
        }
        for (i, o) in a.options.iter().enumerate() {
            if chars(&o.label) == 0 || chars(&o.label) > LABEL_CHARS {
                return Err(format!(
                    "option {}'s `label` is {} characters; it takes 1 to {LABEL_CHARS}",
                    i + 1,
                    chars(&o.label)
                ));
            }
            if o.detail.as_deref().is_some_and(|d| chars(d) > DETAIL_CHARS) {
                return Err(format!(
                    "option {}'s `detail` is over {DETAIL_CHARS} characters",
                    i + 1
                ));
            }
        }
        if let Some(d) = a.default.filter(|d| !(1..=n).contains(d)) {
            return Err(format!(
                "`default` is {d}; it is the number of an option, 1 to {n}"
            ));
        }
        if let Some(m) = a.expires_in.filter(|m| !(1..=MAX_MINUTES).contains(m)) {
            return Err(format!(
                "`expires_in` is {m} minutes; it takes 1 to {MAX_MINUTES} (24 hours)"
            ));
        }
        Ok(a)
    }

    /// How long the question waits, in milliseconds.
    pub fn life_ms(&self) -> u64 {
        self.expires_in.unwrap_or(DEFAULT_MINUTES) * 60_000
    }

    /// The labels, in order: a question's `choices`.
    pub fn choices(&self) -> Vec<String> {
        self.options.iter().map(|o| o.label.clone()).collect()
    }

    /// The default option's index from 0, if there is one.
    pub fn default_index(&self) -> Option<usize> {
        self.default.map(|d| d - 1)
    }

    /// What a question says happens when no one answers.
    pub fn on_expiry(&self) -> String {
        match self.default {
            Some(d) => format!("default {d}: {}", self.options[d - 1].label),
            None => "no default".to_string(),
        }
    }
}

/// The choices of a stored `owner_ask` input, in order; none if it does not
/// read (a question from a build that worded it otherwise).
pub fn choices_of(input: &Value) -> Vec<String> {
    AskInput::parse(input)
        .map(|a| a.choices())
        .unwrap_or_default()
}

/// What an answer's option names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Option `i`, from 0.
    Pick(usize),
    /// "Something else", in the owner's words.
    Other,
}

/// The choice `option` names among `n` options: `choice:<i>` (from 0),
/// `choice:other`, `other`, or a bare number from 1 (what a person types).
pub fn choice_of(option: &str, n: usize) -> Option<Choice> {
    let o = option.trim();
    if matches!(o, "choice:other" | "other") {
        return Some(Choice::Other);
    }
    if let Some(i) = o.strip_prefix("choice:") {
        return i.parse::<usize>().ok().filter(|i| *i < n).map(Choice::Pick);
    }
    o.parse::<usize>()
        .ok()
        .filter(|d| (1..=n).contains(d))
        .map(|d| Choice::Pick(d - 1))
}

/// The answer id of option `i` (from 0), as the work view words it.
pub fn choice_id(i: usize) -> String {
    format!("choice:{i}")
}

/// How long a life reads: `30 minutes`, `1 hour`, `2 hours 30 minutes`.
pub fn within(ms: u64) -> String {
    let m = ms / 60_000;
    let (h, m) = (m / 60, m % 60);
    let hours = |h: u64| format!("{h} hour{}", if h == 1 { "" } else { "s" });
    let mins = |m: u64| format!("{m} minute{}", if m == 1 { "" } else { "s" });
    match (h, m) {
        (0, m) => mins(m.max(1)),
        (h, 0) => hours(h),
        (h, m) => format!("{} {}", hours(h), mins(m)),
    }
}

/// What the model reads back when the owner picked option `i`.
pub fn chose(a: &AskInput, i: usize, note: Option<&str>) -> String {
    let mut t = format!(
        "The owner chose {}, \"{}\"",
        i + 1,
        a.options.get(i).map_or("", |o| o.label.as_str())
    );
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        t.push_str(&format!("; note: {n}"));
    }
    t.push('.');
    t
}

/// What the model reads back when the owner took none of the options.
pub fn said_instead(note: &str) -> String {
    format!(
        "The owner took none of the options and answered in their own words: {}",
        note.trim()
    )
}

/// What the model reads back when no one answered within the question's
/// life: the default applied, or there was none.
pub fn expired(a: &AskInput) -> String {
    let waited = within(a.life_ms());
    match a.default_index() {
        Some(i) => format!(
            "No one answered within {waited}; the default applied: option {}, \"{}\". Go on with \
             it, and say you did.",
            i + 1,
            a.options[i].label
        ),
        None => format!(
            "No one answered within {waited}, and the question had no default. Decide, and say \
             what you decided."
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn input(n: usize) -> Value {
        let options: Vec<Value> = (1..=n)
            .map(|i| json!({"label": format!("Option {i}")}))
            .collect();
        json!({"question": "Which?", "options": options})
    }

    #[test]
    fn two_to_six_options_are_accepted_and_one_and_seven_are_refused() {
        for n in 2..=6 {
            assert_eq!(AskInput::parse(&input(n)).unwrap().options.len(), n);
        }
        for n in [0, 1, 7] {
            let e = AskInput::parse(&input(n)).expect_err("out of range");
            assert!(e.contains("2 to 6 options"), "{e}");
        }
    }

    #[test]
    fn the_limits_are_held_and_named() {
        let mut v = input(2);
        v["question"] = json!("q".repeat(501));
        assert!(AskInput::parse(&v).unwrap_err().contains("`question`"));
        let mut v = input(2);
        v["options"][1]["label"] = json!("l".repeat(61));
        assert!(AskInput::parse(&v)
            .unwrap_err()
            .contains("option 2's `label`"));
        let mut v = input(3);
        v["default"] = json!(4);
        assert!(AskInput::parse(&v).unwrap_err().contains("`default`"));
        v["default"] = json!(0);
        assert!(AskInput::parse(&v).is_err());
        let mut v = input(2);
        v["expires_in"] = json!(1441);
        assert!(AskInput::parse(&v).unwrap_err().contains("24 hours"));
        v["expires_in"] = json!(1440);
        assert_eq!(AskInput::parse(&v).unwrap().life_ms(), 86_400_000);
        let mut v = input(2);
        v["surprise"] = json!(1);
        assert!(AskInput::parse(&v).is_err(), "unknown keys are refused");
    }

    #[test]
    fn a_question_lives_thirty_minutes_unless_it_says_more() {
        assert_eq!(AskInput::parse(&input(2)).unwrap().life_ms(), 1_800_000);
        assert_eq!(within(1_800_000), "30 minutes");
        assert_eq!(within(3_600_000), "1 hour");
        assert_eq!(within(9_000_000), "2 hours 30 minutes");
    }

    #[test]
    fn an_option_is_named_by_id_or_by_number() {
        assert_eq!(choice_of("choice:0", 3), Some(Choice::Pick(0)));
        assert_eq!(choice_of("2", 3), Some(Choice::Pick(1)));
        assert_eq!(choice_of("3", 3), Some(Choice::Pick(2)));
        assert_eq!(choice_of("4", 3), None);
        assert_eq!(choice_of("0", 3), None);
        assert_eq!(choice_of("choice:3", 3), None);
        assert_eq!(choice_of("other", 3), Some(Choice::Other));
        assert_eq!(choice_of("choice:other", 3), Some(Choice::Other));
        assert_eq!(choice_of("approve", 3), None);
    }

    #[test]
    fn the_model_reads_what_was_chosen_and_what_applied() {
        let mut v = input(3);
        v["options"][1]["label"] = json!("Ship it behind the flag");
        v["default"] = json!(2);
        let a = AskInput::parse(&v).unwrap();
        assert_eq!(
            chose(&a, 1, Some("after lunch")),
            "The owner chose 2, \"Ship it behind the flag\"; note: after lunch."
        );
        assert_eq!(
            chose(&a, 1, None),
            "The owner chose 2, \"Ship it behind the flag\"."
        );
        assert!(expired(&a).contains("the default applied: option 2, \"Ship it behind the flag\""));
        assert_eq!(a.on_expiry(), "default 2: Ship it behind the flag");
        let none = AskInput::parse(&input(2)).unwrap();
        assert!(expired(&none).contains("no default"));
        assert_eq!(none.on_expiry(), "no default");
    }
}
