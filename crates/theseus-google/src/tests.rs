//! The client against the fake Google: the single flight, the retries, the
//! error map, the consent's redeem and its listener.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::fake::{Fake, Reply, REFRESH_TOKEN};
use crate::oauth::{self, Loopback, Pkce};
use crate::*;

fn client_for(fake: &Fake) -> (Client, Arc<Tokens>, ClientSecret) {
    let ep = Endpoints::loopback(&fake.url());
    let fast = Policy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(5),
    };
    let client = Client::new(ep.clone()).with_policy(fast);
    let tokens = Arc::new(Tokens::new(client.http().clone(), ep));
    (
        client,
        tokens,
        ClientSecret::new("id-1.apps.example", "s3cret-1"),
    )
}

#[tokio::test]
async fn refresh_is_single_flight() {
    let fake = Fake::start().await;
    fake.set_refresh_delay(Duration::from_millis(150));
    let (_, tokens, secret) = client_for(&fake);
    let secret = Arc::new(secret);
    let mut all = Vec::new();
    for _ in 0..10 {
        let (t, s) = (tokens.clone(), secret.clone());
        all.push(tokio::spawn(async move {
            t.access(&s, REFRESH_TOKEN)
                .await
                .map(|t| t.expose().to_string())
        }));
    }
    let mut seen = std::collections::BTreeSet::new();
    for h in all {
        seen.insert(h.await.unwrap().unwrap());
    }
    assert_eq!(seen.len(), 1, "every caller got the one token");
    assert_eq!(fake.token_requests(), 1);
    // A cached token is not asked for again; one near its end is.
    tokens.access(&secret, REFRESH_TOKEN).await.unwrap();
    assert_eq!(fake.token_requests(), 1);
    fake.set_lifetime(200);
    tokens.forget();
    tokens.access(&secret, REFRESH_TOKEN).await.unwrap();
    tokens.access(&secret, REFRESH_TOKEN).await.unwrap();
    assert_eq!(
        fake.token_requests(),
        3,
        "a token inside its margin is replaced"
    );
}

#[tokio::test]
async fn a_lapsed_grant_is_needs_auth() {
    let fake = Fake::start().await;
    let (_, tokens, secret) = client_for(&fake);
    fake.set_invalid_grant(true);
    let e = tokens.access(&secret, REFRESH_TOKEN).await.unwrap_err();
    assert_eq!(e.kind, Kind::NeedsAuth);
    let e = tokens
        .access(&secret, "1//some-other-token")
        .await
        .unwrap_err();
    assert_eq!(e.kind, Kind::NeedsAuth);
    assert!(!e.to_string().contains("1//"));
}

#[tokio::test]
async fn a_throttle_backs_off_and_a_send_is_not_repeated_after_a_server_error() {
    let fake = Fake::start().await;
    let (client, tokens, secret) = client_for(&fake);
    let token = tokens.access(&secret, REFRESH_TOKEN).await.unwrap();
    let req = Request::get(Service::Gmail, "/gmail/v1/users/me/profile");
    fake.fail_next(2, &Reply::throttled(None));
    let r = client.send(&token, &req).await.unwrap();
    assert_eq!((r.status, r.attempts), (200, 3));
    assert_eq!(r.json()["emailAddress"], "owner@example.test");
    // Three throttles are the end: the error says so.
    fake.fail_next(3, &Reply::throttled(None));
    let f = client.send(&token, &req).await.unwrap_err();
    assert_eq!(
        (f.attempts, f.error.kind.clone(), f.unknown),
        (3, Kind::Throttle, false)
    );
    // A repeatable read retries a 503; a send that may have gone out is `unknown`, once.
    fake.fail_next(1, &Reply::server_error());
    assert_eq!(client.send(&token, &req).await.unwrap().attempts, 2);
    let send = Request {
        verb: "POST".into(),
        path: "/gmail/v1/users/me/drafts/send".into(),
        repeat: Repeat::NonRepeatable,
        body: Body::Json(json!({"id": "d1"})),
        ..req.clone()
    };
    fake.fail_next(1, &Reply::server_error());
    let before = fake.log().len();
    let f = client.send(&token, &send).await.unwrap_err();
    assert!(f.unknown && f.attempts == 1, "{f:?}");
    assert_eq!(fake.log().len(), before + 1, "it was sent once");
    // A throttle was refused, so even a send may go again.
    fake.fail_next(1, &Reply::throttled(Some(0)));
    let r = client.send(&token, &send).await;
    assert!(r.is_err() || r.unwrap().attempts == 2);
}

#[tokio::test]
async fn a_disabled_api_names_the_api_to_enable_through_the_client() {
    let fake = Fake::start().await;
    let (client, tokens, secret) = client_for(&fake);
    let token = tokens.access(&secret, REFRESH_TOKEN).await.unwrap();
    fake.service_disabled(Service::Drive, "424242");
    let f = client
        .send(&token, &Request::get(Service::Drive, "/drive/v3/about"))
        .await
        .unwrap_err();
    assert_eq!(
        f.error.to_string(),
        "enable the Google Drive API in project 424242: https://console.developers.google.com/apis/api/drive.googleapis.com/overview?project=424242"
    );
    assert_eq!(f.attempts, 1, "a disabled API is not retried");
    // Gmail is untouched.
    client
        .send(
            &token,
            &Request::get(Service::Gmail, "/gmail/v1/users/me/profile"),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_code_is_redeemed_with_its_verifier_and_the_grant_says_who_and_what() {
    let fake = Fake::start().await;
    let (client, _, secret) = client_for(&fake);
    let pkce = Pkce::new();
    let code = oauth::parse_redirect(
        &format!("?code={}&state={}", crate::fake::CODE, pkce.state),
        &pkce.state,
    )
    .unwrap();
    let grant = oauth::redeem(
        client.http(),
        client.endpoints(),
        &secret,
        &code,
        pkce.verifier(),
        "http://127.0.0.1:1/",
    )
    .await
    .unwrap();
    assert_eq!(grant.email.as_deref(), Some("owner@example.test"));
    assert_eq!(&*grant.refresh_token, REFRESH_TOKEN);
    assert_eq!(grant.scopes, scopes::requested(&Service::ALL));
    let form = fake.forms().pop().unwrap();
    assert_eq!(form["code_verifier"], pkce.verifier());
    assert_eq!(form["redirect_uri"], "http://127.0.0.1:1/");
    // A wrong code is invalid_grant, and nothing in the message is the code.
    let bad =
        oauth::parse_redirect(&format!("?code=4/wrong&state={}", pkce.state), &pkce.state).unwrap();
    let e = oauth::redeem(
        client.http(),
        client.endpoints(),
        &secret,
        &bad,
        pkce.verifier(),
        "http://127.0.0.1:1/",
    )
    .await
    .unwrap_err();
    assert_eq!(e.kind, Kind::NeedsAuth);
    assert!(!format!("{e} {e:?}").contains("4/wrong"));
    oauth::revoke(client.http(), client.endpoints(), REFRESH_TOKEN)
        .await
        .unwrap();
    assert!(fake.revoked());
}

#[tokio::test]
async fn the_loopback_listener_takes_the_redirect_and_waits_past_strays() {
    let l = Loopback::bind().await.unwrap();
    let (port, state) = (l.port, "state-1".to_string());
    let waiting = tokio::spawn({
        let state = state.clone();
        async move { l.wait(&state, Duration::from_secs(10)).await }
    });
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("http://127.0.0.1:{port}/favicon.ico"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let wrong = http
        .get(format!("http://127.0.0.1:{port}/?code=abc&state=other"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 400);
    let ok = http
        .get(format!(
            "http://127.0.0.1:{port}/?code=4%2Fxyz&state={state}"
        ))
        .send()
        .await
        .unwrap();
    assert!(ok.text().await.unwrap().contains("close this tab"));
    assert_eq!(waiting.await.unwrap().unwrap().expose(), "4/xyz");
}

#[tokio::test(start_paused = true)]
async fn the_loopback_listener_gives_up() {
    let l = Loopback::bind().await.unwrap();
    let e = l.wait("s", Duration::from_secs(600)).await.unwrap_err();
    assert_eq!(e, oauth::AuthError::Timeout);
}
