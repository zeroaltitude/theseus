//! The minted tools' keep-or-retire tender, graduation, the off switches and
//! the morning post, through the core (theseus-pw1q.21.8):
//! - a flow the `"self"` principal loaded retires on the failure-rate rule,
//!   when unused for `retire_after_days`, and on a red attributed to its call;
//!   the frozen copy survives each, and the owner's `flow:`/`ext:` revert is
//!   one verb;
//! - 20 calls with no failure file the item that ports it (not 19, not with a
//!   failure), and a tool the owner loaded is none of the tender's;
//! - with the mode off or the switch halted nothing deploys, retires or posts;
//! - the notice is posted once a day, only with the mode act, to the owner's DM.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfBacklogParams, SelfHaltParams, SelfMode, SelfResumeParams};

use super::tests::test_core;
use super::tests_self_patterns::frames;
use super::{Core, Parts};
use crate::approval::Answerer;
use crate::flow::load::LoadedSet;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rsi::revert::signals::{Red, Signals};
use crate::test_support::rows;

const DAY: u64 = 86_400_000;
const HOUR: u64 = 3_600_000;
const OWNER: u64 = 271_828_182_845_904_523;

pub(super) fn core(
    mode: SelfMode,
    mint: impl FnOnce(&mut crate::config::self_mint::SelfMintConfig),
) -> Arc<Core> {
    let base = test_core("the tide is out");
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mode = mode;
    cfg.self_improve.mint.retire_after_days = 14;
    cfg.self_improve.mint.per_night = 5;
    cfg.self_improve.backlog_starts = Some(0);
    cfg.initiative.morning_hour = 0;
    cfg.server.disk_warn_mb = 0;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    mint(&mut cfg.self_improve.mint);
    Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        base.store.clone(),
    ))
    .unwrap()
}

pub(super) fn acting() -> Arc<Core> {
    let c = core(SelfMode::Act, |_| {});
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    c
}

/// The clock the tests act at: after every row they seed was written.
fn now() -> u64 {
    theseus_protocol::now_unix_ms() + 20 * 60_000
}

/// A one-step flow of invented names, scoped to an invented repository.
pub(super) fn spec(name: &str) -> Value {
    json!({
        "v": 1, "kind": "flow", "name": name,
        "description": "Counts the lines of the notes.",
        "scope": {"repos": ["r:abc123def456"], "places": ["cli"]},
        "inputs": {},
        "steps": [{"id": "count", "tool": "proc.run", "args": {"argv": ["wc", "-l", "notes.txt"]}}],
        "exits": {"ok": "counted"},
        "evidence": {"occurrences": 6, "sessions": 3, "days": 2, "usd_per_occ": 0.1}
    })
}

/// Load `spec(name)` as the `"self"` principal, `age_days` ago.
pub(super) fn load_self(c: &Core, name: &str, age_days: u64) -> theseus_protocol::flow::FlowInfo {
    let path = crate::scratch::file(name, "flow.json");
    std::fs::write(&path, spec(name).to_string()).unwrap();
    let info = c.flow_load(&path, "self", "mint").unwrap();
    let mut set = LoadedSet::read(&c.store).unwrap();
    set.loaded.get_mut(name).unwrap().at_ms -= age_days * DAY;
    c.store.append(&[set.record().unwrap()]).unwrap();
    info
}

/// One call row of `flow:<name>`, `ago` ms before `at`.
pub(super) fn flow_call(c: &Core, name: &str, ok: bool, at: u64) {
    let kind = if ok {
        "action.succeeded"
    } else {
        "action.failed"
    };
    let mut row = LedgerRow::named(
        kind,
        None,
        None,
        json!({"correlation_id": format!("c{at}{name}"), "producer": format!("harness:flow:{name}"),
               "duration_ms": 30, "outcome": if ok { "succeeded" } else { "failed" }}),
    );
    row.at_unix_ms = at;
    c.store.append_ledger(&row).unwrap();
}

struct NoReds;
impl Signals for NoReds {
    fn reds(&self, _: &Core, _: u64, _: u64) -> anyhow::Result<Vec<Red>> {
        Ok(vec![])
    }
}

fn live(c: &Core) -> Vec<String> {
    c.tools
        .flows
        .all()
        .iter()
        .map(|e| e.spec.name.clone())
        .collect()
}

fn retired(c: &Core) -> Vec<Value> {
    rows(c, "self.mint.retired")
}

/// Two failures in five calls retire a flow now; one in five keeps it. The
/// frozen copy is where it was, and the row says so.
#[tokio::test]
async fn retire_on_the_failure_rate_rule() {
    let c = acting();
    let t = now();
    let bad = load_self(&c, "bad_flow", 1);
    let fine = load_self(&c, "fine_flow", 1);
    for i in 0..5 {
        flow_call(&c, "bad_flow", i >= 2, t - (i + 1) * HOUR / 2);
        flow_call(&c, "fine_flow", i >= 1, t - (i + 1) * HOUR / 2);
    }
    let tended = c.mint_tend(&NoReds, t).await.unwrap();
    assert_eq!(tended.retired, [("flow:bad_flow".to_string(), "failures")]);
    assert_eq!(live(&c), ["fine_flow"]);
    let row = &retired(&c)[0];
    assert_eq!(
        (row["tool"].as_str(), row["rule"].as_str()),
        (Some("flow:bad_flow"), Some("failures"))
    );
    assert_eq!(row["why"], "2 of 5 calls failed (40%)");
    assert_eq!(
        (
            row["numbers"]["calls"].as_u64(),
            row["numbers"]["failures"].as_u64()
        ),
        (Some(5), Some(2))
    );
    // The frozen copy survives, and the row says where.
    let frozen = std::path::PathBuf::from(&bad.frozen);
    assert!(frozen.join("spec.json").is_file(), "{frozen:?}");
    assert!(row["undo"].as_str().unwrap().contains(&bad.frozen));
    assert_eq!(rows(&c, "flow.retired").len(), 1);
    assert!(std::path::Path::new(&fine.frozen)
        .join("spec.json")
        .is_file());
    // A second pass retires nothing more.
    assert!(c.mint_tend(&NoReds, t).await.unwrap().retired.is_empty());
}

/// Fewer than 3 calls in `retire_after_days` retires a tool that old: at 14
/// days, and not at 13.
#[tokio::test]
async fn unused_retires_at_14_days() {
    let c = acting();
    let t = now();
    load_self(&c, "idle_flow", 14);
    load_self(&c, "young_flow", 13);
    load_self(&c, "busy_flow", 20);
    flow_call(&c, "idle_flow", true, t - 5 * DAY);
    flow_call(&c, "idle_flow", true, t - 6 * DAY);
    for i in 0..3 {
        flow_call(&c, "busy_flow", true, t - (i + 2) * DAY);
    }
    let tended = c.mint_tend(&NoReds, t).await.unwrap();
    assert_eq!(tended.retired, [("flow:idle_flow".to_string(), "unused")]);
    let mut left = live(&c);
    left.sort();
    assert_eq!(left, ["busy_flow", "young_flow"]);
    assert_eq!(retired(&c)[0]["why"], "2 calls in 14 days");
}

/// A red signal after exactly one tool's call retires it now.
#[tokio::test]
async fn a_red_signal_after_its_call_retires_it_now() {
    struct OneRed(u64);
    impl Signals for OneRed {
        fn reds(&self, _: &Core, _: u64, _: u64) -> anyhow::Result<Vec<Red>> {
            Ok(vec![Red {
                signal: "health",
                at_ms: self.0,
                commit: None,
                detail: "errors doubled".into(),
            }])
        }
    }
    let c = acting();
    let t = now();
    load_self(&c, "culprit", 1);
    load_self(&c, "bystander", 1);
    flow_call(&c, "culprit", true, t - 10 * 60_000);
    flow_call(&c, "bystander", true, t - 3 * HOUR);
    let tended = c.mint_tend(&OneRed(t - 5 * 60_000), t).await.unwrap();
    assert_eq!(tended.retired, [("flow:culprit".to_string(), "health")]);
    assert_eq!(live(&c), ["bystander"]);
}

/// A 13th live minted tool waits (`max_live` 12), and a second deploy inside
/// a day waits (`per_night` 1); an owner's flow counts for neither.
#[test]
fn a_thirteenth_tool_waits_and_per_night_caps_deploys() {
    let c = core(SelfMode::Act, |m| {
        m.max_live = 12;
        m.per_night = 1;
    });
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    let t = now();
    for i in 0..11 {
        load_self(&c, &format!("tool_{i:02}"), 0);
    }
    assert!(c.mint_room(t).is_ok(), "11 live: room for the 12th");
    load_self(&c, "tool_11", 0);
    let e = c.mint_room(t).unwrap_err();
    assert!(format!("{e:#}").contains("max_live"), "{e:#}");
    // Two a night: the second deploy of a day waits, whatever is live.
    let c = core(SelfMode::Act, |m| m.per_night = 1);
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    assert!(c.mint_room(t).is_ok());
    let mut row = LedgerRow::named("self.mint.deployed", None, None, json!({"name": "x"}));
    row.at_unix_ms = t - HOUR;
    c.store.append_ledger(&row).unwrap();
    let e = c.mint_room(t).unwrap_err();
    assert!(format!("{e:#}").contains("per_night"), "{e:#}");
    assert!(
        c.mint_room(t + 2 * DAY).is_ok(),
        "a day later there is room"
    );
}

/// A tool the owner loaded is none of the tender's: no `"self"` loaded it.
#[tokio::test]
async fn the_tender_leaves_a_tool_the_owner_loaded_alone() {
    let c = acting();
    let path = crate::scratch::file("owned", "flow.json");
    std::fs::write(&path, spec("owned_flow").to_string()).unwrap();
    c.flow_load(&path, "owner", "cli").unwrap();
    let mut set = LoadedSet::read(&c.store).unwrap();
    set.loaded.get_mut("owned_flow").unwrap().at_ms -= 40 * DAY;
    c.store.append(&[set.record().unwrap()]).unwrap();
    assert!(c
        .mint_tend(&NoReds, now())
        .await
        .unwrap()
        .retired
        .is_empty());
    assert_eq!(live(&c), ["owned_flow"]);
}

/// 20 calls and no failure file the port; 19 do not, nor does one failure.
#[test]
fn graduation_files_a_self_item() {
    let t = now();
    let item = |c: &Core| {
        c.self_backlog(&SelfBacklogParams::default())
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.title == "graduate flow:ported")
    };
    let calls = |c: &Core, n: u64, failing: Option<u64>| {
        for i in 0..n {
            flow_call(c, "ported", failing != Some(i), t - (i + 1) * HOUR);
        }
    };
    // 20 calls, 0 failures.
    let c = acting();
    load_self(&c, "ported", 10);
    calls(&c, 20, None);
    c.backlog_night(t).unwrap();
    let it = item(&c).expect("the port is filed");
    assert!(
        it.cause.starts_with("graduate.flow:ported:"),
        "{}",
        it.cause
    );
    assert!(
        it.objective
            .contains("Port the loaded flow flow:ported into core"),
        "{}",
        it.objective
    );
    assert!(
        it.objective.contains("self.propose_change"),
        "{}",
        it.objective
    );
    assert_eq!(it.keel, None, "a flow asks for no network and no secret");
    // Once per digest: a second night files nothing more.
    c.backlog_night(t + 1000).unwrap();
    let all = c
        .self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items;
    assert_eq!(
        all.iter()
            .filter(|i| i.title == "graduate flow:ported")
            .count(),
        1
    );
    // 19 calls.
    let c = acting();
    load_self(&c, "ported", 10);
    calls(&c, 19, None);
    c.backlog_night(t).unwrap();
    assert!(item(&c).is_none());
    // 20 calls with one failure.
    let c = acting();
    load_self(&c, "ported", 10);
    calls(&c, 20, Some(7));
    c.backlog_night(t).unwrap();
    assert!(item(&c).is_none());
}

/// A tool retired because its motif's by-hand runs did not fall is filed as
/// a cause for the next night.
#[test]
fn a_tool_retired_for_no_effect_is_filed_as_a_cause() {
    let c = acting();
    let t = now();
    let mut row = LedgerRow::named(
        "self.mint.retired",
        None,
        None,
        json!({"name": "flat_flow", "kind": "flow", "digest": "sha256:0123456789abcdef", "tool": "flow:flat_flow",
               "bench_born": false, "rule": "not-falling",
               "why": "by-hand runs of its motif did not fall: 11 then, 12 now",
               "numbers": {"calls": 40, "failures": 0, "days": 15}, "undo": "kept"}),
    );
    row.at_unix_ms = t - HOUR;
    c.store.append_ledger(&row).unwrap();
    c.backlog_night(t).unwrap();
    let all = c
        .self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items;
    let it = all
        .iter()
        .find(|i| i.cause.starts_with("mint.retired:flow:flat_flow:"))
        .expect("filed");
    assert_eq!(it.title, "minted flow:flat_flow retired (not-falling)");
    assert!(
        it.evidence
            .iter()
            .any(|e| e.contains("mint.retired:flow:flat_flow")),
        "{:?}",
        it.evidence
    );
}

/// Mode off, or the switch halted: the tender reads nothing and writes
/// nothing, a validated flow is not loaded, the notice is not posted.
#[tokio::test]
async fn off_or_halted_deploys_retires_and_posts_nothing() {
    let t = now();
    for halted in [false, true] {
        let c = match halted {
            true => {
                let c = acting();
                load_self(&c, "idle_flow", 30);
                c.self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
                    .unwrap();
                c
            }
            false => core(SelfMode::Off, |_| {}),
        };
        bind(&c);
        let before = frames(&c);
        let t1 = c.mint_tend(&NoReds, t).await.unwrap();
        assert_eq!(
            (t1.retired.len(), t1.deployed.len()),
            (0, 0),
            "halted: {halted}"
        );
        assert!(c.mint_deploy_flow("mint_none").is_err());
        assert!(!c.post_self_notice_if_due());
        assert_eq!(frames(&c), before, "halted: {halted}: nothing was written");
        assert!(retired(&c).is_empty() && rows(&c, "self.mint.deployed").is_empty());
    }
}

fn bind(c: &Core) {
    c.runner.place_rule.bind_one(crate::places::BoundPlace {
        persona: None,
        target: format!("discord:dm:{OWNER}"),
        name: "DM @owner".into(),
        private: true,
        guild: None,
        ceiling: None,
    });
}

/// The morning post: with something landed, one `self_digest` post to the
/// owner's DM and a mark, once a local day; a day with nothing to say writes
/// only its mark; and the mark keeps a restart from posting twice.
#[tokio::test]
async fn the_notice_is_posted_once_a_day_to_the_owners_dm() {
    let c = acting();
    bind(&c);
    let t = theseus_protocol::now_unix_ms();
    let mut row = LedgerRow::named(
        "self.mint.retired",
        None,
        None,
        json!({"name": "stale", "kind": "flow", "digest": "sha256:aa", "tool": "flow:stale",
               "bench_born": false, "rule": "unused", "why": "0 calls in 14 days",
               "numbers": {"calls": 0}, "undo": "its frozen copy is kept at /x"}),
    );
    row.at_unix_ms = t;
    c.store.append_ledger(&row).unwrap();
    let before = frames(&c);
    assert!(c.post_self_notice_if_due(), "something landed");
    assert!(frames(&c) > before);
    assert!(!c.post_self_notice_if_due(), "once a day");
    // A restart reads the mark.
    let again = Core::build(Parts::for_tests(
        (*c.cfg).clone(),
        Arc::new(FakeProvider::default()),
        c.store.clone(),
    ))
    .unwrap();
    bind(&again);
    assert!(!again.post_self_notice_if_due(), "the mark is in the store");
    let q = c
        .store
        .get_meta::<Value>(crate::rsi::NOTICE_KEY)
        .unwrap()
        .unwrap();
    assert_eq!(q["day"].as_str().unwrap().len(), 10);
}

/// The same three commands run six times in a repository under `root`: the
/// planted motif, a flow of `sh`, `cat` and `wc`.
fn scripts_in(c: &Core, root: &std::path::Path, t: u64) -> std::path::PathBuf {
    use super::tests_self_patterns::{session, turn};
    let dir = root.join("harbor");
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(
        dir.join(".git/config"),
        "[remote \"origin\"]\n\turl = git@example.org:harbor/harbord.git\n",
    )
    .unwrap();
    for (n, text) in [("a.sh", "echo alpha\n"), ("b.txt", "beta\n")] {
        std::fs::write(dir.join(n), text).unwrap();
    }
    for i in 0..6u64 {
        let sid = session(c, &dir);
        let at = t - (5 - i / 2) * DAY - HOUR * (i + 1);
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

/// A report whose tiers all passed: the fixtures' and the dry replay's results
/// as `tests_self_mint` proves them, the A/B tier the judge over stand-in arms.
fn passing_report(digest: &str) -> crate::rsi::mint::ValidateReport {
    use crate::rsi::mint::kind::Kind;
    use crate::rsi::mint::validate::ab::{judge, ArmFacts};
    use crate::rsi::mint::validate::dry::DryReport;
    use crate::rsi::mint::validate::fixtures::FixtureReport;
    let turn = |tokens, decisions, called| ArmFacts {
        matched: true,
        tokens,
        wall_ms: tokens / 10,
        decisions,
        called,
        usd: 0.01,
    };
    let ab = judge(&[(turn(10_000, 4, false), turn(4_000, 1, true)); 3]);
    assert!(ab.pass, "{ab:?}");
    crate::rsi::mint::ValidateReport::assemble(
        ("run_scripts", Kind::Flow, digest),
        (
            Some(FixtureReport {
                arms: 1,
                passed: 1,
                failed: vec![],
                pass: true,
            }),
            Some(DryReport {
                instances: 3,
                reproduced: 3,
                deviations: vec![],
                unverified: 3,
                pass: true,
                why: "3 of 3".into(),
            }),
            Some(ab),
        ),
        true,
    )
}

/// End to end, with the stand-in model: a planted motif in a synthetic ledger
/// is mined (R5) and its mint prepared; the validation's report passes (R7:
/// the fixtures' and the dry replay's tests are `tests_self_mint`'s, and the
/// A/B tier here is the judge over stand-in arms); the flow is deployed in its
/// scope; a later turn of a scripted model calls it and it runs; the tender
/// keeps it; and the notice lists it with its undo. Then the owner's one verb
/// takes it back and the frozen copy stays. Off: nothing deploys and nothing posts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_planted_motif_is_mined_deployed_called_and_told() {
    use crate::provider::{ProviderRequest, Scripted};
    use crate::rsi::mint::kind::Kind;
    use crate::rsi::mint::run::Named;
    use crate::tests_places::{answers_a_call, rig_scripted, turn as place_turn};

    let input: Arc<std::sync::Mutex<Value>> = Arc::default();
    let said = input.clone();
    let r = rig_scripted(
        move |c| {
            c.self_improve.mode = SelfMode::Act;
            c.self_improve.mint.mine = true;
            c.self_improve.mint.per_night = 1;
            c.self_improve.backlog_starts = Some(0);
            c.self_improve.mint.retire_after_days = 14;
            c.policy.tools.remove("proc.run");
            c.tools.defer.enabled = false;
            c.initiative.morning_hour = 0;
        },
        move |req: &ProviderRequest| {
            if answers_a_call(req) {
                Scripted::text("Done.")
            } else {
                Scripted::tools(
                    "Running.",
                    &[("R1", "flow__run_scripts", said.lock().unwrap().clone())],
                )
            }
        },
    );
    let c = r.core.clone();
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    bind(&c);
    let t = now();
    let dir = scripts_in(&c, &r.root, t);

    // R5 mines the night and the mint starts: its pending record is on disk.
    let night = c.backlog_night(t).unwrap();
    let ph = night
        .started
        .iter()
        .find_map(|s| s.mint.clone())
        .expect("a mint started");
    let pending = c.mint_pending(&ph).unwrap();
    assert_eq!(pending.kind, Kind::Flow);

    // R7: the writer's answer settles the name; the spec is frozen; the report passes.
    let named = Named {
        name: "run_scripts".into(),
        description: "Runs the script, shows the notes and counts their lines.".into(),
        types: Default::default(),
    };
    let sk = c.mint_spec(&pending, &named).unwrap();
    c.tools.flows.freeze(&sk.spec).unwrap();
    let digest = sk.spec.digest.clone().unwrap();
    let report = passing_report(&digest);
    assert!(report.pass);
    c.mint_keep_report(&ph, &report).unwrap();

    // The deploy: loaded by the self principal, in its scope, with its row and undo.
    let d = c.mint_deploy_flow(&ph).unwrap();
    assert_eq!(d.tool, "flow:run_scripts");
    assert_eq!(live(&c), ["run_scripts"]);
    let row = &rows(&c, "self.mint.deployed")[0];
    assert_eq!(row["undo"], "theseus self revert flow:run_scripts");
    assert_eq!(row["numbers"]["check_same"], 3);

    // A later turn, in a session in the repository, calls it, and it runs.
    let sid = {
        let mut rec =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        rec.dir = Some(dir.display().to_string());
        c.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    let inputs: Vec<String> = c
        .tools
        .flows
        .get("run_scripts")
        .unwrap()
        .spec
        .inputs
        .keys()
        .cloned()
        .collect();
    let _ = inputs;
    *input.lock().unwrap() = json!({});
    place_turn(&c, &sid, "run it").await;
    let ran = rows(&c, "flow.ran");
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0]["name"], "run_scripts");

    // The tender keeps what works: nothing retires.
    let tended = c.mint_tend(&NoReds, now() + HOUR).await.unwrap();
    assert!(tended.retired.is_empty(), "{tended:?}");

    the_notice_tells_it_and_one_verb_undoes_it(&c, t, &report.digest).await;
}

/// The notice lists the flow with its numbers and undo; the owner's one verb
/// takes it back and the frozen copy stays.
async fn the_notice_tells_it_and_one_verb_undoes_it(c: &Arc<Core>, t: u64, digest: &str) {
    // The notice tells it, with its numbers and its undo.
    let text = c
        .self_notice(t - 2 * DAY, now() + HOUR)
        .unwrap()
        .expect("something landed");
    assert!(text.contains("\nMade\n• flow run_scripts: Runs the script, shows the notes and counts their lines. Live in r:"), "{text}");
    assert!(
        text.contains("From ") && text.contains("by-hand runs"),
        "{text}"
    );
    assert!(
        text.contains("undo: theseus self revert flow:run_scripts"),
        "{text}"
    );
    assert!(!text.to_lowercase().contains("in progress"), "{text}");

    // The owner's one verb takes it back; the frozen copy stays.
    let owner = Answerer::from("the CLI");
    let done = c
        .self_revert(
            &theseus_protocol::rsi::SelfRevertParams {
                merge: "flow:run_scripts".into(),
                author: None,
                discord: None,
                from_job: None,
            },
            &owner,
            &Default::default(),
        )
        .await
        .unwrap();
    assert!(done.changed && live(c).is_empty());
    assert!(c
        .tools
        .flows
        .frozen_dir("run_scripts", digest.trim_start_matches("sha256:"))
        .join("spec.json")
        .is_file());
}
