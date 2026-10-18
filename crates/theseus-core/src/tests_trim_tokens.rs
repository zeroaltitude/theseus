//! The trim's token rows (theseus-rwpbg.3): what the model's first request is told of its tools.
//!
//! The tools note names a default posture and the exceptions, the DM's defaults keep reminders and
//! defer tasks, a description names a tool by its wire name, and keep-warm stops at the model's
//! break-even.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::places::PlaceClass;
use crate::policy::Posture;
use crate::secrets::SecretBoard;
use crate::toolrun::{build_runtime, InlineLauncher, ToolRuntime};
use crate::tools_defer::PlaceKind;
use crate::Config;

fn runtime() -> ToolRuntime {
    let cfg = Config::example();
    let board = SecretBoard::new(cfg.secrets.keys().cloned(), Instant::now());
    build_runtime(&cfg, None, Arc::default(), Arc::new(InlineLauncher), board).unwrap()
}

/// The postures the note states, per tool: the listed exceptions, and the default for the rest.
fn stated(note: &str, tools: &[String]) -> BTreeMap<String, Posture> {
    let at = note.find("approval): ").expect("the postures line") + "approval): ".len();
    let line = note[at..].split(['.', '\n']).next().unwrap();
    let mut listed = BTreeMap::new();
    let mut default = None;
    for part in line.split("; ") {
        let (head, names) = part.split_once(": ").expect(part);
        if head == "every other tool" {
            default = Posture::parse(names);
            continue;
        }
        for n in names.split(", ") {
            listed.insert(n.to_string(), Posture::parse(head).expect(head));
        }
    }
    let default = default.expect("a default posture");
    tools
        .iter()
        .map(|t| (t.clone(), listed.get(t).copied().unwrap_or(default)))
        .collect()
}

/// Every tool's posture is still stated, and as the registry has it: the note names the posture most
/// tools share once and lists only the others, so it is shorter and says no less.
#[test]
fn the_tools_note_lists_exceptions() {
    let rt = runtime();
    let mut want = BTreeMap::new();
    for t in rt.registry.all().filter(|t| crate::tools_defer::listed(t.name())) {
        want.insert(t.wire_name(), rt.policy.posture(t.name()).0);
    }
    let note = rt.system_note();
    let wires: Vec<String> = want.keys().cloned().collect();
    assert_eq!(stated(&note, &wires), want, "{note}");
    let shared = rt.system_note_for(PlaceClass::Shared.into());
    let offered: Vec<String> = rt
        .registry
        .all()
        .filter(|t| rt.public_roots.is_empty() && crate::places::public_tool(t.name()))
        .filter(|t| crate::tools_defer::listed(t.name()))
        .map(|t| t.wire_name())
        .collect();
    let want_shared: BTreeMap<String, Posture> =
        offered.iter().map(|w| (w.clone(), want[w])).collect();
    assert_eq!(stated(&shared, &offered), want_shared, "{shared}");
    // The default is the commonest posture, named once; a tool of it is not listed.
    let mut count = BTreeMap::new();
    for p in want.values() {
        *count.entry(*p).or_insert(0) += 1;
    }
    let (default, n) = count.iter().max_by_key(|(_, n)| **n).unwrap();
    assert!(note.contains(&format!("every other tool: {}", default.as_str())));
    let line = note.lines().find(|l| l.contains("every other tool")).unwrap();
    assert!(
        line.split(", ").count() < want.len() - n + 3,
        "only the exceptions are named: {line}"
    );
    // The lines the descriptions say are gone.
    assert!(!note.contains("Prefer fs_read"));
    assert!(!note.contains("continue in the background"));
}

/// The DM keeps `wake_at` and its two companions in its first request and defers the task tools, which
/// had no call in 30 days (the owner's rule of 2026-10-14); a shared channel loses neither.
#[test]
fn the_dm_default_keeps_wake_at_and_defers_tasks() {
    use crate::tools_defer::family::of_wire;
    assert_eq!(of_wire("wake_at"), "reminders");
    assert_eq!(of_wire("wake_move"), "reminders");
    assert_eq!(of_wire("wake_cancel"), "reminders");
    assert_eq!(of_wire("task_create"), "tasks");
    assert_eq!(PlaceKind::Dm.defaults(), ["memory", "reminders", "web"]);
    assert_eq!(
        PlaceKind::Channel.defaults(),
        ["memory", "tasks", "reminders", "web", "channels"]
    );
    assert_eq!(PlaceKind::Cli.defaults(), ["memory"]);
    assert!(PlaceKind::Task.defaults().is_empty());
}

/// Every tool a description names is a tool the model has, by the wire name it sees: no `wake.move`
/// where the model's tool is `wake_move`.
#[test]
fn descriptions_name_wire_tools() {
    let rt = runtime();
    let defs = rt.definitions_for(PlaceClass::Private.into());
    let wires: BTreeSet<String> = rt.registry.all().map(|t| t.wire_name()).collect();
    let dotted: BTreeSet<String> = rt.registry.all().map(|t| t.name().to_string()).collect();
    let prefixes: BTreeSet<&str> = wires.iter().filter_map(|w| w.split('_').next()).collect();
    let mut texts = Vec::new();
    let mut props = BTreeSet::new();
    fn walk(v: &Value, texts: &mut Vec<String>, props: &mut BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if k == "description" {
                        if let Some(s) = x.as_str() {
                            texts.push(s.to_string());
                        }
                    }
                    if k == "properties" {
                        props.extend(x.as_object().into_iter().flatten().map(|(p, _)| p.clone()));
                    }
                    walk(x, texts, props);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, texts, props)),
            _ => {}
        }
    }
    for d in &defs {
        walk(d, &mut texts, &mut props);
    }
    let mut wrong = Vec::new();
    for text in &texts {
        for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')) {
            let token = token.trim_matches(|c| c == '.' || c == '_');
            if dotted.contains(token) {
                wrong.push(format!("{token}: the model's tool is {}", token.replace('.', "_")));
            } else if token.contains('_')
                && prefixes.contains(token.split('_').next().unwrap())
                && !wires.contains(token)
                && !props.contains(token)
                && token.matches('_').count() <= 2
                && !ALLOWED.contains(&token)
            {
                wrong.push(format!("{token}: not a tool the model has"));
            }
        }
    }
    wrong.sort();
    wrong.dedup();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Words that start like a tool's name and are not one.
const ALLOWED: &[&str] = &[];

/// A read costs the cache-read price on the prefix, a return after expiry one 1-hour write, so
/// reads past `(write_1h / read) x keep_warm_minutes` cost more than the rewrite they avoid:
/// Haiku 5.5 (write 20x its read) stops at 18 h 20 min where Sonnet 5.5 (40x) keeps its 24 h.
#[tokio::test]
async fn keep_warm_stops_at_break_even() {
    use crate::keep_warm::{break_even_hours, Outcome};
    use crate::tests_keep_warm::{build, one_hour, reads, session, turn, MIN};
    use theseus_protocol::SessionKind;

    let cat = crate::catalog::Catalog::builtin();
    let hours = |m: &str| break_even_hours(cat.get(m).unwrap(), 55.0).unwrap();
    assert!((hours("claude-haiku-5-5") - 55.0 * 20.0 / 60.0).abs() < 1e-9);
    assert!((hours("claude-sonnet-5-5") - 55.0 * 40.0 / 60.0).abs() < 1e-9);
    for (model, cap) in [
        ("claude-fable-5-1", 73.3),
        ("claude-opus-5-5", 36.7),
        ("claude-sonnet-5-5", 36.7),
        ("claude-sonnet-5", 18.3),
        ("claude-opus-5", 18.3),
        ("claude-haiku-5-5", 18.3),
        ("claude-haiku-4-5", 18.3),
        ("glm-5.3-flash", 4.9),
    ] {
        eprintln!("keep-warm break-even, {model}: {:.1} h", hours(model));
        assert!((hours(model) - cap).abs() < 0.1, "{model}: {}", hours(model));
    }

    let dir = tempfile::tempdir().unwrap();
    let mut cfg = one_hour(24);
    cfg.model.live = "haiku".into();
    let (core, fake) = build(dir.path(), cfg);
    let live = core.live_profile().0;
    let sid = session(&core, SessionKind::Conversation, None);
    tokio::time::pause();
    turn(&core, &sid, "a long conversation").await;
    tokio::time::advance(17 * 60 * MIN).await;
    assert!(
        matches!(core.runner.keep_warm_once(&sid, &live).await, Outcome::Read { .. }),
        "before its model's break-even: read"
    );
    tokio::time::advance(2 * 60 * MIN).await;
    assert_eq!(
        core.runner.keep_warm_once(&sid, &live).await,
        Outcome::Forgotten("its window closed".into()),
        "past it, though the profile's 24 hours are not"
    );
    assert_eq!(reads(&fake).len(), 1);
}

/// A search says a family was wanted, not that it was used: only a call counts toward what a place
/// loads, live and read back from the ledger.
#[test]
fn a_search_alone_is_not_use() {
    use crate::tools_defer::board::{Board, HiddenTool};
    use crate::tools_defer::select::DAY_MS;
    let now = 50 * DAY_MS;
    let hidden = |w: &str| HiddenTool {
        wire: w.into(),
        family: crate::tools_defer::family::of_wire(w),
        line: "x".into(),
    };
    let b = Board::default();
    for i in 0..12 {
        b.note_request("s", PlaceKind::Cli, None, ["files".to_string()].into(), vec![hidden("task_create")]);
        b.searched("s", "tasks", &["task_create".to_string()]);
        let t = b.end_turn("s", PlaceKind::Cli, now + i);
        assert!(t.used.is_empty(), "no call was made");
        assert_eq!(t.searched, ["tasks"]);
    }
    let s = b.stats(PlaceKind::Cli);
    assert!(s.sum_since(0, |d| &d.used).is_empty(), "twelve searches, no use");
    assert_eq!(s.sum_since(0, |d| &d.searched)["tasks"], 12, "kept as its own column");
    assert!(s.last_used.is_empty());
    // Read back from the rows the same turns wrote.
    let back = Board::default();
    for i in 0..12 {
        let row = serde_json::json!({"place": "cli", "loaded": ["files"], "used": [],
            "searched": ["tasks"], "unused": []});
        back.apply_used(now + i, &row);
    }
    assert!(back.stats(PlaceKind::Cli).sum_since(0, |d| &d.used).is_empty());
    // A call is use.
    b.note_request("s", PlaceKind::Cli, None, ["files".to_string()].into(), vec![]);
    b.called("s", "task_create");
    b.end_turn("s", PlaceKind::Cli, now + 99);
    assert_eq!(b.stats(PlaceKind::Cli).sum_since(0, |d| &d.used)["tasks"], 1);
}

/// The owner's config in shape (`theseus-ykc7o`).
const OWNER_SHAPE: &str = include_str!("../fixtures/owner_shape.toml");

fn rig(toml: &str) -> (Arc<crate::Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::parse(toml).unwrap().0;
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake, store);
    parts.cfg.tools.defer.enabled = true;
    (crate::Core::build(parts).unwrap(), dir)
}

/// Twenty turns of a place that each called `families`: the ledger the learned rule reads.
fn seed(core: &crate::Core, place: &str, families: &[&str]) {
    let now = core.kernel.now_ms();
    for i in 0..20 {
        let row = serde_json::json!({"place": place, "loaded": ["files"], "used": families,
            "searched": [], "unused": []});
        core.runner.tools.defer.apply_used(now - 1_000 * (i + 1), &row);
    }
}

/// The owner's cli with the tasks and the AWS tools learned (install #30's ledger counted searches as
/// use, and loaded them: 13,665 tokens exact against 8,000): the first request is back under its
/// budget, and a demoted family is one search away.
#[test]
fn the_owner_shaped_cli_with_learned_tasks_fits_8000() {
    let (core, _dir) = rig(OWNER_SHAPE);
    let fresh = core.first_request("cli").unwrap();
    eprintln!("census, cli, an empty ledger: {} tokens", fresh.tokens);
    seed(&core, "cli", &["tasks", "aws"]);
    let learned = core.first_request("cli").unwrap();
    eprintln!("census, cli, tasks and aws learned: {} tokens", learned.tokens);
    assert!(
        learned.tokens <= crate::first_request::CLI_TASK_TOKENS,
        "{} tokens over the budget",
        learned.tokens
    );
    for demoted in ["task_create", "aws_call"] {
        assert!(!learned.loaded.contains(&demoted.to_string()), "{demoted} is deferred");
    }
    // The listing says why, and the family is still offered: a search away.
    let list = core.tool_defer();
    let cli = list.places.iter().find(|p| p.place == "cli").unwrap();
    let tasks = cli.families.iter().find(|f| f.family == "tasks").unwrap();
    eprintln!("census line, cli: {} {} tokens, loaded {}, {}", tasks.family, tasks.tokens, tasks.loaded, tasks.why);
    assert!(!tasks.loaded && tasks.why.contains("over the place's budget"), "{tasks:?}");
    assert!(tasks.tools.contains(&"task_create".to_string()));
    // Where the learned families fit, they load: the DM's budget has room for tasks.
    seed(&core, "dm", &["tasks"]);
    let dm = core.first_request("dm").unwrap();
    assert!(dm.tokens <= crate::first_request::DM_TOKENS);
}

/// The DM before the trim loaded every task tool (about 5,900 tokens of 13,290); now it loads the
/// reminders alone.
#[test]
fn the_dm_first_request_defers_the_task_tools() {
    let (core, _dir) = rig(OWNER_SHAPE);
    let dm = core.first_request("dm").unwrap();
    eprintln!("census, dm: {} tokens", dm.tokens);
    for t in ["task_create", "task_update", "task_close", "task_claim", "task_split"] {
        assert!(!dm.loaded.contains(&t.to_string()), "{t}");
    }
    for t in ["wake_at", "wake_move", "wake_cancel"] {
        assert!(dm.loaded.contains(&t.to_string()), "{t}");
    }
    let channel = core.first_request("channel").unwrap();
    assert!(channel.loaded.contains(&"task_create".to_string()));
    assert!(channel.loaded.contains(&"wake_at".to_string()));
}
