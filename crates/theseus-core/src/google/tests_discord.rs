//! Google's one-time consent from Discord (issue 59), through the real RPC on the Discord surface
//! with the fake Google: the owner's own press in a private place begins and the pasted address
//! finishes, kept exactly as `theseus google auth` keeps it (`redeem_and_keep`); a stranger, a
//! shared place and a call that names no presser are refused; the revoke by his press; the pasted
//! value and the code in it appear in no exit; and a turn only offers.

use serde_json::{json, Value};
use theseus_google::fake::{CODE, REFRESH_TOKEN};

use super::tests::*;
use crate::approval::Surface;
use crate::tests_mcp_memory::rpc;

/// Discord ids: 15 to 21 digits, as `[places] owner` is checked.
const OWNER: &str = "100000000000000042";
const STRANGER: &str = "100000000000000077";

fn state_of(url: &str) -> String {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .expect("the consent URL carries a state")
        .to_string()
}

fn pasted(state: &str) -> String {
    format!("http://127.0.0.1:41999/?code={CODE}&state={state}&scope=ignored")
}

fn presser(user: &str, dm: bool) -> Value {
    let mut d = json!({"user_id": user, "channel_id": if dm { "500" } else { "900" }});
    if !dm {
        d["guild_id"] = json!("1");
    }
    json!({"author": format!("discord:{user}"), "discord": d})
}

fn with(mut params: Value, who: Value) -> Value {
    for (k, v) in who.as_object().unwrap() {
        params[k] = v.clone();
    }
    params
}

async fn owned() -> Rig {
    rig_with(
        None,
        |d| format!("file:{}/google-refresh-token", d.display()),
        vec![],
        |cfg| cfg.places.owner = Some(vec![format!("discord:{OWNER}")]),
    )
    .await
}

fn refused(r: &Result<Value, theseus_protocol::RpcError>) -> bool {
    r.as_ref().err().is_some_and(|e| {
        e.code == theseus_protocol::error_code::REFUSED
            && e.message
                .starts_with("Google's consent from Discord is the owner's own press")
    })
}

/// The owner's press in a DM begins; the pasted address finishes, the token kept in the store
/// `[secrets]` names (as the CLI's consent keeps it); the pasted address and the code and token
/// are in no row or node.
#[tokio::test]
async fn the_owners_press_connects_google_and_the_paste_is_in_no_exit() {
    let r = owned().await;
    let begun = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.begin",
        with(json!({"manual": true}), presser(OWNER, true)),
    )
    .await
    .unwrap();
    let paste = pasted(&state_of(begun["url"].as_str().unwrap()));
    let done = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.finish",
        with(
            json!({"auth_id": begun["auth_id"], "pasted": paste}),
            presser(OWNER, true),
        ),
    )
    .await
    .unwrap();
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["store"], "file");
    let kept = std::fs::read_to_string(r.dir.path().join("google-refresh-token")).unwrap();
    assert!(kept.contains(REFRESH_TOKEN), "kept where the CLI keeps it");
    let all = everything(&r.core, &[]);
    for v in [paste.as_str(), CODE, REFRESH_TOKEN] {
        assert!(!all.contains(v), "a value is in a row");
    }
    // The revoke, by his press.
    let out = rpc(
        &r.core,
        Surface::Discord,
        "google.revoke",
        with(json!({}), presser(OWNER, true)),
    )
    .await
    .unwrap();
    assert_eq!(out["cleared"], true, "{out}");
}

/// A stranger in a DM and the owner in a shared channel are refused, and shown no address; a
/// Discord call that names no presser is refused in the CLI's old words; `google.status` stays the
/// CLI's alone.
#[tokio::test]
async fn a_stranger_a_shared_place_and_no_presser_are_refused() {
    let r = owned().await;
    for who in [presser(STRANGER, true), presser(OWNER, false)] {
        let begun = rpc(
            &r.core,
            Surface::Discord,
            "google.auth.begin",
            with(json!({"manual": true}), who.clone()),
        )
        .await;
        assert!(refused(&begun), "{begun:?}");
        let out = rpc(
            &r.core,
            Surface::Discord,
            "google.revoke",
            with(json!({}), who),
        )
        .await;
        assert!(refused(&out), "{out:?}");
    }
    let plain = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.begin",
        json!({"manual": true}),
    )
    .await;
    assert!(
        plain
            .as_ref()
            .err()
            .is_some_and(|e| e.message.contains("the operator's, from the CLI")),
        "{plain:?}"
    );
    let status = rpc(
        &r.core,
        Surface::Discord,
        "google.status",
        presser(OWNER, true),
    )
    .await;
    assert!(status.is_err(), "status stays the CLI's: {status:?}");
    assert!(r.fake.forms().is_empty(), "nothing was redeemed");
}

/// A turn's tools never begin, agree to or finish a consent: the offer is among Google's tools and
/// says nothing began.
#[tokio::test]
async fn a_turn_only_offers_the_google_consent() {
    let r = owned().await;
    let tools: Vec<String> = r
        .core
        .tools
        .registry
        .all()
        .map(|t| t.name().to_string())
        .collect();
    assert!(tools.iter().any(|t| t == super::offer::OFFER), "{tools:?}");
    assert!(tools
        .iter()
        .all(|t| !t.starts_with("google.auth") && t != "google.revoke"));
    assert!(super::offer::OFFERED.contains("Nothing has started"));
    assert!(r.fake.forms().is_empty());
}

/// On the Discord surface a failed consent leaves the daemon as one of the fixed categories:
/// neither what was pasted (a redirect carrying an `error`) nor Google's own error description is
/// in the RPC error. The CLI's error for the same inputs is the words it always was.
#[tokio::test]
async fn a_discord_failure_leaves_the_daemon_in_fixed_words_and_the_clis_are_unchanged() {
    use theseus_google::fake::Reply;
    const PASTED_SENTINEL: &str = "SENTINEL-PASTED-7731";
    const GOOGLE_SENTINEL: &str = "SENTINEL-GOOGLE-8842";
    let r = owned().await;
    let fail = |surface: Surface, who: Value| {
        let core = r.core.clone();
        async move {
            let begun = rpc(
                &core,
                surface,
                "google.auth.begin",
                with(json!({"manual": true}), who.clone()),
            )
            .await
            .unwrap();
            let state = state_of(begun["url"].as_str().unwrap());
            let redirect_error =
                format!("http://127.0.0.1:41999/?state={state}&error={PASTED_SENTINEL}");
            let first = rpc(
                &core,
                surface,
                "google.auth.finish",
                with(
                    json!({"auth_id": begun["auth_id"], "pasted": redirect_error}),
                    who.clone(),
                ),
            )
            .await
            .unwrap_err();
            (first, state, redirect_error)
        }
    };
    // Discord: the redirect's error.
    let (first, _, _) = fail(Surface::Discord, presser(OWNER, true)).await;
    assert_eq!(
        first.message,
        crate::rpc::google::DISCORD_REFUSED,
        "{first:?}"
    );
    assert!(!first.message.contains(PASTED_SENTINEL));
    // Discord: Google's own error description at the token endpoint.
    let begun = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.begin",
        with(json!({"manual": true}), presser(OWNER, true)),
    )
    .await
    .unwrap();
    r.fake.fail_oauth_next(&Reply::json(
        400,
        json!({"error": "invalid_request", "error_description": GOOGLE_SENTINEL}),
    ));
    let second = rpc(
        &r.core,
        Surface::Discord,
        "google.auth.finish",
        with(
            json!({"auth_id": begun["auth_id"], "pasted": pasted(&state_of(begun["url"].as_str().unwrap()))}),
            presser(OWNER, true),
        ),
    )
    .await
    .unwrap_err();
    assert!(
        crate::rpc::google::DISCORD_FAILURES.contains(&second.message.as_str())
            || second.message == crate::rpc::google::DISCORD_REFUSED,
        "{second:?}"
    );
    assert!(!second.message.contains(GOOGLE_SENTINEL), "{second:?}");
    // The CLI: the words the unchanged parser gives, byte for byte.
    let (cli, state, redirect_error) = fail(Surface::Cli, json!({})).await;
    let before = theseus_google::oauth::parse_redirect(&redirect_error, &state)
        .unwrap_err()
        .to_string();
    assert_eq!(cli.message, before);
    assert!(
        cli.message.contains(PASTED_SENTINEL),
        "the CLI's words keep it"
    );
}

/// Invented: why a secret did not resolve. Never in a Discord answer or a log line of the begin.
const MARKER: &str = "INVENTED-REASON-4418";

/// A core owned by `OWNER` whose board is `board`, with `tweak` on its settings; no Google fake is
/// reached by a begin that fails.
async fn core_on(
    board: std::sync::Arc<crate::secrets::SecretBoard>,
    tweak: impl FnOnce(&mut crate::Config),
) -> (std::sync::Arc<crate::Core>, tempfile::TempDir) {
    let fake = theseus_google::fake::Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    tweak(&mut cfg);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    let core = crate::Core::build(crate::rpc::Parts {
        secrets: board,
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    (core, dir)
}

/// A board of `names`, with `client` published as the client secret when given.
fn board_of(
    names: &[&str],
    client: Option<Result<&str, String>>,
) -> std::sync::Arc<crate::secrets::SecretBoard> {
    let b = crate::secrets::SecretBoard::new(
        names.iter().map(|n| n.to_string()),
        std::time::Instant::now(),
    );
    if let Some(c) = client {
        let c = c.map(|v| crate::secrets::Secret::new(v.into()));
        b.publish(
            std::collections::BTreeMap::from([("google_client".to_string(), c)]),
            "test",
        );
    }
    b
}

const BOTH: [&str; 2] = ["google_client", "google_refresh_token"];

/// The begin's error: from the CLI, or as `who`'s press on Discord.
async fn begin_err(
    core: &std::sync::Arc<crate::Core>,
    who: Option<Value>,
) -> theseus_protocol::RpcError {
    let (surface, params) = match who {
        Some(who) => (Surface::Discord, with(json!({"manual": true}), who)),
        None => (Surface::Cli, json!({"manual": true})),
    };
    rpc(core, surface, "google.auth.begin", params)
        .await
        .unwrap_err()
}

/// Each kind a test can reach: the owner's press is told its marker and nothing else, and the CLI
/// the daemon's sentence, byte for byte as before, with the same code. (`Other`, the loopback
/// listener failing, is not reached here.)
#[tokio::test]
async fn each_begin_kind_tells_a_press_its_marker_and_the_cli_its_sentence() {
    use super::begin::BeginKind;
    let why = format!("op read failed: {MARKER}");
    let cases = [
        (
            "no account",
            core_on(board(None), |cfg| cfg.google = Default::default()).await,
            BeginKind::NoAccount,
            theseus_protocol::error_code::INVALID_PARAMS,
            "no Google account is bound: the config has no [google] account".to_string(),
        ),
        (
            "did not resolve",
            core_on(board_of(&BOTH, Some(Err(why.clone()))), |_| {}).await,
            BeginKind::Client,
            theseus_protocol::error_code::INTERNAL,
            format!("the client secret `google_client` did not resolve: {why}"),
        ),
        (
            "holds no client",
            core_on(board_of(&BOTH, Some(Ok("nothing here"))), |_| {}).await,
            BeginKind::Client,
            theseus_protocol::error_code::INTERNAL,
            "the client secret `google_client` holds no client_id line".to_string(),
        ),
        (
            "not an entry",
            core_on(board_of(&["google_refresh_token"], None), |_| {}).await,
            BeginKind::Client,
            theseus_protocol::error_code::INTERNAL,
            "`google_client` is not a [secrets] entry".to_string(),
        ),
        (
            "still resolving",
            core_on(board_of(&BOTH, None), |_| {}).await,
            BeginKind::Resolving,
            theseus_protocol::error_code::INTERNAL,
            "the client secret `google_client` is still resolving: try again".to_string(),
        ),
    ];
    for (what, (core, _dir), kind, code, words) in cases {
        let cli = begin_err(&core, None).await;
        assert_eq!(cli.message, words, "{what}: the CLI's sentence");
        assert_eq!(cli.code, code, "{what}");
        let pressed = begin_err(&core, Some(presser(OWNER, true))).await;
        assert_eq!(pressed.message, kind.marker(), "{what}: the press's marker");
        assert_eq!(BeginKind::of_marker(&pressed.message), Some(kind), "{what}");
        assert_eq!(pressed.code, code, "{what}");
    }
}

/// Why a secret did not resolve is in no answer a press is given, and in no log line written
/// while the begin runs; the press learns the kind.
#[tokio::test]
async fn why_a_secret_did_not_resolve_is_in_no_answer_or_log_line() {
    let (core, _dir) = core_on(
        board_of(&BOTH, Some(Err(format!("op read failed: {MARKER}")))),
        |_| {},
    )
    .await;
    let log = crate::test_support::Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(sub);
    let pressed = begin_err(&core, Some(presser(OWNER, true))).await;
    drop(guard);
    assert_eq!(
        pressed.message,
        super::begin::BeginKind::Client.marker(),
        "{pressed:?}"
    );
    assert!(!format!("{pressed:?}").contains(MARKER));
    let text = log.text();
    assert!(text.contains("Google's consent could not begin"), "{text}");
    assert!(!text.contains(MARKER), "{text}");
}

/// A press by someone else, or the owner's in a shared place, gets the refusal's words and learns
/// no kind; no warning line is written for it.
#[tokio::test]
async fn a_strangers_begin_and_a_shared_place_learn_no_kind() {
    let (core, _dir) = core_on(board_of(&BOTH, Some(Ok("nothing here"))), |_| {}).await;
    let log = crate::test_support::Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(sub);
    for who in [presser(STRANGER, true), presser(OWNER, false)] {
        let r = rpc(
            &core,
            Surface::Discord,
            "google.auth.begin",
            with(json!({"manual": true}), who),
        )
        .await;
        assert!(refused(&r), "{r:?}");
        let message = r.unwrap_err().message;
        assert_eq!(super::begin::BeginKind::of_marker(&message), None);
        assert!(!message.contains("could not begin"), "{message}");
    }
    drop(guard);
    assert!(!log.text().contains("could not begin"), "{}", log.text());
}

/// One warning line for a begin that failed, with the kind and none of the daemon's sentence; none
/// for a begin that works.
#[tokio::test]
async fn one_warning_line_names_the_kind_of_a_failed_begin() {
    let (core, _dir) = core_on(board_of(&BOTH, Some(Ok("nothing here"))), |_| {}).await;
    let log = crate::test_support::Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(sub);
    begin_err(&core, None).await;
    let text = log.text();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("Google's consent could not begin"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("WARN"), "{}", lines[0]);
    assert!(lines[0].contains("kind=Client"), "{}", lines[0]);
    assert!(!lines[0].contains("holds no"), "{}", lines[0]);
    let r = owned().await;
    let before = log.text().len();
    let begun = rpc(
        &r.core,
        Surface::Cli,
        "google.auth.begin",
        json!({"manual": true}),
    )
    .await;
    assert!(begun.is_ok(), "{begun:?}");
    drop(guard);
    assert!(!log.text()[before..].contains("could not begin"));
}
