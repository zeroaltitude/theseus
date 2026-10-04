//! Hands at a real daemon (step 40 part 2, theseus-mgw.11): a group of
//! three Lambda hands launched through a real turn, against a stateful fake
//! of AWS on 127.0.0.1 (the stacks, Lambda's Invoke, and the completion
//! queue); the daemon is `kill -9`'d in the middle of the group, its other
//! hands' envelopes (one twice) arrive while it is down, and after the
//! restart every hand and the group settle exactly once.

mod common;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::{safe_note, Daemon};
use serde_json::{json, Value};
use theseus_core::aws::hands::envelope::{Envelope, HandSpec, VERSION};

const ACCOUNT: &str = "111122223333";

/// What the fake keeps: each hand's spec as Lambda was asked to run it, and
/// the queue's messages.
#[derive(Default)]
struct Aws {
    invoked: Mutex<Vec<HandSpec>>,
    queue: Mutex<VecDeque<(String, String)>>,
    sent: Mutex<u64>,
    deleted: Mutex<Vec<String>>,
}

impl Aws {
    fn push(&self, body: String) {
        let mut n = self.sent.lock().unwrap();
        *n += 1;
        self.queue
            .lock()
            .unwrap()
            .push_back((format!("rh-{n}"), body));
    }
}

fn outputs(pairs: &[(&str, &str)]) -> String {
    let o: String = pairs
        .iter()
        .map(|(k, v)| {
            format!("<member><OutputKey>{k}</OutputKey><OutputValue>{v}</OutputValue></member>")
        })
        .collect();
    format!(
        "<DescribeStacksResponse><DescribeStacksResult><Stacks><member>\
         <StackName>s</StackName><StackId>arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/s/1</StackId>\
         <CreationTime>2026-10-01T00:00:00Z</CreationTime><StackStatus>CREATE_COMPLETE</StackStatus>\
         <Outputs>{o}</Outputs><Parameters></Parameters></member></Stacks></DescribeStacksResult>\
         <ResponseMetadata><RequestId>r</RequestId></ResponseMetadata></DescribeStacksResponse>"
    )
}

/// One answer: status, content type, extra headers, body.
fn answer(
    aws: &Aws,
    line: &str,
    headers: &[(String, String)],
    body: &str,
) -> (u16, String, String) {
    let header = |k: &str| {
        headers
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    };
    let xml = |b: String| (200, "text/xml".to_string(), b);
    let json = |v: Value| (200, "application/x-amz-json-1.0".to_string(), v.to_string());
    if body.contains("Action=GetCallerIdentity") {
        return xml(format!(
            "<GetCallerIdentityResponse><GetCallerIdentityResult>\
             <Arn>arn:aws:iam::{ACCOUNT}:user/example</Arn><UserId>AIDATESTEXAMPLE</UserId>\
             <Account>{ACCOUNT}</Account></GetCallerIdentityResult>\
             <ResponseMetadata><RequestId>req-1</RequestId></ResponseMetadata>\
             </GetCallerIdentityResponse>"
        ));
    }
    if body.contains("Action=DescribeStacks") {
        let out = if body.contains("theseus-hands-network") {
            outputs(&[])
        } else if body.contains("theseus-hands") {
            outputs(&[(
                "LambdaHandArn",
                &format!("arn:aws:lambda:us-west-2:{ACCOUNT}:function:theseus-hand-basic"),
            )])
        } else {
            outputs(&[
                ("BucketName", "example-bucket"),
                (
                    "CompletionQueueUrl",
                    &format!("https://sqs.us-west-2.amazonaws.com/{ACCOUNT}/theseus-completions"),
                ),
            ])
        };
        return xml(out);
    }
    if line.starts_with("POST") && line.contains("/invocations") {
        let spec: HandSpec = serde_json::from_str(body).expect("a hand's spec");
        aws.invoked.lock().unwrap().push(spec);
        return (202, "application/json".into(), String::new());
    }
    let target = header("x-amz-target").unwrap_or_default();
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if target.ends_with("ReceiveMessage") {
        let got: Vec<(String, String)> = {
            let mut q = aws.queue.lock().unwrap();
            let k = q.len().min(10);
            q.drain(..k).collect()
        };
        if got.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            return json(json!({}));
        }
        return json(json!({"Messages": got.iter().map(|(r, b)| json!({
            "MessageId": r, "ReceiptHandle": r, "Body": b})).collect::<Vec<_>>()}));
    }
    if target.ends_with("DeleteMessage") {
        aws.deleted
            .lock()
            .unwrap()
            .push(v["ReceiptHandle"].as_str().unwrap_or_default().into());
    }
    json(json!({}))
}

/// The fake, on an ephemeral port, until the process ends.
fn fake_aws() -> (String, Arc<Aws>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let aws: Arc<Aws> = Arc::default();
    let kept = aws.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let aws = kept.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if r.read_line(&mut first).is_err() {
                    return;
                }
                let mut len = 0;
                let mut headers = Vec::new();
                let mut line = String::new();
                while r.read_line(&mut line).is_ok_and(|n| n > 0) && line.trim() != "" {
                    if let Some((k, v)) = line.trim().split_once(':') {
                        if k.eq_ignore_ascii_case("content-length") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                        headers.push((k.trim().to_string(), v.trim().to_string()));
                    }
                    line.clear();
                }
                let mut body = vec![0; len];
                let _ = r.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).into_owned();
                let (status, ctype, out) = answer(&aws, first.trim(), &headers, &body);
                let reason = if status == 202 { "Accepted" } else { "OK" };
                let mut w = stream;
                let _ = w.write_all(
                    format!(
                        "HTTP/1.1 {status} {reason}\r\ncontent-type: {ctype}\r\n\
                         x-amzn-requestid: req-{len}\r\ncontent-length: {}\r\n\
                         connection: close\r\n\r\n{out}",
                        out.len()
                    )
                    .as_bytes(),
                );
            });
        }
    });
    (url, aws)
}

/// The config: the template made safe, the fake model, the account at the
/// fake, the gate at notify, and a stand-in `op` that answers every
/// reference.
fn prepare(dir: &Path, model: &str, endpoint: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = |p: &str| dir.join(p);
    for d in ["bin", "projects", "state"] {
        std::fs::create_dir_all(path(d)).unwrap();
    }
    std::fs::write(
        path("bin/op"),
        "#!/bin/sh\ncase \"$1\" in\n  inject) sed -e 's/{{ [^}]* }}/test-value-0000/g' ;;\n  \
         read) printf '%s' test-value-0000 ;;\n  *) exit 1 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let mut t: toml::Table = safe_note(&theseusd, &path("projects"), 100.0)
        .parse()
        .unwrap();
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
    }
    table(&mut t, "model").insert("api_base".into(), model.into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        p.as_table_mut()
            .unwrap()
            .insert("api_base".into(), model.into());
    }
    table(&mut t, "policy").insert("enforcement".into(), "notify".into());
    let account: toml::Table = toml::from_str(&format!(
        "region = \"us-west-2\"\nendpoint = \"{endpoint}\"\n"
    ))
    .unwrap();
    let mut accounts = toml::Table::new();
    accounts.insert(ACCOUNT.into(), account.into());
    let mut aws = toml::Table::new();
    aws.insert("accounts".into(), accounts.into());
    t.insert("aws".into(), aws.into());
    std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
}

struct Up {
    dir: PathBuf,
    daemon: Daemon,
}

impl Up {
    fn start(dir: &Path) -> Self {
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        let path = |p: &str| dir.join(p);
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path("theseusd.log"))
            .unwrap();
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
        let mut up = Self {
            dir: dir.to_path_buf(),
            daemon,
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        while up.call("health", Value::Null).is_err() {
            if let Some(status) = up.daemon.try_wait() {
                panic!("theseusd exited ({status}):\n{}", up.log());
            }
            assert!(
                Instant::now() < deadline,
                "no health in 15 s:\n{}",
                up.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        up
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("theseusd.log")).unwrap_or_default()
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let s = std::os::unix::net::UnixStream::connect(self.dir.join("sock"))
            .map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.to_string()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err("the connection closed".into())
    }

    /// The ledger's rows of `kind`, each its data.
    fn rows(&self, kind: &str) -> Vec<Value> {
        let r = self
            .call("ledger.tail", json!({"n": 10000, "kind": kind}))
            .unwrap();
        r["rows"]
            .as_array()
            .or_else(|| r.as_array())
            .into_iter()
            .flatten()
            .map(|e| e["data"].clone())
            .collect()
    }

    fn until(&self, what: &str, f: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !f(self) {
            assert!(
                Instant::now() < deadline,
                "{what}: not in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// An envelope as a hand signs it.
fn signed(spec: &HandSpec, outcome: &str) -> String {
    let mut e = Envelope {
        v: VERSION,
        correlation_id: spec.correlation_id.clone(),
        group: spec.group.clone(),
        index: spec.index,
        outcome: outcome.into(),
        exit_code: Some(0),
        timed_out: false,
        result_ref: Some(format!(
            "s3://example-bucket/hands/{}/",
            spec.correlation_id
        )),
        external_op_id: Some("inv-signed".into()),
        started_at_ms: 1_000,
        finished_at_ms: 3_000,
        producer: "hand:lambda".into(),
        tail: format!("hand {} says {outcome}\n", spec.index),
        note: None,
        signature: String::new(),
    };
    e.sign(&spec.key_bytes().unwrap());
    serde_json::to_string(&e).unwrap()
}

fn settled_of(up: &Up, kind: &str, corr: &str) -> usize {
    up.rows(kind)
        .iter()
        .filter(|r| r["correlation_id"] == corr)
        .count()
}

#[test]
fn a_kill_9_mid_group_then_a_restart_settles_each_hand_and_the_group_once() {
    let (endpoint, aws) = fake_aws();
    let model = FakeModel::start(|prompt| {
        if prompt.contains("Run the hands") {
            vec![("aws_hands_run", json!({"argv": ["true"], "count": 3}))]
        } else {
            vec![]
        }
    });
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path(), &model.base, &endpoint);
    let up = Up::start(dir.path());
    up.call(
        "turn.submit",
        json!({"input": "Run the hands", "author": "test", "attachments": []}),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while aws.invoked.lock().unwrap().len() < 3 {
        assert!(Instant::now() < deadline, "no three Invokes:\n{}", up.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut specs = aws.invoked.lock().unwrap().clone();
    specs.sort_by_key(|s| s.index);
    let group = specs[0].group.clone();
    // The first hand comes home and settles.
    aws.push(signed(&specs[0], "succeeded"));
    up.until("the first hand", |u| {
        settled_of(u, "action.succeeded", &specs[0].correlation_id) == 1
    });
    // kill -9 in the middle of the group.
    let pid = up.daemon.id();
    drop(up);
    assert!(
        !Path::new(&format!("/proc/{pid}/status")).exists()
            || std::fs::read_to_string(format!("/proc/{pid}/status"))
                .is_ok_and(|s| s.contains("State:\tZ")),
        "the daemon is gone"
    );
    // While it is down, the others come home, one of them twice.
    aws.push(signed(&specs[1], "succeeded"));
    aws.push(signed(&specs[2], "succeeded"));
    aws.push(signed(&specs[2], "succeeded"));
    let up = Up::start(dir.path());
    up.until("the group", |u| {
        settled_of(u, "action.succeeded", &group) == 1
    });
    up.until("every message deleted", |_| {
        aws.deleted.lock().unwrap().len() == 4
    });
    for s in &specs {
        assert_eq!(
            settled_of(&up, "action.succeeded", &s.correlation_id),
            1,
            "hand {} settled once",
            s.index
        );
    }
    assert_eq!(
        settled_of(&up, "action.succeeded", &group),
        1,
        "the group settled once"
    );
    assert_eq!(up.rows("aws.hands.settled").len(), 1);
    assert_eq!(up.rows("completion.duplicate").len(), 1);
    assert_eq!(
        aws.invoked.lock().unwrap().len(),
        3,
        "nothing launched twice"
    );
}
