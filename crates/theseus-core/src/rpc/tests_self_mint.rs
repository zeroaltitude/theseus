//! The mint start and the validation through the core (theseus-pw1q.21.6): a
//! seeded transcript's motif starts a writer session whose brief holds no
//! check-half id; the check half is on disk; a motif is started once; the
//! caps, the off switches, the data rule and the owner card hold; and a
//! validation runs its tiers, writes one row, spends nothing on a model when
//! an earlier tier fails, and loads nothing.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfBacklogParams, SelfHaltParams, SelfMode};

use super::tests_self_patterns::{acting, core_with, frames, now, repo, seed, session, turn};
use super::{Core, Parts};
use crate::approval::Answerer;
use crate::provider::FakeProvider;
use crate::rsi::mint::brief::{occurrence_id, read_check};

/// An acting core with `mint` set.
fn minting(mint: impl FnOnce(&mut crate::config::self_mint::SelfMintConfig)) -> Arc<Core> {
    minting_knowing(mint, crate::scrub::Scrubber::default())
}

/// `minting`, its core's scrubber knowing the values it was given.
fn minting_knowing(
    mint: impl FnOnce(&mut crate::config::self_mint::SelfMintConfig),
    scrubber: crate::scrub::Scrubber,
) -> Arc<Core> {
    let base = acting(true);
    let mut cfg = (*base.cfg).clone();
    mint(&mut cfg.self_improve.mint);
    let c = Core::build(Parts {
        scrubber: Arc::new(scrubber),
        ..Parts::for_tests(cfg, Arc::new(FakeProvider::default()), base.store.clone())
    })
    .unwrap();
    c.self_resume(&Default::default(), &Answerer::from("the CLI"))
        .unwrap();
    c
}

fn items(c: &Core) -> Vec<theseus_protocol::rsi::SelfBacklogItem> {
    c.self_backlog(&SelfBacklogParams::default()).unwrap().items
}

fn mint_dirs(c: &Core) -> Vec<String> {
    std::fs::read_dir(c.tools.flows.root())
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_motif_starts_a_writer_session_whose_brief_holds_no_check_half_id() {
    let c = minting(|m| m.per_night = 1);
    let dir = repo();
    let t = now();
    seed(&c, &dir, t, None);
    let night = c.backlog_night(t).unwrap();
    assert_eq!(night.started.len(), 1, "{:?}", night.started);
    let s = &night.started[0];
    let placeholder = s.mint.clone().expect("a mint start");
    assert!(placeholder.starts_with("mint_"));
    // The item is in progress in a session of its own, under the self principal.
    let it = items(&c).into_iter().find(|i| i.id == s.item).unwrap();
    assert_eq!(it.state, "in_progress");
    assert_eq!(it.session.as_deref(), Some(s.session_id.as_str()));
    // The check half is on disk, open data; the brief holds none of its ids.
    let check = read_check(c.tools.flows.root(), &placeholder).unwrap();
    assert!(check.len() >= 3, "{check:?}");
    for o in &check {
        assert!(!s.objective.contains(&occurrence_id(o)), "{}", s.objective);
        assert!(!s.objective.contains(&o.turn), "{}", s.objective);
    }
    assert!(s.objective.contains("kind: flow") && s.objective.contains("MINT"));
    // Another night starts nothing from the same motif.
    let again = c.backlog_night(t + 1000).unwrap();
    assert!(
        again
            .started
            .iter()
            .all(|x| x.mint.as_deref() != Some(placeholder.as_str())),
        "{:?}",
        again.started
    );
}

#[test]
fn per_night_caps_the_mint_starts_apart_from_the_fixes() {
    let t = now();
    let (dir2, dir3) = (repo(), repo());
    let none = minting(|m| m.per_night = 0);
    seed(&none, &dir2, t, None);
    seed(&none, &dir3, t, None);
    assert!(none.backlog_night(t).unwrap().started.is_empty());
    let one = minting(|m| m.per_night = 1);
    seed(&one, &dir2, t, None);
    seed(&one, &dir3, t, None);
    let n = one.backlog_night(t).unwrap();
    assert_eq!(n.started.iter().filter(|s| s.mint.is_some()).count(), 1);
    let two = minting(|m| m.per_night = 5);
    seed(&two, &dir2, t, None);
    seed(&two, &dir3, t, None);
    let n = two.backlog_night(t).unwrap();
    assert!(
        n.started.iter().filter(|s| s.mint.is_some()).count() >= 2,
        "{:?}",
        n.started
    );
    assert!(
        n.started.iter().all(|s| s.mint.is_some()),
        "no fix was started"
    );
}

#[test]
fn off_starts_nothing_and_writes_no_mint_state() {
    let dir = repo();
    let t = now();
    // mine = false.
    let c = minting(|m| m.mine = false);
    seed(&c, &dir, t, None);
    assert!(c.backlog_night(t).unwrap().started.is_empty());
    assert!(mint_dirs(&c).is_empty());
    // The mode off.
    let c = core_with(SelfMode::Off, true);
    seed(&c, &dir, t, None);
    let before = frames(&c);
    let n = c.backlog_night(t).unwrap();
    assert_eq!((n.gate.as_str(), n.started.len()), ("off", 0));
    assert_eq!(frames(&c), before);
    assert!(mint_dirs(&c).is_empty());
    // The kill switch halted.
    let c = minting(|m| m.per_night = 1);
    seed(&c, &dir, t, None);
    c.self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    let n = c.backlog_night(t).unwrap();
    assert_eq!((n.gate.as_str(), n.started.len()), ("halted", 0));
    assert!(mint_dirs(&c).is_empty());
}

/// The items a mining night filed stay on the backlog; with `mine` turned off
/// afterwards, a night starts none of them as mints.
#[test]
fn turning_mine_off_stops_the_mint_start_of_items_already_filed() {
    let first = minting(|m| m.per_night = 0);
    let dir = repo();
    let t = now();
    seed(&first, &dir, t, None);
    assert!(first.backlog_night(t).unwrap().started.is_empty());
    assert!(items(&first)
        .iter()
        .any(|i| i.cause.starts_with("pattern:") && i.state == "proposed"));
    let mut cfg = (*first.cfg).clone();
    cfg.self_improve.mint.per_night = 3;
    cfg.self_improve.mint.mine = false;
    let off = Core::build(Parts::for_tests(
        cfg.clone(),
        Arc::new(FakeProvider::default()),
        first.store.clone(),
    ))
    .unwrap();
    off.self_resume(&Default::default(), &Answerer::from("the CLI"))
        .unwrap();
    assert!(off.backlog_night(t + 1000).unwrap().started.is_empty());
    assert!(mint_dirs(&off).is_empty());
    // The same store with it on starts one.
    cfg.self_improve.mint.mine = true;
    let on = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        first.store.clone(),
    ))
    .unwrap();
    on.self_resume(&Default::default(), &Answerer::from("the CLI"))
        .unwrap();
    assert_eq!(on.backlog_night(t + 2000).unwrap().started.len(), 1);
}

#[test]
fn a_private_motif_is_not_started_on_a_china_origin_live_profile() {
    let base = acting(true);
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mint.per_night = 1;
    cfg.model.live = "glm53".into();
    let c = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        base.store.clone(),
    ))
    .unwrap();
    c.self_resume(&Default::default(), &Answerer::from("the CLI"))
        .unwrap();
    let dir = repo();
    let t = now();
    seed(&c, &dir, t, None);
    let n = c.backlog_night(t).unwrap();
    assert!(n.started.is_empty(), "{:?}", n.started);
    assert!(
        mint_dirs(&c).is_empty(),
        "no brief, no check half, no session"
    );
    // The pattern is still on the backlog, proposed.
    assert!(items(&c)
        .iter()
        .any(|i| i.cause.starts_with("pattern:") && i.state == "proposed"));
}

#[test]
fn a_motif_that_would_need_the_network_waits_for_the_owner() {
    let c = minting(|m| m.per_night = 1);
    let dir = repo();
    let t = now();
    // The same recurring stretch, each time reaching a host through a tool a
    // flow may not name.
    for i in 0..6u64 {
        let sid = session(&c, &dir);
        let at = t - (5 - i / 2) * 86_400_000 - 3_600_000 * (i + 1);
        let calls: Vec<(&str, Value, i64, &str)> = vec![
            (
                "http.fetch",
                json!({"url": "https://status.example.org/api"}),
                0,
                "ok",
            ),
            ("proc.run", json!({"argv": ["jq", ".state"]}), 0, "ok"),
            (
                "fs.read",
                json!({"path": format!("crates/x{i}/src/lib.rs")}),
                0,
                "text",
            ),
            (
                "http.fetch",
                json!({"url": "https://status.example.org/api"}),
                0,
                "ok",
            ),
        ];
        turn(&c, &sid, at, &calls);
    }
    let n = c.backlog_night(t).unwrap();
    assert!(n.started.is_empty(), "{:?}", n.started);
    let waiting: Vec<_> = items(&c)
        .into_iter()
        .filter(|i| i.state == "waiting_human")
        .collect();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(waiting[0].cause.starts_with("pattern:"));
    assert!(
        mint_dirs(&c).is_empty(),
        "nothing was minted: no brief, no check half"
    );
    assert!(c.tools.flows.root().read_dir().is_err() || !c.tools.flows.any());
}

fn sh() -> Arc<crate::own_bench::whole::tests::Sh> {
    let sh = Arc::new(crate::own_bench::whole::tests::Sh::default());
    *crate::own_bench::whole::runloop::TEST_PROGRAMS
        .lock()
        .unwrap() = Some(sh.clone());
    sh
}

/// A started mint, ready to validate: its pending record and the core.
fn started() -> (Arc<Core>, crate::rsi::mint::run::Pending) {
    started_with(crate::scrub::Scrubber::default())
}

fn started_with(scrubber: crate::scrub::Scrubber) -> (Arc<Core>, crate::rsi::mint::run::Pending) {
    let c = minting_knowing(|m| m.per_night = 1, scrubber);
    let t = now();
    seed(&c, &repo(), t, None);
    let n = c.backlog_night(t).unwrap();
    let ph = n.started[0].mint.clone().expect("a mint start");
    let pending = c.mint_pending(&ph).unwrap();
    (c, pending)
}

fn named(description: &str) -> crate::rsi::mint::run::Named {
    crate::rsi::mint::run::Named {
        name: "land_it".into(),
        description: description.into(),
        types: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_validation_runs_its_tiers_writes_one_row_and_loads_nothing() {
    let _l = sh();
    let (c, pending) = started();
    let desc = "Fetches, tests, merges the branch and pushes it; hands back what it cannot decide.";
    let report = c.mint_validate(&pending, &named(desc)).await.unwrap();
    *crate::own_bench::whole::runloop::TEST_PROGRAMS
        .lock()
        .unwrap() = None;
    // The fixtures ran every arm of the mined flow; the dry replay found the
    // seeded transcript's directories cannot reproduce a recording (an invented
    // repository holds no merge), so it failed and no model was paid.
    let fx = report.fixtures.as_ref().expect("the fixture tier ran");
    assert!(fx.pass, "{fx:?}");
    let dry = report.dry.as_ref().expect("the dry replay ran");
    assert!(!dry.pass && dry.instances >= 3, "{dry:?}");
    assert!(
        report.ab.is_none(),
        "no model call when a tier before it failed"
    );
    assert!(!report.pass && report.isolated);
    assert_eq!(report.name, "land_it");
    // One row, with its tiers; the model never saw a request.
    let rows = crate::test_support::rows(&c, "self.mint.validated");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "land_it");
    assert_eq!(rows[0]["pass"], false);
    let tiers: Vec<(String, bool)> = rows[0]["tiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["tier"].as_str().unwrap().to_string(),
                t["pass"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        tiers,
        [
            ("fixtures".to_string(), true),
            ("dry".to_string(), false),
            ("ab".to_string(), false),
            ("isolation".to_string(), true)
        ]
    );
    // The spec is frozen read-only; nothing is loaded.
    let digest = report.digest.strip_prefix("sha256:").unwrap();
    let spec = c
        .tools
        .flows
        .root()
        .join("land_it")
        .join(digest)
        .join("spec.json");
    assert!(spec.is_file());
    assert!(std::fs::metadata(&spec).unwrap().permissions().readonly());
    assert!(!c.tools.flows.any());
    assert!(crate::test_support::rows(&c, "flow.loaded").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_vault_value_in_the_writers_description_is_never_frozen_or_recorded() {
    let _l = sh();
    let key = format!("{}{}", "AKIA", "IOSFODNN7EXAMPL3");
    let (c, pending) = started_with(crate::scrub::Scrubber::with_values(vec![(
        key.clone(),
        "deploy_key".into(),
    )]));
    let e = c
        .mint_validate(
            &pending,
            &named(&format!("Lands a branch. Uses key {key} to push.")),
        )
        .await
        .unwrap_err();
    *crate::own_bench::whole::runloop::TEST_PROGRAMS
        .lock()
        .unwrap() = None;
    assert!(format!("{e:#}").contains("scrubber"), "{e:#}");
    assert!(
        !c.tools.flows.root().join("land_it").exists(),
        "nothing was frozen"
    );
    assert!(crate::test_support::rows(&c, "self.mint.validated").is_empty());
    let text = String::from_utf8_lossy(&super::tests_self_patterns::stored_bytes(&c)).into_owned();
    assert!(!text.contains(&key));
}

#[test]
fn the_writers_reply_ends_with_one_line_naming_the_tool() {
    use crate::rsi::mint::run::writer_reply;
    let r = writer_reply("Here you go.\nMINT {\"name\": \"land_it\", \"description\": \"Lands a branch.\", \"types\": {\"ref\": \"ref\"}}\n").unwrap();
    assert_eq!(
        (r.name.as_str(), r.description.as_str()),
        ("land_it", "Lands a branch.")
    );
    assert_eq!(r.types.len(), 1);
    assert!(writer_reply("no line at all").is_none());
    assert!(writer_reply("MINT {\"name\": \"x\"}").is_none());
    assert!(writer_reply("MINT {\"name\": \"\", \"description\": \"d\"}").is_none());
    assert!(writer_reply("MINT not json").is_none());
    assert!(writer_reply(
        "MINT {\"name\": \"x\", \"description\": \"d\", \"types\": {\"a\": \"bogus\"}}"
    )
    .is_none());
}

/// The same three commands run six times in a repository on disk.
fn scripts(c: &Core, t: u64) -> std::path::PathBuf {
    let dir = repo();
    for (n, text) in [("a.sh", "echo alpha\n"), ("b.txt", "beta\n")] {
        std::fs::write(dir.join(n), text).unwrap();
    }
    for i in 0..6u64 {
        let sid = session(c, &dir);
        let at = t - (5 - i / 2) * 86_400_000 - 3_600_000 * (i + 1);
        let calls: Vec<(&str, Value, i64, &str)> = vec![
            ("proc.run", json!({"argv": ["sh", "a.sh"]}), 0, "alpha"),
            ("proc.run", json!({"argv": ["cat", "b.txt"]}), 0, "beta"),
            (
                "proc.run",
                json!({"argv": ["wc", "-l", "b.txt"]}),
                0,
                "1 b.txt",
            ),
        ];
        turn(c, &sid, at, &calls);
    }
    dir
}

/// The check half's occurrences are read back from their sessions into
/// instances (the directory as it was, the inputs, the recorded exit and
/// steps), and the dry replay of the flow over them, run by the replay's own
/// hands in a stand-in for L1, reproduces every one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_check_half_replays_dry_from_recorded_sessions() {
    let _l = sh();
    let c = minting(|m| m.per_night = 1);
    let t = now();
    let dir = scripts(&c, t);
    let n = c.mine_run(t, false).unwrap().mined;
    let motif = n
        .motifs
        .iter()
        .find(|m| m.steps.len() >= 3)
        .expect("the commands' motif")
        .clone();
    let sk = crate::rsi::mint::spec::flow_spec(&motif, "run_scripts", &|s| s.to_string()).unwrap();
    let key = crate::rsi::exams::SplitKey([5; 32]);
    let halves = crate::rsi::mint::brief::split(&key, &motif.digest, &motif.occurrences);
    let check: Vec<_> = halves.check.iter().map(|o| (*o).clone()).collect();
    assert!(check.len() >= 3);
    let (instances, _guards) = c.mint_instances((&motif, &sk), &check).await.unwrap();
    assert_eq!(instances.len(), check.len());
    for i in &instances {
        assert_eq!(i.recorded.exit, "ok", "{i:?}");
        assert_eq!(i.recorded.ran.len(), sk.spec.steps.len());
        assert!(i.recorded.ran.iter().all(|(_, e)| *e == Some(0)));
        // The copy holds the repository as it was; the real directory is as it is.
        assert!(i.dir.join("a.sh").is_file() && i.dir != dir);
        // The recording ran programs, so its paths are not claimed.
        assert!(i.recorded.changed.is_none());
    }
    let rt = c.runner.tools.clone();
    let report = crate::rsi::mint::validate::dry::dry_replay(&sk.spec, &instances, |i| {
        crate::own_bench::whole::hands::Hands::new(
            &rt,
            (i.dir.clone(), i.dir.clone()),
            vec![],
            crate::own_bench::whole::runloop::programs(),
        )
    })
    .await;
    *crate::own_bench::whole::runloop::TEST_PROGRAMS
        .lock()
        .unwrap() = None;
    assert!(report.pass, "{report:?}");
    assert_eq!(
        (report.instances, report.reproduced),
        (check.len(), check.len())
    );
    assert_eq!(
        report.unverified,
        check.len(),
        "paths of a recording that ran programs are unverified"
    );
    drop(_guards);
}

/// A mint that cannot be prepared (here its check half cannot be written)
/// fails its own item with the reason, and the night still ends `Ok`: a
/// bad motif never costs the night its other starts, and it is not tried
/// again every night (a join fix).
#[test]
fn a_mint_that_cannot_be_prepared_fails_its_item_and_the_night_goes_on() {
    let c = minting(|m| m.per_night = 1);
    let dir = repo();
    let t = now();
    seed(&c, &dir, t, None);
    // A file where each motif's mint directory would go.
    let root = c.tools.flows.root().to_path_buf();
    std::fs::create_dir_all(&root).unwrap();
    let mined = c.mine_run(t, false).unwrap().mined;
    assert!(!mined.motifs.is_empty());
    for m in &mined.motifs {
        std::fs::write(
            root.join(crate::rsi::mint::spec::placeholder_name(m)),
            "not a directory",
        )
        .unwrap();
    }
    let night = c.backlog_night(t).unwrap();
    assert!(
        night.started.iter().all(|s| s.mint.is_none()),
        "{:?}",
        night.started
    );
    let failed: Vec<_> = all_items(&c)
        .into_iter()
        .filter(|i| i.state == "failed")
        .collect();
    assert_eq!(failed.len(), 1, "{:?}", all_items(&c));
    let again = c.backlog_night(t + 1000).unwrap();
    assert!(again.started.iter().all(|s| s.mint.is_none()));
    // The failed item is not tried again (the next motif may be).
    let first = all_items(&c)
        .into_iter()
        .find(|i| i.id == failed[0].id)
        .unwrap();
    assert_eq!(first.state, "failed");
}

fn all_items(c: &Core) -> Vec<theseus_protocol::rsi::SelfBacklogItem> {
    c.self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items
}
