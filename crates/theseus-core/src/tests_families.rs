//! A family the message names, or Jev judges relevant, loads past the place's
//! budget by exactly its size, under a hard ceiling (theseus-xlri5). The rig is
//! `tests_detour`'s: the owner's shape at the cli, and a Haiku 5.5 detour, which
//! has no tool search.

use serde_json::Value;
use theseus_judge::fake::FakeJev;
use theseus_protocol::TurnSubmitResult;

use crate::tests_detour::{owner_shape, rig_haiku};
use crate::tests_places::{offered, rows};
use crate::tests_route::{mode, turn};

/// One cli detour of `message` in the owner's shape, with `tweak` on the config: its result, the
/// first request's tool names, its `context.detoured` row and its `tools.used` row.
async fn detour(
    message: &str,
    tweak: impl FnOnce(&mut crate::Config),
) -> (TurnSubmitResult, Vec<String>, Value, Value) {
    let jev = FakeJev::start().unwrap();
    // A quick detour: a trivial one carries `tools_find` alone, nothing for the budget to raise (theseus-d9je7).
    mode(&jev, "quick", 0.95);
    let dir = tempfile::tempdir().unwrap();
    let (r, haiku) = rig_haiku(&jev, 1, |c| {
        owner_shape(c, dir.path());
        tweak(c);
        // A trivial turn on this rig is the small tier (1.0, theseus-d9je7): the raise under
        // test is held at `admit_ceiling`.
        c.tools.defer.admit_ceiling_small = c.tools.defer.admit_ceiling;
    });
    let res = turn(&r.core, None, message, None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    let names = offered(&haiku.requests()[0]);
    let detoured = rows(&r.core, "context.detoured").remove(0);
    let used = rows(&r.core, "tools.used").remove(0);
    (res, names, detoured, used)
}

fn est(row: &Value) -> u64 {
    row["est_tokens"].as_u64().unwrap()
}

/// The key test: a cli detour naming `aws_whoami` on the owner's shape, whose request is about
/// 7,300 tokens before any family it names, loads the AWS family past the cli's 8,000: its request
/// exceeds the same message's under the old cap (`admit_ceiling = 1.0`, which leaves the family out)
/// by the family's own `Fit` cost, within a rounding of a token per tool and the comma between two
/// (the estimate rounds the tools' bytes once, `Fit` once per tool), and stays under the ceiling;
/// `tools.used` and the result's footer name it, `by: named`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cli_detour_naming_aws_loads_it_past_the_budget_by_exactly_its_size() {
    let ask = "Call aws_whoami and tell me in one line which account you are on.";
    let (res, names, with, used) = detour(ask, |_| {}).await;
    let (_, names_before, without, used_before) =
        detour(ask, |c| c.tools.defer.admit_ceiling = 1.0).await;
    assert!(names.contains(&"aws_whoami".to_string()), "{names:?}");
    assert!(
        !names_before.iter().any(|n| n.starts_with("aws_")),
        "{names_before:?}"
    );
    assert!(used_before.get("raised").is_none(), "{used_before}");
    let raised = &used["raised"];
    let fams = raised["families"].as_array().unwrap();
    assert_eq!(fams.len(), 1, "{raised}");
    assert_eq!(
        (fams[0]["family"].as_str(), fams[0]["by"].as_str()),
        (Some("aws"), Some("named"))
    );
    let cost = fams[0]["tokens"].as_u64().unwrap();
    let aws_tools = names.iter().filter(|n| n.starts_with("aws_")).count() as u64;
    let grew = est(&with) - est(&without);
    eprintln!(
        "cli detour naming aws_whoami: {} tokens before (no AWS tool), {} after; the family {cost} by Fit, the request grew {grew}",
        est(&without),
        est(&with)
    );
    assert!(
        grew.abs_diff(cost) <= aws_tools + 1,
        "the request grew {grew}, the family costs {cost}"
    );
    let (budget, ceiling) = (
        crate::first_request::CLI_TASK_TOKENS,
        2 * crate::first_request::CLI_TASK_TOKENS,
    );
    assert!(est(&without) <= budget, "{without}");
    assert!(est(&with) > budget && est(&with) <= ceiling, "{with}");
    assert_eq!(raised["budget"], budget);
    assert_eq!(raised["ceiling"], ceiling);
    let line = res.raised.as_ref().unwrap().line();
    assert!(
        res.harness_lines().contains(&line) && line.starts_with("Tools raised the request to "),
        "{:?}",
        res.harness_lines()
    );
    assert!(
        line.contains(": aws +") && line.ends_with(" (named)."),
        "{line}"
    );
    // The same for a terminal, for the report's table: the terminals' family past the budget.
    let ask = "Open a terminal and run true.";
    let (_, names, with, _) = detour(ask, |_| {}).await;
    let (_, _, without, _) = detour(ask, |c| c.tools.defer.admit_ceiling = 1.0).await;
    assert!(names.contains(&"term_open".to_string()), "{names:?}");
    eprintln!(
        "cli detour naming a terminal: {} tokens before, {} after",
        est(&without),
        est(&with)
    );
}

/// The `families.v1` requests Jev was sent.
fn families_calls(jev: &FakeJev) -> Vec<Value> {
    jev.seen()
        .into_iter()
        .map(|s| s.body)
        .filter(|b| {
            b["questions"]
                .as_object()
                .is_some_and(|q| q.keys().any(|k| k.starts_with("families.v1/")))
        })
        .collect()
}

/// The question that asks about `family` in a `families.v1` request: `relevant.N` for the first
/// ten, `relevant_more.N` (from 1) for the next.
fn question_of(req: &Value, family: &str) -> String {
    let n = req["state"]["families"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == family)
        .and_then(|f| f["family"].as_u64())
        .unwrap_or_else(|| panic!("{family} not asked: {}", req["state"]));
    match n {
        1..=10 => format!("relevant.{n}"),
        _ => format!("relevant_more.{}", n - 10),
    }
}

/// The spans of `res`'s trace about `families.v1`.
fn family_spans(res: &TurnSubmitResult) -> Vec<theseus_protocol::Span> {
    fn walk(s: &theseus_protocol::Span, out: &mut Vec<theseus_protocol::Span>) {
        if s.attrs["pack"] == "families.v1" {
            out.push(s.clone());
        }
        for c in &s.children {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    if let Some(t) = &res.trace {
        walk(t, &mut out);
    }
    out
}

/// A rig in the owner's shape whose detours go to the Haiku stand-in, with `jev` scripted to
/// detour; `tweak` after the shape.
fn rig_on(
    jev: &FakeJev,
    dir: &std::path::Path,
    n: usize,
    tweak: impl FnOnce(&mut crate::Config),
) -> (
    crate::tests_route::Rig,
    std::sync::Arc<crate::provider::FakeProvider>,
) {
    // A quick detour, as `detour` above (theseus-d9je7).
    mode(jev, "quick", 0.95);
    rig_haiku(jev, n, |c| {
        owner_shape(c, dir);
        tweak(c);
        // A trivial turn on this rig is the small tier (1.0, theseus-d9je7): the raise under
        // test is held at `admit_ceiling`.
        c.tools.defer.admit_ceiling_small = c.tools.defer.admit_ceiling;
    })
}

/// A message with no family word and no tool name: Jev, scripted to judge the AWS family relevant,
/// admits it, `by: jev`; with `[tools.defer] judge_families = false` nothing extra loads and no
/// `families.v1` call is made (the rollback).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_family_jev_judges_relevant_loads_without_being_named() {
    let ask = "Which cloud account is this session on right now?";
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // Jev's answer is waited for, not raced: the most the config takes (theseus-powrh).
    let (r, haiku) = rig_on(&jev, dir.path(), 4, |c| c.tools.defer.judge_wait_ms = 2_000);
    // A first message, to read how the request numbers the families.
    turn(&r.core, None, "Say hello.", None).await;
    let calls = families_calls(&jev);
    assert_eq!(calls.len(), 1, "one judgment for the first message");
    jev.script(
        &question_of(&calls[0], "aws"),
        theseus_judge::fake::Scripted::Noul(0.95),
    );
    let res = turn(&r.core, None, ask, None).await;
    let names = offered(haiku.requests().last().unwrap());
    assert!(names.contains(&"aws_whoami".to_string()), "{names:?}");
    let raised = res
        .raised
        .as_ref()
        .expect("the AWS family raised the request");
    assert_eq!(
        raised
            .families
            .iter()
            .map(|f| (f.family.as_str(), f.by.as_str()))
            .collect::<Vec<_>>(),
        [("aws", "jev")]
    );
    assert!(
        res.harness_lines().iter().any(|l| l.ends_with(" (Jev).")),
        "{:?}",
        res.harness_lines()
    );
    // The rollback: Jev's per-family call off.
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (r, haiku) = rig_on(&jev, dir.path(), 2, |c| {
        c.tools.defer.judge_families = false
    });
    jev.script("relevant", theseus_judge::fake::Scripted::Noul(0.95));
    let res = turn(&r.core, None, ask, None).await;
    let names = offered(&haiku.requests()[0]);
    assert!(!names.iter().any(|n| n.starts_with("aws_")), "{names:?}");
    assert!(res.raised.is_none());
    assert!(families_calls(&jev).is_empty());
}

/// Jev holding its answer past `judge_wait_ms`: the turn does not wait for it (its wait ends long
/// before the answer is released, whatever the machine's load), drops the late answer
/// (`answered: false`), and the named rule still loads the family the message names. Jev known unreachable: no call, no wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jev_late_or_unreachable_falls_back_to_the_named_rule() {
    /// When Jev's held answer is released: far past any loaded wait.
    const RELEASE: std::time::Duration = std::time::Duration::from_secs(30);
    let ask = "Call aws_whoami and tell me in one line which account you are on.";
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // The judge's own timeouts are far past RELEASE's half, so a starved turn that reaches its wait
    // late finds the call held, never timed out into an answer.
    let (r, haiku) = rig_on(&jev, dir.path(), 2, |c| {
        c.judge.connect_secs = 120;
        c.judge.total_secs = 120;
    });
    // Only the families state has `holds`: the route's call answers.
    jev.hold_when("\"holds\":");
    let late = jev.clone();
    tokio::spawn(async move {
        tokio::time::sleep(RELEASE).await;
        late.release();
    });
    let t0 = std::time::Instant::now();
    let res = turn(&r.core, None, ask, None).await;
    let took = t0.elapsed();
    let names = offered(&haiku.requests()[0]);
    assert!(names.contains(&"aws_whoami".to_string()), "{names:?}");
    let waits: Vec<_> = family_spans(&res)
        .into_iter()
        .filter(|s| s.kind == "wait")
        .collect();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0].attrs["answered"], false);
    // Ordering, not a deadline: the held answer comes at RELEASE, and both the wait and the turn
    // ended well before it.
    let before = (RELEASE / 2).as_micros() as u64;
    assert!(
        waits[0].duration_us() < before,
        "waited {} µs",
        waits[0].duration_us()
    );
    assert!(took < RELEASE / 2, "the turn took {took:?}");
    let raised = res.raised.as_ref().unwrap();
    assert_eq!(raised.families[0].by, "named");
    // Unreachable: the session's own model has no tool search, so it would wait; Jev is known
    // unreachable, so nothing is asked and nothing waited for.
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (r, _) = rig_on(&jev, dir.path(), 4, |c| {
        c.judge.api_base = "http://127.0.0.1:9".into();
        c.catalog.insert(
            "claude-sonnet-5-5".into(),
            crate::catalog::CatalogRow {
                tool_search: Some(false),
                ..Default::default()
            },
        );
    });
    turn(&r.core, None, "Say done.", None).await;
    let t0 = std::time::Instant::now();
    while !r.core.runner.judge.jev_unreachable() {
        assert!(t0.elapsed().as_secs() < 10, "never unreachable");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let res = turn(&r.core, None, ask, None).await;
    assert!(family_spans(&res).is_empty(), "{:?}", family_spans(&res));
}

/// A three-loop turn on a detour, whose request is compiled again on the routed profile, makes one
/// `families.v1` call; a second prompt makes a second.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_families_judgment_per_prompt() {
    use crate::provider::{FakeProvider, Scripted};
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let dir = tempfile::tempdir().unwrap();
    let haiku = std::sync::Arc::new(FakeProvider::scripted(vec![
        Scripted::tools(
            "",
            &[("t1", "fs_read", serde_json::json!({"path": "notes-a.md"}))],
        ),
        Scripted::tools(
            "",
            &[("t2", "fs_read", serde_json::json!({"path": "notes-b.md"}))],
        ),
        Scripted::text("Neither file is there."),
        Scripted::text("Done."),
    ]));
    let p = haiku.clone();
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        1,
        |c| {
            c.routing.max_wait_ms = 5_000;
            owner_shape(c, dir.path());
            // Waited for, not raced (theseus-powrh).
            c.tools.defer.judge_wait_ms = 2_000;
        },
        move |parts| {
            parts.cfg.tools.defer.enabled = true;
            parts.providers.insert("anthropic".into(), p);
        },
    );
    let res = turn(
        &r.core,
        None,
        "Read the two notes files and say what they hold.",
        None,
    )
    .await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(res.loops, 3, "{res:?}");
    assert_eq!(families_calls(&jev).len(), 1);
    turn(&r.core, Some(&res.session_id), "And now say done.", None).await;
    assert_eq!(families_calls(&jev).len(), 2);
}

/// A model with the provider's tool search, and routing that cannot detour from it: no
/// `families.v1` call, and a family the message names raises nothing (the model searches).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_with_tool_search_asks_no_families_judgment_and_raises_nothing() {
    let jev = FakeJev::start().unwrap();
    // The cli's defaults, whose request (about 7,350 tokens) is under the budget, with an AWS account:
    // a model without tool search would admit the family it names.
    let (r, _) = rig_haiku(&jev, 1, |c| {
        c.aws = toml::from_str(
            "[accounts.123456789012]\nregion = \"us-west-2\"\nendpoint = \"http://127.0.0.1:9\"\n",
        )
        .unwrap();
        c.judge.packs.insert(
            crate::judge::inbound::ROUTE_PACK.into(),
            crate::tests_judge::off(),
        );
    });
    let res = turn(
        &r.core,
        None,
        "Call aws_whoami and tell me in one line which account you are on.",
        None,
    )
    .await;
    assert_eq!(res.profile, "sonnet");
    assert!(families_calls(&jev).is_empty());
    assert!(res.raised.is_none());
    let used = rows(&r.core, "tools.used");
    assert_eq!(used[0]["mechanism"], "search", "{}", used[0]);
    assert!(used[0].get("raised").is_none(), "{}", used[0]);
}

/// The judge's labelled set shows Jev the family lines the table gives the
/// model: a line the table changed is changed in the fixture too, or the set
/// measures a prompt nobody sends (theseus-capcs).
#[test]
fn the_labelled_families_say_the_tables_lines() {
    let cases = theseus_judge::eval_labelled::set("families.v1").unwrap();
    assert!(!cases.is_empty());
    let mut checked = 0;
    for case in &cases {
        for f in case.input["families"].as_array().unwrap() {
            let key = f["key"].as_str().unwrap();
            if let Some((_, line)) = crate::tools_defer::family::FAMILIES
                .iter()
                .find(|(k, _)| *k == key)
            {
                assert_eq!(
                    f["holds"].as_str().unwrap(),
                    *line,
                    "{}: family {key}",
                    case.name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}
