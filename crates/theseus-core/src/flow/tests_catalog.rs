//! Where a flow is offered: in a request's tools, as a deferred family of its
//! own, only in its scope; nothing at all when none is loaded; and gone when
//! it is unloaded.

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfRevertParams;

use super::spec::repo_id;
use super::tests::land;
use super::tests_run::{git, load, result, Repo};
use crate::approval::Answerer;
use crate::catalog::CatalogRow;
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, last_user, rig_deferring, session, turn, Rig};
use crate::tools_defer::{tools_bytes, DEFERRED_TOKENS};

/// A model that asks `tools_find` when told to FIND, else says hello.
fn finds(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    if last_user(req).contains("FIND") {
        return Scripted::tools(
            "",
            &[("f1", "tools_find", json!({"query": "land a branch"}))],
        );
    }
    Scripted::text("Hello.")
}

fn rig(search: bool) -> (Rig, Repo) {
    let r = rig_deferring(
        move |c| {
            c.policy.tools.remove("proc.run");
            if !search {
                c.catalog.insert(
                    "claude-sonnet-5-5".into(),
                    CatalogRow {
                        tool_search: Some(false),
                        ..Default::default()
                    },
                );
            }
        },
        finds,
    );
    let repo = Repo::init(&r.root);
    (r, repo)
}

fn tool<'a>(req: &'a ProviderRequest, name: &str) -> Option<&'a Value> {
    req.tools.iter().find(|t| t["name"] == name)
}

const FLOW: &str = "flow__repo_land";

/// A session working in `dir`.
fn session_in(r: &Rig, dir: &std::path::Path) -> String {
    let sid = session(&r.core, None);
    let mut rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    rec.dir = Some(dir.to_string_lossy().into_owned());
    r.core.store.put_session(&sid, &rec).unwrap();
    sid
}

/// A loaded flow is a deferred tool in its repository's sessions, and in no
/// other's; it costs the constant a deferred tool costs; and where the model
/// has no provider search, `tools_find` names it.
#[tokio::test]
async fn a_flow_is_deferred_and_scoped() {
    let (r, _repo) = rig(true);
    // Before: a session in the repository, no flow loaded.
    let before = session(&r.core, None);
    turn(&r.core, &before, "hello").await;
    let none = r.requests().pop().unwrap();
    assert!(tool(&none, FLOW).is_none());

    load(&r, &land(&repo_id(&r.root)));
    // Another repository's session, beside the work tree.
    let other = r.root.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    let (here, there) = (session(&r.core, None), session_in(&r, &other));
    turn(&r.core, &here, "hello").await;
    let with = r.requests().pop().unwrap();
    turn(&r.core, &there, "hello").await;
    let away = r.requests().pop().unwrap();

    let t = tool(&with, FLOW).expect("offered in its repository");
    assert_eq!(t["defer_loading"], true, "deferred, with no family loaded");
    assert!(t["description"].as_str().unwrap().contains("Land a branch"));
    assert!(
        tool(&away, FLOW).is_none(),
        "never in another repository's session"
    );
    // A deferred tool costs the constant, however many there are.
    assert_eq!(DEFERRED_TOKENS, 93);
    assert_eq!(
        tools_bytes(&with.tools),
        tools_bytes(&none.tools),
        "one more deferred tool adds nothing to the estimate"
    );
}

#[tokio::test]
async fn a_flow_names_the_kinds_of_place_it_is_offered_in() {
    let (r, _repo) = rig(true);
    let mut spec = land(&repo_id(&r.root));
    spec["scope"]["places"] = json!(["task"]);
    load(&r, &spec);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "hello").await;
    assert!(
        tool(&r.requests().pop().unwrap(), FLOW).is_none(),
        "a CLI session is not a task's"
    );
}

/// Where the provider has no tool search, the model finds it by asking.
#[tokio::test]
async fn tool_search_finds_a_flow_by_what_it_does() {
    let (r, _repo) = rig(false);
    let mut spec = land(&repo_id(&r.root));
    spec["description"] = json!("Land a branch: test, merge, push.");
    load(&r, &spec);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "FIND it").await;
    let req = r.requests().into_iter().next().unwrap();
    assert!(tool(&req, FLOW).is_none(), "left out of the first request");
    let (_, text, _) = result(&r, &sid, "f1");
    assert!(text.contains(FLOW), "{text}");
    assert!(text.contains("flow:repo_land"), "{text}");
}

/// With no flow loaded, a request is what it was: the catalog has no flow in
/// it, and one loaded and unloaded leaves the next request's bytes the same.
#[tokio::test]
async fn no_flow_loaded_changes_nothing() {
    let (r, _repo) = rig(true);
    let s1 = session(&r.core, None);
    turn(&r.core, &s1, "hello").await;
    let first = r.requests().pop().unwrap();
    assert!(!r.core.tools.flows.any());
    assert!(!first
        .tools
        .iter()
        .any(|t| t["name"].as_str().unwrap().starts_with("flow")));
    assert!(!crate::tools_defer::family::of_wire("proc_run").starts_with("flow"));

    load(&r, &land(&repo_id(&r.root)));
    let s2 = session(&r.core, None);
    turn(&r.core, &s2, "hello").await;
    let loaded = r.requests().pop().unwrap();
    assert!(tool(&loaded, FLOW).is_some());

    r.core
        .self_revert(
            &SelfRevertParams {
                merge: "flow:repo_land".into(),
                author: None,
                discord: None,
                from_job: None,
            },
            &Answerer::from("the CLI"),
            &Default::default(),
        )
        .await
        .unwrap();
    let s3 = session(&r.core, None);
    turn(&r.core, &s3, "hello").await;
    let after = r.requests().pop().unwrap();
    assert_eq!(
        serde_json::to_string(&after.tools).unwrap(),
        serde_json::to_string(&first.tools).unwrap(),
        "the tools are the bytes they were"
    );
    assert_eq!(after.system, first.system, "and so is the system");
}
