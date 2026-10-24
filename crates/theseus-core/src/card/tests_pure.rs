//! The card's pure parts (theseus-slvl.6): which episodes are evidence, how a
//! text is split into lines, how lines cluster and rank, which clusters are
//! fresh, and how an answer is read back. No store and no model.

use serde_json::json;
use theseus_protocol::{card_options, card_question, StandingKind, StandingScope};

use super::ask::{self, parse, shown};
use super::cluster::{cluster, fresh, similar};
use super::evidence::{classify, lines_of, Episode, Kind, Line};
use crate::import::{AsOf, EpisodeLabels, EpisodePlace, EpisodeTriage, ImportedFrom};

fn imported(kind: &str, name: Option<&str>, file: &str) -> ImportedFrom {
    ImportedFrom {
        tag: "t".into(),
        episode_id: "ep_x".into(),
        hash: "h".into(),
        source: "openclaw-store".into(),
        agent: None,
        place: EpisodePlace {
            kind: kind.into(),
            name: name.map(String::from),
            id: None,
        },
        labels: EpisodeLabels {
            sensitivity: "personal".into(),
            partner: None,
            topic: vec![],
            book_hint: None,
            credential_redacted: false,
        },
        triage: None,
        as_of: AsOf {
            start_ms: 1,
            end_ms: 2,
        },
        messages: 1,
        summary: false,
        file: file.into(),
        line: 1,
        imported_at_ms: 3,
        erased: None,
    }
}

#[test]
fn episodes_are_evidence_by_their_labels_alone() {
    assert_eq!(
        classify(&imported("file", Some("USER.md"), "x.jsonl")),
        Some(Kind::User)
    );
    assert_eq!(
        classify(&imported("file", Some("user-notes.md"), "x.jsonl")),
        Some(Kind::User)
    );
    assert_eq!(
        classify(&imported("file", Some("feedback_tests.md"), "x.jsonl")),
        Some(Kind::Feedback)
    );
    assert_eq!(
        classify(&imported("dm", None, "memory/feedback/a.md")),
        Some(Kind::Feedback)
    );
    let mut sop = imported("file", Some("deploys.md"), "x.jsonl");
    sop.labels.book_hint = Some("sop".into());
    assert_eq!(classify(&sop), Some(Kind::Sop));
    let mut dec = imported("dm", None, "x.jsonl");
    dec.triage = Some(EpisodeTriage {
        category: "decision_or_preference".into(),
        keep: 0.9,
        model: "m".into(),
    });
    assert_eq!(classify(&dec), Some(Kind::Decision));
    dec.triage.as_mut().unwrap().category = "chatter".into();
    assert_eq!(classify(&dec), None);
    assert_eq!(classify(&imported("dm", Some("users"), "x.jsonl")), None);
}

#[test]
fn a_note_splits_into_its_lines_and_a_short_one_is_dropped() {
    let text = "# Feedback\n\n- Never commit before the whole gate is green\n  on the branch\n- ok\n\n1. Run the gate again after a rebase\n\nSome plain paragraph that wraps\nacross two lines of text.";
    let got = lines_of(text);
    assert_eq!(
        got,
        vec![
            "Never commit before the whole gate is green on the branch".to_string(),
            "Run the gate again after a rebase".to_string(),
            "Some plain paragraph that wraps across two lines of text.".to_string(),
        ]
    );
}

fn ep(id: &str, at: u64, lines: &[&str]) -> Episode {
    Episode {
        id: id.into(),
        session_id: format!("ses_{id}"),
        kind: Kind::Decision,
        end_ms: at,
        anchor: format!("{id}_0"),
        lines: lines
            .iter()
            .map(|t| Line {
                text: (*t).into(),
                node: format!("{id}_0"),
                at_ms: at,
            })
            .collect(),
    }
}

#[test]
fn restatements_cluster_and_support_ranks_before_recency() {
    let eps = vec![
        ep(
            "a",
            10,
            &["wants your opinion when you have one, not a menu of options"],
        ),
        ep(
            "b",
            20,
            &["give me your opinion instead of a menu of options"],
        ),
        ep("c", 30, &["keeps every commit behind a green gate"]),
        ep("d", 40, &["I want your opinion, not a menu of options"]),
    ];
    let cs = cluster(&eps);
    assert_eq!(cs.len(), 2, "{cs:?}");
    // The opinion cluster has three sources and ranks first, though the
    // gate line is newer than two of them.
    assert_eq!(cs[0].episodes.len(), 3);
    assert_eq!(cs[1].episodes.len(), 1);
    assert_eq!(cs[0].newest_ms, 40);
}

#[test]
fn a_cluster_is_fresh_while_most_of_its_episodes_are_new() {
    let eps = vec![
        ep(
            "a",
            1,
            &["wants your opinion when you have one, not a menu of options"],
        ),
        ep(
            "b",
            2,
            &["give me your opinion instead of a menu of options"],
        ),
        ep("c", 3, &["I want your opinion, not a menu of options"]),
    ];
    let c = &cluster(&eps)[0];
    let mut seen = std::collections::HashSet::new();
    assert!(fresh(c, &eps, &seen));
    seen.insert("a".to_string());
    assert!(fresh(c, &eps, &seen), "one of three seen: still mostly new");
    seen.insert("b".to_string());
    assert!(!fresh(c, &eps, &seen), "two of three seen: not fresh");
}

#[test]
fn similar_is_a_word_overlap_not_a_substring() {
    assert!(similar(
        "wants your opinion when you have one",
        "wants an opinion when you have one, not a menu of options"
    ));
    assert!(!similar("wants short answers", "keeps the build green"));
}

fn answer(cards: serde_json::Value) -> Vec<serde_json::Value> {
    vec![json!({"type": "tool_use", "id": "tu", "name": ask::TOOL, "input": {"cards": cards}})]
}

#[test]
fn a_statement_is_kept_only_when_it_cites_its_clusters_episodes() {
    let eps = vec![
        ep(
            "a",
            1,
            &["wants your opinion when you have one, not a menu of options"],
        ),
        ep(
            "b",
            2,
            &["give me your opinion instead of a menu of options"],
        ),
        ep(
            "c",
            3,
            &["keeps every commit behind a green gate before pushing"],
        ),
    ];
    let cs = cluster(&eps);
    let picked: Vec<usize> = (0..cs.len()).collect();
    let s = shown(&picked, &cs);
    assert_eq!(s[0].label, "K1");
    let e1 = s[0].episodes[0].0.clone();
    let e_other = s[1].episodes[0].0.clone();
    let content = answer(json!([
        {"cluster": "K1", "text": "wants an opinion, not a menu", "kind": "preference", "scope": "everywhere", "episodes": [e1]},
        {"cluster": "K2", "text": "cites an episode of another cluster", "kind": "fact", "scope": "everywhere", "episodes": [s[0].episodes[0].0.clone()]},
        {"cluster": "K9", "text": "no such cluster", "kind": "fact", "scope": "everywhere", "episodes": ["E1"]},
        {"cluster": "K2", "text": "", "kind": "fact", "scope": "everywhere", "episodes": [e_other]},
        {"cluster": "K2", "text": "gate green before a commit", "kind": "fact", "scope": "terminal", "episodes": [e_other]},
    ]));
    let got = parse(&content, &s, 0).unwrap();
    assert_eq!(got.dropped, 3, "{got:?}");
    assert_eq!(got.statements.len(), 2);
    assert_eq!(got.statements[0].kind, StandingKind::Preference);
    assert_eq!(got.statements[0].scope, StandingScope::Everywhere);
    assert_eq!(got.statements[1].kind, StandingKind::Fact);
    assert_eq!(
        got.statements[1].scope,
        StandingScope::Surface("terminal".into())
    );
    assert!(parse(&[json!({"type": "text", "text": "hm"})], &s, 0).is_none());
}

#[test]
fn a_pasted_line_is_not_a_statement() {
    let line =
        "keeps every commit behind a green gate before pushing anything to the shared branch";
    let eps = vec![ep("a", 1, &[line])];
    let cs = cluster(&eps);
    let s = shown(&[0], &cs);
    let e = s[0].episodes[0].0.clone();
    let got = parse(
        &answer(json!([{"cluster": "K1", "text": line, "kind": "fact", "scope": "everywhere", "episodes": [e]}])),
        &s,
        0,
    )
    .unwrap();
    assert_eq!((got.statements.len(), got.dropped), (0, 1));
}

#[test]
fn the_question_says_what_the_owner_is_asked() {
    assert_eq!(
        card_question("wants your opinion when you have one", &StandingScope::Everywhere, 14),
        "Add to your card: 'wants your opinion when you have one' (from 14 sources)? [Keep] [Edit] [Not this]"
    );
    assert!(
        card_question("x", &StandingScope::Surface("voice".into()), 1).contains("(surface:voice)")
    );
    assert!(card_question("x", &StandingScope::Everywhere, 1).contains("from 1 source)"));
    assert_eq!(card_options(), ["Keep", "Edit", "Not this"]);
}

#[test]
fn the_request_names_no_one_and_carries_the_tool() {
    let eps = vec![ep(
        "a",
        1,
        &["wants your opinion when you have one, not a menu of options"],
    )];
    let cs = cluster(&eps);
    let req = ask::request(
        "m",
        crate::catalog::Catalog::builtin().forcing("m"),
        &shown(&[0], &cs),
        &[],
        &|t| t.replace("opinion", "view"),
    );
    assert_eq!(req.tools.len(), 1);
    let body = req.messages[0]["content"][0]["text"].as_str().unwrap();
    assert!(
        body.contains("K1:") && body.contains("E1") && body.contains("view"),
        "{body}"
    );
    assert!(
        !body.contains("opinion"),
        "the scrub runs over every line sent"
    );
}

/// The committed fixture the daemon's test imports is the builder's export
/// (`THESEUS_GOLDEN=write` rewrites it), and every line is a valid episode.
#[test]
fn the_committed_fixture_is_the_builders_export() {
    let want = super::fixture::export_text(super::fixture::BASE_MS);
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/card/archive.jsonl");
    if std::env::var_os("THESEUS_GOLDEN").is_some_and(|v| v == "write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
    }
    let got = std::fs::read_to_string(&path).expect("tests/fixtures/card/archive.jsonl");
    assert!(
        got == want,
        "the fixture is stale: THESEUS_GOLDEN=write rewrites it"
    );
    for line in want.lines() {
        crate::import::episode::parse(line).expect("every fixture line is a valid episode");
    }
}

/// What an assistant's setup is, by a line's words and a file's name; what
/// the owner's lines are; and the count of what is dropped (theseus-ly75u).
#[test]
fn an_assistants_setup_is_no_evidence_and_the_owners_lines_stay() {
    use super::evidence::{about_owner, addressed, kept, setup_file, Scan};
    for l in [
        "You are the lantern keeper's helper and you answer in a calm voice",
        "Your first job is to greet the harbour master",
        "Read this file first at every session start",
        "Keep the daily log under the notes folder",
    ] {
        assert!(addressed(l), "{l}");
    }
    for l in [
        "Never commit before the whole gate is green on the branch",
        "Prefers short answers in the terminal and longer ones in documents",
        "When you have an opinion on the mooring layout, give me your opinion",
    ] {
        assert!(!addressed(l), "{l}");
    }
    assert!(setup_file("memory.md") && setup_file("2026-10-01.md") && setup_file("heartbeat"));
    assert!(!setup_file("user-harbour.md") && !setup_file("feedback_gate.md"));
    for l in [
        "Prefers tide tables in metric units",
        "Lives in Mountain time",
        "The owner keeps a boat at the north quay",
        "He works best in the mornings",
    ] {
        assert!(about_owner(l), "{l}");
    }
    for l in [
        "Always answer in a calm voice",
        "Be warm but brief",
        "You keep the ledger",
    ] {
        assert!(!about_owner(l), "{l}");
    }
    let mut scan = Scan::default();
    // A note: the assistant's lines go, the owner's order stays.
    assert!(!kept("You keep the harbour ledger", true, false, &mut scan));
    assert!(kept(
        "Never commit before the gate is green",
        true,
        false,
        &mut scan
    ));
    // A setup file keeps only the owner's lines.
    assert!(!kept(
        "Be warm but brief with everyone",
        true,
        true,
        &mut scan
    ));
    assert!(kept(
        "Prefers tide tables in metric units",
        true,
        true,
        &mut scan
    ));
    // A conversation's "you" is the owner speaking to his assistant.
    assert!(kept(
        "You should give me your opinion first",
        false,
        false,
        &mut scan
    ));
    // One task's line, anywhere.
    assert!(!kept(
        "No builds in this task, the runner is busy",
        false,
        false,
        &mut scan
    ));
    assert_eq!((scan.setup, scan.one_task), (2, 1));
}

#[test]
fn one_tasks_words_are_found_and_a_lasting_line_is_not() {
    use super::evidence::one_task;
    for l in [
        "No builds in this task",
        "For this repo, merge lanes with a plain merge",
        "Keep this branch rebased on main",
        "In this run, skip the benches",
    ] {
        assert!(one_task(l), "{l}");
    }
    for l in [
        "Read this runbook before a release",
        "Wants the gate green before any commit",
        "Prefers this terminal's colours dark",
    ] {
        assert!(!one_task(l), "{l}");
    }
}

/// The instructions carry the who-and-on-whose-behalf rule, the specific
/// wording rule, and the setup rule; the tool offers `one_task` and
/// `conflicts`.
#[test]
fn the_instructions_keep_who_acts_and_on_whose_behalf() {
    let i = ask::INSTRUCTIONS;
    assert!(
        i.contains("Keep who acts and on whose behalf exactly as the lines say"),
        "{i}"
    );
    assert!(i.contains("not to speak on the owner's behalf"), "{i}");
    assert!(i.contains("in words a later turn could act on"), "{i}");
    assert!(i.contains("an assistant's instructions to itself"), "{i}");
    let t = ask::tool();
    let item = &t["input_schema"]["properties"]["cards"]["items"]["properties"];
    assert!(item["scope"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!(ask::ONE_TASK)));
    assert_eq!(item["conflicts"]["type"], "array");
}

/// A statement that turns "do not speak on the owner's behalf" into "do not
/// speak as yourself" is dropped; one that keeps it stays; a thin one is
/// dropped; a `one_task` one is counted and not kept.
#[test]
fn a_turned_thin_or_one_task_statement_is_not_kept() {
    let eps = vec![
        ep(
            "a",
            1,
            &["Do not speak on the owner's behalf in the harbour group chat"],
        ),
        ep(
            "b",
            2,
            &["Have a real personality when answering questions"],
        ),
        ep("c", 3, &["For this repo, merge lanes with a plain merge"]),
    ];
    let cs = cluster(&eps);
    let picked: Vec<usize> = (0..cs.len()).collect();
    let s = shown(&picked, &cs);
    let label_of = |id: &str| {
        let i = eps.iter().position(|e| e.id == id).unwrap();
        let k = s
            .iter()
            .find(|x| cs[x.cluster].episodes.contains(&i))
            .unwrap();
        (k.label.clone(), k.episodes[0].0.clone())
    };
    let (ka, ea) = label_of("a");
    let (kb, eb) = label_of("b");
    let (kc, ec) = label_of("c");
    let turned = answer(json!([
        {"cluster": ka, "text": "does not speak as itself in group chats", "kind": "preference", "scope": "everywhere", "episodes": [ea]},
        {"cluster": kb, "text": "Have a personality", "kind": "preference", "scope": "everywhere", "episodes": [eb]},
        {"cluster": kc, "text": "merges lanes with a plain merge", "kind": "preference", "scope": "one_task", "episodes": [ec]},
    ]));
    let got = parse(&turned, &s, 0).unwrap();
    assert_eq!(
        (got.statements.len(), got.dropped, got.one_task),
        (0, 2, 1),
        "{got:?}"
    );
    let kept = answer(json!([
        {"cluster": ka, "text": "never speaks on the owner's behalf in group chats", "kind": "preference", "scope": "everywhere", "episodes": [ea]},
    ]));
    let got = parse(&kept, &s, 0).unwrap();
    assert_eq!(got.statements.len(), 1, "{got:?}");
}

/// Of two statements that conflict, the better supported stays and the
/// other is noted; statements that conflict with none all stay.
#[test]
fn of_two_conflicting_statements_the_better_supported_stays() {
    use super::run::settle_conflicts;
    let eps = vec![
        ep("a", 1, &["ask before acting on anything in the harbour"]),
        ep("b", 2, &["please ask before acting on the harbour work"]),
        ep("c", 3, &["go ahead and act without waiting for a yes"]),
        ep("d", 4, &["keeps tide tables in metric units always"]),
    ];
    let cs = cluster(&eps);
    assert_eq!(cs[0].episodes.len(), 2, "{cs:?}");
    let st = |c: usize, text: &str, conflicts: Vec<usize>| ask::Statement {
        cluster: c,
        text: text.into(),
        kind: StandingKind::Preference,
        scope: StandingScope::Everywhere,
        conflicts,
    };
    let c_act = (0..cs.len()).find(|&i| cs[i].episodes == vec![2]).unwrap();
    let c_metric = (0..cs.len()).find(|&i| cs[i].episodes == vec![3]).unwrap();
    // The weaker names the stronger: the stronger stays.
    let (kept, notes) = settle_conflicts(
        &[
            st(c_act, "acts without asking", vec![0]),
            st(0, "asks before acting", vec![]),
            st(c_metric, "keeps tide tables metric", vec![]),
        ],
        &cs,
    );
    let texts: Vec<&str> = kept.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["asks before acting", "keeps tide tables metric"]);
    assert_eq!(
        notes,
        ["kept 'asks before acting' over 'acts without asking': they conflict"]
    );
}

/// A setup line that starts with the owner's name is the owner's; an order
/// with a plural noun is not (theseus-0ly2a).
#[test]
fn a_setup_line_that_starts_with_the_owners_name_is_kept() {
    use super::evidence::about_owner;
    for l in [
        "Mara prefers tide tables in metric units",
        "Mara is vegetarian",
    ] {
        assert!(about_owner(l), "{l}");
    }
    for l in [
        "Keep notes short",
        "Check logs every morning",
        "Always answer in a calm voice",
        "Be warm but brief",
        "You keep the ledger",
    ] {
        assert!(!about_owner(l), "{l}");
    }
}

/// A note's line naming a wake update or a heartbeat is the owner's, unless
/// it is addressed to its reader (theseus-0ly2a).
#[test]
fn a_notes_wake_update_and_heartbeat_line_is_the_owners() {
    use super::evidence::addressed;
    for l in [
        "Post the wake update before nine",
        "Heartbeat agents must claim before working",
    ] {
        assert!(!addressed(l), "{l}");
    }
    for l in [
        "Read this file first at every session start",
        "Keep the daily log under the notes folder",
        "On each heartbeat you check the tide board",
    ] {
        assert!(addressed(l), "{l}");
    }
}

/// A statement is turned only by the lines of the episodes it cites
/// (theseus-0ly2a).
#[test]
fn a_statement_on_a_clusters_other_lines_is_not_turned() {
    let line = |e: &str, t: &str| (e.to_string(), t.to_string());
    let s = vec![ask::Shown {
        label: "K1".into(),
        cluster: 0,
        episodes: vec![("E1".into(), 0), ("E2".into(), 1), ("E3".into(), 2)],
        lines: vec![
            line(
                "E1",
                "Do not speak on the owner's behalf in the harbour group chat",
            ),
            line(
                "E2",
                "Do not post in the harbour group chat before the tide bell",
            ),
            line(
                "E3",
                "Never post in the harbour group chat during the lantern round",
            ),
        ],
    }];
    let (behalf, bell, round) = ("E1", "E2", "E3");
    let k = &s[0].label;
    let others = answer(json!([
        {"cluster": k, "text": "never posts in the harbour group chat around the tide bell", "kind": "preference", "scope": "everywhere", "episodes": [bell, round]},
    ]));
    let got = parse(&others, &s, 0).unwrap();
    assert_eq!((got.statements.len(), got.dropped), (1, 0), "{got:?}");
    let turned = answer(json!([
        {"cluster": k, "text": "never posts in the harbour group chat", "kind": "preference", "scope": "everywhere", "episodes": [behalf]},
    ]));
    let got = parse(&turned, &s, 0).unwrap();
    assert_eq!((got.statements.len(), got.dropped), (0, 1), "{got:?}");
}

/// The card call forces its tool where thinking is off, and asks `auto` with
/// a strict, closed tool where it cannot be (theseus-yym79). (Planted
/// revert: the helper always forces.)
#[test]
fn the_card_request_forces_its_tool_only_where_thinking_is_off() {
    let eps = vec![ep("a", 1, &["wants your opinion when you have one"])];
    let cs = cluster(&eps);
    crate::provider::one_tool::assert_both_shapes(
        |how| ask::request("m", how, &shown(&[0], &cs), &[], &|t| t.to_string()),
        ask::TOOL,
    );
}
