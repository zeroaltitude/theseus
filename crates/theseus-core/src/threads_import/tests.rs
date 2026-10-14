//! The thread import's pure parts (theseus-71ib): the working set the rules
//! find, the candidates (the window, a DM cluster's minimum, a persona only
//! when configured, partner text kept out), and the card (a line must cite,
//! must not paste, and carries the handles recall uses). The whole call
//! through the core is `rpc/tests_thread_import.rs`.

use std::collections::BTreeSet;

use serde_json::json;

use super::candidates::{candidates, slugify, Opts, Plan};
use super::card;
use super::extract::working_set;
use super::fixture::{self, SECRET, SECRET_REPO};
use super::{Msg, Seen, Summary};

const NOW: u64 = fixture::BASE_MS;
const DAY: u64 = 86_400_000;

fn ws(texts: &[&str]) -> theseus_protocol::threads::WorkingSet {
    working_set(texts.iter().copied())
}

#[test]
fn the_working_set_is_what_the_text_names_by_rule() {
    let w = ws(&[
        "See https://github.com/reef-lab/gauge-log/pull/12 and #12, tracked as theseus-ab12.",
        "I pushed lane/gauge-sync; also `git checkout -b fix/drift` and branch hotfix-7.",
        "Edit src/gauge.rs, notes/drift.md and Cargo.toml, then ask https://tides.example.org/api/x.",
        "clone reef-lab/other-repo, plus git@github.com:reef-lab/third.git and -R reef-lab/fourth",
        "Issue #14 too, and #12 again; version v1.2 and e.g. nothing.",
    ]);
    assert_eq!(
        w.repos,
        [
            "reef-lab/fourth",
            "reef-lab/gauge-log",
            "reef-lab/other-repo",
            "reef-lab/third"
        ],
        "{w:?}"
    );
    assert_eq!(w.refs[0], "#12", "the most named first: {:?}", w.refs);
    for r in ["#14", "reef-lab/gauge-log#12", "theseus-ab12"] {
        assert!(w.refs.iter().any(|x| x == r), "{r} in {:?}", w.refs);
    }
    for b in ["lane/gauge-sync", "fix/drift"] {
        assert!(w.branches.iter().any(|x| x == b), "{b} in {:?}", w.branches);
    }
    for f in ["src/gauge.rs", "notes/drift.md", "Cargo.toml"] {
        assert!(w.files.iter().any(|x| x == f), "{f} in {:?}", w.files);
    }
    assert_eq!(
        w.hosts,
        ["tides.example.org"],
        "github is a repo, not a host"
    );
    assert!(
        !w.files.iter().any(|f| f.contains("v1.2") || f == "e.g"),
        "{:?}",
        w.files
    );
    assert!(w.roots.is_empty());
    assert!(ws(&["nothing to see here", "plain words only."]).is_empty());
}

#[test]
fn a_working_set_keeps_its_most_named_entries_in_a_fixed_order() {
    let text: String = (1..=20).map(|n| format!("#{n} ")).collect::<String>() + "#20 #20 #7";
    let w = ws(&[&text]);
    assert_eq!(w.refs.len(), super::extract::KEEP);
    assert_eq!(&w.refs[..2], ["#20", "#7"], "{:?}", w.refs);
    assert_eq!(w, ws(&[&text]), "the same text, the same set");
}

fn msg(node: &str, days_ago: u64, text: &str) -> Msg {
    Msg {
        node: node.into(),
        at_ms: NOW - days_ago * DAY,
        text: text.into(),
    }
}

fn seen(id: &str, kind: &str, name: Option<&str>, topics: &[&str], owner: Vec<Msg>) -> Seen {
    Seen {
        session_id: id.into(),
        agent: Some("wren".into()),
        place_kind: kind.into(),
        place_name: name.map(str::to_string),
        place_id: name.map(|n| format!("id-{n}")),
        topics: topics.iter().map(|t| t.to_string()).collect(),
        sensitivity: "personal".into(),
        partner: false,
        end_ms: owner.iter().map(|m| m.at_ms).max().unwrap_or(0),
        owner,
        summary: None,
    }
}

fn plan(seen: &[Seen], personas: &[&str], existing: &[&str]) -> Plan {
    let personas: BTreeSet<String> = personas.iter().map(|p| p.to_string()).collect();
    let existing: BTreeSet<String> = existing.iter().map(|p| p.to_string()).collect();
    candidates(
        seen,
        &Opts {
            now_ms: NOW,
            since_days: 30,
            min_cluster: 8,
            personas: &personas,
            existing: &existing,
        },
    )
}

#[test]
fn a_channel_is_a_candidate_only_for_owner_messages_in_the_window() {
    let old = seen(
        "a",
        "slack-channel",
        Some("#old"),
        &[],
        vec![msg("n1", 40, "long ago")],
    );
    let mixed = seen(
        "b",
        "slack-channel",
        Some("#mixed"),
        &[],
        vec![
            msg("n2", 40, "before"),
            msg("n3", 2, "inside"),
            msg("n4", 1, "inside too"),
        ],
    );
    let p = plan(&[old, mixed], &[], &[]);
    assert_eq!(p.drafts.len(), 1);
    let c = &p.drafts[0].cand;
    assert_eq!(
        (c.slug.as_str(), c.owner_messages, c.n),
        ("mixed", 2, 1),
        "{c:?}"
    );
    assert_eq!(c.last_active_ms, NOW - DAY);
    assert_eq!(
        p.drafts[0].inputs[0].owner.len(),
        2,
        "the card reads the window's lines only"
    );
    assert_eq!(p.notes, ["1 streams with no owner message in the window"]);
}

#[test]
fn a_dm_cluster_needs_its_minimum_of_owner_messages_across_episodes() {
    let line = |n: u64| msg(&format!("n{n}"), 3, "about the moorings");
    let ep = |id: &str, from: u64, count: u64, topic: &str| {
        seen(
            id,
            "dm",
            Some("sable"),
            &[topic],
            (from..from + count).map(line).collect(),
        )
    };
    let seven = [
        ep("a", 0, 4, "harbor/moorings"),
        ep("b", 10, 3, "harbor/moorings/winter"),
    ];
    let p = plan(&seven, &[], &[]);
    assert!(p.drafts.is_empty(), "7 < 8: {:?}", p.drafts);
    assert_eq!(p.notes, ["1 DM topic clusters under the minimum"]);
    let eight = [
        ep("a", 0, 4, "harbor/moorings"),
        ep("b", 10, 3, "Harbor/Moorings/winter"),
        ep("c", 20, 1, "harbor/moorings"),
        ep("d", 30, 5, "garden/irrigation"),
    ];
    let p = plan(&eight, &[], &[]);
    assert_eq!(p.drafts.len(), 1, "{:?}", p.drafts);
    let c = &p.drafts[0].cand;
    assert_eq!(
        (
            c.slug.as_str(),
            c.kind.as_str(),
            c.owner_messages,
            c.episodes
        ),
        ("dm-harbor-moorings", "dm", 8, 3)
    );
    assert_eq!(c.title, "DMs: moorings");
    assert_eq!(
        c.topics.len(),
        2,
        "both labels, at most three: {:?}",
        c.topics
    );
    let bare = [seen(
        "e",
        "dm",
        Some("sable"),
        &[],
        vec![msg("x", 1, "no label")],
    )];
    assert_eq!(
        plan(&bare, &[], &[]).notes,
        ["1 DM episodes with no topic label, in no cluster"]
    );
}

#[test]
fn a_persona_is_named_only_when_it_is_configured() {
    let ch = [seen(
        "a",
        "discord-channel",
        Some("#reef"),
        &[],
        vec![msg("n", 1, "x")],
    )];
    assert_eq!(plan(&ch, &[], &[]).drafts[0].cand.persona, None);
    assert_eq!(plan(&ch, &["gull"], &[]).drafts[0].cand.persona, None);
    assert_eq!(
        plan(&ch, &["gull", "Wren"], &[]).drafts[0]
            .cand
            .persona
            .as_deref(),
        Some("Wren")
    );
}

#[test]
fn a_partner_episode_counts_but_gives_no_text() {
    let mut deal = seen(
        "p",
        "discord-channel",
        Some("#reef"),
        &["deals/kestrel"],
        vec![msg(
            "p1",
            1,
            &format!("{SECRET} see https://github.com/{SECRET_REPO}"),
        )],
    );
    deal.sensitivity = "partner-confidential".into();
    deal.partner = true;
    deal.summary = Some(Summary {
        node: "ps".into(),
        text: SECRET.into(),
    });
    let mut open = seen(
        "o",
        "discord-channel",
        Some("#reef"),
        &["projects/reef"],
        vec![msg("o1", 2, "plain talk")],
    );
    open.place_id = deal.place_id.clone();
    let p = plan(&[deal.clone(), open], &[], &[]);
    let d = &p.drafts[0];
    assert_eq!(d.cand.owner_messages, 2, "it counts");
    assert_eq!(d.cand.sensitivity["partner-confidential"], 1);
    assert_eq!(d.cand.topics, ["projects/reef"], "its label names the deal");
    assert!(d.cand.working_set.is_empty(), "{:?}", d.cand.working_set);
    assert_eq!(d.inputs.len(), 1);
    assert_eq!(d.inputs[0].session_id, "o");
    // Alone, the stream is a candidate with a title that names nothing.
    let only = plan(&[deal], &[], &[]);
    let c = &only.drafts[0].cand;
    assert_eq!(c.title, "confidential discord-channel conversation");
    assert!(c.slug.starts_with("confidential-") && c.topics.is_empty());
    assert!(only.drafts[0].inputs.is_empty());
    assert!(!format!("{c:?}").contains("kestrel"), "{c:?}");
}

#[test]
fn slugs_are_unique_numbered_in_order_and_existing_ones_flagged() {
    let a = seen(
        "a",
        "slack-channel",
        Some("#Reef Ops"),
        &[],
        vec![msg("n1", 1, "x")],
    );
    let mut b = seen(
        "b",
        "discord-channel",
        Some("reef ops"),
        &[],
        vec![msg("n2", 2, "x")],
    );
    b.place_id = Some("other".into());
    let c = seen(
        "c",
        "slack-channel",
        Some("#harbor"),
        &[],
        vec![msg("n3", 3, "x")],
    );
    let p = plan(&[a, b, c], &[], &["harbor"]);
    let got: Vec<(u32, &str, bool)> = p
        .drafts
        .iter()
        .map(|d| (d.cand.n, d.cand.slug.as_str(), d.cand.exists))
        .collect();
    assert_eq!(
        got,
        [
            (1, "reef-ops", false),
            (2, "reef-ops-2", false),
            (3, "harbor", true)
        ]
    );
    assert_eq!(slugify("  The #Tide / Gauges!! "), "the-tide-gauges");
    assert_eq!(slugify("###"), "stream");
    assert!(slugify(&"x".repeat(80)).len() <= 36);
}

fn episodes() -> Vec<card::Episode> {
    let input = |id: &str, node: &str, summary: Option<&str>, owner: &[(&str, &str)]| {
        super::candidates::Input {
            session_id: id.into(),
            end_ms: NOW,
            summary: summary.map(|t| Summary {
                node: format!("{node}_summary"),
                text: t.into(),
            }),
            owner: owner
                .iter()
                .map(|(n, t)| Msg {
                    node: (*n).into(),
                    at_ms: NOW,
                    text: (*t).into(),
                })
                .collect(),
        }
    };
    card::episodes(&[
        input("s1", "imp_aa", Some("The gauge sync moved to its own lane."), &[("imp_aa_0", "Gauge 4 drifts after dusk and I want the drift fix in before the next survey run starts.")]),
        input("s2", "imp_bb", None, &[("imp_bb_0", "first"), ("imp_bb_1", "second"), ("imp_bb_2", "third"), ("imp_bb_3", "fourth")]),
    ])
}

fn answer(lines: serde_json::Value) -> Vec<serde_json::Value> {
    vec![json!({"type": "tool_use", "id": "t", "name": card::TOOL, "input": {"lines": lines}})]
}

#[test]
fn an_episode_shows_its_summary_or_its_newest_owner_lines_and_is_cited_by_one_node() {
    let eps = episodes();
    assert_eq!(
        (eps[0].label.as_str(), eps[0].anchor.as_str()),
        ("E1", "imp_aa_summary")
    );
    assert_eq!(eps[0].shown, ["The gauge sync moved to its own lane."]);
    assert_eq!(
        (eps[1].label.as_str(), eps[1].anchor.as_str()),
        ("E2", "imp_bb_3")
    );
    assert_eq!(
        eps[1].shown,
        ["second", "third", "fourth"],
        "the newest three"
    );
}

#[test]
fn a_card_line_must_cite_an_episode_the_call_was_given() {
    let eps = episodes();
    let got = card::parse(
        &answer(json!([
            {"text": "Gauge 4 drifts after dusk.", "episodes": ["E1", "E2"]},
            {"text": "No source at all.", "episodes": []},
            {"text": "A source that was never sent.", "episodes": ["E9"]},
            {"text": "  ", "episodes": ["E1"]},
            {"text": "Both again.", "episodes": ["E2", "E1", "E1"]},
        ])),
        &eps,
    )
    .unwrap();
    assert_eq!(got.dropped, 3);
    let lines: Vec<(&str, &[usize])> = got
        .lines
        .iter()
        .map(|l| (l.text.as_str(), l.cites.as_slice()))
        .collect();
    assert_eq!(
        lines,
        [
            ("Gauge 4 drifts after dusk.", &[0, 1][..]),
            ("Both again.", &[0, 1][..])
        ]
    );
    assert!(card::parse(&[json!({"type": "text", "text": "a card"})], &eps).is_none());
}

#[test]
fn a_card_line_that_repeats_a_message_is_dropped_as_history() {
    let eps = episodes();
    let long =
        "Gauge 4 drifts after dusk and I want the drift fix in before the next survey run starts.";
    let got = card::parse(
        &answer(json!([
            {"text": long, "episodes": ["E1"]},
            {"text": format!("  {}  ", long.to_uppercase()), "episodes": ["E1"]},
            {"text": "Gauge 4 drifts after dusk and I want the drift fix in before", "episodes": ["E1"]},
            {"text": "second", "episodes": ["E2"]},
            {"text": "Drift fixes are wanted before the next run.", "episodes": ["E1"]},
        ])),
        &eps,
    )
    .unwrap();
    let kept: Vec<&str> = got.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        kept,
        ["second", "Drift fixes are wanted before the next run."],
        "a short message may be said again: it is no history"
    );
    assert_eq!(got.dropped, 3);
}

#[test]
fn a_card_is_at_most_twelve_short_lines_with_four_handles_at_most() {
    let eps = episodes();
    let many: Vec<_> = (0..20)
        .map(|i| json!({"text": format!("fact {i} {}", "x".repeat(400)), "episodes": ["E1", "E2"]}))
        .collect();
    let got = card::parse(&answer(json!(many)), &eps).unwrap();
    assert_eq!(got.lines.len(), card::LINES);
    assert!(got
        .lines
        .iter()
        .all(|l| l.text.chars().count() <= card::TEXT_CHARS + 1));
    let (rendered, cited) = card::render(&got.lines, &eps);
    assert_eq!(rendered.len(), 12);
    assert!(
        rendered
            .iter()
            .all(|l| l.chars().count() <= theseus_protocol::threads::MAX_LINE_CHARS),
        "{rendered:?}"
    );
    assert_eq!(cited.len(), 2, "each episode once");
}

#[test]
fn a_handle_names_one_imported_node_across_the_store() {
    let digest = "0123456789abcdef".repeat(4);
    let (msg, sum) = (format!("imp_{digest}_3"), format!("imp_{digest}_summary"));
    let (hm, hs) = (card::handle_of(&msg), card::handle_of(&sum));
    assert!(
        hm.ends_with(&format!("{}_3", &digest[52..])) && hm.len() == 2 + 12 + 2,
        "{hm}"
    );
    assert!(
        hs.ends_with(&format!("{}_summary", &digest[52..])) && hs.len() == 2 + 12 + 8,
        "{hs}"
    );
    for h in [&hm, &hs] {
        assert!(
            theseus_protocol::cite::is_handle(h),
            "{h} reads as a handle"
        );
    }
    assert!(msg.ends_with(&hm[2..]) && sum.ends_with(&hs[2..]));
    let eps = episodes();
    let got = card::parse(
        &answer(json!([{"text": "Drift.", "episodes": ["E2", "E1"]}])),
        &eps,
    )
    .unwrap();
    let (rendered, cited) = card::render(&got.lines, &eps);
    assert_eq!(
        rendered[0],
        format!("Drift. [{}] [{}]", cited[0].handle, cited[1].handle)
    );
    assert_eq!(
        (cited[0].node.as_str(), cited[1].session_id.as_str()),
        ("imp_aa_summary", "s2")
    );
}

#[test]
fn the_request_holds_the_episodes_scrubbed_and_nothing_else() {
    let eps = episodes();
    let req = card::request("claude-haiku-5-5", "Tide gauges", &eps, &|t| {
        t.replace("Gauge", "G")
    });
    let body = req.messages[0]["content"][0]["text"].as_str().unwrap();
    assert!(body.starts_with("Thread: Tide gauges\n"), "{body}");
    assert!(
        body.contains("E1:\n- The gauge sync") && body.contains("E2:\n- second"),
        "{body}"
    );
    assert!(
        body.contains("G 4 drifts") || !body.contains("Gauge 4"),
        "scrubbed: {body}"
    );
    assert_eq!(req.tools[0]["name"], card::TOOL);
    assert_eq!(req.extra["tool_choice"]["name"], card::TOOL);
    assert!(
        req.system[0]["text"].as_str().unwrap().len() < 200,
        "one short sentence"
    );
}

#[test]
fn the_committed_fixture_is_the_builders_export() {
    let want = fixture::export_text(fixture::BASE_MS);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/threads-import/openclaw.jsonl");
    if std::env::var_os("THESEUS_GOLDEN").is_some_and(|v| v == "write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
    }
    let got = std::fs::read_to_string(&path).expect("tests/fixtures/threads-import/openclaw.jsonl");
    assert!(
        got == want,
        "the fixture is stale: THESEUS_GOLDEN=write rewrites it"
    );
    for line in want.lines() {
        crate::import::episode::parse(line).expect("every fixture line is a valid episode");
    }
}

#[test]
fn a_slug_always_fits_the_thread_records_bound() {
    let mut deal = seen(
        "p",
        "discord-channel",
        None,
        &[],
        vec![msg("p1", 1, "terms")],
    );
    deal.partner = true;
    deal.place_id = Some("1234567890123456789012345678901234567890".into());
    let long_dm = seen(
        "d",
        "dm",
        Some("sable"),
        &["a-very-long-topic-name-that-keeps-going/and-going-and-going-for-ages"],
        (0..8).map(|n| msg(&format!("n{n}"), 1, "x")).collect(),
    );
    for d in plan(&[deal, long_dm], &[], &[]).drafts {
        assert!(
            theseus_protocol::threads::slug_ok(&d.cand.slug),
            "{} is no slug",
            d.cand.slug
        );
    }
}
