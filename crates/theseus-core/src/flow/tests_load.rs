//! Freeze, load, retire: the frozen copy is the digest's, a secret never
//! reaches it, a self-minted load asks the gate, and the owner's revert
//! unloads and keeps the copy.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfRevertParams;

use super::load::Board;
use super::spec::{repo_id, Spec};
use super::tests::land;
use super::tests_run::{git, load, result, Repo};
use crate::approval::Answerer;
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, rig_knowing, session, turn, Rig};

fn rig() -> (Rig, Repo) {
    rig_for(vec![])
}

/// `rig`, its core's scrubber knowing `values` as the vault's.
fn rig_for(values: Vec<(String, String)>) -> (Rig, Repo) {
    let r = rig_knowing(
        |c| {
            c.policy.tools.remove("proc.run");
            c.tools.defer.enabled = false;
        },
        |req: &ProviderRequest| {
            if answers_a_call(req) {
                Scripted::text("Done.")
            } else {
                Scripted::tools(
                    "Landing.",
                    &[("L1", "flow__repo_land", json!({"branch": "topic"}))],
                )
            }
        },
        values,
    );
    let repo = Repo::init(&r.root);
    (r, repo)
}

fn rows_by_kind(r: &Rig) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (_, row) in r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(5000)
        .unwrap()
    {
        *out.entry(row.kind).or_default() += 1;
    }
    out
}

fn revert(name: &str) -> SelfRevertParams {
    SelfRevertParams {
        merge: name.into(),
        author: None,
        discord: None,
        from_job: None,
    }
}

/// A flow's steps are ordinary calls: each writes the rows any call of the
/// model writes, one set a step, and the run adds one `flow.ran`.
#[tokio::test]
async fn each_step_writes_its_own_rows() {
    let (r, repo) = rig();
    repo.branch("topic", "b.txt", "tide\n");
    load(&r, &land(&repo_id(&r.root)));
    let sid = session(&r.core, None);
    let base = rows_by_kind(&r);
    turn(&r.core, &sid, "land it").await;
    let now = rows_by_kind(&r);
    let grew = |k: &str| now.get(k).copied().unwrap_or(0) - base.get(k).copied().unwrap_or(0);
    // Four steps ran (test, merge, retest, push) beside the flow's own call.
    assert_eq!(grew("flow.ran"), 1);
    assert_eq!(grew("tool.job_started"), 4, "a job a step: {now:?}");
    assert_eq!(
        grew("tool.notified"),
        4,
        "each at the posture its own call has: {now:?}"
    );
    // The flow's call, its four steps, and the model's two requests.
    assert_eq!(grew("action.planned"), 7, "{now:?}");
    assert_eq!(grew("action.succeeded"), 7, "{now:?}");
    for i in 1..=4 {
        let (status, _, meta) = result(&r, &sid, &format!("L1.{i}"));
        assert_eq!(status, crate::node::ResultStatus::Ok);
        assert!(meta["exit_code"] == 0, "step {i}: {meta}");
    }
    // The step calls are in the transcript as the model's would be.
    let calls = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| matches!(&n.body, crate::node::Body::ToolCall { tool, .. } if tool == "proc.run"))
        .count();
    assert_eq!(calls, 4);
}

/// A byte changed in the frozen copy, or the copy swapped for another valid
/// one, fails its read; a load over a different spec of a frozen digest
/// leaves the copy as it is.
#[tokio::test]
async fn the_digest_freezes_the_spec() {
    let (r, _repo) = rig();
    let info = load(&r, &land(&repo_id(&r.root)));
    let frozen = std::path::PathBuf::from(&info.frozen).join("spec.json");
    let flows = Board::new(r.core.tools.flows.root().to_path_buf());
    assert!(flows.read_frozen("repo_land", &info.digest).is_ok());
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&frozen).unwrap().permissions());
    assert_eq!(mode & 0o222, 0, "read-only");

    crate::extend::freeze::make_writable(frozen.parent().unwrap()).unwrap();
    let good = std::fs::read(&frozen).unwrap();
    std::fs::set_permissions(&frozen, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    // A byte of the content changed, its digest field left as it was.
    let tampered = String::from_utf8(good.clone())
        .unwrap()
        .replace("landed", "lended");
    std::fs::write(&frozen, tampered).unwrap();
    let e = flows
        .read_frozen("repo_land", &info.digest)
        .unwrap_err()
        .to_string();
    assert!(e.contains("changed after it was sealed"), "{e}");
    // Another valid, sealed spec put in its place.
    let mut other: Value = serde_json::from_slice(&good).unwrap();
    other["exits"]["ok"] = json!("landed anything");
    other.as_object_mut().unwrap().remove("digest");
    let swapped = Spec::parse(other.to_string().as_bytes()).unwrap();
    std::fs::write(&frozen, swapped.frozen_bytes()).unwrap();
    let e = flows
        .read_frozen("repo_land", &info.digest)
        .unwrap_err()
        .to_string();
    assert!(e.contains("not repo_land"), "{e}");
    std::fs::write(&frozen, &good).unwrap();
    assert!(flows.read_frozen("repo_land", &info.digest).is_ok());
}

/// The scrubber over a spec: a value the vault holds is refused, and nothing
/// is frozen or recorded.
#[tokio::test]
async fn a_vault_value_in_a_constant_never_reaches_a_stored_spec() {
    let token = format!("{}{}", "ghp_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8");
    let (r, _repo) = rig_for(vec![(token.clone(), "deploy_key".into())]);
    let mut spec = land(&repo_id(&r.root));
    spec["steps"][0]["args"] = json!({"argv": ["echo", token]});
    let path = r.root.parent().unwrap().join("secret.flow.json");
    std::fs::write(&path, spec.to_string()).unwrap();
    let e = r
        .core
        .flow_load(&path, "test", "test")
        .unwrap_err()
        .to_string();
    assert!(e.contains("scrubber"), "{e}");
    assert!(!e.contains(&token));
    assert!(r.core.flow_list().is_empty());
    assert!(!rows_by_kind(&r).contains_key("flow.loaded"));
    assert!(!r.core.tools.flows.root().exists(), "nothing frozen");
}

/// The scrubber guesses no shape: a constant that merely looks like a token
/// is the flow's own and loads.
#[tokio::test]
async fn a_token_shaped_constant_the_vault_never_gave_loads() {
    let (r, _repo) = rig();
    let token = format!("{}{}", "ghp_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8");
    let mut spec = land(&repo_id(&r.root));
    spec["steps"][0]["args"] = json!({"argv": ["echo", token]});
    let path = r.root.parent().unwrap().join("shaped.flow.json");
    std::fs::write(&path, spec.to_string()).unwrap();
    r.core.flow_load(&path, "test", "test").unwrap();
    assert_eq!(r.core.flow_list().len(), 1);
}

/// `theseus self revert flow:<name>` unloads it with `flow.retired` by the
/// owner, keeps the frozen copy, and answers; a flow not loaded is refused.
#[tokio::test]
async fn the_owners_revert_unloads_a_flow_and_keeps_its_copy() {
    let (r, _repo) = rig();
    let info = load(&r, &land(&repo_id(&r.root)));
    assert_eq!(r.core.flow_list().len(), 1);
    let done = r
        .core
        .self_revert(
            &revert("flow:repo_land"),
            &Answerer::from("the CLI"),
            &Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        (done.merge.as_str(), done.revert.as_str(), done.changed),
        ("flow:repo_land", info.digest.as_str(), true)
    );
    assert!(r.core.flow_list().is_empty());
    assert!(
        std::path::Path::new(&info.frozen)
            .join("spec.json")
            .exists(),
        "never deleted"
    );
    let rows = rows_by_kind(&r);
    assert_eq!((rows["flow.loaded"], rows["flow.retired"]), (1, 1));
    assert!(super::load::LoadedSet::read(&r.core.store)
        .unwrap()
        .loaded
        .is_empty());
    let again = r
        .core
        .self_revert(
            &revert("flow:repo_land"),
            &Answerer::from("the CLI"),
            &Default::default(),
        )
        .await
        .unwrap_err();
    assert!(format!("{again:#}").contains("no flow named"), "{again:#}");
    // A job's shell is refused as any revert is.
    load(&r, &land(&repo_id(&r.root)));
    let job = SelfRevertParams {
        from_job: Some("ses_job".into()),
        ..revert("flow:repo_land")
    };
    assert!(r
        .core
        .self_revert(&job, &Answerer::from("the CLI"), &Default::default())
        .await
        .is_err());
    assert_eq!(r.core.flow_list().len(), 1, "still loaded");
}

/// A load or retire by the self principal is a self step: with the mode off
/// (the default) or the switch halted it reads and writes nothing.
#[tokio::test]
async fn a_self_minted_load_asks_the_gate_first() {
    let (r, _repo) = rig();
    let path = r.root.parent().unwrap().join("land.flow.json");
    std::fs::write(&path, land(&repo_id(&r.root)).to_string()).unwrap();
    let before = rows_by_kind(&r);
    let e = r
        .core
        .flow_load(&path, "self", "self")
        .unwrap_err()
        .to_string();
    assert!(e.contains("self-improvement is off"), "{e}");
    assert_eq!(rows_by_kind(&r), before, "nothing written");
    assert!(r.core.flow_list().is_empty());
    assert!(!r.core.tools.flows.root().exists(), "nothing frozen");
    assert!(r
        .core
        .flow_retire("repo_land", "self", "self")
        .unwrap_err()
        .to_string()
        .contains("off"));
}

/// A flow called outside its repository is refused at the call, whatever
/// reached it.
#[tokio::test]
async fn a_flow_called_out_of_scope_does_not_run() {
    let (r, repo) = rig();
    repo.branch("topic", "b.txt", "tide\n");
    load(&r, &land("r:abcdef012345"));
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "land it").await;
    let (status, text, _) = result(&r, &sid, "L1");
    assert_eq!(status, crate::node::ResultStatus::Error, "{text}");
    assert!(text.contains("is not offered in"), "{text}");
    assert_eq!(
        git(&repo.origin, &["rev-parse", "main"]),
        git(&repo.work, &["rev-parse", "origin/main"])
    );
}
