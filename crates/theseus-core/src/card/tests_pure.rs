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
    let got = parse(&content, &s).unwrap();
    assert_eq!(got.dropped, 3, "{got:?}");
    assert_eq!(got.statements.len(), 2);
    assert_eq!(got.statements[0].kind, StandingKind::Preference);
    assert_eq!(got.statements[0].scope, StandingScope::Everywhere);
    assert_eq!(got.statements[1].kind, StandingKind::Fact);
    assert_eq!(
        got.statements[1].scope,
        StandingScope::Surface("terminal".into())
    );
    assert!(parse(&[json!({"type": "text", "text": "hm"})], &s).is_none());
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
    let req = ask::request("m", None, &shown(&[0], &cs), &|t| {
        t.replace("opinion", "view")
    });
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
