//! The consent and the minting (the Google design §2.2): the account check
//! before anything is stored, the state, the paste, the three store kinds
//! (`op://` through a fake `op`, `file:`, and `env:` refused), the live hold,
//! and the revoke.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::{CODE, REFRESH_TOKEN};
use theseus_protocol::google::{GoogleAuthFinishParams, GoogleAuthFinishResult};

use super::tests::*;
use crate::approval::Surface;
use crate::secrets::OpReader;

fn state_of(url: &str) -> String {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .expect("the consent URL carries a state")
        .to_string()
}

fn pasted(state: &str) -> String {
    format!("http://127.0.0.1:41999/?code={CODE}&state={state}&scope=ignored")
}

async fn begin(r: &Rig) -> (String, String) {
    let b = r.g.auth_begin(true).await.unwrap();
    assert_eq!(b.paste_after_secs, 0, "manual goes straight to the paste");
    (b.auth_id, state_of(&b.url))
}

async fn finish(r: &Rig, id: &str, paste: &str) -> Result<GoogleAuthFinishResult, String> {
    let p = GoogleAuthFinishParams {
        auth_id: id.into(),
        pasted: Some(paste.into()),
        wait_secs: 0,
    };
    r.g.auth_finish(&p, None).await
}

fn file_ref(dir: &Path) -> String {
    format!("file:{}/google-refresh-token", dir.display())
}

fn token_file(r: &Rig) -> std::path::PathBuf {
    r.dir.path().join("google-refresh-token")
}

async fn unminted() -> Rig {
    rig_with(None, file_ref, vec![], |_| {}).await
}

#[tokio::test]
async fn a_refresh_token_for_another_account_is_refused_and_not_stored() {
    let r = unminted().await;
    r.fake.set_account("collaborator@example.test");
    let (id, state) = begin(&r).await;
    let e = finish(&r, &id, &pasted(&state)).await.unwrap_err();
    assert!(
        e.contains("different account") && e.contains("nothing was stored"),
        "{e}"
    );
    assert!(
        !e.contains("collaborator"),
        "the other address is not echoed: {e}"
    );
    // The code was redeemed (the id_token names the account), and then nothing.
    assert_eq!(r.fake.forms().len(), 1);
    assert!(!token_file(&r).exists(), "nothing was written to the store");
    assert!(
        r.core.secrets.get("google_refresh_token").is_none(),
        "nothing was held"
    );
    assert_ne!(r.g.check_now().phase, super::Phase::Ok);
    assert!(ledgered(&r.core, "google.authorized").is_empty());
}

#[tokio::test]
async fn pkce_state_mismatch_is_refused() {
    let r = unminted().await;
    let (id, state) = begin(&r).await;
    let e = finish(&r, &id, &pasted("someone-elses-state"))
        .await
        .unwrap_err();
    assert!(e.contains("state"), "{e}");
    assert!(r.fake.forms().is_empty(), "the code was never redeemed");
    assert!(!token_file(&r).exists());
    // The consent is still open: the right address finishes it.
    assert_eq!(
        finish(&r, &id, &pasted(&state)).await.unwrap().state,
        "done"
    );
}

#[tokio::test]
async fn a_pasted_redirect_url_finishes_auth() {
    let r = unminted().await;
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(
        (
            done.state.as_str(),
            done.store.as_str(),
            done.account.as_str()
        ),
        ("done", "file", ACCOUNT)
    );
    assert!(
        done.not_granted.is_empty() && done.note.is_empty(),
        "{done:?}"
    );
    // The token is in the file at 0600, held on the board, and the state is ok.
    let f = token_file(&r);
    assert_eq!(std::fs::read_to_string(&f).unwrap().trim(), REFRESH_TOKEN);
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        r.core.secrets.get("google_refresh_token").unwrap().expose(),
        REFRESH_TOKEN
    );
    assert_eq!(r.g.status().state, "ok");
    assert_eq!(r.g.status().store, "file");
    // No restart: a tool works at once, on the held token.
    assert!(r.g.ready("google.call").await.is_ok());
    // The row names the account, the scopes and the store: never the token.
    let rows = ledgered(&r.core, "google.authorized");
    assert_eq!(rows.len(), 0, "the row is the RPC's, written by the method");
    // A second paste of the same code finds no consent open.
    assert!(finish(&r, &id, &pasted(&state))
        .await
        .unwrap_err()
        .contains("no sign-in is open"));
}

#[tokio::test]
async fn the_google_methods_are_the_clis_alone_and_the_row_is_the_methods() {
    let r = unminted().await;
    for surface in [Surface::Discord, Surface::Web, Surface::Unnamed] {
        for (m, params) in [
            ("google.auth.begin", json!({})),
            ("google.auth.finish", json!({"auth_id": "x"})),
            ("google.revoke", json!({})),
            ("google.status", json!({})),
        ] {
            let e = crate::tests_mcp_memory::rpc(&r.core, surface, m, params)
                .await
                .unwrap_err();
            assert!(
                e.message.contains("the operator's, from the CLI"),
                "{m}: {e:?}"
            );
        }
    }
    let begun = crate::tests_mcp_memory::rpc(
        &r.core,
        Surface::Cli,
        "google.auth.begin",
        json!({"manual": true}),
    )
    .await
    .unwrap();
    let state = state_of(begun["url"].as_str().unwrap());
    let done = crate::tests_mcp_memory::rpc(
        &r.core,
        Surface::Cli,
        "google.auth.finish",
        json!({"auth_id": begun["auth_id"], "pasted": pasted(&state)}),
    )
    .await
    .unwrap();
    assert_eq!(done["state"], "done");
    let rows = ledgered(&r.core, "google.authorized");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["account"].as_str(), rows[0]["store"].as_str()),
        (Some(ACCOUNT), Some("file"))
    );
    assert!(rows[0]["scopes"].as_array().unwrap().len() >= 6);
    assert!(
        !everything(&r.core, &[]).contains(REFRESH_TOKEN),
        "the token is in no row"
    );
    let status = crate::tests_mcp_memory::rpc(&r.core, Surface::Cli, "google.status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["state"], "ok");
}

#[tokio::test]
async fn partial_grant_turns_off_only_the_tools_missing_a_scope() {
    let r = unminted().await;
    let asked = theseus_google::scopes::requested(&theseus_google::Service::ALL);
    let all_but_drive: Vec<&str> = asked
        .iter()
        .map(String::as_str)
        .filter(|s| *s != "https://www.googleapis.com/auth/drive")
        .collect();
    r.fake.set_granted(&all_but_drive);
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(
        done.not_granted,
        ["Drive was not granted: the drive tools stay off"]
    );
    assert!(done.tools_on.contains(&"google.call".to_string()));
    assert!(
        done.tools_on.iter().all(|t| !t.starts_with("drive.")),
        "{:?}",
        done.tools_on
    );
    assert!(r.g.ready("google.call").await.is_ok());
    // A call of the service that was not granted is refused before it is sent;
    // one of a service that was, goes.
    let meta = |m: &str| super::Meta {
        tool: "google.call",
        method: m.into(),
        resource: None,
        class: "read",
        recipients: None,
    };
    let before = r.fake.log().len();
    let f =
        r.g.call(
            meta("drive.files.list"),
            theseus_google::Request::get(theseus_google::Service::Drive, "/drive/v3/files"),
        )
        .await
        .err()
        .unwrap();
    assert!(f.message.contains("Drive is off"), "{}", f.message);
    assert_eq!(r.fake.log().len(), before, "nothing was sent");
    r.g.call(
        meta("gmail.users.getProfile"),
        theseus_google::Request::get(theseus_google::Service::Gmail, "/gmail/v1/users/me/profile"),
    )
    .await
    .ok()
    .unwrap();
    // Nothing granted at all stores nothing.
    let r = unminted().await;
    r.fake
        .set_granted(&["openid", "https://www.googleapis.com/auth/userinfo.email"]);
    let (id, state) = begin(&r).await;
    assert!(finish(&r, &id, &pasted(&state))
        .await
        .unwrap_err()
        .contains("none of the services"));
    assert!(!token_file(&r).exists());
}

#[tokio::test]
async fn env_ref_refresh_token_is_refused() {
    let r = rig_with(
        None,
        |_| "env:GOOGLE_REFRESH_TOKEN_TEST".into(),
        vec![],
        |_| {},
    )
    .await;
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(done.store, "memory");
    assert!(
        done.note
            .contains("an env ref can't be written; point it at op:// or file:"),
        "{}",
        done.note
    );
    // It is held in memory, and works.
    assert_eq!(
        r.core.secrets.get("google_refresh_token").unwrap().expose(),
        REFRESH_TOKEN
    );
    assert!(r.g.ready("google.call").await.is_ok());
    assert_eq!(r.g.status().store, "memory");
}

/// A fake `op` in a script: it logs its argv and each stdin it was given, keeps
/// the value a create or an edit gave it (`mode` says whether it writes, and
/// whether `read` tells the truth), and answers `item get` by whether it holds one.
fn fake_op(dir: &Path, mode: &str) -> Arc<OpReader> {
    let script = format!(
        r#"#!/bin/sh
LOG='{d}/op.argv'; STORE='{d}/op.stored'; MODE='{mode}'
printf '%s\n' "$*" >> "$LOG"
case "$1 $2" in
  "item get")
    if [ -f "$STORE" ]; then echo '{{}}'; exit 0; fi
    echo '[ERROR] 2026/10/19 08:00:00 "theseus-google" isn'"'"'t an item in the "vault" vault.' >&2; exit 1 ;;
  "item create"|"item edit")
    # The real op reads a create's template from stdin only after `-`.
    if [ "$2" = create ] && [ "$3" != - ]; then echo '[ERROR] 2026/10/19 08:00:00 a category is required' >&2; exit 1; fi
    if [ "$MODE" = deny ]; then echo '[ERROR] 2026/10/19 08:00:00 you do not have write_items on this vault' >&2; exit 1; fi
    n=$(ls '{d}'/op.stdin.* 2>/dev/null | wc -l)
    cat > '{d}'/op.stdin.$n
    sed -n 's/.*"value":"\([^"]*\)".*/\1/p' '{d}'/op.stdin.$n > "$STORE"
    echo '{{}}' ;;
  "read --no-newline")
    [ -f "$STORE" ] || exit 1
    if [ "$MODE" = tamper ]; then printf '%s' 'a-different-value'; else printf '%s' "$(cat "$STORE")"; fi ;;
  *) echo "[ERROR] unexpected: $*" >&2; exit 1 ;;
esac
"#,
        d = dir.display(),
        mode = mode
    );
    let bin = dir.join("op");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    Arc::new(OpReader::with_bin("ops-service-account-token-test", bin))
}

async fn op_rig(mode: &str) -> (Rig, Arc<OpReader>) {
    let r = rig_with(
        None,
        |_| "op://vault/theseus-google/refresh_token".into(),
        vec![],
        |_| {},
    )
    .await;
    let op = fake_op(r.dir.path(), mode);
    *r.g.op.lock().unwrap() = Some(op.clone());
    (r, op)
}

fn lines(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

#[tokio::test]
async fn the_refresh_token_never_reaches_op_argv() {
    let (r, _) = op_rig("ok").await;
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(
        (done.store.as_str(), done.note.as_str()),
        ("op", ""),
        "{done:?}"
    );
    let argv = lines(r.dir.path(), "op.argv");
    assert!(!argv.is_empty());
    assert!(
        !argv.contains(REFRESH_TOKEN) && !argv.contains("fake-refresh"),
        "argv: {argv}"
    );
    // The token is in stdin, once, as the item JSON's one concealed field.
    let stdin = lines(r.dir.path(), "op.stdin.0");
    let v: Value = serde_json::from_str(&stdin).unwrap();
    assert_eq!(v["fields"][0]["value"], REFRESH_TOKEN);
    assert_eq!(v["fields"][0]["type"], "CONCEALED");
    assert_eq!(v["title"], "theseus-google");
    // No temp file holds it either.
    for e in std::fs::read_dir(r.dir.path()).unwrap().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("op.stdin.") || n == "op.stored" || n == "op" {
            continue;
        }
        if e.path().is_file() {
            assert!(
                !std::fs::read_to_string(e.path())
                    .unwrap_or_default()
                    .contains(REFRESH_TOKEN),
                "{n}"
            );
        }
    }
}

#[tokio::test]
async fn a_minted_token_is_read_back_before_success() {
    let (r, _) = op_rig("ok").await;
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(done.store, "op");
    let argv = lines(r.dir.path(), "op.argv");
    let order: Vec<String> = argv
        .lines()
        .map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
        .collect();
    let at = |what: &str| order.iter().position(|l| l.starts_with(what));
    assert!(at("item get") < at("item create"), "{order:?}");
    assert!(
        at("item create") < at("read --no-newline"),
        "created, then read back: {order:?}"
    );
    // An item that is there is edited, not created again.
    let again = begin(&r).await;
    assert_eq!(
        finish(&r, &again.0, &pasted(&again.1)).await.unwrap().store,
        "op"
    );
    assert!(lines(r.dir.path(), "op.argv").contains("item edit"));
    // A store that reads back another value is not a success.
    let (r, _) = op_rig("tamper").await;
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(done.store, "memory");
    assert!(
        done.note.contains("did not read back as written"),
        "{}",
        done.note
    );
    assert!(!done.note.contains(REFRESH_TOKEN));
}

#[tokio::test]
async fn a_refused_op_write_holds_the_token_in_memory_and_says_so() {
    let (r, _) = op_rig("deny").await;
    let (id, state) = begin(&r).await;
    let done = finish(&r, &id, &pasted(&state)).await.unwrap();
    assert_eq!(done.state, "done");
    assert_eq!(done.store, "memory");
    assert!(
        done.note.contains("could not be stored") && done.note.contains("write_items"),
        "{}",
        done.note
    );
    assert!(done.note.contains("until the next restart"));
    assert_eq!(
        r.core.secrets.get("google_refresh_token").unwrap().expose(),
        REFRESH_TOKEN
    );
    assert_eq!(r.g.status().store, "memory");
    assert!(
        r.g.ready("google.call").await.is_ok(),
        "Google works until the restart"
    );
}

#[tokio::test]
async fn revoke_clears_the_store_drops_the_token_and_turns_the_tools_off() {
    let r = unminted().await;
    let (id, state) = begin(&r).await;
    finish(&r, &id, &pasted(&state)).await.unwrap();
    assert!(token_file(&r).exists());
    let revoked = crate::tests_mcp_memory::rpc(&r.core, Surface::Cli, "google.revoke", json!({}))
        .await
        .unwrap();
    assert_eq!(
        (
            revoked["revoked_at_google"].as_bool(),
            revoked["cleared"].as_bool()
        ),
        (Some(true), Some(true))
    );
    assert!(r.fake.revoked());
    assert!(!token_file(&r).exists());
    assert!(r.core.secrets.get("google_refresh_token").is_none());
    assert_eq!(r.g.status().state, "needs_auth");
    assert!(r
        .g
        .ready("google.call")
        .await
        .unwrap_err()
        .contains("revoked"));
    assert_eq!(ledgered(&r.core, "google.revoked").len(), 1);
}
