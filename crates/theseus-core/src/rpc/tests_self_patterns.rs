//! The pattern miner through the core (theseus-pw1q.21.3): a seeded
//! transcript (sessions in an invented repository, six on four days, the
//! owner's land flow among noise) is folded into compact day lists and the
//! motif is filed on the backlog as a pattern cause beside the fixes; a planted
//! secret reaches no stored shape; the cursor makes the second night read
//! nothing twice; with `mine = false`, the mode off or the switch halted the
//! night reads nothing and writes nothing; `self patterns` reads and writes
//! nothing, with the mode off, and is the owner's.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::patterns::SelfPatternsParams;
use theseus_protocol::rsi::{SelfBacklogParams, SelfMode, SelfResumeParams};
use theseus_store::kinds;

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::Answerer;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, ResultStatus};
use crate::provider::FakeProvider;
use crate::session::SessionRecord;

const DAY: u64 = 86_400_000;
const OWNER: u64 = 271_828_182_845_904_523;

pub(super) fn core_with(mode: SelfMode, mine: bool) -> Arc<Core> {
    let base = test_core("the tide is out");
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mode = mode;
    cfg.self_improve.mint.mine = mine;
    // The miner's night alone starts nothing (the mint start has its own
    // tests, `tests_self_mint`).
    cfg.self_improve.mint.per_night = 0;
    cfg.self_improve.backlog_starts = Some(1);
    cfg.server.disk_warn_mb = 0;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        base.store.clone(),
    ))
    .unwrap()
}

pub(super) fn acting(mine: bool) -> Arc<Core> {
    let c = core_with(SelfMode::Act, mine);
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    c
}

/// An invented repository on disk: a `.git/config` with an origin.
pub(super) fn repo() -> PathBuf {
    let d = crate::scratch::dir("mine");
    std::fs::create_dir_all(d.join(".git")).unwrap();
    std::fs::write(
        d.join(".git/config"),
        "[remote \"origin\"]\n\turl = git@example.org:harbor/harbord.git\n",
    )
    .unwrap();
    d
}

pub(super) fn session(c: &Core, dir: &std::path::Path) -> String {
    let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.dir = Some(dir.display().to_string());
    c.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// One turn: an assistant answer and, for each call, its node and its result.
pub(super) fn turn(c: &Core, sid: &str, at_ms: u64, calls: &[(&str, Value, i64, &str)]) {
    let tid = crate::new_id("trn");
    let mut a = Node::assistant(
        sid,
        &tid,
        0,
        Body::AssistantMessage {
            blocks: vec![],
            model: "claude-test".into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: theseus_protocol::Usage {
                input_tokens: 1000,
                output_tokens: 300,
                cache_read_input_tokens: 40_000,
                cache_creation_input_tokens: 2000,
                cache_creation_1h_input_tokens: 0,
            },
            cost_usd: Some(0.02),
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    a.created_at_ms = at_ms;
    let mut recs = vec![a.record().unwrap()];
    for (i, (tool, input, exit, out)) in calls.iter().enumerate() {
        let id = format!("tu_{}_{i}", crate::new_id("c"));
        let mut call = Node::tool_call(
            sid,
            Some(&tid),
            Some(0),
            Body::ToolCall {
                tool_use_id: id.clone(),
                tool: (*tool).into(),
                wire_name: tool.replace('.', "_"),
                input: input.clone(),
                assistant_node: a.id.clone(),
                correlation_id: Some(id.clone()),
                gate: None,
            },
        );
        call.created_at_ms = at_ms + 1 + i as u64;
        let mut res = Node::tool_result(
            sid,
            Some(&tid),
            Some(0),
            Body::ToolResult {
                tool_use_id: id.clone(),
                tool: (*tool).into(),
                status: ResultStatus::Ok,
                is_error: *exit != 0,
                content: (*out).into(),
                correlation_id: Some(id),
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: Some(1500),
                late: false,
                meta: json!({"exit_code": exit}),
                image: None,
                external: None,
            },
        );
        res.created_at_ms = at_ms + 2 + i as u64;
        recs.push(call.record().unwrap());
        recs.push(res.record().unwrap());
    }
    c.store.append(&recs).unwrap();
    let mut row = LedgerRow::named(
        "provider.call",
        Some(sid),
        None,
        json!({"node_id": a.id, "timing": {"total_ms": 3000}}),
    );
    row.at_unix_ms = at_ms;
    c.store.append_ledger(&row).unwrap();
}

fn run(argv: &[&str]) -> (&'static str, Value) {
    ("proc.run", json!({ "argv": argv }))
}

/// The owner's flow, `n` times over `n` sessions on four days. `secret`
/// rides one call's argv.
pub(super) fn seed(c: &Core, dir: &std::path::Path, now: u64, secret: Option<&str>) {
    let test = ["cargo", "nextest", "run", "-p", "harbord"];
    for i in 0..6u64 {
        let sid = session(c, dir);
        let at = now - (5 - i / 2) * DAY - 3_600_000 * (i + 1);
        let merge = ["git", "merge", "--no-ff", &format!("feat/job-{i}")];
        let (m_exit, m_out) = match i % 3 {
            0 => (0, "Merge made by the 'ort' strategy."),
            1 => (
                1,
                "Resolved 'src/a.rs' using previous resolution.\nAutomatic merge failed; fix conflicts",
            ),
            _ => (0, "Merge made by the 'ort' strategy."),
        };
        let mut calls: Vec<(&str, Value, i64, &str)> = vec![];
        let pairs = [run(&["git", "fetch", "origin"]), run(&test), run(&merge)];
        let outs = [(0, ""), (0, "test result: ok. 12 passed"), (m_exit, m_out)];
        for ((t, v), (e, o)) in pairs.iter().zip(outs) {
            calls.push((t, v.clone(), e, o));
        }
        let post = [run(&test), run(&["git", "push", "origin", "main"])];
        if m_exit == 1 {
            calls.push((
                "proc.run",
                json!({"argv": ["git", "commit", "--no-edit"]}),
                0,
                "[main abc] Merge",
            ));
        }
        for (t, v) in &post {
            calls.push((t, v.clone(), 0, "ok"));
        }
        if i % 2 == 0 {
            if let Some(s) = secret {
                calls.push((
                    "proc.run",
                    json!({"argv": ["deploy", "--key", s]}),
                    0,
                    "deployed",
                ));
            }
        }
        turn(c, &sid, at, &calls);
    }
}

fn aws_key() -> String {
    format!("{}{}", "AKIA", "IOSFODNN7EXAMPL3")
}
fn api_key() -> String {
    format!("{}{}", "sk-", "proj-q8Zr3VnL0aXw9TgB5mYc2HdK")
}

/// Every META, LEDGER and TASK byte the store holds.
pub(super) fn stored_bytes(c: &Core) -> Vec<u8> {
    let mut all = Vec::new();
    for r in theseus_store::Store::scan(&**c.store.inner(), 0, None, usize::MAX).unwrap() {
        if matches!(r.kind, kinds::META | kinds::LEDGER | kinds::TASK) {
            all.extend_from_slice(&r.payload);
            all.push(b'\n');
        }
    }
    all
}

pub(super) fn frames(c: &Core) -> u64 {
    c.store.last_position()
}

pub(super) fn now() -> u64 {
    theseus_protocol::now_unix_ms() + 20 * 60_000
}

#[test]
fn a_seeded_transcript_files_a_pattern_cause_and_starts_nothing() {
    let c = acting(true);
    let dir = repo();
    let t = now();
    seed(&c, &dir, t, None);
    let night = c.backlog_night(t).unwrap();
    assert_eq!(night.gate, "allowed");
    let b = c.self_backlog(&SelfBacklogParams::default()).unwrap();
    let pat: Vec<_> = b
        .items
        .iter()
        .filter(|i| i.cause.starts_with("pattern:"))
        .collect();
    assert!(
        !pat.is_empty(),
        "{:?}",
        b.items.iter().map(|i| &i.cause).collect::<Vec<_>>()
    );
    assert!(
        pat.iter()
            .all(|i| i.state == "proposed" && i.session.is_none()),
        "{pat:?}"
    );
    assert!(night.started.is_empty(), "nothing starts from a pattern");
    let row = crate::test_support::rows(&c, "self.mine.ran");
    assert_eq!(row.len(), 1);
    let ran = &row[0];
    assert_eq!(ran["runs"], 6);
    assert!(ran["motifs"].as_u64().unwrap() >= 1);
    assert!(ran["top"][0][0].as_str().unwrap().starts_with("pattern:"));
    // Day lists are in META, keyed by date, with token ids only.
    let metas = stored_bytes(&c);
    let text = String::from_utf8_lossy(&metas);
    assert!(
        text.contains("self.mine") || text.contains("\"calls\""),
        "day lists written"
    );
    for leaked in ["feat/job-3", "harbord", "src/a.rs"] {
        let in_day = c
            .store
            .get_meta::<Value>(&format!(
                "self.mine.shapes.{}",
                super::super::rsi::mine::read::date_of(t - 2 * DAY)
            ))
            .unwrap();
        if let Some(d) = in_day {
            assert!(!d.to_string().contains(leaked), "{leaked} in a day list");
        }
    }
}

#[test]
fn a_planted_secret_never_reaches_a_stored_shape() {
    let c = acting(true);
    let dir = repo();
    let t = now();
    let (aws, key) = (aws_key(), api_key());
    seed(&c, &dir, t, Some(&aws));
    seed(&c, &dir, t, Some(&key));
    c.backlog_night(t).unwrap();
    let stored = stored_bytes(&c);
    let text = String::from_utf8_lossy(&stored);
    assert!(
        text.contains("self.mine.ran") || text.contains("calls_read"),
        "the night wrote its row"
    );
    for secret in [&aws, &key] {
        assert!(
            !text.contains(secret.as_str()),
            "a secret reached a stored byte"
        );
    }
    // Not in the vocabulary as a constant, not as a shape: the raw value is
    // in the transcript's own nodes (not scrubbed at rest) and nowhere the
    // miner wrote.
    let vocab = c
        .store
        .get_meta::<Value>("self.mine.vocab")
        .unwrap()
        .unwrap()
        .to_string();
    assert!(!vocab.contains("AKIA") && !vocab.contains("sk-"));
    assert!(vocab.contains("deploy"), "the shape of the call is kept");
    assert!(
        vocab.contains("harbord"),
        "a recurring value's text is a constant"
    );
    assert!(
        !vocab.contains("feat/job-3"),
        "a value that varies is kept as a digest alone"
    );
}

#[test]
fn the_cursor_means_the_next_night_reads_only_what_is_new() {
    let c = acting(true);
    let dir = repo();
    let t = now();
    seed(&c, &dir, t, None);
    c.backlog_night(t).unwrap();
    let first = crate::test_support::rows(&c, "self.mine.ran");
    assert_eq!(first.len(), 1);
    assert!(first[0]["calls_read"].as_u64().unwrap() > 20);
    c.backlog_night(t + 1000).unwrap();
    let all = crate::test_support::rows(&c, "self.mine.ran");
    assert_eq!(all.len(), 2);
    assert_eq!(all[1]["calls_read"], 0, "nothing new was read");
    assert_eq!(all[1]["runs"], 6, "the window still holds the days");
}

#[test]
fn off_reads_nothing_and_writes_nothing() {
    let dir = repo();
    let t = now();
    // mine = false, the mode act.
    let c = acting(false);
    seed(&c, &dir, t, None);
    let before = frames(&c);
    c.backlog_night(t).unwrap();
    assert!(c
        .store
        .get_meta::<Value>("self.mine.cursor")
        .unwrap()
        .is_none());
    assert!(crate::test_support::rows(&c, "self.mine.ran").is_empty());
    let _ = before;
    // The mode off.
    let c = core_with(SelfMode::Off, true);
    seed(&c, &dir, t, None);
    let before = frames(&c);
    assert_eq!(c.backlog_night(t).unwrap().gate, "off");
    assert_eq!(frames(&c), before);
    // The switch halted: a store never resumed.
    let c = core_with(SelfMode::Act, true);
    seed(&c, &dir, t, None);
    let before = frames(&c);
    assert_eq!(c.backlog_night(t).unwrap().gate, "halted");
    assert_eq!(frames(&c), before);
    assert!(c.mine_obs(t, "2026-10-17").is_empty());
}

#[test]
fn self_patterns_reads_and_writes_nothing_even_with_the_mode_off() {
    let c = core_with(SelfMode::Off, false);
    let dir = repo();
    let t = now();
    seed(&c, &dir, t, None);
    let before = frames(&c);
    let r = c
        .self_patterns_at(
            &SelfPatternsParams {
                top: Some(5),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
            t,
        )
        .unwrap();
    assert_eq!(frames(&c), before, "not even the cursor");
    assert!(c
        .store
        .get_meta::<Value>("self.mine.cursor")
        .unwrap()
        .is_none());
    assert_eq!(r.runs, 6);
    assert!(r.found >= 1 && r.rows.len() <= 5);
    let m = &r.rows[0];
    assert_eq!((m.occurrences, m.sessions), (6, 6));
    assert!(m.steps.iter().any(|s| s.contains("git merge --no-ff $REF")));
    assert!(m.tokens_per_occ > 0 && m.usd_per_occ > 0.0);
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("feat/job"), "shapes, never values");
}

#[test]
fn self_patterns_is_the_owners_alone() {
    let c = core_with(SelfMode::Off, false);
    let stranger = Answerer {
        label: "discord:314159265358979323".into(),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "314159265358979323".into(),
            channel_id: "900000000000000007".into(),
            guild_id: Some("900000000000000001".into()),
        }),
        agent: None,
    };
    let e = c
        .self_patterns(&SelfPatternsParams::default(), &stranger)
        .unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    let job = SelfPatternsParams {
        from_job: Some("ses_job".into()),
        ..Default::default()
    };
    assert!(c.self_patterns(&job, &Answerer::from("the CLI")).is_err());
}
