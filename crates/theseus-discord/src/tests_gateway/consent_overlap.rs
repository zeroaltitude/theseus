//! Google's consent through the fake gateway and the fake Google (issue 59, the second fix
//! round): an old form cannot clear a newer consent when two genuine presses overlap, and the
//! revoke says what happened, from both of its results.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_google::fake::{Fake, Reply, CODE, REFRESH_TOKEN};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::{Forged, Msg};

use super::{bindings, guild, Rig, ANA, ANA_DM};
use crate::runtime::consent::{pause_at, SENT, WAITING};
use crate::runtime::google_sign_in::{EXPIRED, NEITHER, NOT_AT_GOOGLE, NOT_CLEARED, REVOKED};

const ACCOUNT: &str = "owner@example.test";

/// A rig with Google bound at the fake, its refresh token kept in a file of `keep`; the card.
async fn google_rig(keep: &tempfile::TempDir) -> (Rig, Fake, PathBuf, Msg) {
    let google = Fake::start().await;
    let token_file = keep.path().join("google-refresh-token");
    super::MORE_SECRETS.with(|m| {
        m.borrow_mut().extend([
            (
                "google_client".to_string(),
                Some("client_id: id-1.apps.example\nclient_secret: s3cret-client-0123".to_string()),
            ),
            ("google_refresh_token".to_string(), None),
        ]);
    });
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let file_ref = format!("file:{}", token_file.display());
    let url = google.url();
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], move |cfg| {
        cfg.google = theseus_core::config::GoogleConfig {
            account: ACCOUNT.into(),
            endpoint: Some(url),
            credentials: theseus_core::config::GoogleCredentialNames::default(),
            ..Default::default()
        };
        cfg.secrets.insert(
            "google_client".into(),
            "op://vault/google client/notesPlain".into(),
        );
        cfg.secrets.insert("google_refresh_token".into(), file_ref);
    })
    .await;
    let body = json!({"kind": "google_sign_in", "account": ACCOUNT,
                      "text": theseus_core::google::offer::card_text(ACCOUNT)});
    let (post, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&post);
    let card_of = |m: &Msg| m.versions[0].contains("Connect Google for");
    r.until("the card", || r.posted(ANA_DM).iter().any(card_of))
        .await;
    let card = r.posted(ANA_DM).into_iter().find(card_of).unwrap();
    (r, google, token_file, card)
}

fn answer(r: &Rig, interaction: &str) -> Option<String> {
    r.fake
        .replies()
        .iter()
        .filter(|x| x.interaction.as_deref() == Some(interaction))
        .filter_map(|x| x.content.clone())
        .find(|c| !c.is_empty())
}

/// The consent the answer to `began` shows: its state, and the form its paste button opens.
async fn its_form(r: &Rig, card: &Msg, began: &str) -> (String, String) {
    let shown = || {
        let text = r
            .fake
            .replies()
            .iter()
            .filter(|x| x.interaction.as_deref() == Some(began))
            .filter_map(|x| x.content.clone())
            .find(|c| c.contains("](<"))?;
        let address = text.split("](<").nth(1)?.split(">)").next()?.to_string();
        let state = address
            .split(['?', '&'])
            .find_map(|p| p.strip_prefix("state="))?
            .to_string();
        let button = r
            .fake
            .replies()
            .iter()
            .filter(|x| x.interaction.as_deref() == Some(began))
            .find_map(|x| {
                x.components.get(0)?["components"][0]["custom_id"]
                    .as_str()
                    .map(str::to_string)
            })?;
        Some((state, button))
    };
    r.until("the consent is shown", || shown().is_some()).await;
    let (state, button) = shown().unwrap();
    let paste = r
        .fake
        .press_forged(&Forged {
            message: &card.id,
            custom_id: &button,
            user: (ANA, "ana"),
            channel: None,
        })
        .unwrap();
    let form = || {
        r.fake
            .replies()
            .iter()
            .find(|x| x.interaction.as_deref() == Some(paste.as_str()))
            .and_then(|x| x.modal.as_ref())
            .and_then(|m| m["custom_id"].as_str().map(str::to_string))
    };
    r.until("the form opens", || form().is_some()).await;
    (state, form().unwrap())
}

fn send(r: &Rig, form: &str, state: &str) -> String {
    let pasted = format!("http://127.0.0.1:41999/?code={CODE}&state={state}&scope=x");
    r.fake
        .submit_modal(
            &Sender {
                user: ANA,
                name: "ana",
                channel: None,
            },
            form,
            &[("address", pasted.as_str())],
            &[],
            None,
        )
        .unwrap()
}

fn finishes_sent() -> usize {
    SENT.lock()
        .unwrap()
        .iter()
        .filter(|m| **m == "google.auth.finish")
        .count()
}

/// Form A is sent while a second genuine press's begin B holds the daemon's slot (held, by the
/// test, after the daemon answered and before the binding changed its record): A waits for the
/// slot, then is refused in fixed words and sends nothing, and B still finishes.
#[tokio::test]
async fn an_old_form_cannot_clear_a_newer_consent_when_presses_overlap() {
    let keep = tempfile::tempdir().unwrap();
    let (r, _google, _file, card) = google_rig(&keep).await;
    let a = r.press(&card.id, "Connect Google", (ANA, "ana"));
    let (_, form_a) = its_form(&r, &card, &a).await;
    let held = pause_at("google-begun");
    let b = r.press(&card.id, "Connect Google", (ANA, "ana"));
    tokio::time::timeout(Duration::from_secs(10), held.reached.notified())
        .await
        .expect("B's begin reaches the daemon");
    let sent_a = send(&r, &form_a, "whatever-a-had");
    r.until("A waits for the slot, or was sent", || {
        WAITING.load(Ordering::SeqCst) >= 1 || finishes_sent() > 0
    })
    .await;
    held.release.notify_one();
    r.until("A is answered", || answer(&r, &sent_a).is_some())
        .await;
    assert_eq!(answer(&r, &sent_a).as_deref(), Some(EXPIRED));
    assert_eq!(finishes_sent(), 0, "A sent nothing to the daemon");
    let (state_b, form_b) = its_form(&r, &card, &b).await;
    let sent_b = send(&r, &form_b, &state_b);
    r.until("B finishes", || {
        answer(&r, &sent_b).is_some_and(|t| t.starts_with("✅ Google is connected for"))
    })
    .await;
}

/// The revoke's answer is built from both of its results, in the four cases: ended at Google and
/// cleared; Google's revoke failing at the fake; the kept file that cannot be removed (its folder
/// made read-only); both.
#[tokio::test]
async fn googles_revoke_says_what_happened_in_all_four_cases() {
    let keep = tempfile::tempdir().unwrap();
    let (r, google, file, card) = google_rig(&keep).await;
    let folder = |mode: u32| {
        std::fs::set_permissions(keep.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    };
    for (google_fails, store_fails, want) in [
        (false, false, REVOKED),
        (true, false, NOT_AT_GOOGLE),
        (false, true, NOT_CLEARED),
        (true, true, NEITHER),
    ] {
        folder(0o755);
        std::fs::write(&file, REFRESH_TOKEN).unwrap();
        r.core.secrets.hold(
            "google_refresh_token",
            theseus_core::secrets::Secret::new(REFRESH_TOKEN.into()),
        );
        if google_fails {
            google.fail_oauth_next(&Reply::json(400, json!({"error": "invalid_request"})));
        }
        if store_fails {
            folder(0o555);
        }
        let out = r.press(&card.id, "Revoke Google", (ANA, "ana"));
        r.until("the revoke is answered", || answer(&r, &out).is_some())
            .await;
        assert_eq!(
            answer(&r, &out).as_deref(),
            Some(want),
            "google fails {google_fails}, store fails {store_fails}"
        );
    }
    folder(0o755);
}
