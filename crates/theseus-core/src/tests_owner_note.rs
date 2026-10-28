//! Every message Theseus sends the owner unasked starts with a kind and a
//! title, then a hint at what triggered it (theseus-x8b9k): one fixture per
//! family, through its producer's text function. The Discord binding's own
//! families are its `tests_owner_note.rs`.

use serde_json::json;

use crate::owner_note::is_headed;

/// Self-improvement's messages.
fn self_families() -> Vec<(&'static str, String)> {
    use crate::rsi::backlog::{start, Night, Started};
    let night = Night {
        filed: vec!["tsk_ab12".into(), "tsk_cd34".into()],
        waiting: vec!["tsk_cd34".into()],
        started: vec![Started {
            item: "tsk_ef56".into(),
            title: "Turns fail with timeout".into(),
            session_id: "ses_1".into(),
            objective: String::new(),
            mint: None,
        }],
        trimmed: vec!["tsk_gh78".into()],
        ..Night::default()
    };
    let backlog = start::night_text(&night, |id| start::Item {
        title: format!("Calls from harness fail ({id})"),
        count: 3,
        since_ms: 1_790_000_000_000,
    });
    let week = crate::rsi::digest::week_post(&theseus_protocol::rsi::SelfDigestResult {
        since_ms: 1_790_000_000_000,
        until_ms: 1_790_600_000_000,
        text: "What Theseus changed about itself this week (x to y)\nNothing changed.\n".into(),
        rows: 0,
    });
    let self_notice = crate::rsi::digest::notice::notice_text(
        &[],
        &crate::rsi::digest::notice::Night {
            until_ms: 1_790_000_000_000,
            spent_usd: 1.0,
            day_usd: 30.0,
            waits: vec![
                "• graduate ext-fetch asks for network (keel): theseus self backlog show 7".into(),
            ],
            mint_items: Default::default(),
        },
    )
    .expect("a wait is something to say");
    let joined = json!({"branch": "self/2026-10-20-lsp", "item": "tsk_ab12", "what": "w", "why": "y",
        "gate": "green", "numbers": {"gate_secs": 30, "tries": 1}, "cost_usd": 0.5});
    let installed = crate::rsi::join::install::notice(&joined, None, "4a1c9e0b77aa55");
    use crate::rsi::join::joining;
    let keel = joining::keel_card("self/b", "tsk_1", "sealed", "- sealed: x\n", "1 file");
    let merge_keel = joining::merge_keel_card("self/b", "tsk_1", "/state/self/gates/b.log");
    let unsigned = joining::unsigned_card("self/b", "tsk_1");
    let conflict = joining::conflict_card("self/b", "tsk_1", "it conflicts with main in same.txt");
    let conflict_dropped =
        joining::conflict_dropped_card("self/b", "tsk_1", "it conflicts with main in same.txt");
    let rejected_twice =
        crate::rsi::pipeline::rejected_twice_text(&crate::rsi::pipeline::queues::Entry {
            item: "tsk_1".into(),
            title: "Trim the sails".into(),
            findings: vec!["a".into(), "b".into()],
            ..Default::default()
        });
    let pr_keel = crate::rsi::pr::keel_text("self/b", "tsk_1", "/state/self/reports/b.md");
    let j = crate::rsi::revert::attribute::Joined {
        merge: "4a1c9e0b77aa55".into(),
        branch: "self/b".into(),
        item: "tsk_1".into(),
        head: "bb".into(),
        at_ms: 0,
    };
    use crate::rsi::revert::doing;
    let revert_held = doing::held_text(&j, "gate", "main's gate is red");
    let reverted = doing::reverted_text(&j, "cc11", "gate", 24, "a red gate", 3, true);
    let vetoed = doing::vetoed_text(&j, "cc11");
    let narrow = crate::aws::narrow::proposal::tests::sample().card();
    let narrow_red = crate::rsi::narrow::red_card("theseus-hand", "nar_1", "s3:GetObject", 2);
    vec![
        ("self backlog night", backlog),
        ("weekly self digest", week),
        ("self notice", self_notice),
        ("self-join installed", installed),
        ("self-join keel card", keel),
        ("self-join merge keel card", merge_keel),
        ("self-join unsigned card", unsigned),
        ("self-join conflict card", conflict),
        ("self-join conflict dropped card", conflict_dropped),
        ("self-pipeline rejected twice", rejected_twice),
        ("keel stop", pr_keel),
        ("revert held", revert_held),
        ("reverted", reverted),
        ("veto done", vetoed),
        ("narrowed policy card", narrow),
        ("narrowed policy red", narrow_red),
    ]
}

/// Turn warnings, spend and AWS, Google, and personas.
fn spend_families() -> Vec<(&'static str, String)> {
    let home = crate::turn::home_size_step::warning_text("DM @owner", 40_000, 1_000_000, None);
    use crate::turn::personal_step;
    let privacy = personal_step::warning_text(
        "draw_out",
        0.91,
        crate::places::PlaceClass::Shared,
        "where does your owner live",
    );
    let privacy_held = personal_step::held_text(
        "draw_out",
        0.91,
        ("guest", "#lobby"),
        "where does your owner live",
    );
    let reached = theseus_kernel::Reached {
        day: "2026-10-22".into(),
        limit: 5_000_000,
        spent: 5_000_000,
        held: 0,
        needed: 1,
        turns_at_ms: 0,
        turns_at: "2026-10-23 00:00".into(),
        since: None,
        runaway: false,
    };
    let ceiling = crate::day_ceiling::notice_text(&reached, "turn");
    let runaway_day = crate::day_ceiling::runaway_text(&reached, "turn");
    let warning = crate::day_ceiling::warning_text(
        &theseus_kernel::Warning {
            day: "2026-10-22".into(),
            level: 1,
            limit: 5_000_000,
            step: 1,
            spent: 6_000_000,
            runaway: 0,
            turns_at: "2026-10-23 00:00".into(),
        },
        "`theseus stop`",
    );
    let aws_month = crate::aws::month_warn::warning_text("111122223333", 9_000, 8_000, 10_000);
    let hands_hour = crate::aws::hands::watch::words(&crate::aws::hands::watch::HourAlert {
        account: "111122223333",
        hour_start_ms: 0,
        micros: 3_000_000,
        line_micros: 2_000_000,
        groups: 1,
    });
    let hands_runaway = crate::aws::hands::runaway::entered(&crate::aws::hands::runaway::Runaway {
        account: "111122223333".into(),
        line: crate::aws::hands::runaway::Line::Hour,
        micros: 30_000_000,
        line_micros: 2_000_000,
        factor: 10.0,
        since_ms: 0,
        until_ms: 3_600_000,
        at_ms: 0,
    });
    let google = crate::google::tend::lapse_text("wren@example.com");
    let persona = crate::turn::persona_step::missing_text("wren", "/p/SOUL.md", "denied");
    vec![
        ("home size", home),
        ("privacy warning", privacy),
        ("privacy held", privacy_held),
        ("day ceiling", ceiling),
        ("day runaway line", runaway_day),
        ("day warning", warning),
        ("AWS month warning", aws_month),
        ("AWS hands hour alert", hands_hour),
        ("AWS hands runaway", hands_runaway),
        ("Google lapse", google),
        ("persona file missing", persona),
    ]
}

/// Initiative, the ontology, Jev, routines and watches, and the DM's cards.
fn card_families() -> Vec<(&'static str, String)> {
    let morning = crate::initiative::speak::headed("morning", "Good morning: 2 tasks ended.");
    let finding = crate::initiative::speak::headed("consolidation", "Two notes say the same.");
    let digest = crate::initiative::digest::posted_text("◆ 2 reported\n📋 task a: done");
    use crate::judge::onto::stage;
    let minted = stage::minted_text("topic", "harbour › tides", 0.9, "jdg_1");
    let onto_ask = stage::ask_text(
        "Is this harbour?",
        0.6,
        "\n`theseus ontology member ses_1 +c` files it.",
    );
    let paused = crate::judge::notice::paused_text("more than 30 notices today");
    let routine = crate::rpc::routines_watch::routine_note("✗ Tide check failed: a timeout");
    let routine_month = crate::routine::month_spent_text("Tide check", 1.0, "ab12cd");
    let watch = crate::watch::run::notice_text(
        &theseus_kernel::ProbeVerdict::Met { turn: false },
        "the deploy landed",
        "👁 the deploy landed → MERGED",
    );
    let seal = crate::rpc::thread_seal::seal_text("harbour", "harbour", "it looks embargoed");
    let link = crate::thread_link_seen::Pair {
        here: ("pipeline".into(), "datatidy pipeline".into()),
        other: ("stats".into(), "datatidy stats".into()),
    }
    .text();
    let retraction = crate::retract::card_text("the tide is at 5", "wrong date", &[]);
    let kept = crate::capture::kept_card("📌 Kept as a standing order everywhere: x");
    let asked = crate::capture::ask_card("📌 Keep as a standing order everywhere?");
    let new_persona = crate::personas::note::text("pilot", "Pilot", "the CLI", 2000);
    let moved =
        crate::personas::migrate::note_text(&["alder", "brook"], &["cinder: /p/runbook.md".into()]);
    vec![
        ("new persona card", new_persona),
        ("personas moved note", moved),
        ("morning note", morning),
        ("consolidation finding", finding),
        ("posted digest", digest),
        ("ontology mint card", minted),
        ("ontology ask card", onto_ask),
        ("Jev notices paused", paused),
        ("routine failed", routine),
        ("routine month spent", routine_month),
        ("watch notice", watch),
        ("seal card", seal),
        ("link card", link),
        ("retraction card", retraction),
        ("standing kept card", kept),
        ("standing ask card", asked),
    ]
}

fn families() -> Vec<(&'static str, String)> {
    let mut all = self_families();
    all.extend(spend_families());
    all.extend(card_families());
    all
}

/// The key test: each in-scope family's first line is a kind tag and a
/// title, and its second a trigger hint; a family that lost its head is named.
#[test]
fn every_unasked_message_starts_with_a_kind_and_a_title() {
    let bad: Vec<String> = families()
        .into_iter()
        .filter(|(_, text)| !is_headed(text))
        .map(|(name, text)| format!("{name}: {text:?}"))
        .collect();
    assert!(
        bad.is_empty(),
        "families without a head:\n{}",
        bad.join("\n")
    );
}

/// The owner's example: each night's item says what it is, how often and
/// since when, and the commands come last.
#[test]
fn the_backlog_night_says_each_item_in_plain_words() {
    let text = families().remove(0).1;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "✋ Action · Self-improvement backlog: 1 waiting for your word, 2 new, 1 started, 1 dropped"
    );
    assert_eq!(
        lines[1],
        "from last night's read of the day's failures and slow turns"
    );
    let day = crate::owner_note::day(1_790_000_000_000);
    assert_eq!(
        lines[2],
        format!("- started work on: Calls from harness fail (tsk_ef56): seen 3 times since {day} (item tsk_ef56)")
    );
    assert!(
        lines[4].starts_with("- new, waits for your word (keel): "),
        "{text}"
    );
    assert!(lines
        .last()
        .unwrap()
        .starts_with("Reorder or drop with `theseus self move"));
}

/// A night that only kept candidates out (the owner's drops, or an item whose
/// build was never judged) still tells the owner, and names the way back.
#[test]
fn a_night_that_only_kept_things_out_says_so_and_names_the_way_back() {
    let night = crate::rsi::backlog::Night {
        kept_out: 2,
        ..Default::default()
    };
    assert!(!night.said_nothing(), "a keep-out-only night is not silent");
    let text =
        crate::rsi::backlog::start::night_text(&night, |id| crate::rsi::backlog::start::Item {
            title: id.to_string(),
            count: 0,
            since_ms: 0,
        });
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0], "ℹ️ FYI · Self-improvement backlog: 2 kept out",
        "{text}"
    );
    assert!(text.contains("2 kept out"), "{text}");
    assert!(text.contains("`theseus self backlog --all`"), "{text}");
    assert!(text.contains("`theseus self retry <item>`"), "{text}");
    assert!(
        lines.last().unwrap().starts_with("Reorder or drop with"),
        "the commands stay last: {text}"
    );
}

/// The kept-out line is for a night that kept something out: the default
/// night's words (what the pipeline's and the join's suites compare against)
/// do not carry it.
#[test]
fn a_night_that_kept_nothing_out_says_nothing_of_it() {
    let night = crate::rsi::backlog::Night {
        filed: vec!["tsk_a".into()],
        ..Default::default()
    };
    assert!(!night.said_nothing());
    let text =
        crate::rsi::backlog::start::night_text(&night, |id| crate::rsi::backlog::start::Item {
            title: id.to_string(),
            count: 0,
            since_ms: 0,
        });
    assert!(!text.contains("kept out"), "{text}");
    assert!(!text.contains("self retry"), "{text}");
}
