//! `docs/glossary.md` holds the plans-and-steps words (theseus-3lik6): the six nouns the owner named, each with
//! its one line as the design sets it down, plus the three the UI needs (*turn*, *ask*, and the *step* the old
//! *plan item* became). The old record words are gone from it. Command and wire names stay in the entries.

const GLOSSARY: &str = include_str!("../../../docs/glossary.md");

/// The glossary with every run of whitespace one space, so a line the file wraps still reads as one.
fn flat() -> String {
    GLOSSARY.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Each noun's entry opens with its one line, word for word.
#[test]
fn the_six_nouns_open_with_their_one_line() {
    let g = flat();
    for (noun, line) in [
        (
            "conversation",
            "Where you talk with Theseus in one place (a DM, the CLI, a channel, the cockpit). It takes one turn at a time.",
        ),
        (
            "thread",
            "A named line of work with its own conversation. On the Graph it is a swimlane, and everything it started hangs in it.",
        ),
        (
            "plan",
            "A piece of work broken into parts. It holds steps and smaller plans, shows *n of m* done, and folds open and shut.",
        ),
        (
            "step",
            "One piece of work, checked off when done. When something is listed under it, it becomes a plan: same card, same history.",
        ),
        (
            "task",
            "An agent Theseus starts to carry out a step or a plan on its own, with its own budget and a fresh context. It reports back to where it came from.",
        ),
        (
            "job",
            "A command run for a turn or a task, such as a build or a test run.",
        ),
    ] {
        let entry = format!("**{noun}**: {line}");
        assert!(g.contains(&entry), "the entry for {noun} must open with: {line}");
    }
}

/// *turn* and *ask* take their one lines too, and *step* has an entry of its own.
#[test]
fn turn_ask_and_step_have_entries() {
    let g = flat();
    for (noun, line) in [
        (
            "turn",
            "One run of the model in a conversation or a task, from a message to its reply. (\"working\" counts turns.)",
        ),
        (
            "ask",
            "Anything waiting on your answer: an approval, a question, a budget, a failure to look at, a change to a step's goal. The Needs-you list holds the asks.",
        ),
    ] {
        let entry = format!("**{noun}**: {line}");
        assert!(g.contains(&entry), "the entry for {noun} must open with: {line}");
    }
}

/// A record is a plan or a step, never a task: the old words do not come back, and the wire names stay.
#[test]
fn the_old_record_words_are_gone_and_the_wire_names_stay() {
    let g = flat().to_lowercase();
    for gone in ["plan item", "task graph", "task record"] {
        assert!(!g.contains(gone), "the glossary still says `{gone}`");
    }
    for stays in ["`task.record`", "`taskrecord`", "`planitem`"] {
        assert!(g.contains(stays), "the glossary must keep the name {stays}");
    }
}
