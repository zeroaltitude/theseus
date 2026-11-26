//! A server that signs in, round 6 of its review (issue 55): what is shown
//! is read from the board's own producers, so a token equal to a keyless
//! server's prompt name, to an unknown `read` name in the status, or to the
//! words the catalog renders for a bare tool is refused; the authorization
//! code is a door, by the listener and by a paste, and no redemption carries
//! a code that is refused; a keyless server's failure note is stored as main
//! stores it. Every test scans every exit after its last turn, with the log
//! read at its end.

use std::time::Duration;

use serde_json::json;
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth};

use super::oauth::{call, rig_edit, Rig};
use super::oauth_doors::{plain_lists, rig_doors, sign_in, state, NAME};
use super::oauth_exits::{all_exits, capture, issued_in};
use super::{session, turn, until};
use crate::approval::Surface;
use crate::config::mcp::McpOauth;
use crate::mcp::attached::{Attached, NOTE_AUTHOR};
use crate::mcp::doors::{RENEWAL_SHOWN, SHOWN_WORDS};
use crate::mcp::State;

/// What a redemption whose access token is `candidate` does, `plain` set up
/// by `ready` first: refused at the door, nothing written, nothing sent, and
/// no other value in any exit of the whole test.
async fn refused_at_the_finish(
    what: &str,
    candidate: &str,
    ready: impl FnOnce(&Rig, &theseus_mcp::fake::Fake),
    ready_tool: &str,
) {
    let (buf, _guard) = capture();
    let (r, plain) = rig_doors(AuthConfig {
        access_token_is: Some(candidate.into()),
        ..AuthConfig::default()
    })
    .await;
    let sid = session(&r.core, None);
    ready(&r, &plain);
    r.core.mcp.start();
    plain_lists(&r, ready_tool).await;
    let done = sign_in(&r).await;
    assert_eq!(done["error"]["message"], SHOWN_WORDS, "{what}: {done}");
    let kept = crate::mcp::oauth::file(&crate::mcp::oauth::dir(&r.dir.path().join("store")), NAME);
    assert!(!kept.exists(), "{what}: nothing is written");
    assert_eq!(state(&r, NAME), State::NeedsSignIn.as_str(), "{what}");
    assert_ne!(r.auth.last_bearer().as_deref(), Some(candidate), "{what}");
    turn(&r.core, &sid, "use the docs").await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[candidate]);
    assert!(
        found.is_empty(),
        "{what}: {} values in the exits",
        found.len()
    );
}

/// A token equal to a prompt name of a server with no sign-in, a name in no
/// tool list, is refused at the finish; and at a renewal, the sign-in lost.
#[tokio::test]
async fn a_token_equal_to_a_keyless_prompt_name_is_refused() {
    let name = "prompt-only-name-1";
    refused_at_the_finish(
        "a keyless prompt's name",
        name,
        |_, plain| plain.set_prompt_only(Some(name.into()), false),
        "echo",
    )
    .await;
    // At a renewal.
    let (buf, _guard) = capture();
    let (r, plain) = rig_doors(AuthConfig {
        access_ttl: 30,
        ..AuthConfig::default()
    })
    .await;
    let sid = session(&r.core, None);
    plain.set_prompt_only(Some(name.into()), false);
    r.core.mcp.start();
    let done = sign_in(&r).await;
    assert_eq!(done["connected"], true, "{done}");
    until("plain's prompt", || {
        r.core
            .mcp
            .prompt_infos(Some("plain"))
            .iter()
            .any(|p| p.prompt == name)
    })
    .await;
    r.auth.set(|c| c.renewed_access_is = Some(name.into()));
    let client = r
        .core
        .mcp
        .server(NAME)
        .unwrap()
        .client(Duration::from_secs(5))
        .await
        .unwrap();
    let _ = client.call_tool("echo", json!({"text": "a"})).await;
    until("the sign-in lost", || {
        state(&r, NAME) == State::NeedsSignIn.as_str()
    })
    .await;
    assert_ne!(r.auth.last_bearer().as_deref(), Some(name));
    turn(&r.core, &sid, "use the docs").await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(log.contains(RENEWAL_SHOWN));
    let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[name]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}

/// A token equal to a `read` name of a server with no sign-in that no tool
/// of it has (the status's `unknown_read`) is refused at the finish.
#[tokio::test]
async fn a_token_equal_to_an_unknown_read_name_is_refused() {
    let (buf, _guard) = capture();
    let missing = "missing_read_tool";
    let (r, _plain) = super::oauth_doors::rig_doors_with(
        AuthConfig {
            access_token_is: Some(missing.into()),
            ..AuthConfig::default()
        },
        |plain| plain.read = vec![missing.into()],
    )
    .await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    plain_lists(&r, "echo").await;
    let status = r.core.mcp.status();
    let plain = status.iter().find(|s| s.name == "plain").unwrap();
    assert_eq!(plain.unknown_read, vec![missing.to_string()]);
    let done = sign_in(&r).await;
    assert_eq!(done["error"]["message"], SHOWN_WORDS, "{done}");
    assert_ne!(r.auth.last_bearer().as_deref(), Some(missing));
    turn(&r.core, &sid, "use the docs").await;
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[missing]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}

/// A token equal to the words the catalog renders for a tool with no
/// description ("<name> (no description given)") is refused at the finish.
#[tokio::test]
async fn a_token_equal_to_a_rendered_description_is_refused() {
    refused_at_the_finish(
        "a rendered description",
        "no description given",
        |_, plain| plain.set_prompt_only(None, true),
        "bare_tool",
    )
    .await;
}

/// An authorization code equal to the server's configured name, by the
/// listener and by a paste: the finish fails at the door, and no token
/// request carries it (the fake issuer's received requests).
#[tokio::test]
async fn a_code_by_the_listener_equal_to_the_servers_name_is_not_redeemed() {
    code_equal_to_the_name(false).await;
}

/// The same code, by a paste.
#[tokio::test]
async fn a_code_by_a_paste_equal_to_the_servers_name_is_not_redeemed() {
    code_equal_to_the_name(true).await;
}

async fn code_equal_to_the_name(paste: bool) {
    {
        let (buf, _guard) = capture();
        let (r, _plain) = rig_doors(AuthConfig {
            code_is: Some(NAME.into()),
            ..AuthConfig::default()
        })
        .await;
        let sid = session(&r.core, None);
        r.core.mcp.start();
        let begun = call(
            &r.core,
            Surface::Cli,
            "mcp.login.begin",
            json!({"name": NAME, "manual": paste}),
        )
        .await;
        let url = begun["url"].as_str().unwrap_or_else(|| panic!("{begun}"));
        let sent_to = r.auth.browse(url, !paste).await;
        let finish = match paste {
            true => json!({"login_id": begun["login_id"], "pasted": sent_to}),
            false => json!({"login_id": begun["login_id"], "wait_secs": 10}),
        };
        let done = call(&r.core, Surface::Cli, "mcp.login.finish", finish).await;
        assert_eq!(
            done["error"]["message"], SHOWN_WORDS,
            "paste {paste}: {done}"
        );
        let seen = r.auth.seen();
        assert!(
            seen.codes_sent.is_empty(),
            "paste {paste}: a code was sent: {}",
            seen.codes_sent.len()
        );
        assert_eq!(seen.redeemed, 0);
        let known = crate::scrub::Scrubber::from_board(r.core.secrets.clone());
        assert!(known.scrub(NAME).1 > 0, "the code stays held");
        turn(&r.core, &sid, "use the docs").await;
        let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
        let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[NAME]);
        assert!(
            found.is_empty(),
            "paste {paste}: {} values in the exits",
            found.len()
        );
    }
}

/// A server with no sign-in that fails tells the conversation that attached
/// it as main does: the note is appended at once, off the runtime's workers,
/// even while a turn of that conversation holds it.
#[tokio::test]
async fn a_keyless_failure_note_is_stored_as_main_stores_it() {
    let (buf, _guard) = capture();
    let (r, _plain) = rig_doors(AuthConfig::default()).await;
    let sid = session(&r.core, None);
    r.core.mcp.start();
    turn(&r.core, &sid, "use the docs").await;
    let mut set = crate::mcp::attached::AttachedSet::new();
    set.insert(
        "plain".into(),
        Attached {
            by: "the owner".into(),
            via: "the CLI".into(),
            at_ms: 1,
            session_id: sid.clone(),
        },
    );
    r.core.mcp.note_attached(set);
    let exec = r
        .core
        .store
        .get_session::<crate::session::SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .execution_id
        .expect("the conversation's execution");
    let held = r.core.kernel.hold_turn(&exec).expect("a turn holds it");
    r.core.mcp.tell_failed("plain", "HTTP 500 (server_error)");
    until("the note, while the turn holds", || {
        super::oauth::notes(&r, &sid)
            .iter()
            .any(|n| n.contains("The MCP server `plain` this conversation attached"))
    })
    .await;
    drop(held);
    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let note = nodes
        .iter()
        .find(|(_, n)| n.author.as_deref() == Some(NOTE_AUTHOR))
        .expect("the note node");
    assert_eq!(
        format!("{:?}", note.1.body),
        format!(
            "{:?}",
            crate::node::Node::relayed(
                &sid,
                None,
                crate::node::Origin::Harness,
                NOTE_AUTHOR,
                &crate::mcp::attached::failed_note("plain", "HTTP 500 (server_error)"),
            )
            .body
        ),
        "main's note, as main writes it"
    );
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[]);
    assert!(found.is_empty(), "{} values in the exits", found.len());
}

/// A genuine tool list from an earlier run, and its own digest.
fn earlier_list() -> crate::mcp::StoredList {
    let tools: Vec<crate::mcp::Listed> = vec![serde_json::from_value(json!({
        "name": "previous_tool",
        "description": "from an earlier run",
        "inputSchema": {"type": "object"}
    }))
    .unwrap()];
    crate::mcp::StoredList {
        digest: crate::mcp::digest(&tools),
        tools,
    }
}

/// The stored tool record is read whole (round 7): a token equal to the
/// digest of a signed-in server's stored list from an earlier run (in no
/// live status: the server is not seeded and has not listed) is refused at
/// the finish, before anything is kept or sent; and so is one equal to the
/// digest of a list an attach's trial retained.
#[tokio::test]
async fn a_token_equal_to_a_stored_lists_digest_is_refused() {
    let list = earlier_list();
    let digest = list.digest.clone();
    // The store's record of the server that signs in.
    {
        let (buf, _guard) = capture();
        let stored = list.clone();
        let r = rig_edit(
            FakeAuth::start(AuthConfig {
                access_token_is: Some(digest.clone()),
                ..AuthConfig::default()
            })
            .await,
            McpOauth::default(),
            move |store_dir| {
                crate::store::Store::open(store_dir)
                    .unwrap()
                    .put_meta(&format!("{}{NAME}", crate::mcp::STORED_PREFIX), &stored)
                    .unwrap();
            },
            |cfg| {
                let docs = cfg.mcp.servers.remove("docs").unwrap();
                cfg.mcp.servers.insert(NAME.into(), docs);
            },
        )
        .await;
        let sid = session(&r.core, None);
        r.core.mcp.start();
        let status = r.core.mcp.status();
        let mine = status.iter().find(|s| s.name == NAME).unwrap();
        assert_ne!(
            mine.digest, digest,
            "not seeded: the digest is in no live status"
        );
        assert!(serde_json::to_string(&status)
            .unwrap()
            .find(&digest)
            .is_none());
        let done = sign_in(&r).await;
        assert_eq!(
            done["error"]["message"], SHOWN_WORDS,
            "the store's record: {done}"
        );
        let kept =
            crate::mcp::oauth::file(&crate::mcp::oauth::dir(&r.dir.path().join("store")), NAME);
        assert!(!kept.exists(), "nothing is written");
        assert_ne!(r.auth.last_bearer().as_deref(), Some(digest.as_str()));
        turn(&r.core, &sid, "use the docs").await;
        let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
        let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[&digest]);
        assert!(found.is_empty(), "{} values in the exits", found.len());
    }
    // A list an attach's trial retained.
    {
        let (buf, _guard) = capture();
        let (r, _plain) = rig_doors(AuthConfig {
            access_token_is: Some(digest.clone()),
            ..AuthConfig::default()
        })
        .await;
        let sid = session(&r.core, None);
        r.core.mcp.keep_listed("trialled", list.clone());
        r.core.mcp.start();
        let done = sign_in(&r).await;
        assert_eq!(
            done["error"]["message"], SHOWN_WORDS,
            "the trial's list: {done}"
        );
        let kept =
            crate::mcp::oauth::file(&crate::mcp::oauth::dir(&r.dir.path().join("store")), NAME);
        assert!(!kept.exists(), "nothing is written");
        assert_ne!(r.auth.last_bearer().as_deref(), Some(digest.as_str()));
        turn(&r.core, &sid, "use the docs").await;
        let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
        let found = issued_in(&r, &all_exits(&r, &sid, &log).await, &[&digest]);
        assert!(found.is_empty(), "{} values in the exits", found.len());
    }
}
