//! A trivial detour carries `tools_find` and what its message names (theseus-d9je7). The audit
//! found the tools were 86 to 88 per cent of a greeting's request; the owner asked for the one
//! tool that can find the rest. A quick detour keeps its place's set.

use theseus_judge::fake::FakeJev;

use crate::tests_detour::rig_haiku;
use crate::tests_places::{offered, rows};
use crate::tests_route::{mode, turn};
use crate::tests_route_correct::ask;

/// The tokens the request's tools are, counted as `tests_route_defer` counts them.
fn tools_tokens(req: &crate::provider::ProviderRequest) -> u64 {
    crate::tools_defer::tools_bytes(&req.tools) * 10 / 24
}

fn with_aws(c: &mut crate::Config) {
    c.places.owner = Some(vec![format!("discord:{}", crate::tests_places::OWNER)]);
    c.aws = toml::from_str(
        "[accounts.123456789012]\nregion = \"us-west-2\"\nendpoint = \"http://127.0.0.1:9\"\n",
    )
    .unwrap();
}

/// The key test: a greeting judged trivial, at the cli and in the owner's DM, sends `tools_find`
/// alone, and its row says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trivial_detour_carries_only_tools_find() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let (r, haiku) = rig_haiku(&jev, 2, with_aws);
    let owner = crate::tests_places::OWNER;
    let dm = crate::tests_places::session(&r.core, Some(&format!("dm:{owner}")));
    let cli = turn(&r.core, None, "hey, thanks!", None).await;
    assert_eq!(cli.route.as_ref().unwrap().reason, "detour");
    ask(&r.core, Some(&dm), "hello again").await;
    for (place, req) in ["cli", "dm"].iter().zip(haiku.requests()) {
        assert_eq!(offered(&req), ["tools_find"], "{place}");
        let tokens = tools_tokens(&req);
        assert!(tokens < 1_000, "{place}: the tools are {tokens} tokens");
    }
    let parts: Vec<String> = rows(&r.core, "context.detoured")
        .iter()
        .flat_map(|row| row["parts"].as_array().unwrap().clone())
        .filter(|p| p["block"] == "tools")
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(parts, ["trivial: tools_find only"; 2]);
}

/// A family the message names still loads on a trivial turn, and no other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trivial_detour_naming_a_family_still_loads_it() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let (r, haiku) = rig_haiku(&jev, 1, with_aws);
    turn(&r.core, None, "hey, call aws_whoami for me", None).await;
    let names = offered(&haiku.requests()[0]);
    assert!(names.contains(&"aws_whoami".to_string()), "{names:?}");
    assert!(names.contains(&"tools_find".to_string()), "{names:?}");
    for gone in ["fs_read", "proc_run", "aws_s3_list"] {
        assert!(!names.contains(&gone.to_string()), "{gone} in {names:?}");
    }
}

/// A quick detour keeps its place's families (`files` and `commands` at the cli).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quick_detour_keeps_its_tools() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "quick", 0.92);
    let (r, haiku) = rig_haiku(&jev, 1, |_| {});
    let res = turn(&r.core, None, "Which mooring did we survey first?", None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "detour");
    let names = offered(&haiku.requests()[0]);
    for kept in ["fs_read", "proc_run", "tools_find"] {
        assert!(names.contains(&kept.to_string()), "{kept} in {names:?}");
    }
}

/// route.v9's two detours (theseus-gezlj, theseus-tqomi): a `run_tool` turn
/// keeps its place's tools, the command tool among them, though its message
/// names none; a `commentary` one carries `tools_find` alone, as a trivial one
/// does, and its row says so. The fake Jev answers from the acting pack's own
/// options, so this proof needs route.v9 to be the acting pack. It is ignored
/// until it is, and `--run-ignored all` shows it failing on route.v8: a proof
/// that returned `Ok` without asserting anything was a pass by an empty set.
///
/// Beyond the offered tools it pins the two things that make them so: the
/// `[routing.classes.run_tool]` table the rig resolved (Haiku, then Sonnet, the
/// cn rubric's §7.4) and `BARE_DETOURS`' membership (`commentary` in it,
/// `run_tool` out, or the turn would be offered `tools_find` alone).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "runs once route.v9 is the acting pack (theseus-tqomi)"]
async fn a_run_tool_detour_keeps_its_tools_and_a_commentary_one_carries_tools_find() {
    let acting = theseus_judge::pack::by_name(crate::judge::inbound::ROUTE_PACK).unwrap();
    for kind in ["run_tool", "commentary"] {
        let named = acting
            .question("mode")
            .is_some_and(|q| q.options.iter().any(|o| o.id == kind));
        assert!(named, "the acting pack ({}) names no {kind}", acting.name());
    }
    assert!(crate::config::routing::is_bare_detour("commentary"));
    assert!(!crate::config::routing::is_bare_detour("run_tool"));
    assert!(crate::config::routing::is_detour("run_tool"));
    for (kind, message, bare) in [
        (
            "run_tool",
            "Run slow.sh census 3 and give me its last line.",
            false,
        ),
        (
            "commentary",
            "Harnesses writing harnesses, it all fits.",
            true,
        ),
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, kind, 0.95);
        let table = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = table.clone();
        let (r, haiku) = rig_haiku(&jev, 1, move |c| {
            *seen.lock().unwrap() = c.routing.classes.of(kind).to_vec();
        });
        let table = table.lock().unwrap().clone();
        match bare {
            false => assert_eq!(table, ["haiku", "sonnet"], "the {kind} table"),
            true => assert_eq!(table, ["haiku"], "the {kind} table"),
        }
        let res = turn(&r.core, None, message, None).await;
        assert_eq!(res.route.as_ref().unwrap().reason, "detour", "{kind}");
        assert_eq!(res.profile, "haiku", "{kind}");
        let requests = haiku.requests();
        assert_eq!(requests.len(), 1, "{kind}: one request reached the model");
        let names = offered(&requests[0]);
        match bare {
            true => assert_eq!(names, ["tools_find"], "{kind}"),
            false => {
                for kept in ["fs_read", "proc_run", "tools_find"] {
                    assert!(
                        names.contains(&kept.to_string()),
                        "{kind}: {kept} in {names:?}"
                    );
                }
            }
        }
        let row = rows(&r.core, "context.detoured").pop().expect("a detour");
        assert_eq!(row["kind"], kind);
    }
}
