//! An extension's hands through the broker (theseus-pw1q.14), on the hands
//! rig's fake AWS with a queue: a seeded 200-file batch inside the cap; the
//! refusals of what the frozen manifest did not name, of a request over the
//! extension's own cap while the global line has room, of a hand's request,
//! and of one after a revoke; both accounts reserved; runaway mode; a
//! revoke that cancels a running Fargate group and closes the channel; and
//! an extension without `hands`, which has no channel.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{ActionState, CancelState};

use super::tests_hands::{rig, rows, signed, state_of, until, Rig};
use crate::extend::broker::{self, Book};
use crate::extend::hands::HandsCap;
use crate::extend::load::{Loaded, LoadedSet};
use crate::extend::{Capabilities, ProposedBy};

const NAME: &str = "batcher";

/// A loaded extension, as an ack of the owner's left it: `hands` its grant.
fn loaded(hands: Option<HandsCap>) -> Loaded {
    Loaded {
        name: NAME.into(),
        digest: "d1".repeat(32),
        description: "Transforms files in batches.".into(),
        command: vec!["python3".into(), "server.py".into()],
        frozen: "/nowhere/frozen".into(),
        source: "/nowhere/source".into(),
        tools: vec!["transform".into()],
        capabilities: Capabilities {
            network: vec![],
            scratch: true,
            hands,
        },
        acked_by: "the CLI".into(),
        acked_via: "cli".into(),
        acked_at_ms: 1_790_000_000_000,
        question: "act_q".into(),
        proposed_by: ProposedBy::default(),
        place_name: None,
        ceiling: None,
        replaced: None,
    }
}

fn grant(usd_cap: f64, backends: &[&str], max_group: u32) -> HandsCap {
    serde_json::from_value(
        json!({"usd_cap": usd_cap, "backends": backends, "max_group": max_group}),
    )
    .unwrap()
}

/// `l` loaded: its record, and the board's server with its channel.
fn load(r: &Rig, l: &Loaded) {
    let mut set = LoadedSet::read(&r.core.store).unwrap();
    set.loaded.insert(l.name.clone(), l.clone());
    r.core.store.append(&[set.record().unwrap()]).unwrap();
    r.core.board_load(l, None);
}

async fn ask(core: &Arc<crate::Core>, method: &str, params: Value) -> Result<Value, (i64, String)> {
    core.extension_hands(NAME, method, params).await
}

/// The 200 files, ten to a hand.
fn batch() -> Value {
    let files: Vec<Value> = (0..20)
        .map(|h| {
            json!((0..10)
                .map(|f| format!("in/file-{:03}.txt", h * 10 + f))
                .collect::<Vec<_>>())
        })
        .collect();
    json!({"argv": ["sh", "-c", "transform {input}"], "inputs": files, "ttl_secs": 60})
}

/// Each refusal's reason, in order.
fn refusals(r: &Rig) -> Vec<String> {
    rows(&r.core, "extend.hands.refused")
        .iter()
        .map(|x| x["reason"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// The measurement (rsi-plan): a seeded 200-file batch, twenty hands of ten
/// files on Lambda, done inside a $0.25 cap. The group starts through
/// `aws.hands.run`'s path on the extension's own day session, whose limit is
/// the cap; its hands' envelopes come home and are checked; it settles with
/// its row; and no hand's spec names anything that reaches the broker.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_200_file_batch_runs_inside_the_cap() {
    let r = rig(vec![]);
    load(&r, &loaded(Some(grant(0.25, &["lambda"], 20))));
    assert!(r.core.mcp.has_channel(&format!("ext-{NAME}")));
    let got = ask(&r.core, "hands.run", batch()).await.unwrap();
    let g = got["id"].as_str().unwrap().to_string();
    assert_eq!(
        (got["hands"].as_u64(), got["backend"].as_str()),
        (Some(20), Some("lambda"))
    );
    let worst = got["worst_usd"].as_f64().unwrap();
    assert!(worst > 0.0 && worst <= 0.25, "{got}");
    // The day's session: its own, its limit the cap, the group's worst case
    // reserved on it, under the extension's principal.
    let book = Book::read(&r.core.store, NAME).unwrap();
    assert_eq!(book.sessions.len(), 1);
    let day = &book.sessions[0];
    let e = r.core.kernel.execution(&day.execution_id).unwrap().unwrap();
    assert_eq!((e.budget.limit_micros, e.budget.pinned), (250_000, true));
    assert_eq!(e.authority.principal, broker::principal(NAME));
    assert_eq!(e.budget.reserved_micros, (worst * 1e6).round() as u64);
    // Both accounts: the account's hour line counts the same reservation.
    let hour = super::watch::hour_of(theseus_protocol::now_unix_ms());
    let figure = super::runaway::spend_since(
        &r.core.kernel,
        &r.core.store,
        crate::aws::tests::ACCOUNT,
        hour,
    )
    .unwrap();
    assert_eq!(figure, e.budget.reserved_micros);
    let rec = super::tests_hands::record(&r.core, &g);
    assert_eq!(rec.extension.as_deref(), Some(NAME));
    assert_eq!(rec.request.profile, "basic");
    // Twenty Invokes, two hundred files, and nothing in a spec that reaches
    // the broker: no socket, no extension's name, no key but its own.
    let specs = r.state.invoked();
    assert_eq!(specs.len(), 20);
    let files: usize = specs
        .iter()
        .map(|s| s.input.as_array().unwrap().len())
        .sum();
    assert_eq!(files, 200);
    for s in &specs {
        let text = serde_json::to_string(s).unwrap();
        for never in [NAME, "ext-", "sock", "THESEUS_SESSION"] {
            assert!(!text.contains(never), "{never} in {text}");
        }
    }
    assert_eq!(rows(&r.core, "extend.hands.launched").len(), 1);
    // The hands finish, and their envelopes are checked as every hand's.
    for s in &specs {
        r.state.push(signed(s, "succeeded", 0));
    }
    r.core.poll_hands_after_serving();
    until("the group to settle", || {
        state_of(&r.core, &g) == ActionState::Succeeded
    })
    .await;
    let settled = rows(&r.core, "extend.hands.settled");
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(
        (settled[0]["name"].as_str(), settled[0]["settled"].as_str()),
        (Some(NAME), Some("met"))
    );
    let e = r.core.kernel.execution(&day.execution_id).unwrap().unwrap();
    assert_eq!(e.budget.reserved_micros, 0);
    assert!(e.budget.spent_micros <= 250_000, "{:?}", e.budget);
    // Its status, read on the channel: settled, with each hand.
    let st = ask(&r.core, "hands.status", json!({"id": g}))
        .await
        .unwrap();
    assert_eq!(
        (st["settled"].as_str(), st["succeeded"].as_u64()),
        (Some("met"), Some(20))
    );
    assert!(
        st["result"]
            .as_str()
            .unwrap()
            .contains("20 of 20 succeeded"),
        "{st}"
    );
    // The day's session never woke for a turn: no model was asked.
    assert_eq!(e.state, theseus_kernel::ExecState::Waiting);
    drop(r.dir);
}

/// What the frozen manifest did not name is refused and ledgered, and
/// nothing launches: another backend, more hands than `max_group`, a field
/// an extension may not name, a hand's own request (depth one), and another
/// extension's group.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_broker_refuses_what_the_manifest_did_not_name() {
    let r = rig(vec![]);
    load(&r, &loaded(Some(grant(2.0, &["lambda"], 20))));
    let e = ask(
        &r.core,
        "hands.run",
        json!({"argv": ["true"], "backend": "fargate"}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.0, broker::REFUSED);
    assert!(e.1.contains("on lambda only, not fargate"), "{}", e.1);
    let e = ask(&r.core, "hands.run", json!({"argv": ["true"], "count": 21}))
        .await
        .unwrap_err();
    assert!(e.1.contains("at most 20 hands at a time"), "{}", e.1);
    for field in [
        json!({"image": "example.invalid/other"}),
        json!({"profile": "owner"}),
        json!({"account": "1"}),
    ] {
        let mut p = json!({"argv": ["true"]});
        p.as_object_mut()
            .unwrap()
            .extend(field.as_object().unwrap().clone());
        let e = ask(&r.core, "hands.run", p).await.unwrap_err();
        assert!(e.1.contains("basic profile"), "{}", e.1);
    }
    let e = ask(
        &r.core,
        "hands.run",
        json!({"argv": ["true"], "hand": "act_0a1b"}),
    )
    .await
    .unwrap_err();
    assert!(e.1.contains("a hand cannot launch a hand"), "{}", e.1);
    let e = ask(&r.core, "hands.status", json!({"id": "act_someone_elses"}))
        .await
        .unwrap_err();
    assert!(e.1.contains("no hands group of extension"), "{}", e.1);
    assert_eq!(
        refusals(&r),
        [
            "backend",
            "group_size",
            "invalid",
            "invalid",
            "invalid",
            "from_hand",
            "not_its_group"
        ]
    );
    assert!(r.state.invoked().is_empty() && r.state.ran.lock().unwrap().is_empty());
    assert!(
        Book::read(&r.core.store, NAME).unwrap().sessions.is_empty(),
        "no session for a refusal"
    );
    drop(r.dir);
}

/// A group over the extension's own cap is refused while the account's
/// global lines have all the room it needs, and nothing launches; a group
/// inside it then runs. The cap is the day session's pinned limit, held by
/// the kernel in the group's frame.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_over_the_extensions_cap_is_refused_though_the_global_line_has_room() {
    let r = rig(vec![]);
    load(&r, &loaded(Some(grant(0.01, &["lambda"], 20))));
    let e = ask(&r.core, "hands.run", batch()).await.unwrap_err();
    assert!(e.1.contains("over extension batcher's day cap"), "{}", e.1);
    assert_eq!(refusals(&r), ["cap"]);
    assert!(r.state.invoked().is_empty());
    assert!(
        rows(&r.core, "aws.runaway").is_empty(),
        "the global line had room"
    );
    // Its call settled failed at nothing; the day's limit is the cap.
    let day = Book::read(&r.core.store, NAME).unwrap().sessions.remove(0);
    let ex = r.core.kernel.execution(&day.execution_id).unwrap().unwrap();
    assert_eq!(
        (
            ex.budget.limit_micros,
            ex.budget.reserved_micros,
            ex.budget.spent_micros
        ),
        (10_000, 0, 0)
    );
    let one = ask(
        &r.core,
        "hands.run",
        json!({"argv": ["true"], "ttl_secs": 60}),
    )
    .await
    .unwrap();
    assert_eq!(one["hands"], 1);
    assert_eq!(r.state.invoked().len(), 1);
    drop(r.dir);
}

/// Runaway mode refuses an extension's group as it refuses any launch: the
/// account's hour line at a cent, times one, with the cap's room to spare.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runaway_mode_refuses_an_extensions_group() {
    let r = super::tests_hands::rig_with(vec![], |cfg| {
        for a in cfg.aws.accounts.values_mut() {
            a.hourly_alert_usd = 0.01;
            a.runaway_factor = 1.0;
        }
    });
    load(&r, &loaded(Some(grant(2.0, &["lambda"], 20))));
    let e = ask(&r.core, "hands.run", batch()).await.unwrap_err();
    assert!(e.1.contains("AWS runaway mode"), "{}", e.1);
    assert_eq!(refusals(&r), ["runaway"]);
    assert_eq!(rows(&r.core, "aws.runaway").len(), 1);
    assert!(r.state.invoked().is_empty());
    drop(r.dir);
}

/// A revoke cancels the extension's running group, verified by its
/// backend's means (Fargate's StopTask, then STOPPED), closes the channel,
/// and refuses its next request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_revoke_cancels_a_running_group_and_closes_the_channel() {
    let r = rig(vec![]);
    r.state.nat.store(true, std::sync::atomic::Ordering::SeqCst);
    r.state
        .stop_at_once
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let l = loaded(Some(grant(5.0, &["fargate"], 4)));
    load(&r, &l);
    let got = ask(
        &r.core,
        "hands.run",
        json!({"argv": ["sleep", "600"], "count": 2, "backend": "fargate", "ttl_secs": 900}),
    )
    .await
    .unwrap();
    let g = got["id"].as_str().unwrap().to_string();
    assert_eq!(r.state.ran.lock().unwrap().len(), 2);
    let p = theseus_protocol::extend::ExtensionRevokeParams {
        name: NAME.into(),
        ..Default::default()
    };
    r.core.extension_revoke(&p, "cli").unwrap();
    assert!(!r.core.mcp.has_channel(&format!("ext-{NAME}")));
    // The call's cancel, then its record's and rows' frame.
    until("the group's cancel and its row", || {
        state_of(&r.core, &g) == ActionState::Cancelled
            && !rows(&r.core, "extend.hands.settled").is_empty()
    })
    .await;
    let rec = super::tests_hands::record(&r.core, &g);
    for h in &rec.hands {
        let a = r.core.kernel.action(&h.correlation_id).unwrap().unwrap();
        assert_eq!(
            (a.state, a.cancel),
            (
                ActionState::Cancelled,
                Some(CancelState::TerminationVerified)
            ),
            "{a:?}"
        );
    }
    assert_eq!(r.state.stops.lock().unwrap().len(), 2);
    let settled = rows(&r.core, "extend.hands.settled");
    assert_eq!(settled[0]["settled"], "cancelled");
    let e = ask(&r.core, "hands.run", json!({"argv": ["true"]}))
        .await
        .unwrap_err();
    assert!(e.1.contains("is not loaded"), "{}", e.1);
    assert_eq!(refusals(&r), ["revoked"]);
    drop(r.dir);
}

/// An extension loaded without `hands` has no channel: its connection
/// answers none of the broker's methods (the client's "method not found"),
/// and a request that reached the broker anyway is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_extension_without_hands_has_no_channel() {
    let r = rig(vec![]);
    let l = loaded(None);
    load(&r, &l);
    assert!(r.core.tools.extend.channel(&l).is_none());
    assert!(!r.core.mcp.has_channel(&format!("ext-{NAME}")));
    let e = ask(&r.core, "hands.run", json!({"argv": ["true"]}))
        .await
        .unwrap_err();
    assert!(e.1.contains("loaded without hands"), "{}", e.1);
    assert_eq!(refusals(&r), ["not_granted"]);
    assert!(r.state.invoked().is_empty());
    drop(r.dir);
}

/// The channel itself: the board's `Serve` for a granted extension answers
/// through the core, bound to its own name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_channel_answers_for_its_own_extension_alone() {
    let r = rig(vec![]);
    let l = loaded(Some(grant(1.0, &["lambda"], 2)));
    load(&r, &l);
    let served = r
        .core
        .tools
        .extend
        .channel(&l)
        .expect("a granted extension's channel");
    assert_eq!(served.0.methods(), broker::METHODS);
    let got = served
        .0
        .answer(
            "hands.run".into(),
            json!({"argv": ["true"], "ttl_secs": 60}),
        )
        .await
        .unwrap();
    let g = got["id"].as_str().unwrap().to_string();
    let other = crate::extend::broker::Channel::served(Arc::downgrade(&r.core), "someone-else");
    let e = other
        .0
        .answer("hands.status".into(), json!({"id": g}))
        .await
        .unwrap_err();
    assert!(
        e.1.contains("no hands group of extension someone-else's"),
        "{}",
        e.1
    );
    drop(r.dir);
}

/// A loop of refused requests is answered every time, but the ledger takes
/// at most `REFUSALS_A_MINUTE` of one extension's rows a minute; the next
/// minute's first row counts the ones it left, and another extension's rows
/// are its own.
#[test]
fn a_flood_of_refusals_writes_a_bounded_number_of_rows() {
    let r = broker::Refusals::default();
    let t0 = 1_790_000_080_000; // 40 s into a minute
    let written = (0..25)
        .filter(|i| r.admit(NAME, t0 + i * 100).is_some())
        .count();
    assert_eq!(written, broker::REFUSALS_A_MINUTE as usize);
    assert_eq!(r.admit("another", t0), Some(0), "its own count");
    assert_eq!(
        r.admit(NAME, t0 + 30_000),
        Some(5),
        "the next minute counts what was left"
    );
    assert_eq!(r.admit(NAME, t0 + 30_100), Some(0));
}
