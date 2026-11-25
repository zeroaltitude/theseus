//! The kept sign-in (issue 55): its file, private and written whole; what a
//! file must be to be used; the scrubber's knowledge of every value for the
//! daemon's life; and a loss that gives no token and renews nothing.

use super::*;

fn kept() -> Kept {
    Kept {
        url: "https://docs.example.invalid/mcp".into(),
        resource: "https://docs.example.invalid/mcp".into(),
        issuer: "https://as.example.invalid".into(),
        token_endpoint: "https://as.example.invalid/token".into(),
        client_id: "c-1".into(),
        access_token: "access-value-0001".into(),
        refresh_token: Some("refresh-value-0001".into()),
        // No expiry: nothing here renews, so nothing reaches a network.
        expires_at_ms: None,
        signed_in_at_ms: 1,
        revocation_endpoint: None,
        elsewhere: vec![],
        client_secret: None,
        client_basic: false,
    }
}

fn reach() -> Reach {
    Reach::new(daemon_rule(), Arc::new(|_, _| {})).unwrap()
}

fn known(board: &Arc<SecretBoard>, value: &str) -> bool {
    let (cut, _) = crate::scrub::Scrubber::from_board(board.clone()).scrub(value);
    !cut.contains(value)
}

/// Written whole at 0600 in a 0700 directory; read back; a file others may
/// read, a link, or a file too large is not used; a stray temporary file
/// never replaces the kept one, and is never written through.
#[test]
fn the_file_is_private_and_written_whole() {
    let d = tempfile::tempdir().unwrap();
    let p = file(&dir(d.path()), "docs");
    write(&p, &kept()).unwrap();
    let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let dmode = std::fs::metadata(p.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dmode, 0o700);
    let back = read(&p).unwrap().unwrap();
    assert_eq!(back.access_token, "access-value-0001");
    assert!(!format!("{back:?}").contains("access-value"));
    assert!(!format!("{back:?}").contains("as.example.invalid"));
    // A stray file beside it, even a link to elsewhere, is neither read nor
    // written through.
    let elsewhere = d.path().join("elsewhere");
    std::fs::write(&elsewhere, b"untouched").unwrap();
    std::os::unix::fs::symlink(&elsewhere, p.with_extension("json.new")).unwrap();
    write(&p, &kept()).unwrap();
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"untouched");
    let left: Vec<_> = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    let e = read(&p).unwrap_err();
    assert!(e.contains("readable by others"), "{e}");
    assert!(!e.contains("access-value"));
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&p, b"not json").unwrap();
    assert_eq!(read(&p).unwrap_err(), "the kept sign-in does not parse");
    std::fs::write(&p, vec![b' '; MAX_FILE as usize + 1]).unwrap();
    assert!(read(&p).unwrap_err().contains("too large"));
    remove(&p).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &p).unwrap();
    assert!(read(&p).unwrap_err().contains("not a file of its own"));
    remove(&p).unwrap();
    remove(&p).unwrap();
    assert_eq!(read(&p).unwrap().map(|k| k.url.clone()), None);
    // A directory that is a link is not the daemon's own.
    let other = tempfile::tempdir().unwrap();
    let linked = d.path().join("linked");
    std::os::unix::fs::symlink(other.path(), &linked).unwrap();
    assert!(write(&file(&linked, "docs"), &kept()).is_err());
}

/// A renewal's refresh token replaces the kept one; a renewal with none
/// keeps it; an expiry is bounded and cannot overflow.
#[test]
fn a_rotated_refresh_token_replaces_the_kept_one() {
    let mut k = kept();
    let t = Tokens {
        access: "access-value-0002".to_string().into(),
        refresh: Some("refresh-value-0002".to_string().into()),
        expires_in: Some(30),
        scope: None,
    };
    k.renewed(&t, 10_000);
    assert_eq!(k.refresh_token.as_deref(), Some("refresh-value-0002"));
    assert_eq!(k.expires_at_ms, Some(40_000));
    assert!(k.due(0));
    let t = Tokens {
        access: "access-value-0003".to_string().into(),
        refresh: None,
        expires_in: None,
        scope: None,
    };
    k.renewed(&t, 0);
    assert_eq!(k.refresh_token.as_deref(), Some("refresh-value-0002"));
    assert!(!k.due(u64::MAX / 2));
    let t = Tokens {
        access: "access-value-0004".to_string().into(),
        refresh: None,
        expires_in: Some(u64::MAX),
        scope: None,
    };
    k.renewed(&t, u64::MAX - 5);
    assert_eq!(k.expires_at_ms, Some(u64::MAX));
    k.renewed(&t, 0);
    assert_eq!(k.expires_at_ms, Some(MAX_EXPIRES_SECS * 1000));
    assert!(!k.due(0));
    assert!(k.due(u64::MAX), "no sum overflows");
}

/// A kept file is used only when it means what it should, read by the
/// daemon's own rule; otherwise it is `needs sign-in`, and none of it shown.
#[test]
fn a_kept_file_is_checked_for_meaning_before_it_is_used() {
    let url = "https://docs.example.invalid/mcp";
    let r = reach();
    assert_eq!(kept().check(url, &r, 0), Ok(()));
    let cases: Vec<(&str, Box<dyn Fn(&mut Kept)>)> = vec![
        (
            "another address",
            Box::new(|k| k.url = "https://moved.example.invalid/mcp".into()),
        ),
        (
            "an empty access token",
            Box::new(|k| k.access_token.clear()),
        ),
        (
            "a short access token",
            Box::new(|k| k.access_token = "short".into()),
        ),
        (
            "a short refresh token",
            Box::new(|k| k.refresh_token = Some("r".into())),
        ),
        (
            "a short client secret",
            Box::new(|k| k.client_secret = Some("s".into())),
        ),
        ("no client id", Box::new(|k| k.client_id.clear())),
        (
            "another resource",
            Box::new(|k| k.resource = "https://other.example.invalid/mcp".into()),
        ),
        (
            "a plaintext token endpoint",
            Box::new(|k| k.token_endpoint = "http://as.example.invalid/token".into()),
        ),
        (
            "a private token endpoint",
            Box::new(|k| k.token_endpoint = "https://127.0.0.1/token".into()),
        ),
        (
            "a token endpoint elsewhere",
            Box::new(|k| k.token_endpoint = "https://evil.example.invalid/token".into()),
        ),
        (
            "a revocation endpoint elsewhere",
            Box::new(|k| k.revocation_endpoint = Some("https://evil.example.invalid/r".into())),
        ),
        (
            "a plaintext issuer",
            Box::new(|k| k.issuer = "http://as.example.invalid".into()),
        ),
        (
            "an expiry past a year",
            Box::new(|k| k.expires_at_ms = Some(u64::MAX)),
        ),
        (
            "a token endpoint that is not an address",
            Box::new(|k| k.token_endpoint = "token".into()),
        ),
    ];
    for (what, change) in cases {
        let mut k = kept();
        change(&mut k);
        let e = k.check(url, &r, 0).unwrap_err();
        assert!(
            !e.contains("access-value") && !e.contains("example"),
            "{what}: {e}"
        );
    }
    // An endpoint elsewhere the owner agreed to at the sign-in.
    let mut k = kept();
    k.token_endpoint = "https://tokens.example.invalid/token".into();
    k.elsewhere = vec!["https://tokens.example.invalid".into()];
    assert_eq!(k.check(url, &r, 0), Ok(()));
}

/// A kept sign-in's values are on the secret board at once; one for another
/// address, or one that is not valid, is not used, and its values are held
/// all the same.
#[tokio::test]
async fn a_kept_sign_in_is_read_held_and_bound_to_its_address() {
    let d = tempfile::tempdir().unwrap();
    let p = file(&dir(d.path()), "docs");
    write(&p, &kept()).unwrap();
    let board = SecretBoard::empty();
    let s = Signed::load(
        "docs",
        "https://docs.example.invalid/mcp",
        Some(p.clone()),
        board.clone(),
        daemon_rule(),
    );
    assert!(s.signed_in().await);
    assert!(known(&board, "refresh-value-0001"));
    let other = Signed::load(
        "docs",
        "https://moved.example.invalid/mcp",
        Some(p.clone()),
        SecretBoard::empty(),
        daemon_rule(),
    );
    assert!(!other.signed_in().await);
    let mut bad = kept();
    bad.token_endpoint = "https://evil.example.invalid/token".into();
    write(&p, &bad).unwrap();
    let board = SecretBoard::empty();
    let s = Signed::load(
        "docs",
        "https://docs.example.invalid/mcp",
        Some(p),
        board.clone(),
        daemon_rule(),
    );
    assert!(!s.signed_in().await);
    assert!(known(&board, "access-value-0001"));
}

/// Every value held stays known to the scrubber: past any number of
/// renewals, across a sign-in loaded again, and after a sign-out.
#[tokio::test]
async fn every_value_held_stays_known_for_the_daemons_life() {
    let board = SecretBoard::empty();
    let d = tempfile::tempdir().unwrap();
    let p = file(&dir(d.path()), "docs");
    let s = Signed::load(
        "docs",
        "https://docs.example.invalid/mcp",
        Some(p.clone()),
        board.clone(),
        daemon_rule(),
    );
    let values: Vec<String> = (0..12).map(|i| format!("access-value-{i:04}")).collect();
    for v in &values {
        s.hold_value("access", v);
    }
    s.put(kept()).await.unwrap();
    let again = Signed::load(
        "docs",
        "https://docs.example.invalid/mcp",
        Some(p),
        board.clone(),
        daemon_rule(),
    );
    again.hold_value("access", "access-value-late");
    let _ = again.forget().await;
    for v in values
        .iter()
        .map(String::as_str)
        .chain(["access-value-late"])
    {
        assert!(known(&board, v), "{v} is no longer known");
    }
}

/// A loss is terminal under the lock: no token is given, a 401 renews
/// nothing, until a new sign-in; the sign-in that cannot be written is not
/// used, and the one before it stays.
#[tokio::test]
async fn a_lost_sign_in_gives_no_token_until_a_new_one() {
    use theseus_mcp::TokenSource;
    let d = tempfile::tempdir().unwrap();
    let p = file(&dir(d.path()), "docs");
    let s = Signed::load(
        "docs",
        "https://docs.example.invalid/mcp",
        Some(p.clone()),
        SecretBoard::empty(),
        daemon_rule(),
    );
    let mut k = kept();
    k.refresh_token = None;
    s.put(k).await.unwrap();
    assert_eq!(s.token().await.as_deref(), Some("access-value-0001"));
    // A 401 with no way to renew: lost.
    assert!(!s.refused("access-value-0001").await);
    assert!(s.lost().is_some());
    assert_eq!(s.token().await, None);
    assert!(!s.refused("access-value-0001").await);
    assert!(!s.signed_in().await);
    // A new sign-in ends the loss.
    s.put(kept()).await.unwrap();
    assert!(s.lost().is_none());
    assert!(s.signed_in().await);
    // One that cannot be written is refused, and the one before stays: its
    // directory is now a file, so no write can land.
    let dir_path = p.parent().unwrap().to_path_buf();
    std::fs::remove_dir_all(&dir_path).unwrap();
    std::fs::write(&dir_path, b"in the way").unwrap();
    let mut next = kept();
    next.access_token = "access-value-0099".into();
    assert!(s.put(next).await.is_err());
    assert_eq!(s.token().await.as_deref(), Some("access-value-0001"));
    // A sign-in that does not check is refused before anything is written.
    let mut bad = kept();
    bad.access_token = "short".into();
    assert!(s.put(bad).await.is_err());
}

/// A server that signs in is at an `https` address: a test's loopback server
/// is taken by the tests' rule alone, never the daemon's; and the daemon's
/// rule reaches no address that is not public, nor a plaintext one.
#[test]
fn the_daemons_rule_takes_https_at_a_public_address_alone() {
    let u = |s: &str| Url::parse(s).unwrap();
    assert!(signs_in_at_by(
        &u("https://docs.example.invalid/mcp"),
        false
    ));
    assert!(!signs_in_at_by(&u("http://127.0.0.1:9/mcp"), false));
    assert!(!signs_in_at_by(
        &u("http://docs.example.invalid/mcp"),
        false
    ));
    assert!(signs_in_at_by(&u("http://127.0.0.1:9/mcp"), true));
    assert!(!signs_in_at_by(&u("http://docs.example.invalid/mcp"), true));
    let r = reach();
    for bad in [
        "http://127.0.0.1:9/m",
        "https://127.0.0.1/m",
        "https://localhost/m",
        "https://10.0.0.5/m",
        "https://169.254.169.254/latest",
        "https://[::1]/m",
        "https://192.168.1.1/m",
    ] {
        assert!(r.endpoint(bad, "metadata").is_err(), "{bad}");
    }
    assert!(r
        .endpoint("https://as.example.invalid/m", "metadata")
        .is_ok());
}

/// A name that resolves to an address that is not public is refused at the
/// connection, by the web fetch's own resolver: a server cannot name one to
/// reach this machine or its network.
#[tokio::test]
async fn a_name_that_resolves_to_a_private_address_is_never_reached() {
    let mut dns = crate::web::net::Dns::checked();
    dns.hosts.insert(
        "rebind.example.invalid".into(),
        vec!["127.0.0.1".parse().unwrap()],
    );
    dns.hosts.insert(
        "metadata.example.invalid".into(),
        vec!["169.254.169.254".parse().unwrap()],
    );
    let rule = Rule {
        resolver: Arc::new(dns),
        ..daemon_rule()
    };
    let r = Reach::new(rule, Arc::new(|_, _| {})).unwrap();
    for host in ["rebind.example.invalid", "metadata.example.invalid"] {
        let url = Url::parse(&format!("https://{host}/.well-known/x")).unwrap();
        let e = r.get_json(&url, "the metadata").await.unwrap_err();
        assert_eq!(
            e.to_string(),
            "the metadata is at an address that is not public: refused",
            "{host}"
        );
    }
}
