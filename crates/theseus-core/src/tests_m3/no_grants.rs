//! What is left once earned autonomy and scoped grants are gone: a question
//! carries no "don't ask again" (a plan's other yes, edits mode, is the one
//! offer a question may make), the rows the two wrote read back as they were
//! written, and a config that still has `[policy.earned]` loads.

use super::*;
use serde_json::json;

fn run_of(id: &str, argv: &[&str]) -> Scripted {
    Scripted::tools("", &[(id, "proc_run", json!({"argv": argv}))])
}

fn write_of(id: &str, path: &str) -> Scripted {
    Scripted::tools(
        "",
        &[(id, "fs_write", json!({"path": path, "content": "x\n"}))],
    )
}

/// A command's question and an edit's each carry the plain yes and the no,
/// and no offer: the daemon makes no grant, so none is offered.
#[tokio::test]
async fn a_question_the_posture_asked_offers_no_grant() {
    for script in [
        run_of("g0", &["echo", "hello", "x"]),
        write_of("g1", "a.txt"),
    ] {
        let r = rig(vec![script, Scripted::text("asked")]);
        let sid = {
            let rec = SessionRecord::new(SessionKind::Conversation, None);
            r.core.store.put_session(&rec.session_id, &rec).unwrap();
            rec.session_id
        };
        let res = turn(&r.core, Some(&sid), "do it").await;
        let corr = res.awaiting_confirm.clone().expect("the posture asks");
        let pending = r
            .core
            .pending_confirms(&sid)
            .unwrap()
            .into_iter()
            .find(|q| q.correlation_id == corr)
            .expect("still pending");
        assert!(pending.grant_offer.is_none(), "{:?}", pending.grant_offer);
        let ids: Vec<&str> = pending.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["approve", "decline"]);
        let asked = ledgered(&r, "tool.confirm_requested")
            .into_iter()
            .rfind(|q| q["correlation_id"] == corr.as_str())
            .expect("the question's row");
        assert!(asked["grant_offer"].is_null(), "{asked}");
    }
}

/// The owner's config of the weeks before has a `[policy.earned]` section;
/// it names nothing now, and the daemon still starts on it.
#[test]
fn a_config_with_the_retired_earned_section_still_loads() {
    let mut doc: toml::Table = crate::Config::EXAMPLE_TOML.parse().unwrap();
    let section: toml::Table =
        "enabled = false\nevery_days = 3\nmin_approvals = 9\nwindow_days = 14\n"
            .parse()
            .unwrap();
    doc.get_mut("policy")
        .and_then(|p| p.as_table_mut())
        .expect("the template's [policy]")
        .insert("earned".into(), section.into());
    let (_, warnings) = crate::Config::parse(&doc.to_string()).expect("a retired section loads");
    assert!(
        warnings.iter().any(|w| w.contains("[policy.earned]")),
        "the owner is told it names nothing: {warnings:?}"
    );
}
