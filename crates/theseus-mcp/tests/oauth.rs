//! Signing in to a remote MCP server (issue 55), through the real client and
//! the sign-in's parts, against the fake authorization server and the fake
//! MCP server in this process: the 401's challenge, discovery by the header
//! and by each fallback, registration and a client id of the settings', PKCE
//! (a wrong verifier refused), the refusals of what comes from outside, the
//! refresh and its rotation, a renewal on a 401, no redirect followed with a
//! token, and no value in any error's words.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use reqwest::Url;
use serde_json::json;
use theseus_mcp::client::{HttpTarget, Options};
use theseus_mcp::fake::{Config, Fake};
use theseus_mcp::oauth::discover::{self, Discovered};
use theseus_mcp::oauth::exchange::{self, Renewal, Tokens};
use theseus_mcp::oauth::fake::{AuthConfig, FakeAuth, MetadataAt};
use theseus_mcp::oauth::register::{self, ClientId};
use theseus_mcp::oauth::request::{self, Loopback, Pkce};
use theseus_mcp::oauth::{self, OauthError, Reach, Rule};
use theseus_mcp::{Client, Error, TokenSource, Transport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The system's resolver, unchecked: these tests' servers are on loopback.
struct System;

impl reqwest::dns::Resolve for System {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<std::net::SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// These tests' rule, which only a test builds: a test's `http` server on
/// this machine, and no address checked.
fn rule() -> Rule {
    Rule {
        resolver: Arc::new(System),
        private_host: |_| None,
        refused: |_| false,
        plaintext: oauth::is_loopback,
    }
}

/// A reach by these tests' rule, and every value its holder was given.
fn reach_holding() -> (Reach, Arc<Mutex<Vec<String>>>) {
    let held = Arc::new(Mutex::new(Vec::new()));
    let h = held.clone();
    let reach = Reach::new(
        rule(),
        Arc::new(move |_, v: &str| h.lock().unwrap().push(v.to_string())),
    )
    .unwrap();
    (reach, held)
}

fn reach() -> Reach {
    reach_holding().0
}

fn opts() -> Options {
    Options {
        request_timeout: Duration::from_secs(10),
        call_timeout: Duration::from_secs(10),
        ..Options::default()
    }
}

/// A fake MCP server that signs in at `auth`, and its address.
async fn mcp_at(auth: &Arc<FakeAuth>) -> (Arc<Fake>, String) {
    let fake = Fake::new(Config {
        oauth: Some(auth.clone()),
        ..Config::default()
    });
    let addr = fake.clone().serve_http(0).await.expect("a loopback port");
    (fake, format!("http://{addr}/mcp"))
}

/// The 401 a connection with no token gets, and its challenge.
async fn refused_challenge(url: &str) -> Option<discover::Challenge> {
    let e = Client::connect(Transport::Http(HttpTarget::new(url)), opts())
        .await
        .err()
        .expect("a refusal");
    match e {
        Error::Http {
            status: 401,
            challenge,
            ..
        } => challenge
            .as_ref()
            .and_then(|c| discover::challenge(c.as_str())),
        other => panic!("not a 401: {other:?}"),
    }
}

/// A whole sign-in: discovery from the 401, registration (or `named`), the
/// browser's trip through the loopback listener, and the exchange.
async fn sign_in(
    auth: &Arc<FakeAuth>,
    url: &str,
    named: Option<&str>,
) -> (Discovered, ClientId, Tokens) {
    let http = reach();
    let c = refused_challenge(url).await;
    let d = discover::discover(&http, &Url::parse(url).unwrap(), c.as_ref())
        .await
        .expect("discovery");
    let lo = Loopback::bind().await.unwrap();
    let redirect = lo.redirect();
    let client = match (&d.server.registration_endpoint, named) {
        (Some(ep), None) => register::register(&http, ep, &redirect).await.unwrap(),
        (_, Some(id)) => ClientId::named(id),
        (None, None) => panic!("no way to a client"),
    };
    let pkce = Pkce::new();
    let at = request::authorize_url(&d.server, &client.id, &redirect, &pkce, &d.resource, None);
    let expect = lo.expect(&pkce, &d.server);
    let hold = http.holder();
    let waiting = tokio::spawn(async move { lo.wait(expect, hold, Duration::from_secs(10)).await });
    auth.browse(&at, true).await;
    let code = waiting.await.unwrap().expect("the code");
    let tokens = exchange::redeem(
        &http,
        &d.server.token_endpoint,
        &client,
        &code,
        pkce.verifier(),
        &redirect,
        &d.resource,
    )
    .await
    .expect("tokens");
    (d, client, tokens)
}

/// A token source as a core keeps one: the token, renewed with the refresh,
/// one renewal at a time (as the daemon's holds its lock across a renewal).
struct Kept {
    renewing: tokio::sync::Mutex<()>,
    access: Mutex<String>,
    refresh: Mutex<String>,
    server: Discovered,
    client: ClientId,
    renewals: AtomicU32,
    /// How the last renewal went, in the daemon's words.
    last: Mutex<String>,
}

impl TokenSource for Kept {
    fn token(&self) -> BoxFuture<'_, Option<String>> {
        Box::pin(async move { Some(self.access.lock().unwrap().clone()) })
    }

    fn refused<'a>(&'a self, sent: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let _one = self.renewing.lock().await;
            if *self.access.lock().unwrap() != sent {
                return true;
            }
            self.renewals.fetch_add(1, Ordering::SeqCst);
            let refresh = self.refresh.lock().unwrap().clone();
            match exchange::refresh(
                &reach(),
                &self.server.server.token_endpoint,
                &self.client,
                &refresh,
                &self.server.resource,
            )
            .await
            {
                Renewal::Renewed(t) => {
                    *self.access.lock().unwrap() = t.access.to_string();
                    if let Some(r) = t.refresh {
                        *self.refresh.lock().unwrap() = r.to_string();
                    }
                    *self.last.lock().unwrap() = "renewed".into();
                    true
                }
                other => {
                    *self.last.lock().unwrap() = format!("{other:?}");
                    false
                }
            }
        })
    }
}

fn kept(d: Discovered, client: ClientId, t: &Tokens) -> Arc<Kept> {
    Arc::new(Kept {
        renewing: tokio::sync::Mutex::new(()),
        access: Mutex::new(t.access.to_string()),
        refresh: Mutex::new(t.refresh.as_ref().unwrap().to_string()),
        server: d,
        client,
        renewals: AtomicU32::new(0),
        last: Mutex::default(),
    })
}

async fn connect_with(url: &str, source: Arc<Kept>) -> Result<Client, Error> {
    let mut t = HttpTarget::new(url);
    t.token = Some(source);
    Client::connect(Transport::Http(t), opts())
        .await
        .map(|(c, _)| c)
}

#[tokio::test]
async fn the_whole_path_from_the_401_to_a_tool_call() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    assert_eq!(d.server.issuer, auth.base());
    assert_eq!(d.resource, url, "the resource is the server itself");
    assert_eq!(auth.seen().registrations, 1);
    let c = connect_with(&url, kept(d, client, &tokens)).await.unwrap();
    let r = c
        .call_tool("echo", json!({"text": "signed"}))
        .await
        .unwrap();
    assert_eq!(r.text_for_model(), "signed");
    // The resource rode the authorization and the exchange.
    assert!(
        auth.seen().resources.iter().all(|r| *r == url),
        "{:?}",
        auth.seen().resources
    );
    assert_eq!(
        auth.seen().bearers_seen,
        0,
        "no token went to the authorization server"
    );
    assert!(fake.seen().unauthorized >= 1);
}

/// Discovery by the header, by the path-inserted well-known address, by the
/// root one, and with no resource metadata at all (the server's origin).
#[tokio::test]
async fn discovery_by_the_header_and_by_each_fallback() {
    for at in [
        MetadataAt::Header,
        MetadataAt::PathWellKnown,
        MetadataAt::RootWellKnown,
    ] {
        let auth = FakeAuth::start(AuthConfig {
            metadata_at: at,
            ..AuthConfig::default()
        })
        .await;
        let (_fake, url) = mcp_at(&auth).await;
        let c = refused_challenge(&url).await.expect("a Bearer challenge");
        assert_eq!(
            c.resource_metadata.is_some(),
            at == MetadataAt::Header,
            "{at:?}"
        );
        let d = discover::discover(&reach(), &Url::parse(&url).unwrap(), Some(&c))
            .await
            .unwrap_or_else(|e| panic!("{at:?}: {e}"));
        assert_eq!(
            d.server.token_endpoint.as_str(),
            format!("{}/token", auth.base())
        );
    }
    // No resource metadata: the server's own origin is asked.
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let url = format!("{}/mcp", auth.base());
    let d = discover::discover(&reach(), &Url::parse(&url).unwrap(), None)
        .await
        .unwrap();
    assert_eq!(d.resource, url);
    assert_eq!(d.server.issuer, auth.base());
    // OpenID discovery's address alone.
    let auth = FakeAuth::start(AuthConfig {
        oidc_only: true,
        ..AuthConfig::default()
    })
    .await;
    let as_ = discover::authorization_server(&reach(), &auth.base())
        .await
        .unwrap();
    assert!(as_.registration_endpoint.is_some());
    assert!(auth
        .seen()
        .paths
        .contains(&"/.well-known/openid-configuration".to_string()));
}

#[tokio::test]
async fn a_client_id_of_the_settings_when_the_server_offers_no_registration() {
    let auth = FakeAuth::start(AuthConfig {
        register: false,
        ..AuthConfig::default()
    })
    .await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, Some("settings-client")).await;
    assert!(d.server.registration_endpoint.is_none());
    assert_eq!(auth.seen().registrations, 0);
    assert_eq!(auth.seen().clients, ["settings-client"]);
    let c = connect_with(&url, kept(d, client, &tokens)).await.unwrap();
    c.ping().await.unwrap();
}

#[tokio::test]
async fn a_wrong_verifier_is_refused_by_the_server() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (_fake, url) = mcp_at(&auth).await;
    let http = reach();
    let c = refused_challenge(&url).await;
    let d = discover::discover(&http, &Url::parse(&url).unwrap(), c.as_ref())
        .await
        .unwrap();
    let lo = Loopback::bind().await.unwrap();
    let redirect = lo.redirect();
    let client = register::register(
        &http,
        d.server.registration_endpoint.as_ref().unwrap(),
        &redirect,
    )
    .await
    .unwrap();
    let pkce = Pkce::new();
    let at = request::authorize_url(&d.server, &client.id, &redirect, &pkce, &d.resource, None);
    let sent_to = auth.browse(&at, false).await;
    let code =
        request::parse_pasted(&sent_to, &lo.expect(&pkce, &d.server), &http.holder()).unwrap();
    let other = Pkce::new();
    let e = exchange::redeem(
        &http,
        &d.server.token_endpoint,
        &client,
        &code,
        other.verifier(),
        &redirect,
        &d.resource,
    )
    .await
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "the token endpoint answered HTTP 400 (invalid_grant)"
    );
    assert_eq!(auth.seen().wrong_verifiers, 1);
    // And a redirect with another sign-in's state gives no code.
    assert!(
        request::parse_pasted(&sent_to, &lo.expect(&other, &d.server), &http.holder()).is_err()
    );
}

/// What comes from outside, refused: an endpoint that is not https, an issuer
/// that is not the one asked, no PKCE S256, a resource that is not this
/// server. No error's words hold a body.
#[tokio::test]
async fn what_comes_from_outside_is_refused_in_words() {
    let cases: [(AuthConfig, &str); 4] = [
        (
            AuthConfig {
                token_endpoint: Some("http://as.example.invalid/token".into()),
                ..AuthConfig::default()
            },
            "the sign-in's token endpoint is not https: refused",
        ),
        (
            AuthConfig {
                issuer: Some("https://evil.example.invalid".into()),
                ..AuthConfig::default()
            },
            "the authorization server's metadata names another issuer: refused",
        ),
        (
            AuthConfig {
                s256: false,
                ..AuthConfig::default()
            },
            "the authorization server does not offer PKCE S256: refused",
        ),
        (
            AuthConfig {
                resource: Some("https://other.example.invalid/mcp".into()),
                ..AuthConfig::default()
            },
            "the server's metadata names another resource: refused",
        ),
    ];
    for (cfg, says) in cases {
        let auth = FakeAuth::start(cfg).await;
        let (_fake, url) = mcp_at(&auth).await;
        let c = refused_challenge(&url).await;
        let e = discover::discover(&reach(), &Url::parse(&url).unwrap(), c.as_ref())
            .await
            .unwrap_err();
        assert!(matches!(e, OauthError::Refused(_)), "{e:?}");
        assert_eq!(e.to_string(), says);
    }
}

#[tokio::test]
async fn a_refresh_rotates_and_a_spent_or_refused_one_is_for_good() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    let http = reach();
    let first = tokens.refresh.unwrap().to_string();
    let Renewal::Renewed(t) = exchange::refresh(
        &http,
        &d.server.token_endpoint,
        &client,
        &first,
        &d.resource,
    )
    .await
    else {
        panic!("not renewed")
    };
    let second = t.refresh.expect("a rotated refresh token").to_string();
    assert_ne!(first, second);
    // The spent one is refused for good, in the daemon's words.
    match exchange::refresh(
        &http,
        &d.server.token_endpoint,
        &client,
        &first,
        &d.resource,
    )
    .await
    {
        Renewal::ForGood(why) => {
            assert_eq!(why, "the token endpoint answered HTTP 400 (invalid_grant)")
        }
        other => panic!("{other:?}"),
    }
    auth.set(|c| c.refuse_refresh = true);
    assert!(matches!(
        exchange::refresh(
            &http,
            &d.server.token_endpoint,
            &client,
            &second,
            &d.resource
        )
        .await,
        Renewal::ForGood(_)
    ));
    // No answer at all may pass.
    let gone = Url::parse("http://127.0.0.1:9/token").unwrap();
    assert!(matches!(
        exchange::refresh(&http, &gone, &client, &second, &d.resource).await,
        Renewal::Passing(_)
    ));
}

#[tokio::test]
async fn a_401_renews_the_token_once_and_the_call_goes_again() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    let source = kept(d, client, &tokens);
    let c = connect_with(&url, source.clone()).await.unwrap();
    auth.expire_access();
    let r = c
        .call_tool("echo", json!({"text": "again"}))
        .await
        .unwrap_or_else(|e| panic!("{e:?}; the renewal: {}", source.last.lock().unwrap()));
    assert_eq!(r.text_for_model(), "again");
    assert_eq!(source.renewals.load(Ordering::SeqCst), 1);
    assert_eq!(auth.seen().refreshes, 1);
    // A refresh refused for good: the call fails with the 401, once.
    auth.revoke_all();
    let e = c
        .call_tool("echo", json!({"text": "no"}))
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Http { status: 401, .. }), "{e:?}");
    assert_eq!(source.renewals.load(Ordering::SeqCst), 2);
    assert_eq!(auth.seen().refused_refreshes, 1, "one refresh, no loop");
}

/// A server that answers with a redirect to another origin: a signed-in
/// connection does not follow it, so the token never reaches the other one.
#[tokio::test]
async fn the_token_never_goes_to_another_origin() {
    let other = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_addr = other.local_addr().unwrap();
    let reached = Arc::new(AtomicU32::new(0));
    let r = reached.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = other.accept().await {
            r.fetch_add(1, Ordering::SeqCst);
            let _ = s
                .write_all(b"HTTP/1.1 500 X\r\ncontent-length: 0\r\n\r\n")
                .await;
        }
    });
    let mover = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mover_addr = mover.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = mover.accept().await {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 307 X\r\nlocation: http://{other_addr}/mcp\r\ncontent-length: 0\r\n\r\n"
            );
            let _ = s.write_all(head.as_bytes()).await;
        }
    });
    struct Fixed;
    impl TokenSource for Fixed {
        fn token(&self) -> BoxFuture<'_, Option<String>> {
            Box::pin(async { Some("fixed-token-for-the-test".to_string()) })
        }
        fn refused<'a>(&'a self, _: &'a str) -> BoxFuture<'a, bool> {
            Box::pin(async { false })
        }
    }
    let mut t = HttpTarget::new(format!("http://{mover_addr}/mcp"));
    t.token = Some(Arc::new(Fixed));
    let e = Client::connect(Transport::Http(t), opts())
        .await
        .err()
        .unwrap();
    assert!(matches!(e, Error::Http { status: 307, .. }), "{e:?}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        reached.load(Ordering::SeqCst),
        0,
        "the other origin was never reached"
    );
}

/// Every value the fake issued or was sent (codes, tokens, the verifier) is
/// in no error's words after a sign-in, a renewal and a refused refresh.
#[tokio::test]
async fn no_value_is_in_any_errors_words() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    let source = kept(d.clone(), client.clone(), &tokens);
    let c = connect_with(&url, source.clone()).await.unwrap();
    let mut words = Vec::new();
    auth.revoke_all();
    words.push(
        c.call_tool("echo", json!({"text": "x"}))
            .await
            .unwrap_err()
            .to_string(),
    );
    words.push(format!(
        "{:?}",
        c.call_tool("echo", json!({"text": "x"})).await.unwrap_err()
    ));
    let r = tokens.refresh.as_ref().unwrap().to_string();
    if let Renewal::ForGood(w) =
        exchange::refresh(&reach(), &d.server.token_endpoint, &client, &r, &d.resource).await
    {
        words.push(w);
    }
    words.push(format!("{client:?} {tokens:?}"));
    let secrets = auth.seen().secrets;
    assert!(secrets.len() >= 4, "{}", secrets.len());
    for w in &words {
        for s in &secrets {
            assert!(!w.contains(s.as_str()), "a value is in {w:?}");
        }
        assert!(!w.contains("harbour portal"), "a body is in {w:?}");
    }
}

/// The tolerant readings accepted where the specification is not sure, each
/// tested: an authorization server that lists no PKCE methods (taken as
/// offering S256, which the code exchange then proves), a resource's metadata
/// with no `resource` (taken as naming this server), and an issuer that
/// differs from the one asked by a trailing `/` alone.
#[tokio::test]
async fn the_tolerant_readings_are_accepted() {
    let auth = FakeAuth::start(AuthConfig {
        omit: vec!["code_challenge_methods_supported".into(), "resource".into()],
        ..AuthConfig::default()
    })
    .await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    assert_eq!(d.resource, url, "no `resource`: this server");
    let c = connect_with(&url, kept(d, client, &tokens)).await.unwrap();
    c.ping().await.unwrap();
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let slash = format!("{}/", auth.base());
    auth.set(|c| c.issuer = Some(slash));
    let as_ = discover::authorization_server(&reach(), &auth.base())
        .await
        .expect("an issuer with a trailing slash is the one asked");
    assert_eq!(as_.issuer, auth.base());
}

/// Every value a test must look for: what the fake issued or was sent.
fn every_value(auth: &FakeAuth) -> Vec<String> {
    auth.seen().secrets
}

/// A hostile server that signs in echoes the token it was sent in a JSON-RPC
/// error's message and data, and in its 401's `WWW-Authenticate`: no error
/// that leaves the client holds it, in its words or its `Debug`.
#[tokio::test]
async fn a_signed_in_servers_words_never_leave_the_client() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    let c = connect_with(&url, kept(d, client, &tokens)).await.unwrap();
    auth.set(|c| c.echo = true);
    let mut words = Vec::new();
    let e = c.list_tools().await.unwrap_err();
    assert!(matches!(e, Error::Rpc { code: -32001, .. }), "{e:?}");
    words.push(format!("{e} {e:?}"));
    let e = c.call_tool("echo", json!({"text": "x"})).await.unwrap_err();
    words.push(format!("{e} {e:?}"));
    // A 401 that echoes the token it refused, then a renewal, then the echo.
    auth.expire_access();
    let e = c.call_tool("echo", json!({"text": "x"})).await.unwrap_err();
    words.push(format!("{e} {e:?}"));
    auth.revoke_all();
    let e = c.call_tool("echo", json!({"text": "x"})).await.unwrap_err();
    assert!(matches!(e, Error::Http { status: 401, .. }), "{e:?}");
    words.push(format!("{e} {e:?}"));
    let values = every_value(&auth);
    assert!(values.iter().any(|v| v.starts_with("fake-at-")));
    for w in &words {
        for v in &values {
            assert!(!w.contains(v.as_str()), "a value is in {w:?}");
        }
        assert!(!w.contains("harbour portal"), "a body is in {w:?}");
    }
    // Where a 401's challenge is kept (a connection that is not signed in),
    // its Debug holds none of it.
    let e = Client::connect(Transport::Http(HttpTarget::new(&url)), opts())
        .await
        .err()
        .unwrap();
    let Error::Http {
        challenge: Some(h), ..
    } = &e
    else {
        panic!("{e:?}")
    };
    let echoed: Vec<&String> = values
        .iter()
        .filter(|v| h.as_str().contains(v.as_str()))
        .collect();
    assert!(!echoed.is_empty(), "the header echoes a token");
    for v in echoed {
        assert!(
            !format!("{e:?}").contains(v.as_str()),
            "the header is in a Debug"
        );
    }
}

/// A GET's 401 (the server's own stream, reopened) renews the token once,
/// as a POST's does: the stream comes back with no request of the client's.
#[tokio::test]
async fn a_streams_401_renews_the_token_once() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    let source = kept(d, client, &tokens);
    let _c = connect_with(&url, source.clone()).await.unwrap();
    assert!(
        fake.wait_until(Duration::from_secs(5), |s| s.server_streams >= 1)
            .await
    );
    auth.expire_access();
    fake.end_streams();
    assert!(
        fake.wait_until(Duration::from_secs(10), |s| s.server_streams >= 2)
            .await,
        "the stream came back"
    );
    assert_eq!(source.renewals.load(Ordering::SeqCst), 1);
    assert_eq!(auth.seen().refreshes, 1);
}

/// Two requests refused together renew once: each 401 names the token its
/// own request carried, so the second sees the first's renewal and goes again
/// with the new token.
#[tokio::test]
async fn two_401s_at_once_renew_once() {
    let auth = FakeAuth::start(AuthConfig::default()).await;
    let (_fake, url) = mcp_at(&auth).await;
    let (d, client, tokens) = sign_in(&auth, &url, None).await;
    let source = kept(d, client, &tokens);
    let c = connect_with(&url, source.clone()).await.unwrap();
    auth.expire_access();
    let (a, b) = tokio::join!(
        c.call_tool("echo", json!({"text": "a"})),
        c.call_tool("echo", json!({"text": "b"}))
    );
    let last = source.last.lock().unwrap().clone();
    assert_eq!(a.expect(&last).text_for_model(), "a");
    assert_eq!(b.expect(&last).text_for_model(), "b");
    assert_eq!(source.renewals.load(Ordering::SeqCst), 1);
    assert_eq!(auth.seen().refreshes, 1);
}

/// A mix-up: a hostile authorization server's metadata names an honest
/// one's authorization endpoint and its own token endpoint. Taken only when
/// its metadata says the redirect names its issuer; then the honest server's
/// redirect (with no `iss`, or its own) is refused, and no code, verifier or
/// secret is ever sent to the hostile token endpoint.
#[tokio::test]
async fn a_mixed_up_authorization_server_gets_no_code() {
    let honest = FakeAuth::start(AuthConfig {
        iss: false,
        ..AuthConfig::default()
    })
    .await;
    let hostile = FakeAuth::start(AuthConfig {
        authorization_endpoint: Some(format!("{}/authorize", honest.base())),
        ..AuthConfig::default()
    })
    .await;
    let (_fake, url) = mcp_at(&hostile).await;
    let c = refused_challenge(&url).await;
    let http = reach();
    let e = discover::discover(&http, &Url::parse(&url).unwrap(), c.as_ref())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("not on its own address"), "{e}");
    hostile.set(|c| c.iss_supported = true);
    let d = discover::discover(&http, &Url::parse(&url).unwrap(), c.as_ref())
        .await
        .unwrap();
    assert_eq!(d.server.elsewhere, [honest.base()]);
    let lo = Loopback::bind().await.unwrap();
    let redirect = lo.redirect();
    let client = ClientId::named("settings-client");
    for iss in [false, true] {
        honest.set(|c| c.iss = iss);
        let pkce = Pkce::new();
        let at = request::authorize_url(&d.server, &client.id, &redirect, &pkce, &d.resource, None);
        let sent_to = honest.browse(&at, false).await;
        let e = request::parse_pasted(&sent_to, &lo.expect(&pkce, &d.server), &http.holder())
            .unwrap_err();
        let want = match iss {
            false => request::RedirectError::NoIssuer,
            true => request::RedirectError::IssuerMismatch,
        };
        assert_eq!(e, want);
    }
    assert!(
        !hostile.seen().paths.iter().any(|p| p == "/token"),
        "{:?}",
        hostile.seen().paths
    );
}

/// Every value is held where it is first read: the client secret at
/// registration, the code, the tokens of an answer that is then refused (a
/// token type that is not Bearer), and a token too short to keep from view
/// is refused, never used.
#[tokio::test]
async fn every_value_is_held_where_it_is_first_read() {
    let auth = FakeAuth::start(AuthConfig {
        client_secret: true,
        token_type: "DPoP".into(),
        ..AuthConfig::default()
    })
    .await;
    let (_fake, url) = mcp_at(&auth).await;
    let (http, held) = reach_holding();
    let c = refused_challenge(&url).await;
    let d = discover::discover(&http, &Url::parse(&url).unwrap(), c.as_ref())
        .await
        .unwrap();
    let lo = Loopback::bind().await.unwrap();
    let redirect = lo.redirect();
    let client = register::register(
        &http,
        d.server.registration_endpoint.as_ref().unwrap(),
        &redirect,
    )
    .await
    .unwrap();
    assert!(client.secret.is_some());
    let pkce = Pkce::new();
    let at = request::authorize_url(&d.server, &client.id, &redirect, &pkce, &d.resource, None);
    let sent_to = auth.browse(&at, false).await;
    let code =
        request::parse_pasted(&sent_to, &lo.expect(&pkce, &d.server), &http.holder()).unwrap();
    let e = exchange::redeem(
        &http,
        &d.server.token_endpoint,
        &client,
        &code,
        pkce.verifier(),
        &redirect,
        &d.resource,
    )
    .await
    .unwrap_err();
    assert!(e.to_string().contains("not a Bearer"), "{e}");
    let held = held.lock().unwrap().clone();
    for v in auth
        .seen()
        .secrets
        .iter()
        .filter(|v| v.starts_with("fake-"))
    {
        assert!(held.contains(v), "{v} was not held");
    }
    assert!(auth
        .seen()
        .secrets
        .iter()
        .any(|v| v.starts_with("fake-cs-")));
    // A token too short to keep from view: a bad answer.
    auth.set(|c| {
        c.token_type = "Bearer".into();
        c.short_access = true;
    });
    let pkce = Pkce::new();
    let at = request::authorize_url(&d.server, &client.id, &redirect, &pkce, &d.resource, None);
    let sent_to = auth.browse(&at, false).await;
    let code =
        request::parse_pasted(&sent_to, &lo.expect(&pkce, &d.server), &http.holder()).unwrap();
    let e = exchange::redeem(
        &http,
        &d.server.token_endpoint,
        &client,
        &code,
        pkce.verifier(),
        &redirect,
        &d.resource,
    )
    .await
    .unwrap_err();
    assert!(matches!(e, OauthError::Undecodable { .. }), "{e:?}");
}

/// A resource's metadata that names many authorization servers has the
/// first few tried, no more.
#[tokio::test]
async fn only_the_first_few_authorization_servers_are_tried() {
    let auth = FakeAuth::start(AuthConfig {
        decoys: 10,
        ..AuthConfig::default()
    })
    .await;
    let (_fake, url) = mcp_at(&auth).await;
    let c = refused_challenge(&url).await;
    assert!(
        discover::discover(&reach(), &Url::parse(&url).unwrap(), c.as_ref())
            .await
            .is_err()
    );
    let decoys = auth
        .seen()
        .paths
        .iter()
        .filter(|p| p.contains("decoy-"))
        .map(|p| {
            p.split("decoy-")
                .nth(1)
                .unwrap()
                .split('/')
                .next()
                .unwrap()
                .to_string()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(decoys.len(), oauth::MAX_ISSUERS, "{decoys:?}");
}
