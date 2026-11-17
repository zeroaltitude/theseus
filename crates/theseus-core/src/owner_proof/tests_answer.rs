//! The owner's second proof at the daemon's answer path and its enrolment
//! methods, through real protocol connections accepted as each surface: a
//! question the asker marked is approved only on a good code (or an open
//! window of the answer's own place), a wrong or missing code changes nothing
//! and is recorded, the chat's tool and words never carry a proof, enrolment is
//! the owner's own terminal's alone, and neither the secret nor a code is in
//! anything a model's tools can read.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::{error_code, method, RpcError};

use super::totp::{self, Secret};
use super::{file, mark};
use crate::approval::{Answerer, Client, Surface};
use crate::ledger::LedgerRow;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::test_support::turn_in as turn;
use crate::tests_config::{own_files, OwnFiles};
use crate::tests_m3::{rpc_as, surface};

/// A core with two questions waiting, in two sessions: each a write to the
/// config file, which the floor holds for the owner, so real questions with
/// real correlation ids. Both are asked before either is answered, so no
/// answer's continuation can take the other's scripted turn.
async fn asked() -> (OwnFiles, String, String, String) {
    let r = own_files(Posture::Open, |_, config, _| {
        let input = json!({"path": config.to_string_lossy(), "content": "x"});
        vec![
            Scripted::tools("", &[("w1", "fs_write", input.clone())]),
            Scripted::tools("", &[("w2", "fs_write", input)]),
            Scripted::text("done"),
            Scripted::text("done again"),
        ]
    });
    let res = turn(&r.core, None, "change the settings").await;
    let corr = res.awaiting_confirm.clone().expect("the floor waits");
    let other = turn(&r.core, None, "and the other settings").await;
    let second = other
        .awaiting_confirm
        .clone()
        .expect("the floor waits again");
    (r, corr, res.session_id, second)
}

fn mark_it(r: &OwnFiles, corr: &str, fresh: bool) {
    r.core.store.append(&[mark(corr, fresh).unwrap()]).unwrap();
}

/// A freshly random secret, enrolled the way an enrolment leaves the file:
/// the last step well behind now.
fn enrol(r: &OwnFiles) -> Secret {
    use std::io::Read;
    let mut bytes = [0u8; 20];
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    let secret = Secret::from_bytes(bytes);
    let now = theseus_protocol::now_unix_ms();
    let held = file::Held::new(&secret, now, totp::step_at(now / 1000).saturating_sub(10));
    file::write(&r.agents.parent().unwrap().join(file::FILE), &held).unwrap();
    secret
}

/// The code the app shows `ahead` steps from now.
fn code(secret: &Secret, ahead: u64) -> String {
    totp::code_at_step(
        secret,
        totp::step_at(theseus_protocol::now_unix_ms() / 1000) + ahead,
    )
}

fn pending(r: &OwnFiles, corr: &str) -> bool {
    r.core
        .kernel
        .pending_confirms()
        .unwrap()
        .iter()
        .any(|a| a.correlation_id == corr)
}

fn rows(r: &OwnFiles, kind: &str) -> Vec<LedgerRow> {
    r.core
        .store
        .ledger_tail::<LedgerRow>(5000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.kind == kind)
        .collect()
}

fn cli() -> Client {
    surface("sock#1", Surface::Cli)
}

fn web() -> Client {
    surface("web#1", Surface::Web)
}

async fn confirm(
    r: &OwnFiles,
    client: Client,
    corr: &str,
    approve: bool,
    code: Option<&str>,
) -> Result<Value, RpcError> {
    rpc_as(
        &r.core,
        client,
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": approve, "code": code}),
    )
    .await
}

fn refused(e: &RpcError) -> bool {
    e.code == error_code::REFUSED
}

/// The yes alone, on each surface that may answer at all: nothing changes, it
/// says the code is needed, and it is recorded.
#[tokio::test]
async fn a_marked_question_is_not_approved_on_a_yes_alone() {
    let (r, corr, _, _) = asked().await;
    mark_it(&r, &corr, false);
    let _ = enrol(&r);
    for client in [cli(), web()] {
        let e = confirm(&r, client, &corr, true, None).await.unwrap_err();
        assert!(refused(&e), "{e:?}");
        assert!(e.message.contains("second proof"), "{}", e.message);
        assert!(pending(&r, &corr), "a yes alone changed nothing");
    }
    let missing: Vec<_> = rows(&r, "owner.proof.checked")
        .into_iter()
        .map(|row| row.data["outcome"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(missing, ["missing", "missing"], "each is recorded");
    assert_eq!(rows(&r, "approval.refused").len(), 2);
}

/// The control: the same question, unmarked, is approved on the yes alone.
#[tokio::test]
async fn an_unmarked_question_is_approved_as_ever() {
    let (r, corr, _, _) = asked().await;
    let _ = enrol(&r);
    confirm(&r, cli(), &corr, true, None).await.unwrap();
    assert!(!pending(&r, &corr));
    assert!(rows(&r, "owner.proof.checked").is_empty());
}

/// A wrong code changes nothing, is recorded, and counts toward the lock.
#[tokio::test]
async fn a_wrong_code_changes_nothing_and_is_recorded() {
    let (r, corr, _, _) = asked().await;
    mark_it(&r, &corr, false);
    let _ = enrol(&r);
    let e = confirm(&r, cli(), &corr, true, Some("000000"))
        .await
        .unwrap_err();
    assert!(refused(&e), "{e:?}");
    assert!(pending(&r, &corr));
    let checked = rows(&r, "owner.proof.checked");
    assert_eq!(checked.len(), 1);
    assert_eq!(checked[0].data["outcome"], "wrong");
    assert_eq!(checked[0].data["place"], "cli");
    assert!(!serde_json::to_string(&checked[0])
        .unwrap()
        .contains("000000"));
}

/// A good code approves it once; the same code again does not.
#[tokio::test]
async fn a_good_code_approves_once() {
    let (r, corr, _, second) = asked().await;
    mark_it(&r, &corr, false);
    let secret = enrol(&r);
    let good = code(&secret, 0);
    confirm(&r, cli(), &corr, true, Some(&good)).await.unwrap();
    assert!(!pending(&r, &corr), "it went through");
    // A second question, marked as needing a fresh code, the same code: used.
    mark_it(&r, &second, true);
    let e = confirm(&r, web(), &second, true, Some(&good))
        .await
        .unwrap_err();
    assert!(refused(&e) && e.message.contains("already used"), "{e:?}");
    assert!(pending(&r, &second));
}

/// A decline never needs it: it tightens.
#[tokio::test]
async fn a_decline_needs_no_proof() {
    let (r, corr, _, _) = asked().await;
    mark_it(&r, &corr, true);
    let _ = enrol(&r);
    confirm(&r, cli(), &corr, false, None).await.unwrap();
    assert!(!pending(&r, &corr));
}

/// `work.answer` carries the code too, and says the footing it went through on.
#[tokio::test]
async fn work_answer_carries_the_code_and_says_the_footing() {
    let (r, corr, _, _) = asked().await;
    mark_it(&r, &corr, false);
    let secret = enrol(&r);
    let good = code(&secret, 0);
    let e = rpc_as(
        &r.core,
        cli(),
        method::WORK_ANSWER,
        json!({"question_id": corr, "option": "approve"}),
    )
    .await
    .unwrap_err();
    assert!(refused(&e) && pending(&r, &corr), "{e:?}");
    let done = rpc_as(
        &r.core,
        cli(),
        method::WORK_ANSWER,
        json!({"question_id": corr, "option": "approve", "code": good}),
    )
    .await
    .unwrap();
    assert!(!pending(&r, &corr));
    let detail = done["detail"].as_str().unwrap();
    assert!(detail.starts_with("second proof: given here"), "{detail}");
}

/// A code typed or spoken as words is no proof, whatever came with it and
/// whatever window is open: and the code it carried stays unspent.
#[tokio::test]
async fn an_answer_typed_as_words_never_carries_a_proof() {
    let (r, corr, _, _) = asked().await;
    mark_it(&r, &corr, false);
    let secret = enrol(&r);
    let good = code(&secret, 0);
    let e = rpc_as(
        &r.core,
        cli(),
        method::WORK_ANSWER,
        json!({"question_id": corr, "option": "approve", "code": good, "typed": true}),
    )
    .await
    .unwrap_err();
    assert!(
        refused(&e) && e.message.contains("typed or spoken"),
        "{e:?}"
    );
    assert!(pending(&r, &corr));
    // Nothing was checked, so nothing was spent: the same code still goes on the card.
    confirm(&r, cli(), &corr, true, Some(&good)).await.unwrap();
    assert!(!pending(&r, &corr));
}

/// The chat's answer tool: a yes to such a question is never taken from chat.
#[tokio::test]
async fn the_chat_answer_tool_never_approves_a_marked_question() {
    let (r, corr, session, second) = asked().await;
    mark_it(&r, &corr, false);
    let secret = enrol(&r);
    let q = r
        .core
        .pending_confirms(&session)
        .unwrap()
        .into_iter()
        .find(|q| q.correlation_id == corr)
        .unwrap();
    assert!(q.needs_proof, "the request carries the mark");
    let who = Answerer::from("owner");
    let said = r
        .core
        .answer_plain_act(&q, "yes", None, who.clone())
        .unwrap_err();
    assert!(
        said.contains("second proof") && said.contains("card"),
        "{said}"
    );
    assert!(pending(&r, &corr));
    // And a good code said in chat is nothing: it is no answer, and it is not spent.
    let said = r
        .core
        .answer_plain_act(&q, &code(&secret, 0), None, who.clone())
        .unwrap_err();
    assert!(!said.is_empty());
    assert!(pending(&r, &corr));
    confirm(&r, cli(), &corr, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    assert!(!pending(&r, &corr), "the code was never spent by chat");
    // A no is never held back: the other question, marked, declined in chat.
    mark_it(&r, &second, false);
    let session2 = r.core.kernel.action(&second).unwrap().unwrap().session_id;
    let q2 = r
        .core
        .pending_confirms(&session2)
        .unwrap()
        .into_iter()
        .find(|q| q.correlation_id == second)
        .unwrap();
    assert!(r.core.answer_plain_act(&q2, "no", None, who).is_ok());
    assert!(!pending(&r, &second));
}

/// `confirm.list` carries the mark and, for the connection's place, the card's words.
#[tokio::test]
async fn the_list_says_which_footing_a_card_goes_through_on() {
    let (r, corr, _, q2) = asked().await;
    mark_it(&r, &corr, false);
    mark_it(&r, &q2, false);
    let secret = enrol(&r);
    let card_of = |list: Value, id: &str| {
        list["confirms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["correlation_id"] == id)
            .unwrap()
            .clone()
    };
    let list = rpc_as(&r.core, cli(), method::CONFIRM_LIST, json!({}))
        .await
        .unwrap();
    let before = card_of(list, &q2);
    assert_eq!(before["needs_proof"], true);
    assert_eq!(before["proof"]["code_needed"], true);
    assert_eq!(
        before["proof"]["line"],
        "second proof: needed, enter the code from your authenticator app"
    );
    // A good code on the other question opens this place's window; the card then says so.
    confirm(&r, cli(), &corr, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    let list = rpc_as(&r.core, cli(), method::CONFIRM_LIST, json!({}))
        .await
        .unwrap();
    let card = card_of(list, &q2);
    assert_eq!(card["proof"]["code_needed"], false);
    assert!(card["proof"]["line"]
        .as_str()
        .unwrap()
        .starts_with("second proof: given here"));
    // From the web UI it is still needed: a window is a place's.
    let list = rpc_as(&r.core, web(), method::CONFIRM_LIST, json!({}))
        .await
        .unwrap();
    assert_eq!(card_of(list, &q2)["proof"]["code_needed"], true);
}

/// Inside a window the same place needs no code and another does; the use is a row.
#[tokio::test]
async fn inside_a_window_the_same_place_needs_no_code_and_another_does() {
    let (r, corr, _, second) = asked().await;
    mark_it(&r, &corr, false);
    mark_it(&r, &second, false);
    let secret = enrol(&r);
    confirm(&r, cli(), &corr, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    // Another place needs its own code.
    let e = confirm(&r, web(), &second, true, None).await.unwrap_err();
    assert!(refused(&e), "{e:?}");
    assert!(pending(&r, &second));
    // This place does not.
    confirm(&r, cli(), &second, true, None).await.unwrap();
    assert!(!pending(&r, &second));
    assert_eq!(rows(&r, "owner.proof.window_used").len(), 1);
    assert_eq!(rows(&r, "owner.proof.window_opened").len(), 1);
}

/// A question that needs a fresh proof asks again inside a window.
#[tokio::test]
async fn a_fresh_question_asks_for_a_code_inside_a_window() {
    let (r, corr, _, second) = asked().await;
    mark_it(&r, &corr, false);
    mark_it(&r, &second, true);
    let secret = enrol(&r);
    confirm(&r, cli(), &corr, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    let e = confirm(&r, cli(), &second, true, None).await.unwrap_err();
    assert!(refused(&e) && pending(&r, &second), "{e:?}");
    confirm(&r, cli(), &second, true, Some(&code(&secret, 1)))
        .await
        .unwrap();
    assert!(!pending(&r, &second));
}

/// A marked question with nothing enrolled cannot be approved at all.
#[tokio::test]
async fn with_nothing_enrolled_a_marked_question_cannot_be_approved() {
    let (r, corr, _, _) = asked().await;
    mark_it(&r, &corr, false);
    for code in [None, Some("123456")] {
        let e = confirm(&r, cli(), &corr, true, code).await.unwrap_err();
        assert!(
            refused(&e) && e.message.contains("theseus owner proof enroll"),
            "{e:?}"
        );
    }
    assert!(pending(&r, &corr));
}

/// The base32 an app is told, back to bytes: for the tests only.
fn unbase32(s: &str) -> [u8; 20] {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => panic!("not base32"),
        };
        buf = (buf << 5) | u32::from(v);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out.try_into().unwrap()
}

fn secret_bytes(s: &Secret) -> [u8; 20] {
    unbase32(&s.base32())
}

async fn proof_rpc(
    r: &OwnFiles,
    client: Client,
    m: &str,
    params: Value,
) -> Result<Value, RpcError> {
    rpc_as(&r.core, client, m, params).await
}

/// Enrolment, reset and status are the owner's own terminal's alone: not a
/// job's shell, not the web UI, Discord, MCP or an unnamed connection; each
/// refusal says where to run it and is a row. (MCP is stopped earlier still,
/// by the server's own allowlist, before the method is reached.)
#[tokio::test]
async fn enrolment_is_refused_everywhere_but_the_owners_own_terminal() {
    let (r, _, _, _) = asked().await;
    let discord = Client::new("discord", Surface::Discord);
    let places = [
        (web(), json!({})),
        (discord, json!({})),
        (surface("mcp#1", Surface::Mcp), json!({})),
        (surface("x", Surface::Unnamed), json!({})),
        (cli(), json!({"from_job": "ses_0000aa1b2c3"})),
    ];
    for m in [
        method::OWNER_PROOF_ENROLL,
        method::OWNER_PROOF_STATUS,
        method::OWNER_PROOF_CONFIRM,
        method::OWNER_PROOF_RESET,
    ] {
        for (client, mut params) in places.clone() {
            params["code"] = json!("123456");
            let mcp = client.surface == Surface::Mcp;
            let e = proof_rpc(&r, client, m, params).await.unwrap_err();
            let said = if mcp {
                "may not call"
            } else {
                "your own terminal"
            };
            assert!(e.message.contains(said), "{m}: {}", e.message);
        }
    }
    assert!(!r.agents.parent().unwrap().join(file::FILE).exists());
    assert!(!r.core.proofs.enrolling());
    let refusals = rows(&r, "owner.proof.changed");
    assert_eq!(refusals.len(), 4 * 4, "each but MCP's is a row");
    assert!(refusals.iter().all(|row| row.data["what"] == "refused"));
}

/// An agent's process behind the CLI is refused too.
#[tokio::test]
async fn enrolment_is_refused_to_an_agents_process() {
    let (r, _, _, _) = asked().await;
    let agent = cli().behind(crate::peer_proc::Behind::Agent("an-agent".into()));
    let e = proof_rpc(&r, agent, method::OWNER_PROOF_ENROLL, json!({}))
        .await
        .unwrap_err();
    assert!(
        refused(&e) && e.message.contains("agent's process"),
        "{e:?}"
    );
}

/// The whole of enrolment from the owner's terminal: nothing is kept until the
/// app's first code, then status says so without the secret, a reset needs a code.
#[tokio::test]
async fn enrolment_from_the_owners_terminal_keeps_nothing_until_a_code_comes() {
    let (r, _, _, _) = asked().await;
    let path = r.agents.parent().unwrap().join(file::FILE);
    let begun = proof_rpc(&r, cli(), method::OWNER_PROOF_ENROLL, json!({}))
        .await
        .unwrap();
    let secret = Secret::from_bytes(unbase32(begun["secret"].as_str().unwrap()));
    assert!(begun["uri"]
        .as_str()
        .unwrap()
        .starts_with("otpauth://totp/Theseus:owner"));
    assert!(!path.exists(), "begun, and nothing kept");
    let e = proof_rpc(
        &r,
        cli(),
        method::OWNER_PROOF_CONFIRM,
        json!({"code": "000000"}),
    )
    .await
    .unwrap_err();
    assert!(
        refused(&e) && e.message.contains("Nothing was enrolled"),
        "{e:?}"
    );
    assert!(!path.exists());
    proof_rpc(
        &r,
        cli(),
        method::OWNER_PROOF_CONFIRM,
        json!({"code": code(&secret, 0)}),
    )
    .await
    .unwrap();
    assert!(path.exists());
    let status = proof_rpc(&r, cli(), method::OWNER_PROOF_STATUS, json!({}))
        .await
        .unwrap();
    assert_eq!(status["enrolled"], true);
    assert_eq!(status["window_hours"], 24);
    let text = status.to_string();
    assert!(
        !text.contains(&secret.base32()) && !text.contains(&hex::encode(secret_bytes(&secret)))
    );
    // A reset needs a valid code.
    let e = proof_rpc(
        &r,
        cli(),
        method::OWNER_PROOF_RESET,
        json!({"code": "000000"}),
    )
    .await
    .unwrap_err();
    assert!(refused(&e) && path.exists(), "{e:?}");
    proof_rpc(
        &r,
        cli(),
        method::OWNER_PROOF_RESET,
        json!({"code": code(&secret, 1)}),
    )
    .await
    .unwrap();
    assert!(!path.exists());
}

/// `lock` ends every window and is a row; the owner's terminal may, an MCP
/// client and a job's shell may not.
#[tokio::test]
async fn lock_ends_the_windows_for_the_owner_and_for_no_one_else() {
    let (r, corr, _, second) = asked().await;
    mark_it(&r, &corr, false);
    mark_it(&r, &second, false);
    let secret = enrol(&r);
    confirm(&r, cli(), &corr, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    let e = proof_rpc(
        &r,
        surface("mcp#1", Surface::Mcp),
        method::OWNER_PROOF_LOCK,
        json!({}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("may not call"), "{e:?}");
    let e = proof_rpc(
        &r,
        cli(),
        method::OWNER_PROOF_LOCK,
        json!({"from_job": "ses_0000aa1b2c3"}),
    )
    .await
    .unwrap_err();
    assert!(refused(&e), "{e:?}");
    let done = proof_rpc(&r, cli(), method::OWNER_PROOF_LOCK, json!({}))
        .await
        .unwrap();
    assert_eq!(done["closed"], 1);
    let e = confirm(&r, cli(), &second, true, None).await.unwrap_err();
    assert!(refused(&e) && pending(&r, &second), "{e:?}");
}

/// Neither the secret nor a code is in anything a model's tools can read: the
/// state dir but for the one file (the store, the spool, the ledger), the
/// rows, health, the lists and answers, the session's transcript, and the log.
#[tokio::test]
async fn neither_the_secret_nor_a_code_is_anywhere_a_model_can_read() {
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sink = Sink::default();
    let w = sink.clone();
    let sub = tracing_subscriber::fmt()
        .with_writer(move || w.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _g = tracing::subscriber::set_default(sub);

    let (r, corr, session, _) = asked().await;
    mark_it(&r, &corr, false);
    let begun = proof_rpc(&r, cli(), method::OWNER_PROOF_ENROLL, json!({}))
        .await
        .unwrap();
    let base32 = begun["secret"].as_str().unwrap().to_string();
    let secret = Secret::from_bytes(unbase32(&base32));
    let first = code(&secret, 0);
    proof_rpc(
        &r,
        cli(),
        method::OWNER_PROOF_CONFIRM,
        json!({"code": first}),
    )
    .await
    .unwrap();
    // A wrong code, a good one on the card, and a used one.
    let wrong = "246810";
    let _ = confirm(&r, cli(), &corr, true, Some(wrong)).await;
    let good = code(&secret, 1);
    confirm(&r, cli(), &corr, true, Some(&good)).await.unwrap();
    let _ = confirm(&r, cli(), &corr, true, Some(&good)).await;
    let list = rpc_as(&r.core, cli(), method::CONFIRM_LIST, json!({}))
        .await
        .unwrap();
    let tail = rpc_as(&r.core, cli(), method::LEDGER_TAIL, json!({"n": 5000}))
        .await
        .unwrap();
    let health = serde_json::to_string(&r.core.health()).unwrap();
    let nodes = serde_json::to_string(&r.core.store.session_nodes(&session).unwrap()).unwrap();
    let log = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    let rows_text = serde_json::to_string(
        &r.core
            .store
            .ledger_tail::<LedgerRow>(5000)
            .unwrap()
            .into_iter()
            .map(|(_, row)| row)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let hex = hex::encode(secret_bytes(&secret));
    for (what, text) in [
        ("the confirm list", list.to_string()),
        ("the ledger tail", tail.to_string()),
        ("health", health),
        ("the transcript", nodes),
        ("the log", log),
        ("the ledger's rows", rows_text),
    ] {
        // A code as a quoted string: six digits inside a millisecond timestamp are no leak.
        let quoted = |c: &str| format!("\"{c}\"");
        for needle in [
            &base32,
            &hex,
            &quoted(&first),
            &quoted(&good),
            &quoted(wrong),
        ] {
            assert!(
                !text.contains(needle.as_str()),
                "{what} holds a secret or a code"
            );
        }
    }
    // The state dir's other files: no secret, base32, hex or raw bytes.
    let state = r.agents.parent().unwrap();
    let mut seen = 0;
    let mut stack = vec![state.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n != file::FILE) {
                let bytes = std::fs::read(&p).unwrap_or_default();
                seen += 1;
                let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
                assert!(
                    !has(base32.as_bytes()) && !has(hex.as_bytes()) && !has(&secret_bytes(&secret)),
                    "{} holds the secret",
                    p.display()
                );
            }
        }
    }
    assert!(seen > 0, "the walk read the store's files");
    // It is in the one file, which only the daemon writes.
    let kept = std::fs::read_to_string(state.join(file::FILE)).unwrap();
    assert!(kept.contains(&hex));
}

/// An agent's process on the owner's own socket never rides the owner's
/// window and never carries a code: the owner's good code opened the `cli`
/// window, and an agent's yes from the same place changes nothing, spends none
/// of the owner's tries, and is recorded.
#[tokio::test]
async fn an_agents_process_never_rides_the_owners_window() {
    let (r, corr, _, second) = asked().await;
    mark_it(&r, &corr, false);
    mark_it(&r, &second, false);
    let secret = enrol(&r);
    confirm(&r, cli(), &corr, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    assert_eq!(rows(&r, "owner.proof.window_opened").len(), 1);
    let agent = || cli().behind(crate::peer_proc::Behind::Agent("an-agent".into()));
    let e = confirm(&r, agent(), &second, true, None).await.unwrap_err();
    assert!(
        refused(&e) && e.message.contains("agent's process"),
        "{e:?}"
    );
    assert!(pending(&r, &second), "the agent's yes changed nothing");
    // Nor can it carry a code, and the code it sent is not spent.
    let good = code(&secret, 1);
    let e = confirm(&r, agent(), &second, true, Some(&good))
        .await
        .unwrap_err();
    assert!(refused(&e) && pending(&r, &second), "{e:?}");
    assert_eq!(
        rows(&r, "owner.proof.checked").len(),
        1,
        "only the owner's own"
    );
    // The owner, from the same place, still goes through on the window.
    confirm(&r, cli(), &second, true, None).await.unwrap();
    assert!(!pending(&r, &second));
}
