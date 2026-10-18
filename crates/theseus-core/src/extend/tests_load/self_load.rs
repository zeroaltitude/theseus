//! A minted extension's self-load (theseus-pw1q.21.8), through the whole core
//! with 43a's stand-in for the frozen copy in L1: the `"self"` principal
//! answers an extension's card for a mint only when its manifest asks for no
//! network and no hands and every declared test passed; it loads under the
//! minted ceiling; it answers no other card; and with the mode off, the switch
//! halted, or no room left, nothing is read or written.

use std::path::Path;

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfMode;

use super::{call, rig_with, text, Rig, WC};
use crate::approval::{Answerer, Surface};
use crate::extend::load::{minted_ceiling, LoadedSet};
use crate::extend::Manifest;
use crate::policy::Posture;
use crate::rsi::mint::deploy::self_load_refusal as refusal;

const MINT: &str = "mint_0123456789ab";

/// A rig whose core acts and has been resumed.
fn acting(f: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = rig_with(|c| {
        c.self_improve.mode = SelfMode::Act;
        f(c)
    });
    r.core
        .self_resume(&Default::default(), &Answerer::from("the CLI"))
        .unwrap();
    r
}

/// Propose `name` from `dir` with `tests` and the `network`, in a new session:
/// the question's id.
async fn propose_as(r: &Rig, name: &str, dir: &Path, tests: &[Value], network: &[&str]) -> String {
    let sid = r.session(None);
    let mut p = json!({
        "name": name, "dir": dir.display().to_string(),
        "command": ["python3", "server.py"],
        "description": "Counts the words of a text.",
        "tests": tests,
    });
    if !network.is_empty() {
        p["network"] = json!(network);
    }
    r.script(vec![call("extend_propose", p), text()]);
    r.turn(&sid, "propose it").await;
    let q = r.core.confirm_list().unwrap();
    q.iter()
        .filter(|c| c.tool == crate::extend::ACK)
        .find(|c| c.input["name"] == name)
        .unwrap_or_else(|| panic!("no question for {name}: {q:?}"))
        .correlation_id
        .clone()
}

fn passing() -> Vec<Value> {
    vec![
        json!({"tool": "echo", "arguments": {"text": "two words"}, "expect": {"contains": "two words"}}),
        json!({"tool": "add", "arguments": {"a": 2, "b": 3}, "expect": {"contains": "5"}}),
    ]
}

fn manifest(r: &Rig, name: &str) -> Manifest {
    crate::extend::manifests(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|m| m.name == name)
        .unwrap()
}

fn kinds(r: &Rig) -> usize {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .len()
}

/// The three cases: a network ask, an ask for hands (a credential), and a test
/// that failed. Each is refused with its reason, on the manifest alone and
/// through the core (no `extend.acked`, the card still the owner's); the
/// manifest that has none of the three is allowed.
#[tokio::test]
async fn self_load_is_refused_for_network_secrets_or_failed_tests() {
    let r = acting(|_| {});
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    let mut tests = passing();
    tests.push(
        json!({"tool": "echo", "arguments": {"text": "three"}, "expect": {"equals": "four"}}),
    );
    let q = propose_as(&r, "wordcount", &src, &tests, &[]).await;
    let m = manifest(&r, "wordcount");
    assert_eq!(m.passed(), 2, "one declared test fails");

    // A failed test.
    let why = refusal(&m).expect("a failed test is refused");
    assert!(why.contains("2 of 3 declared tests passed"), "{why}");
    let e = r
        .core
        .mint_self_load(MINT, "wordcount", &m.digest)
        .unwrap_err();
    assert!(format!("{e:#}").contains("2 of 3"), "{e:#}");

    // The same extension with every test passing is allowed, and the three
    // refusals are each one change from it.
    let mut ok = m;
    ok.tests.retain(|t| t.passed);
    assert_eq!(refusal(&ok), None);
    let mut net = ok.clone();
    net.capabilities.network = vec!["example.org:443".into()];
    assert!(
        refusal(&net).unwrap().contains("network"),
        "{:?}",
        refusal(&net)
    );
    let mut hands = ok.clone();
    hands.capabilities.hands = Some(
        serde_json::from_value(json!({"usd_cap": 2.0, "backends": ["lambda"], "max_group": 4}))
            .unwrap(),
    );
    assert!(
        refusal(&hands).unwrap().contains("credential"),
        "{:?}",
        refusal(&hands)
    );
    let mut none = ok.clone();
    none.tests.clear();
    assert!(refusal(&none).unwrap().contains("no test"));
    let mut came_up = ok.clone();
    came_up.error = Some("exited".into());
    assert!(refusal(&came_up).unwrap().contains("did not come up"));

    // Through the core: a network ask, stored as the manifest, is refused
    // and nothing is answered.
    r.core.store.append(&[net.record().unwrap()]).unwrap();
    let before = kinds(&r);
    let e = r
        .core
        .mint_self_load(MINT, "wordcount", &net.digest)
        .unwrap_err();
    assert!(format!("{e:#}").contains("network"), "{e:#}");
    assert_eq!(kinds(&r), before, "nothing was written");
    assert!(r.rows("extend.acked").is_empty() && r.rows("self.mint.deployed").is_empty());
    assert_eq!(
        r.core.confirm_list().unwrap().len(),
        1,
        "the card is still the owner's"
    );
    assert_eq!(q, r.core.confirm_list().unwrap()[0].correlation_id);
}

/// An extension that passes loads by `"self"`: `extend.acked {by: "self", mint}`,
/// one `self.mint.deployed` row with its undo, the minted ceiling recorded, its
/// tools offered from the next turn, and the owner's `ext:<name>` revert
/// takes it back.
#[tokio::test]
async fn an_eligible_extension_is_self_loaded_under_the_minted_ceiling() {
    let r = acting(|_| {});
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    propose_as(&r, "wordcount", &src, &passing(), &[]).await;
    let m = manifest(&r, "wordcount");
    let d = r.core.mint_self_load(MINT, "wordcount", &m.digest).unwrap();
    assert_eq!(
        (d.tool.as_str(), d.digest.as_str()),
        ("ext-wordcount", m.digest.as_str())
    );

    let acked = &r.rows("extend.acked")[0];
    assert_eq!(
        (acked["by"].as_str(), acked["mint"].as_str()),
        (Some("self"), Some(MINT))
    );
    let deployed = &r.rows("self.mint.deployed")[0];
    assert_eq!(deployed["undo"], "theseus self revert ext:wordcount");
    assert_eq!(deployed["numbers"]["tests"], 2);
    let loaded = &LoadedSet::read(&r.core.store).unwrap().loaded["wordcount"];
    assert_eq!(loaded.acked_by, "self");
    assert_eq!(
        loaded.ceiling.as_ref().unwrap().posture_floor.as_deref(),
        Some("notify"),
        "never looser than a notice"
    );
    assert!(
        r.core.confirm_list().unwrap().is_empty(),
        "its card is answered"
    );

    // One verb undoes it: the owner's `self revert ext:wordcount`; an extension
    // the owner loaded is not that verb's.
    let owner = Answerer::from("the CLI");
    let rev = |what: &str| {
        let core = r.core.clone();
        let what = what.to_string();
        let owner = owner.clone();
        async move {
            core.self_revert(
                &theseus_protocol::rsi::SelfRevertParams {
                    merge: what,
                    author: None,
                    discord: None,
                    from_job: None,
                },
                &owner,
                &Default::default(),
            )
            .await
        }
    };
    let done = rev("ext:wordcount").await.unwrap();
    assert!(done.changed);
    assert!(LoadedSet::read(&r.core.store).unwrap().loaded.is_empty());
    assert!(
        format!("{:#}", rev("ext:wordcount").await.unwrap_err()).contains("no minted extension"),
    );
}

/// The ceiling is the place's, and never looser than `notify`.
#[test]
fn the_minted_ceiling_is_the_places_and_never_looser_than_notify() {
    let floor = |c: &theseus_protocol::PlaceCeiling| c.posture_floor.clone();
    assert_eq!(floor(&minted_ceiling(None)).as_deref(), Some("notify"));
    let place = |f: &str| theseus_protocol::PlaceCeiling {
        posture_floor: Some(f.into()),
        tools: Some(vec!["fs".into()]),
        ..Default::default()
    };
    assert_eq!(
        floor(&minted_ceiling(Some(&place("open")))).as_deref(),
        Some("notify")
    );
    assert_eq!(
        floor(&minted_ceiling(Some(&place("approve")))).as_deref(),
        Some("approve")
    );
    assert_eq!(
        minted_ceiling(Some(&place("open"))).tools,
        Some(vec!["fs".to_string()])
    );
    assert_eq!(Posture::parse("notify"), Some(Posture::Notify));
}

/// The `"self"` principal answers no other card: no surface answers as it, the
/// place rule refuses the name on any card, and its one path takes a manifest's
/// own extension card, never a call's.
#[tokio::test]
async fn a_self_principal_answers_no_other_card() {
    let r = acting(|c| {
        c.policy.tools.insert("proc.run".into(), Posture::Approve);
    });
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    propose_as(&r, "wordcount", &src, &passing(), &[]).await;
    // A card of another kind: a command that waits for approval.
    let sid = r.session(None);
    r.script(vec![
        call("proc_run", json!({"argv": ["echo", "hi"]})),
        text(),
    ]);
    r.turn(&sid, "run it").await;
    let cards = r.core.confirm_list().unwrap();
    let other = cards
        .iter()
        .find(|c| c.tool == "proc.run")
        .expect("a call's card")
        .correlation_id
        .clone();
    let ack = cards
        .iter()
        .find(|c| c.tool == crate::extend::ACK)
        .expect("the ack's card")
        .correlation_id
        .clone();

    // No surface is "self": the name answers nothing, the extension's card included.
    let me = Answerer {
        label: "self".into(),
        surface: Surface::Unnamed,
        discord: None,
    };
    for q in [&other, &ack] {
        assert!(
            r.core.confirm_action(q, true, None, me.clone()).is_err(),
            "{q}"
        );
    }
    assert_eq!(r.core.confirm_list().unwrap().len(), 2, "both still wait");

    // The one path names a manifest and answers its extension card alone:
    // pointed at a call's card it refuses and answers nothing.
    let mut m = manifest(&r, "wordcount");
    m.question = Some(other);
    r.core.store.append(&[m.record().unwrap()]).unwrap();
    let e = r
        .core
        .mint_self_load(MINT, "wordcount", &m.digest)
        .unwrap_err();
    assert!(format!("{e:#}").contains("none else"), "{e:#}");
    assert_eq!(r.core.confirm_list().unwrap().len(), 2);
    assert!(r.rows("extend.acked").is_empty());
}

/// Off, halted, or no room: nothing is read or written, and the card waits.
#[tokio::test]
async fn off_halted_or_full_loads_nothing() {
    let (r, off) = (acting(|c| c.self_improve.mint.max_live = 0), acting(|_| {}));
    for rig in [&r, &off] {
        rig.core.mcp.start();
    }
    let src = r.server("tools/wc", "ok", WC);
    propose_as(&r, "wordcount", &src, &passing(), &[]).await;
    let m = manifest(&r, "wordcount");
    // No room under max_live.
    let e = r
        .core
        .mint_self_load(MINT, "wordcount", &m.digest)
        .unwrap_err();
    assert!(format!("{e:#}").contains("max_live"), "{e:#}");
    // Halted.
    let src = off.server("tools/wc", "ok", WC);
    propose_as(&off, "wordcount", &src, &passing(), &[]).await;
    let m = manifest(&off, "wordcount");
    off.core
        .self_halt(&Default::default(), &Answerer::from("the CLI"))
        .unwrap();
    let before = kinds(&off);
    let e = off
        .core
        .mint_self_load(MINT, "wordcount", &m.digest)
        .unwrap_err();
    assert!(format!("{e:#}").contains("halted"), "{e:#}");
    assert_eq!(kinds(&off), before);
    for rig in [&r, &off] {
        assert!(rig.rows("extend.acked").is_empty() && rig.rows("self.mint.deployed").is_empty());
        assert_eq!(rig.core.confirm_list().unwrap().len(), 1);
    }
}
