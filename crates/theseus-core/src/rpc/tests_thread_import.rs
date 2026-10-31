//! `thread.import` (theseus-71ib) through the whole core, over the invented
//! export (`threads_import::fixture`): the dry run lists the streams and
//! writes nothing; the apply makes exactly the named threads, each with a
//! card whose lines cite episodes by recall's handles; partner text never
//! reaches a title, a topic, a working set, a card or the model; a spent day
//! makes a thread without a card; imported history stays testimony (no node
//! is written into any session, and no thread has a session of its own); a
//! Vestige note is imported once. Every name here is invented.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::import::{ImportEpisodesParams, ImportEpisodesResult};
use theseus_protocol::method;
use theseus_protocol::threads::ThreadListParams;
use theseus_protocol::threads_import::{ThreadImportParams, ThreadImportResult};
use theseus_store::{kinds, NewRecord};

use super::thread_import::{ImportDay, DAY_KEY};
use super::Core;
use crate::approval::{Answerer, Surface};
use crate::config::{Config, MemoryMode, PersonaConfig};
use crate::import::tests::{call, lines};
use crate::ledger::LedgerRow;
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::test_support::ledger_rows_2k as rows;
use crate::threads_import::card::TOOL;
use crate::threads_import::fixture::{self, SECRET, SECRET_REPO};

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

pub(crate) fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.memory.mode = MemoryMode::Off;
    // A pin: an unset key asks Jev, who is not here, and runs on `live`.
    cfg.threads.import_profile = Some("haiku".into());
    cfg.personas.insert(
        "wren".into(),
        PersonaConfig {
            files: vec![crate::personas::test_soul("wren")],
            ..Default::default()
        },
    );
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(super::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

pub(crate) async fn import_export(c: &Arc<Core>) {
    let eps = fixture::export(theseus_protocol::now_unix_ms());
    let p = ImportEpisodesParams {
        file: "export.jsonl".into(),
        lines: lines(&eps),
        batch: None,
        batches: None,
    };
    let got = call(c, method::IMPORT_EPISODES, serde_json::to_value(p).unwrap())
        .await
        .unwrap();
    let got: ImportEpisodesResult = serde_json::from_value(got).unwrap();
    assert_eq!((got.imported, got.rejected.len()), (12, 0), "{got:?}");
}

pub(crate) fn params(apply: Option<&[&str]>) -> ThreadImportParams {
    ThreadImportParams {
        from: "openclaw".into(),
        apply: apply.map(|a| a.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    }
}

pub(crate) async fn run(c: &Arc<Core>, p: &ThreadImportParams) -> ThreadImportResult {
    c.thread_import(p, cli()).await.unwrap()
}

pub(crate) fn card_for(lines: Value) -> Scripted {
    Scripted::tools("", &[("tu", TOOL, json!({ "lines": lines }))])
}

fn sent(fake: &FakeProvider) -> Vec<String> {
    fake.requests()
        .iter()
        .map(|r| serde_json::to_string(&r.messages).unwrap())
        .collect()
}

fn slugs(r: &ThreadImportResult) -> Vec<&str> {
    r.candidates.iter().map(|c| c.slug.as_str()).collect()
}

/// The dry run: the four streams, the window and the minimum applied, the
/// persona only where configured, the working set from the lines, the
/// partner stream counted and unnamed, and nothing written.
#[tokio::test]
async fn the_dry_run_lists_the_streams_and_writes_nothing() {
    let r = rig(vec![], |_| {});
    import_export(&r.core).await;
    let before = r.core.store.ledger_tail::<LedgerRow>(5000).unwrap().len();
    let got = run(&r.core, &params(None)).await;
    let mut names = slugs(&got);
    names.sort();
    assert_eq!(
        names,
        [
            "boat-ops",
            "confidential-4777",
            "dm-harbor-moorings",
            "tide-gauges"
        ],
        "no #archive (a year old), no irrigation cluster (3 lines), no CLI: {got:?}"
    );
    assert_eq!(
        got.candidates.iter().map(|c| c.n).collect::<Vec<_>>(),
        [1, 2, 3, 4],
        "numbered in the table's order"
    );
    let by = |s: &str| got.candidates.iter().find(|c| c.slug == s).unwrap();
    let gauges = by("tide-gauges");
    assert_eq!(gauges.title, "tide-gauges");
    assert_eq!(gauges.persona.as_deref(), Some("wren"));
    assert_eq!(
        by("boat-ops").persona,
        None,
        "gull is no configured persona"
    );
    assert_eq!((gauges.owner_messages, gauges.episodes), (5, 3));
    assert_eq!(gauges.sensitivity["company-confidential"], 3);
    assert_eq!(gauges.sensitivity["personal"], 2);
    assert_eq!(gauges.topics[0], "projects/tide-gauges");
    assert_eq!(gauges.channel.as_deref(), Some("discord:channel:4101"));
    let w = &gauges.working_set;
    assert_eq!(w.repos, ["reef-lab/gauge-log"]);
    for (list, want) in [
        (&w.refs, "theseus-ab12"),
        (&w.refs, "#12"),
        (&w.refs, "#14"),
        (&w.branches, "lane/gauge-sync"),
        (&w.files, "src/gauge.rs"),
        (&w.hosts, "tides.example.org"),
    ] {
        assert!(list.iter().any(|x| x == want), "{want} in {w:?}");
    }
    let dm = by("dm-harbor-moorings");
    assert_eq!(
        (dm.kind.as_str(), dm.owner_messages, dm.episodes),
        ("dm", 9, 3)
    );
    assert!(
        got.notes.iter().any(|n| n.contains("under the minimum")),
        "{:?}",
        got.notes
    );
    assert!(
        got.notes.iter().any(|n| n.contains("no topic label")),
        "{:?}",
        got.notes
    );
    // The partner stream counts, and says nothing of the deal.
    let deal = by("confidential-4777");
    assert_eq!(deal.owner_messages, 2);
    assert_eq!(deal.sensitivity["partner-confidential"], 2);
    let shown = serde_json::to_string(&got).unwrap();
    for leak in [
        SECRET,
        SECRET_REPO,
        "kestrel",
        "term-sheet",
        "fix/kestrel-terms",
        "theseus-kk77",
    ] {
        assert!(!shown.contains(leak), "{leak} in the dry run: {shown}");
    }
    assert!(got.applied.is_empty() && got.skipped.is_empty());
    // Nothing was written, no thread was made, and no model was asked.
    assert_eq!(
        r.core.store.ledger_tail::<LedgerRow>(5000).unwrap().len(),
        before
    );
    assert!(r.core.threads().all().next().is_none());
    assert!(r.fake.requests().is_empty());
}

/// The apply makes exactly the named threads. Each card is the model's cited
/// lines, each ending in the handles of the episodes it came from; a line
/// that cites nothing is dropped and counted; the row holds the call's cost
/// and its citations; the day's spend is kept.
#[tokio::test]
async fn the_apply_makes_exactly_the_ticked_threads_with_cited_cards() {
    let script = vec![
        card_for(json!([
            {"text": "The gauge sync lives on lane/gauge-sync; the drift fix is pull request 12.", "episodes": ["E3"]},
            {"text": "The calibration table is issue 14.", "episodes": ["E2"]},
            {"text": "The fix ships tonight.", "episodes": ["E1"]},
            {"text": "An uncited line.", "episodes": []},
        ])),
        card_for(
            json!([{"text": "The skiff's mooring line is to be replaced.", "episodes": ["E1"]}]),
        ),
    ];
    let r = rig(script, |_| {});
    import_export(&r.core).await;
    let before = r.core.store.get_meta::<ImportDay>(DAY_KEY).unwrap();
    assert!(before.is_none());
    let got = run(&r.core, &params(Some(&["tide-gauges", "boat-ops"]))).await;
    assert_eq!(got.applied.len(), 2, "{got:?}");
    assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    let made: Vec<String> = r.core.threads().all().map(|t| t.slug.clone()).collect();
    assert_eq!(
        made.len(),
        2,
        "the partner stream and the DMs were not ticked: {made:?}"
    );
    let t = r.core.threads().get("tide-gauges").cloned().unwrap();
    assert_eq!(t.persona.as_deref(), Some("wren"));
    assert_eq!(t.working_set.repos, ["reef-lab/gauge-log"]);
    assert_eq!(t.topics[0], "projects/tide-gauges");
    assert_eq!(
        t.card.len(),
        3,
        "the uncited line was dropped: {:?}",
        t.card
    );
    assert!(
        t.card.iter().all(|l| l.contains("[m:") && l.ends_with(']')),
        "{:?}",
        t.card
    );
    assert!(t.session_id.is_none(), "no session until its first message");
    let row = rows(&r.core, "thread.imported")
        .into_iter()
        .find(|x| x.data["slug"] == "tide-gauges")
        .unwrap();
    assert_eq!(
        (row.data["lines"].as_u64(), row.data["dropped"].as_u64()),
        (Some(3), Some(1))
    );
    let cited = row.data["cited"].as_array().unwrap();
    assert_eq!(cited.len(), 3);
    // Each citation's handle is the tail of an imported node's id, the way
    // recall's handles are, and the card shows it.
    for c in cited {
        let handle = c["handle"].as_str().unwrap();
        let tail = handle.strip_prefix("m:").unwrap();
        let node = c["node"].as_str().unwrap();
        assert!(node.ends_with(tail), "{handle} for {node}");
        assert!(
            r.core.store.get_node(node).unwrap().is_some(),
            "{node} is a real node"
        );
        assert!(
            t.card.iter().any(|l| l.contains(&format!("[{handle}]"))),
            "{handle} in {:?}",
            t.card
        );
    }
    assert_eq!(
        row.data["model"].as_str().map(|m| m.contains("haiku")),
        Some(true),
        "{}",
        row.data
    );
    let day = r
        .core
        .store
        .get_meta::<ImportDay>(DAY_KEY)
        .unwrap()
        .unwrap();
    let spent: f64 = rows(&r.core, "thread.imported")
        .iter()
        .map(|x| x.data["cost_usd"].as_f64().unwrap())
        .sum();
    assert!((day.spent_usd - spent).abs() < 1e-9 && (got.spent_usd - spent).abs() < 1e-9);
    assert_eq!(got.cap_usd, 2.0);
}

/// The same slugs again are threads that exist (and one that is no candidate):
/// skipped, with no new call.
#[tokio::test]
async fn a_thread_that_exists_and_an_unknown_slug_are_skipped_without_a_call() {
    let r = rig(
        vec![card_for(
            json!([{"text": "The skiff's line is to be replaced.", "episodes": ["E1"]}]),
        )],
        |_| {},
    );
    import_export(&r.core).await;
    run(&r.core, &params(Some(&["boat-ops"]))).await;
    let again = run(&r.core, &params(Some(&["boat-ops", "no-such"]))).await;
    assert!(again.applied.is_empty());
    assert_eq!(again.skipped.len(), 2, "{:?}", again.skipped);
    assert!(
        again.skipped[0].why.contains("exists"),
        "{:?}",
        again.skipped
    );
    assert!(
        again.skipped[1].why.contains("no such candidate"),
        "{:?}",
        again.skipped
    );
    assert_eq!(r.fake.requests().len(), 1);
    assert_eq!(
        r.core
            .thread_list(&ThreadListParams::default(), cli())
            .unwrap()
            .total,
        1
    );
}

/// A partner-confidential line never reaches the model, and a stream made of
/// nothing else is made with no call at all and a title that names nothing.
#[tokio::test]
async fn partner_text_never_reaches_the_model_a_title_or_a_card() {
    let r = rig(
        vec![card_for(
            json!([{"text": "Everything is on lane/gauge-sync.", "episodes": ["E1"]}]),
        )],
        |_| {},
    );
    import_export(&r.core).await;
    let got = run(
        &r.core,
        &params(Some(&["confidential-4777", "tide-gauges"])),
    )
    .await;
    assert_eq!(got.applied.len(), 2, "{got:?}");
    let deal = r.core.threads().get("confidential-4777").cloned().unwrap();
    assert_eq!(deal.title, "confidential discord-channel conversation");
    assert!(deal.card.is_empty() && deal.topics.is_empty() && deal.working_set.is_empty());
    let why = got.applied[0].card_failed.as_deref().unwrap();
    assert!(why.contains("no episode text"), "{why}");
    assert_eq!(r.fake.requests().len(), 1, "one call, for the open stream");
    for body in sent(&r.fake) {
        for leak in [
            SECRET,
            SECRET_REPO,
            "term sheet",
            "second tranche",
            "fix/kestrel-terms",
        ] {
            assert!(!body.contains(leak), "{leak} was sent: {body}");
        }
    }
    let shown = serde_json::to_string(&r.core.threads().all().collect::<Vec<_>>()).unwrap();
    assert!(
        !shown.contains(SECRET) && !shown.to_lowercase().contains("kestrel"),
        "{shown}"
    );
}

/// Imported history stays testimony: applying writes no node into any
/// session, and a card holds no message of the owner's word for word.
#[tokio::test]
async fn nothing_of_the_history_enters_a_thread_or_a_session() {
    let long =
        "Gauge 4 drifts after dusk, see https://github.com/reef-lab/gauge-log/pull/12 and #12";
    let r = rig(
        vec![card_for(json!([
            {"text": long, "episodes": ["E3"]},
            {"text": "Gauge 4 drifts after dusk (a summary).", "episodes": ["E3"]},
        ]))],
        |_| {},
    );
    import_export(&r.core).await;
    let nodes = |c: &Core| -> usize {
        c.store
            .live_sessions::<crate::session::SessionRecord>()
            .unwrap()
            .len()
            + crate::import::write::list(&c.store)
                .unwrap()
                .tags
                .iter()
                .map(|t| t.nodes as usize)
                .sum::<usize>()
    };
    let (sessions_before, nodes_before) = (
        r.core
            .store
            .live_sessions::<crate::session::SessionRecord>()
            .unwrap()
            .len(),
        nodes(&r.core),
    );
    let got = run(&r.core, &params(Some(&["tide-gauges"]))).await;
    let t = r.core.threads().get("tide-gauges").cloned().unwrap();
    assert_eq!(
        t.card.len(),
        1,
        "the pasted message was dropped: {:?}",
        t.card
    );
    assert!(!t.card[0].contains("github.com"), "{:?}", t.card);
    assert_eq!(got.applied[0].card.len(), 1);
    assert_eq!(rows(&r.core, "thread.imported")[0].data["dropped"], 1);
    assert_eq!(
        r.core
            .store
            .live_sessions::<crate::session::SessionRecord>()
            .unwrap()
            .len(),
        sessions_before,
        "no session was opened for the thread"
    );
    assert_eq!(nodes(&r.core), nodes_before, "no node was written");
    assert!(t.session_id.is_none());
}

/// A day that has spent its import line makes the thread without a card,
/// says why, and asks no model.
#[tokio::test]
async fn a_spent_day_makes_a_thread_without_a_card() {
    let r = rig(vec![], |_| {});
    import_export(&r.core).await;
    let today = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let seeded = ImportDay {
        day: today,
        spent_usd: 1.9999,
    };
    r.core
        .store
        .append(&[NewRecord::json(kinds::META, Some(DAY_KEY), &seeded).unwrap()])
        .unwrap();
    let got = run(&r.core, &params(Some(&["boat-ops"]))).await;
    let a = &got.applied[0];
    assert!(a.card.is_empty() && a.cost_usd == 0.0);
    let why = a.card_failed.as_deref().unwrap();
    assert!(
        why.contains("import budget") && why.contains("$2.00"),
        "{why}"
    );
    assert!(r.fake.requests().is_empty());
    assert!(
        r.core.threads().get("boat-ops").is_some(),
        "the thread is made"
    );
    assert_eq!(got.spent_today_usd, 1.9999);
    // A cap of nothing refuses before any call, too.
    let r = rig(vec![], |c| c.threads.import_day_usd = 0.0);
    import_export(&r.core).await;
    let got = run(&r.core, &params(Some(&["boat-ops"]))).await;
    assert!(got.applied[0]
        .card_failed
        .as_deref()
        .unwrap()
        .contains("$0.00"));
    assert!(r.fake.requests().is_empty());
}

/// A model that answers without a card leaves a thread without one, and the
/// call is paid for and booked.
#[tokio::test]
async fn an_answer_with_no_card_is_a_thread_without_one() {
    let r = rig(vec![Scripted::text("I cannot.")], |_| {});
    import_export(&r.core).await;
    let got = run(&r.core, &params(Some(&["boat-ops"]))).await;
    let a = &got.applied[0];
    assert!(a.card.is_empty());
    assert!(a
        .card_failed
        .as_deref()
        .unwrap()
        .contains("no write_card call"));
    assert_eq!(rows(&r.core, "thread.imported").len(), 1);
}

/// A card call cut at its cap names the cap and turned thinking off
/// (theseus-opypg), where "no write_card call" hid the cause.
#[tokio::test]
async fn a_card_cut_at_its_cap_says_so_and_thinks_off() {
    let cut = Scripted::Blocks {
        blocks: vec![json!({"type": "tool_use", "id": "tu", "name": TOOL, "input": {}})],
        stop_reason: "max_tokens".into(),
    };
    let r = rig(vec![cut], |_| {});
    import_export(&r.core).await;
    let got = run(&r.core, &params(Some(&["boat-ops"]))).await;
    let why = got.applied[0].card_failed.as_deref().unwrap();
    assert!(why.contains("max_tokens") && why.contains("1,200"), "{why}");
    assert!(!why.contains("no write_card call"), "{why}");
    let sent = r.fake.requests();
    let want = r.core.runner.catalog.thinking_off(&sent[0].model);
    assert!(
        want.is_some(),
        "{}: a model that turns thinking off",
        sent[0].model
    );
    assert_eq!(sent[0].thinking, want);
}

/// The card call asks its own target model's forcing: on a Sonnet profile it
/// is not forced (theseus-7q9wt).
#[tokio::test]
async fn on_a_sonnet_profile_the_thread_card_call_is_not_forced() {
    let r = rig(vec![card_for(json!([]))], |c| {
        c.threads.import_profile = Some("sonnet".into())
    });
    import_export(&r.core).await;
    run(&r.core, &params(Some(&["boat-ops"]))).await;
    let sent = r.fake.requests();
    assert_eq!(sent.len(), 1);
    crate::provider::one_tool::assert_unforced(&sent[0], TOOL);
}

/// The import is the owner's, from a private place: a person in a shared
/// channel is refused, and with threads off so is everyone.
#[tokio::test]
async fn the_import_is_the_owners_and_needs_threads_on() {
    let r = rig(vec![], |_| {});
    import_export(&r.core).await;
    let stranger = Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "42".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    assert!(r.core.thread_import(&params(None), stranger).await.is_err());
    let off = rig(vec![], |c| c.threads.enabled = false);
    let e = off
        .core
        .thread_import(&params(None), cli())
        .await
        .unwrap_err();
    assert!(e.message.contains("threads are off"), "{}", e.message);
    let mut bad = params(None);
    bad.from = "Open Claw!".into();
    assert!(r.core.thread_import(&bad, cli()).await.is_err());
}

/// A Vestige note is an episode of the `vestige` source, stored as it is
/// written, and the same note again is skipped.
#[tokio::test]
async fn a_vestige_note_imports_once() {
    let r = rig(vec![], |_| {});
    let secret = format!("{}_{}", "ghp", "Zy7aB9cD2eF4gH6iJ8kL0mN2oP4qR6sT8uV0");
    let mut v = json!({
        "format": 1, "import_tag": "vestige", "episode_id": format!("ep_{:064x}", 0x5e57),
        "source": "vestige", "agent": "vestige",
        "place": {"kind": "file", "name": "vestige.json", "id": null},
        "as_of": {"start": "2026-10-01T10:00:00.000Z", "end": "2026-10-01T10:00:00.000Z"},
        "labels": {"sensitivity": "personal", "partner": null, "topic": [], "book_hint": null,
                   "credential_redacted": false},
        "summary": null,
        "messages": [{"idx": 0, "time": "2026-10-01T10:00:00.000Z", "author": "vestige",
            "integrity": "agent", "text": format!("The skiff key is {secret}"),
            "unit": "vestige.json#0", "sha256": "ab".repeat(32)}],
    });
    v["hash"] = json!(crate::import::episode::hash_of(&v));
    let send = |c: Arc<Core>, v: Value| async move {
        let p = ImportEpisodesParams {
            file: "vestige.json".into(),
            lines: lines(&[v]),
            batch: None,
            batches: None,
        };
        let got = call(
            &c,
            method::IMPORT_EPISODES,
            serde_json::to_value(p).unwrap(),
        )
        .await
        .unwrap();
        serde_json::from_value::<ImportEpisodesResult>(got).unwrap()
    };
    let first = send(r.core.clone(), v.clone()).await;
    assert_eq!(
        (first.imported, first.skipped, first.rejected.len()),
        (1, 0, 0),
        "{first:?}"
    );
    let again = send(r.core.clone(), v).await;
    assert_eq!((again.imported, again.skipped), (0, 1), "{again:?}");
    let s = crate::import::session_id_of(&format!("ep_{:064x}", 0x5e57));
    let node = r
        .core
        .store
        .get_node(&crate::import::node_id_of(&s, 0))
        .unwrap()
        .unwrap()
        .1;
    let text = format!("{:?}", node.body);
    assert!(
        text.contains(&format!("The skiff key is {secret}")),
        "stored as it came: {text}"
    );
}
