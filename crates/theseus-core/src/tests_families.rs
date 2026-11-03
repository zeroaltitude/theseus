//! The families judgment (`families.v1`, theseus-xlri5): one per prompt, none on a model with tool
//! search, and the labelled table. (The raise of a named or Jev-judged family past the place's budget
//! was tested on a Haiku detour's lean request, which route.v10 does not make: no pick detours.)

use serde_json::Value;
use theseus_judge::fake::FakeJev;

use crate::tests_detour::{owner_shape, rig_haiku};
use crate::tests_places::rows;
use crate::tests_route::{mode, turn};

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
