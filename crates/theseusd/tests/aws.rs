//! AWS at a real daemon's start (row 29, C1; AWS design §3.10): a config that
//! binds an account sends nothing to AWS before the socket answers. The
//! account's check is a background startup phase, `aws.check`, that begins
//! only after the socket's phase has ended, by the daemon's own clock; it
//! sends AWS one request, STS's `GetCallerIdentity`; and while AWS has not
//! answered, health answers and says the account is checking.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{safe_note, Daemon, Served};
use serde_json::Value;

/// The account the tests bind: AWS's documentation's example id.
const ACCOUNT: &str = "111122223333";

/// What the fake saw: each request's first line and body.
type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// A stand-in for AWS on 127.0.0.1, bound before the daemon starts (a
/// connect to a port nothing listens on hangs here): it keeps each request,
/// and answers STS's `GetCallerIdentity` for the account after `hold`.
fn fake_aws(hold: Duration) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let kept = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let kept = kept.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if r.read_line(&mut first).is_err() {
                    return;
                }
                let mut len = 0;
                let mut line = String::new();
                while r.read_line(&mut line).is_ok_and(|n| n > 0) && line.trim() != "" {
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let mut body = vec![0; len];
                let _ = r.read_exact(&mut body);
                kept.lock().unwrap().push((
                    first.trim().to_string(),
                    String::from_utf8_lossy(&body).into_owned(),
                ));
                std::thread::sleep(hold);
                let xml = format!(
                    "<GetCallerIdentityResponse><GetCallerIdentityResult>\
                     <Arn>arn:aws:iam::{ACCOUNT}:user/example</Arn><UserId>AIDATESTEXAMPLE</UserId>\
                     <Account>{ACCOUNT}</Account></GetCallerIdentityResult>\
                     <ResponseMetadata><RequestId>req-1</RequestId></ResponseMetadata>\
                     </GetCallerIdentityResponse>"
                );
                let mut w = stream;
                let _ = w.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\ncontent-length: {}\r\n\
                         connection: close\r\n\r\n{xml}",
                        xml.len()
                    )
                    .as_bytes(),
                );
            });
        }
    });
    (url, seen)
}

/// A socket daemon on `safe_note` with the account bound at `endpoint`, and
/// a stand-in `op` that answers every reference at once. Health answers when
/// this returns.
fn start(endpoint: &str) -> Served {
    use std::os::unix::fs::PermissionsExt;
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let path = |p: &str| dir.path().join(p);
    std::fs::create_dir_all(path("bin")).unwrap();
    std::fs::create_dir_all(path("projects")).unwrap();
    std::fs::write(
        path("bin/op"),
        "#!/bin/sh\ncase \"$1\" in\n  inject) sed -e 's/{{ [^}]* }}/test-value-0000/g' ;;\n  \
         read) printf '%s' test-value-0000 ;;\n  *) exit 1 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut t: toml::Table = safe_note(&theseusd, &path("projects"), 100.0)
        .parse()
        .unwrap();
    let account: toml::Table = toml::from_str(&format!(
        "region = \"us-west-2\"\nendpoint = \"{endpoint}\""
    ))
    .unwrap();
    let mut accounts = toml::Table::new();
    accounts.insert(ACCOUNT.into(), account.into());
    let mut aws = toml::Table::new();
    aws.insert("accounts".into(), accounts.into());
    t.insert("aws".into(), aws.into());
    std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    let log = std::fs::File::create(path("theseusd.log")).unwrap();
    let daemon = Daemon::spawn(
        Command::new(&theseusd)
            .arg("--config")
            .arg(path("config.toml"))
            .arg("--state-dir")
            .arg(path("state"))
            .arg("--socket")
            .arg(path("sock"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    path("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_OPERATOR_UMASK")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log),
    );
    let mut s = Served { dir, daemon };
    let deadline = Instant::now() + Duration::from_secs(15);
    while s.call("health", Value::Null).is_err() {
        if let Some(status) = s.daemon.try_wait() {
            panic!("theseusd exited ({status}):\n{}", s.log());
        }
        assert!(Instant::now() < deadline, "no health in 15 s:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(5));
    }
    s
}

/// Health, asked until `ok` holds, for at most 20 s.
fn until(s: &Served, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(h) = s.call("health", Value::Null) {
            if ok(&h) {
                return h;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: not in 20 s; health's aws: {}\n{}",
                h["aws"],
                s.log()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A startup phase by name: its start and its end, in microseconds from the
/// process's start, and whether it is a background one.
fn phase(h: &Value, name: &str) -> (u64, Option<u64>, bool) {
    let p = h["startup"]
        .as_array()
        .and_then(|a| a.iter().find(|p| p["name"] == name))
        .unwrap_or_else(|| panic!("no {name} phase in {}", h["startup"]));
    (
        p["start_us"].as_u64().unwrap(),
        p["end_us"].as_u64(),
        p["background"].as_bool().unwrap_or(false),
    )
}

fn account_state(h: &Value) -> &str {
    h["aws"]["accounts"][0]["state"].as_str().unwrap_or("")
}

#[test]
fn the_account_is_checked_after_the_socket_answers_with_one_request() {
    let (url, seen) = fake_aws(Duration::ZERO);
    let s = start(&url);
    // The account is bound a moment before its phase ends (the phase ends
    // once every account's check has returned), so wait for both.
    let h = until(&s, "the account bound and its check ended", |h| {
        account_state(h) == "bound" && phase(h, "aws.check").1.is_some()
    });
    let (_, socket_end, _) = phase(&h, "socket");
    let (check_start, _, background) = phase(&h, "aws.check");
    let socket_end = socket_end.expect("the socket's phase ended");
    assert!(background, "the check is a background phase");
    assert!(
        check_start >= socket_end,
        "the check began at {check_start} µs, before the socket answered at {socket_end} µs"
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "one request to AWS: {seen:?}");
    assert!(seen[0].0.starts_with("POST "), "{:?}", seen[0]);
    assert!(
        seen[0].1.contains("Action=GetCallerIdentity"),
        "{:?}",
        seen[0]
    );
    let a = &h["aws"]["accounts"][0];
    assert_eq!(a["account"], ACCOUNT);
    assert_eq!(a["arn"], format!("arn:aws:iam::{ACCOUNT}:user/example"));
    assert_eq!(a["calls"], 1);
    assert!(
        !s.log().contains("test-value-0000"),
        "a secret's value in the log"
    );
}

#[test]
fn health_answers_while_aws_does_not() {
    let (url, seen) = fake_aws(Duration::from_secs(30));
    let s = start(&url);
    let h = until(&s, "the check's request at AWS", |h| {
        !seen.lock().unwrap().is_empty() && account_state(h) == "checking"
    });
    let (check_start, check_end, _) = phase(&h, "aws.check");
    assert_eq!(check_end, None, "the check waits on AWS: {check_start}");
    // The socket answers every request meanwhile, at once.
    let t0 = Instant::now();
    for _ in 0..5 {
        s.call("health", Value::Null).unwrap();
    }
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
}
