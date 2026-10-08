//! The owner's correction in words (theseus-q31l): a short message that says
//! the last reply should have run elsewhere ("that should have been on
//! fable", "use opus for that", "that needed more effort", "rerun that on
//! opus"). Read deterministically, here, from the message alone: no model
//! and no Jev request decides it, and nothing the model calls reaches it.
//!
//! - **Narrow.** The whole message is the correction: a filler before it
//!   ("hey", "no,", "actually") and a courtesy after it ("please",
//!   "instead", "next time") are all it may carry. A message that goes on to
//!   ask for something else is a message, not a correction.
//! - **A name it knows.** What it names is a configured profile, a profile's
//!   model, one of the acting route pack's modes (their ids, read from the
//!   pack, so any version's), or a direction: stronger ("a stronger model",
//!   "more effort") or cheaper ("a cheaper model", "less effort").

/// Where the owner says the turn should have run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum To {
    /// A configured profile, by name.
    Profile(String),
    /// One of the route pack's modes, by id.
    Mode(String),
    /// A dearer profile than the one it ran on.
    Stronger,
    /// A cheaper one.
    Cheaper,
}

/// A correction, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub to: To,
    /// "rerun that on opus": the corrected turn is to be answered again.
    pub rerun: bool,
}

/// What a correction may name.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// Each configured profile, by name, with its model's id.
    pub profiles: Vec<(String, String)>,
    /// The route pack's modes, by id (`deep_coding`).
    pub modes: Vec<String>,
}

/// The longest message read as a correction, in characters.
pub const MAX_CHARS: usize = 160;

const BEFORE: [&str; 16] = [
    "hey", "hi", "no", "nope", "oh", "hmm", "actually", "jev", "theseus", "please", "ok", "okay",
    "so", "but", "um", "fyi",
];
const AFTER: [&str; 9] = [
    "please", "thanks", "thank", "you", "instead", "next", "time", "then", "pls",
];
const SUBJECT: [&str; 4] = ["that", "this", "it", "those"];
const SHOULD: [&str; 4] = ["should", "shoulda", "ought", "could"];
const VERB: [&str; 8] = ["been", "be", "gone", "go", "run", "ran", "used", "use"];
const PREP: [&str; 6] = ["on", "to", "with", "through", "using", "via"];
const NEEDED: [&str; 7] = [
    "needed",
    "needs",
    "need",
    "deserved",
    "wanted",
    "warranted",
    "required",
];
const RERUN: [&str; 6] = ["rerun", "redo", "retry", "re-run", "re-do", "re-try"];
const STRONGER: [&str; 9] = [
    "stronger", "better", "smarter", "bigger", "larger", "heavier", "deeper", "more", "harder",
];
const CHEAPER: [&str; 5] = ["cheaper", "smaller", "lighter", "less", "simpler"];
const MODEL_WORDS: [&str; 7] = [
    "model",
    "effort",
    "thought",
    "thinking",
    "reasoning",
    "brain",
    "one",
];

/// The message's words: lower case, a contraction opened ("should've"),
/// and the punctuation around each word dropped, inside kept (`glm-5.3`).
pub fn tokens(text: &str) -> Vec<String> {
    let text = text.replace(['\u{2019}', '\u{2018}'], "'").to_lowercase();
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let w = raw.trim_matches(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '_'));
        let w = w.trim_matches('\'');
        if w.is_empty() {
            continue;
        }
        match w.split_once('\'') {
            Some((a, "ve")) => out.extend([a.to_string(), "have".to_string()]),
            Some((a, "s")) if SUBJECT.contains(&a) || a == "that" => {
                out.extend([a.to_string(), "is".to_string()])
            }
            Some((a, "d")) => out.extend([a.to_string(), "would".to_string()]),
            _ => out.push(w.to_string()),
        }
    }
    out
}

/// The correction `text` says, if it is one.
pub fn read(text: &str, names: &Names) -> Option<Said> {
    if text.chars().count() > MAX_CHARS || text.trim_start().starts_with('/') {
        return None;
    }
    let all = tokens(text);
    let mut w: &[String] = &all;
    while let Some((first, rest)) = w.split_first() {
        if !BEFORE.contains(&first.as_str()) {
            break;
        }
        w = rest;
    }
    while let Some((last, rest)) = w.split_last() {
        if !AFTER.contains(&last.as_str()) {
            break;
        }
        w = rest;
    }
    let w: Vec<&str> = w.iter().map(String::as_str).collect();
    rerun(&w, names)
        .or_else(|| should(&w, names))
        .or_else(|| use_for(&w, names))
        .or_else(|| needed(&w))
}

/// "rerun that on opus", "redo it with fable", "run that again on opus",
/// "try that again using a stronger model".
fn rerun(w: &[&str], names: &Names) -> Option<Said> {
    let rest = match w {
        [v, s, rest @ ..] if RERUN.contains(v) && SUBJECT.contains(s) => rest,
        [v, s, "again", rest @ ..]
            if ["run", "do", "try", "answer"].contains(v) && SUBJECT.contains(s) =>
        {
            rest
        }
        [v, rest @ ..] if RERUN.contains(v) => rest,
        _ => return None,
    };
    let rest = rest.strip_prefix(&["again"]).unwrap_or(rest);
    let (p, name) = rest.split_first()?;
    PREP.contains(p).then_some(())?;
    Some(Said {
        to: name_of(name, names)?,
        rerun: true,
    })
}

/// "that should have been on fable", "it should've gone to opus", "this
/// should have used a stronger model", "that should be deep coding".
fn should(w: &[&str], names: &Names) -> Option<Said> {
    let [s, sh, rest @ ..] = w else { return None };
    (SUBJECT.contains(s) && SHOULD.contains(sh)).then_some(())?;
    let rest = rest.strip_prefix(&["have"]).unwrap_or(rest);
    let rest = rest.strip_prefix(&["to"]).unwrap_or(rest);
    let rest = rest.strip_prefix(&["have"]).unwrap_or(rest);
    let (v, rest) = rest.split_first()?;
    VERB.contains(v).then_some(())?;
    let to = match rest.split_first() {
        Some((p, r)) if PREP.contains(p) => name_of(r, names)?,
        _ => named_plainly(rest, names)?,
    };
    Some(Said { to, rerun: false })
}

/// A name after a bare verb ("it should have been fable", "use opus"): a
/// profile, a model, a mode named in more than one word ("deep coding",
/// "quick mode"), or a direction with its noun ("a stronger model"). A lone
/// adjective there ("that should be trivial", "this should be simpler",
/// "use something simpler") says what the work is, not where it ran.
fn named_plainly(w: &[&str], names: &Names) -> Option<To> {
    let to = name_of(w, names)?;
    let plain = match &to {
        To::Profile(_) => true,
        To::Mode(_) => w.len() > 1,
        To::Stronger | To::Cheaper => w.last().is_some_and(|n| MODEL_WORDS.contains(n)),
    };
    plain.then_some(to)
}

/// "use opus for that", "use fable for this one", "use a stronger model".
fn use_for(w: &[&str], names: &Names) -> Option<Said> {
    let (v, rest) = w.split_first()?;
    (*v == "use").then_some(())?;
    let at = rest.iter().position(|x| *x == "for").unwrap_or(rest.len());
    let tail = &rest[at..];
    let tail_ok = match tail {
        [] => true,
        ["for", s] | ["for", s, "one"] => SUBJECT.contains(s),
        ["for", s, "kind", "of", ..] => SUBJECT.contains(s),
        _ => false,
    };
    tail_ok.then_some(())?;
    Some(Said {
        to: named_plainly(&rest[..at], names)?,
        rerun: false,
    })
}

/// "that needed more effort", "this needs a stronger model", "that was
/// overkill", "it deserved more thought".
fn needed(w: &[&str]) -> Option<Said> {
    let [s, rest @ ..] = w else { return None };
    SUBJECT.contains(s).then_some(())?;
    let to = match rest {
        ["was" | "is", "overkill"] => To::Cheaper,
        [v, what @ ..] if NEEDED.contains(v) => direction(what)?,
        ["could", "use" | "have", "used", what @ ..] | ["could", "use", what @ ..] => {
            direction(what)?
        }
        _ => return None,
    };
    Some(Said { to, rerun: false })
}

/// What a name names: a profile, a profile's model, a mode, or a direction.
fn name_of(w: &[&str], names: &Names) -> Option<To> {
    if let Some(d) = direction(w) {
        return Some(d);
    }
    let w = match w {
        ["the", rest @ ..] => rest,
        _ => w,
    };
    let w = match w {
        [rest @ .., "model" | "profile" | "mode"] => rest,
        _ => w,
    };
    let joined = w.join(" ");
    if joined.is_empty() {
        return None;
    }
    let under = w.join("_");
    if let Some((p, _)) = names
        .profiles
        .iter()
        .find(|(p, _)| p.to_lowercase() == joined)
    {
        return Some(To::Profile(p.clone()));
    }
    if let Some((p, _)) = names
        .profiles
        .iter()
        .find(|(_, m)| m.to_lowercase() == joined)
    {
        return Some(To::Profile(p.clone()));
    }
    names
        .modes
        .iter()
        .find(|m| m.to_lowercase() == under)
        .map(|m| To::Mode(m.clone()))
}

/// "a stronger model", "more effort", "something cheaper", "less thought".
fn direction(w: &[&str]) -> Option<To> {
    let w = match w {
        ["a" | "an" | "something" | "some", rest @ ..] => rest,
        _ => w,
    };
    let (adj, rest) = match w {
        [a, rest @ ..] => (*a, rest),
        [] => return None,
    };
    let noun_ok = match rest {
        [] => adj != "more" && adj != "less",
        [n] => MODEL_WORDS.contains(n),
        _ => false,
    };
    noun_ok.then_some(())?;
    if STRONGER.contains(&adj) {
        Some(To::Stronger)
    } else if CHEAPER.contains(&adj) {
        Some(To::Cheaper)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        Names {
            profiles: [
                ("sonnet", "claude-sonnet-5-5"),
                ("opus", "claude-opus-5-5"),
                ("fable", "claude-fable-5-1"),
                ("haiku", "claude-haiku-5-5"),
                ("glm53", "glm-5.3"),
            ]
            .iter()
            .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
            .collect(),
            modes: ["trivial", "quick", "chat", "sophisticated", "deep_coding"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }

    fn said(text: &str) -> Option<Said> {
        read(text, &names())
    }

    fn to(text: &str) -> Option<To> {
        said(text).map(|s| s.to)
    }

    #[test]
    fn the_owners_ways_of_saying_it_name_where_it_should_have_run() {
        let fable = Some(To::Profile("fable".into()));
        for t in [
            "hey that should have been on fable",
            "That should've been on Fable.",
            "no, that should have gone to fable",
            "this should have used fable",
            "it should have been fable",
            "that should have been on the fable model",
            "that ought to have been on fable",
            "use fable for that",
            "use fable for this one",
            "Use fable, please",
            "that should have been on claude-fable-5-1",
            "that should’ve been on fable next time",
        ] {
            assert_eq!(to(t), fable, "{t}");
            assert!(!said(t).unwrap().rerun, "{t}");
        }
        assert_eq!(
            to("that should have been deep coding"),
            Some(To::Mode("deep_coding".into()))
        );
        // A mode named as one, or after a preposition, still counts.
        for t in [
            "that should have been quick mode",
            "that should have been on quick",
            "use the quick mode for that",
        ] {
            assert_eq!(to(t), Some(To::Mode("quick".into())), "{t}");
        }
        assert_eq!(
            to("that should have been on glm-5.3"),
            Some(To::Profile("glm53".into()))
        );
        for t in [
            "that needed more effort",
            "this needs a stronger model",
            "it deserved more thought",
            "that should have been on a stronger model",
            "use a bigger model for that",
            "that could have used more reasoning",
        ] {
            assert_eq!(to(t), Some(To::Stronger), "{t}");
        }
        for t in [
            "that was overkill",
            "that needed a cheaper model",
            "that should have been on something cheaper",
            "this needs less effort",
        ] {
            assert_eq!(to(t), Some(To::Cheaper), "{t}");
        }
    }

    #[test]
    fn a_rerun_names_where_to_answer_again() {
        for t in [
            "rerun that on fable",
            "redo it with fable please",
            "run that again on fable",
            "retry on fable",
            "re-run this using the fable model",
        ] {
            assert_eq!(
                said(t),
                Some(Said {
                    to: To::Profile("fable".into()),
                    rerun: true
                }),
                "{t}"
            );
        }
        assert_eq!(
            said("redo that with a stronger model"),
            Some(Said {
                to: To::Stronger,
                rerun: true
            })
        );
    }

    /// Narrow: a message that asks for more, names nothing it knows, or is
    /// long, is a message. So is a slash command.
    #[test]
    fn anything_else_is_a_message() {
        for t in [
            "use fable for that and then fix the parser",
            "that should have been on fable, and also what about the tests?",
            "write a sonnet about the sea",
            "use sonnet structure for that poem",
            "that should have been on mars",
            "that should have been faster",
            "should we use opus here?",
            "rerun the tests on the new branch",
            "that needed more",
            "/model fable",
            "I think the code you wrote that should have been on fable is fine",
            "the build ran on opus",
            // Remarks about the work, not about where it ran (the review's
            // join fix): a bare adjective after a bare verb.
            "that should be trivial",
            "this should be quick",
            "it should have been quick",
            "this should be simpler",
            "that should be better",
            "use something simpler",
            "",
        ] {
            assert_eq!(said(t), None, "{t:?}");
        }
        let long = format!("that should have been on fable {}", "x ".repeat(80));
        assert_eq!(said(&long), None);
    }

    #[test]
    fn its_words_open_contractions_and_keep_a_names_inside() {
        assert_eq!(
            tokens("That's it, should've gone to glm-5.3!"),
            ["that", "is", "it", "should", "have", "gone", "to", "glm-5.3"]
        );
    }
}
