//! The durable-versus-run-state rule, read against notes of the shapes it is
//! meant to tell apart (`b4-theseus-holds-its-own-notes`). The fixtures are
//! invented, in the shape of an agent's own workspace notes, and hold no
//! person's name and no real path.

use super::*;

/// A rules note of the shape source (1) and source (4) have: how we build,
/// the gates, the lanes.
const RULES: &str = "\
# How we build

- The gate runs before every commit, and you chain the commit with `&&`, never with a semicolon.
- Every commit subject is `area: what changed (<issue id>)`, the area in lower case.
- A lane never writes a shared file: those change only at the join.
- Never raise a ceiling to get a green gate; put the new lines in a sibling module instead.
";

/// A run log of the shape most of source (2) has.
const LOG: &str = "\
# Gate run

Ran the gate on the lane after the rebase. Thirty-six reds, all of them the sandbox family.
Pushed it. Next: wait for the reviewer.
Had to rerun the suite once because the machine was busy.
";

/// A note that states one rule and nothing else: under the floor.
const THIN: &str = "# Note\n\nAlways chain the gate and the commit with `&&`.\n";

#[test]
fn a_rules_note_is_durable_and_says_how_many_rules_it_found() {
    let r = read("refs/building.md", RULES);
    assert_eq!(r.verdict, Verdict::Durable, "{r:?}");
    assert!(
        r.rules >= MIN_RULES,
        "it reached the floor on its own lines: {r:?}"
    );
    assert_eq!(r.why, format!("{} rule lines", r.rules));
    assert_eq!(r.line(), format!("durable ({} rule lines)", r.rules));
}

#[test]
fn a_run_log_is_run_state_even_without_a_dated_name() {
    // The name holds no date and no log word, so question 1 does not fire
    // and question 2 has to carry it alone: the narration is not imperative
    // and not normative, so it reaches no rule line.
    let r = read("notes/after-the-rebase.md", LOG);
    assert_eq!(r.verdict, Verdict::RunState, "{r:?}");
    assert_eq!(r.rules, 0, "its lines are narration, not rules: {r:?}");
    assert_eq!(r.why, "it states no rule");
}

#[test]
fn a_dated_log_is_run_state_however_many_rules_it_quotes() {
    // The whole rules note, under a dated name: question 1 wins, and the
    // reading still says how many rules it held, so nothing is hidden.
    let plain = read("refs/building.md", RULES);
    assert_eq!(plain.verdict, Verdict::Durable);
    for name in [
        "2026-11-09.md",
        "2026-11-09-1432.md",
        "journal/2026-11-09/evening.md",
        "notes/log.md",
        "worklog.md",
        "daily/second.md",
    ] {
        let r = read(name, RULES);
        assert_eq!(r.verdict, Verdict::RunState, "{name}: {r:?}");
        assert_eq!(
            r.rules, plain.rules,
            "{name} counted its rules anyway: {r:?}"
        );
        assert!(
            r.why.starts_with("a dated log:") || r.why.starts_with("a log:"),
            "{name} says why: {r:?}"
        );
    }
}

#[test]
fn a_log_word_inside_a_longer_name_is_not_a_log() {
    // Whole name parts only. Each of these is a rules note by its text, and
    // the rule must not drop it over a substring.
    for name in [
        "logging.md",
        "dialogue.md",
        "refs/catalogue/entries.md",
        "blog-style.md",
    ] {
        let r = read(name, RULES);
        assert_eq!(r.verdict, Verdict::Durable, "{name}: {r:?}");
    }
}

#[test]
fn a_note_with_one_rule_is_under_the_floor_and_says_so() {
    let r = read("notes/thin.md", THIN);
    assert_eq!(r.verdict, Verdict::RunState, "{r:?}");
    assert_eq!(r.rules, 1);
    assert!(
        r.why.contains(&MIN_RULES.to_string()),
        "the reason names the floor: {r:?}"
    );
}

#[test]
fn a_line_that_holds_for_one_task_is_no_rule_line() {
    // `card::evidence::one_task`'s own predicate, reused rather than written
    // again: a line scoped to one task, repository, branch, run or ticket.
    assert!(rule_line("Always chain the gate and the commit with `&&`."));
    assert!(!rule_line("Always run the gate twice in this repository."));
    assert!(!rule_line(
        "You must rebase this branch before you push it."
    ));
    assert!(!rule_line("Never force-push this pull request again."));
}

#[test]
fn a_line_that_names_one_run_is_no_rule_line() {
    assert!(names_a_run(
        "Run the gate on lane/b4-notes before you push."
    ));
    assert!(names_a_run("Always close #4120 when the fix lands."));
    assert!(names_a_run("Keep the brief of ses_9a1 open while it runs."));
    assert!(names_a_run(
        "Always measure at base 97310d4e before you grow it."
    ));
    assert!(names_a_run("Keep the 2026-11-09 export until the join."));
    assert!(!names_a_run(
        "Always chain the gate and the commit with `&&`."
    ));
    // A plain number is not a commit: eight digits and no letter.
    assert!(!names_a_run("Keep every note under 20000000 bytes always."));
    for line in [
        "Run the gate on lane/b4-notes before you push.",
        "Always close #4120 when the fix lands.",
    ] {
        assert!(!rule_line(line), "a run's own line is no rule: {line:?}");
    }
}

#[test]
fn a_thin_line_counts_for_nothing_however_normative_it_reads() {
    // The floor is `MIN_WORDS` whitespace-separated words, counted exactly as
    // `card::evidence` counts them, so a heading and a three-word order are
    // out and a four-word rule is in.
    assert_eq!(MIN_WORDS, 4);
    assert!(!rule_line("# Always"));
    assert!(!rule_line("- never push it"));
    assert!(!rule_line("Always run it"));
    assert!(rule_line("Always run the gate"));
    assert!(rule_line("Always run the gate before you commit."));
}

#[test]
fn a_bullet_and_a_quote_marker_do_not_hide_a_rule_line() {
    for line in [
        "- The gate runs before every commit, and you chain it with `&&`.",
        "* Every commit subject names its issue id in parentheses.",
        "> Never raise a ceiling to buy a green gate, in any file.",
        "### Every lane keeps its own target directory, never the main tree's.",
    ] {
        assert!(rule_line(line), "a marked line is still a rule: {line:?}");
    }
}

/// A "zero X" bar is met by an empty set, so the positive count is asserted
/// beside it: an empty note reads as run state AND a rules note does not.
#[test]
fn an_empty_note_is_run_state_and_a_rules_note_is_not() {
    let empty = read("notes/empty.md", "");
    assert_eq!(empty.verdict, Verdict::RunState);
    assert_eq!(empty.rules, 0);
    assert_eq!(read("refs/building.md", RULES).verdict, Verdict::Durable);
}

#[test]
fn the_words_a_refusal_prints_are_the_readings_own() {
    assert_eq!(Verdict::Durable.word(), "durable");
    assert_eq!(Verdict::RunState.word(), "run state");
    let r = read("notes/after-the-rebase.md", LOG);
    assert_eq!(r.line(), "run state (it states no rule)");
}

// ------------------------------------------------------- the gate, pure ---

use crate::config::import::NotesImportConfig;
use crate::import::episode;
use serde_json::{json, Value};

/// One note's episode, parsed as the import parses it.
fn episode_of(name: &str, text: &str, source: &str) -> episode::Episode {
    let mut v: Value = json!({
        "format": 1,
        "import_tag": "agent-notes-2026-11",
        "episode_id": format!("ep_{:064x}", 0x51a7u64),
        "source": source,
        "agent": "wren",
        "place": {"kind": "file", "name": name},
        "as_of": {"start": "2026-11-09T08:00:00Z", "end": "2026-11-09T08:00:00Z"},
        "labels": {"sensitivity": "company-confidential"},
        "summary": null,
        "messages": [{
            "idx": 0, "time": "2026-11-09T08:00:00Z", "author": "wren",
            "integrity": "operator", "text": text, "unit": "note-1",
            "sha256": "cd".repeat(32),
        }],
    });
    v["hash"] = json!(episode::hash_of(&v));
    episode::parse(&serde_json::to_string(&v).unwrap()).expect("a valid episode")
}

fn gate(enabled: bool, keep_run_state: bool) -> Gate {
    Gate::of(&NotesImportConfig {
        enabled,
        keep_run_state,
    })
}

#[test]
fn a_gate_that_names_no_config_writes_no_note() {
    // The safe state is the one nobody has to remember: every gateless call
    // site gets `Gate::off`.
    assert_eq!(Gate::off(), gate(false, false));
    let e = episode_of("refs/building.md", RULES, episode::NOTES_SOURCE);
    let why = refused(&e, &Gate::off()).expect("refused");
    assert!(why.contains("[import.notes] enabled is off"), "{why}");
}

#[test]
fn the_off_states_refusal_carries_the_reading_and_the_rules_document() {
    for (name, text, word) in [
        ("refs/building.md", RULES, "durable"),
        ("notes/after.md", LOG, "run state"),
    ] {
        let e = episode_of(name, text, episode::NOTES_SOURCE);
        let why = refused(&e, &gate(false, false)).expect("refused");
        assert!(why.contains(word), "{name}: {why}");
        assert!(why.contains(RULE_DOC), "{name}: {why}");
        assert!(why.contains("nothing was written"), "{name}: {why}");
    }
}

#[test]
fn with_the_path_on_the_rule_decides_and_only_keep_run_state_admits_a_log() {
    let rules = episode_of("refs/building.md", RULES, episode::NOTES_SOURCE);
    let log = episode_of("notes/after.md", LOG, episode::NOTES_SOURCE);
    assert_eq!(refused(&rules, &gate(true, false)), None);
    let why = refused(&log, &gate(true, false)).expect("refused");
    assert!(why.contains("keep_run_state is off"), "{why}");
    assert!(why.contains(RULE_DOC), "{why}");
    // Both on is the one state a run log enters in.
    assert_eq!(refused(&log, &gate(true, true)), None);
    assert_eq!(refused(&rules, &gate(true, true)), None);
}

#[test]
fn no_other_source_is_this_rules_business() {
    // The positive count beside the zero: a notes episode of the same text
    // IS refused in the same gate, so an empty check cannot pass this.
    for source in [
        episode::SYNC_SOURCE,
        episode::VESTIGE_SOURCE,
        "claude-code",
        "beads",
    ] {
        let e = episode_of("refs/building.md", LOG, source);
        assert_eq!(refused(&e, &Gate::off()), None, "{source}");
    }
    let note = episode_of("refs/building.md", LOG, episode::NOTES_SOURCE);
    assert!(refused(&note, &Gate::off()).is_some());
}

#[test]
fn a_note_is_read_by_its_places_name_and_its_messages_text() {
    let e = episode_of("refs/building.md", RULES, episode::NOTES_SOURCE);
    let (name, text) = note_of(&e);
    assert_eq!(name, "refs/building.md");
    assert_eq!(text.trim_end_matches('\n'), RULES.trim_end_matches('\n'));
    assert_eq!(read(&name, &text).verdict, Verdict::Durable);
    // No place name: read by the id, which states no rule, so it reads as
    // run state rather than as a rules note.
    let mut e = e;
    e.place.name = None;
    let (name, _) = note_of(&e);
    assert!(name.starts_with("ep_"), "{name}");
    assert_eq!(read(&name, "").verdict, Verdict::RunState);
}
