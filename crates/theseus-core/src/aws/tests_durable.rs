//! The durability tender (step 15) against a local fake with state: S3's
//! objects and multipart uploads (each checksum checked as S3 checks it),
//! DynamoDB's items (with unprocessed items when asked), and STS for the
//! tender's role session. What each test proves is in its name; the fake
//! counts every request, so "once" is counted, not assumed.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_store::{kinds, wal, NewRecord, Wal, WalConfig};

use super::durable::cursor::Paths;
use super::durable::{self, Halt, Hooks, Measure, Shipper, Tuning};
use super::tests::{board, ACCOUNT};
use super::Aws;
use crate::config::{AwsAccountConfig, AwsConfig};

// ------------------------------------------------------------------ the fake

/// A request as the fake saw it, its body as bytes.
#[derive(Clone, Debug)]
pub(super) struct Req {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) query: BTreeMap<String, String>,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

impl Req {
    pub(super) fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.as_str())
    }

    /// What the request is: `PutObject`, `UploadPart`, `BatchWriteItem`, …
    pub(super) fn op(&self) -> String {
        let text = String::from_utf8_lossy(&self.body);
        if let Some(a) = text.split('&').find_map(|kv| kv.strip_prefix("Action=")) {
            return a.to_string();
        }
        if let Some(t) = self.header("x-amz-target") {
            return t.rsplit('.').next().unwrap_or(t).to_string();
        }
        let q = |k: &str| self.query.contains_key(k);
        match self.method.as_str() {
            "PUT" if q("partNumber") => "UploadPart",
            "PUT" => "PutObject",
            "POST" if q("uploads") => "CreateMultipartUpload",
            "POST" if q("uploadId") => "CompleteMultipartUpload",
            "GET" if q("uploadId") => "ListParts",
            "HEAD" => "HeadObject",
            "GET" => "GetObject",
            _ => "Unknown",
        }
        .to_string()
    }

    /// The S3 object's key: the path past the bucket.
    pub(super) fn key(&self) -> String {
        let p = self.path.trim_start_matches('/');
        p.split_once('/')
            .map(|(_, k)| k.to_string())
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct Upload {
    key: String,
    parts: BTreeMap<u32, (Vec<u8>, String)>,
}

/// AWS as the tender needs it, with state.
#[derive(Default)]
pub(super) struct State {
    pub(super) seen: Vec<Req>,
    /// Each object: its bytes and its checksum as S3 states it.
    pub(super) objects: BTreeMap<String, (Vec<u8>, String)>,
    uploads: BTreeMap<String, Upload>,
    next_upload: u32,
    /// The table's items, by (pk, sk).
    pub(super) items: BTreeMap<(String, String), Value>,
    /// How many of the next `BatchWriteItem`s leave their last two items
    /// unprocessed.
    unprocessed: u32,
    /// Requests of this operation are refused (403) while set.
    pub(super) refuse: Option<String>,
    /// Each session's inline policy, by the access key it was given.
    policies: BTreeMap<String, Option<Value>>,
}

/// Whether a session's inline policy lets it know a key is missing, as S3
/// decides it: `s3:ListBucket` on the bucket under no condition that needs
/// `s3:prefix`, which a `GetObject` does not carry (a `StringLike` on it
/// fails without it; `StringLikeIfExists` passes). Without it, a missing
/// key is 403, not 404.
fn lists_without_prefix(policy: &Value) -> bool {
    let actions = |st: &Value| match &st["Action"] {
        Value::String(a) => vec![a.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    };
    policy["Statement"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|st| {
            st["Effect"] == "Allow"
                && actions(st)
                    .iter()
                    .any(|a| a == "s3:ListBucket" || a == "s3:*")
                && st["Condition"].as_object().is_none_or(|ops| {
                    ops.iter().all(|(op, keys)| {
                        op.ends_with("IfExists")
                            || keys
                                .as_object()
                                .is_none_or(|k| !k.contains_key("s3:prefix"))
                    })
                })
        })
}

type Reply = (u16, Vec<(String, String)>, Vec<u8>);

pub(super) struct Fake {
    pub(super) url: String,
    pub(super) state: Arc<Mutex<State>>,
}

pub(super) fn sha_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes))
}

fn xml(n: usize, body: String) -> Reply {
    (
        200,
        vec![
            ("x-amz-request-id".into(), format!("req-{n}")),
            ("content-type".into(), "text/xml".into()),
        ],
        body.into_bytes(),
    )
}

fn s3_error(status: u16, code: &str) -> Reply {
    (
        status,
        vec![("x-amz-request-id".into(), "req-err".into())],
        format!("<Error><Code>{code}</Code><Message>the fake says {code}</Message><RequestId>req-err</RequestId></Error>")
            .into_bytes(),
    )
}

impl Fake {
    pub(super) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state: Arc<Mutex<State>> = Arc::default();
        let kept = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let kept = kept.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &kept);
                });
            }
        });
        Fake { url, state }
    }

    pub(super) fn ops(&self, op: &str) -> Vec<Req> {
        let s = self.state.lock().unwrap();
        s.seen.iter().filter(|r| r.op() == op).cloned().collect()
    }

    pub(super) fn object(&self, key: &str) -> Option<Vec<u8>> {
        self.state
            .lock()
            .unwrap()
            .objects
            .get(key)
            .map(|(b, _)| b.clone())
    }

    pub(super) fn keys(&self, prefix: &str) -> Vec<String> {
        let s = self.state.lock().unwrap();
        s.objects
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect()
    }
}

fn serve(stream: TcpStream, state: &Mutex<State>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            break;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let (path, q) = target.split_once('?').unwrap_or((&target, ""));
    let query = q
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k.to_string(), v.to_string())
        })
        .collect();
    let req = Req {
        method: method.clone(),
        path: path.to_string(),
        query,
        headers,
        body,
    };
    let (status, headers, body) = {
        let mut s = state.lock().unwrap();
        s.seen.push(req.clone());
        let n = s.seen.len();
        answer(&mut s, &req, n)
    };
    let mut out = format!(
        "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n",
        if method == "HEAD" { 0 } else { body.len() }
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let mut w = stream;
    w.write_all(out.as_bytes())?;
    if method != "HEAD" {
        w.write_all(&body)?;
    }
    w.flush()
}

#[expect(clippy::too_many_lines, reason = "one fake, one match")]
fn answer(s: &mut State, r: &Req, n: usize) -> Reply {
    let op = r.op();
    if s.refuse.as_deref() == Some(op.as_str()) {
        return s3_error(403, "AccessDenied");
    }
    let checked = |r: &Req| r.header("x-amz-checksum-sha256") == Some(sha_b64(&r.body).as_str());
    match op.as_str() {
        "GetCallerIdentity" => xml(
            n,
            format!(
                "<GetCallerIdentityResponse><GetCallerIdentityResult>\
                 <Arn>arn:aws:iam::{ACCOUNT}:user/example</Arn><UserId>AIDATESTEXAMPLE</UserId>\
                 <Account>{ACCOUNT}</Account></GetCallerIdentityResult>\
                 <ResponseMetadata><RequestId>req-{n}</RequestId></ResponseMetadata></GetCallerIdentityResponse>"
            ),
        ),
        "AssumeRole" => {
            let form = super::tests_c2::form(&String::from_utf8_lossy(&r.body));
            let policy = form.get("Policy").and_then(|p| serde_json::from_str(p).ok());
            let key = format!("ASIATEST{n:08}");
            s.policies.insert(key.clone(), policy);
            xml(
            n,
            format!(
                "<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>{key}</AccessKeyId>\
                 <SecretAccessKey>test-session-secret</SecretAccessKey><SessionToken>test-token</SessionToken>\
                 <Expiration>2099-01-01T00:00:00Z</Expiration></Credentials><AssumedRoleUser>\
                 <Arn>arn:aws:sts::{ACCOUNT}:assumed-role/theseus-owner/theseus-durability</Arn>\
                 <AssumedRoleId>AROATEST:theseus-durability</AssumedRoleId></AssumedRoleUser></AssumeRoleResult>\
                 <ResponseMetadata><RequestId>req-{n}</RequestId></ResponseMetadata></AssumeRoleResponse>"
            ),
        )
        }
        "PutObject" => {
            if !checked(r) {
                return s3_error(400, "BadDigest");
            }
            let sha = sha_b64(&r.body);
            s.objects.insert(r.key(), (r.body.clone(), sha.clone()));
            let mut reply = xml(n, String::new());
            reply.1.push(("ETag".into(), format!("\"put-{n}\"")));
            reply.1.push(("x-amz-checksum-sha256".into(), sha));
            reply
        }
        "HeadObject" => match s.objects.get(&r.key()) {
            Some((b, sha)) => (
                200,
                vec![
                    ("x-amz-request-id".into(), format!("req-{n}")),
                    ("x-amz-checksum-sha256".into(), sha.clone()),
                    ("x-amz-meta-length".into(), b.len().to_string()),
                ],
                Vec::new(),
            ),
            None => (404, vec![("x-amz-request-id".into(), format!("req-{n}"))], Vec::new()),
        },
        "CreateMultipartUpload" => {
            s.next_upload += 1;
            let id = format!("up{}", s.next_upload);
            s.uploads.insert(
                id.clone(),
                Upload {
                    key: r.key(),
                    ..Default::default()
                },
            );
            xml(
                n,
                format!(
                    "<InitiateMultipartUploadResult><Bucket>b</Bucket><Key>{}</Key>\
                     <UploadId>{id}</UploadId></InitiateMultipartUploadResult>",
                    r.key()
                ),
            )
        }
        "UploadPart" => {
            let id = &r.query["uploadId"];
            let pn: u32 = r.query["partNumber"].parse().unwrap();
            if !checked(r) {
                return s3_error(400, "BadDigest");
            }
            let Some(u) = s.uploads.get_mut(id) else {
                return s3_error(404, "NoSuchUpload");
            };
            let sha = sha_b64(&r.body);
            u.parts.insert(pn, (r.body.clone(), sha.clone()));
            let mut reply = xml(n, String::new());
            reply.1.push(("ETag".into(), format!("\"{id}-{pn}\"")));
            reply.1.push(("x-amz-checksum-sha256".into(), sha));
            reply
        }
        "ListParts" => {
            let Some(u) = s.uploads.get(&r.query["uploadId"]) else {
                return s3_error(404, "NoSuchUpload");
            };
            let parts: String = u
                .parts
                .iter()
                .map(|(pn, (b, sha))| {
                    format!(
                        "<Part><PartNumber>{pn}</PartNumber><ETag>\"{}-{pn}\"</ETag>\
                         <Size>{}</Size><ChecksumSHA256>{sha}</ChecksumSHA256></Part>",
                        r.query["uploadId"],
                        b.len()
                    )
                })
                .collect();
            xml(
                n,
                format!("<ListPartsResult><IsTruncated>false</IsTruncated>{parts}</ListPartsResult>"),
            )
        }
        "CompleteMultipartUpload" => {
            let id = r.query["uploadId"].clone();
            let Some(u) = s.uploads.remove(&id) else {
                return s3_error(404, "NoSuchUpload");
            };
            let text = String::from_utf8_lossy(&r.body).into_owned();
            let named: Vec<u32> = text
                .split("<PartNumber>")
                .skip(1)
                .filter_map(|p| p.split('<').next()?.parse().ok())
                .collect();
            let mut bytes = Vec::new();
            let mut h = Sha256::new();
            for pn in &named {
                let (b, sha) = &u.parts[pn];
                bytes.extend_from_slice(b);
                h.update(base64::engine::general_purpose::STANDARD.decode(sha).unwrap());
            }
            let composite = format!(
                "{}-{}",
                base64::engine::general_purpose::STANDARD.encode(h.finalize()),
                named.len()
            );
            s.objects.insert(u.key.clone(), (bytes, composite.clone()));
            xml(
                n,
                format!(
                    "<CompleteMultipartUploadResult><Bucket>b</Bucket><Key>{}</Key>\
                     <ETag>\"done-{n}\"</ETag><ChecksumSHA256>{composite}</ChecksumSHA256>\
                     </CompleteMultipartUploadResult>",
                    u.key
                ),
            )
        }
        "BatchWriteItem" => {
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            let mut left = serde_json::Map::new();
            for (table, puts) in v["RequestItems"].as_object().unwrap() {
                let puts = puts.as_array().unwrap();
                assert!(puts.len() <= 25, "a BatchWriteItem of {} puts", puts.len());
                let keep = if s.unprocessed > 0 {
                    puts.len().saturating_sub(2)
                } else {
                    puts.len()
                };
                for p in &puts[..keep] {
                    let i = &p["PutRequest"]["Item"];
                    let k = (
                        i["pk"]["S"].as_str().unwrap().to_string(),
                        i["sk"]["S"].as_str().unwrap().to_string(),
                    );
                    s.items.insert(k, i.clone());
                }
                if keep < puts.len() {
                    left.insert(table.clone(), json!(puts[keep..]));
                }
            }
            s.unprocessed = s.unprocessed.saturating_sub(1);
            (
                200,
                vec![
                    ("x-amzn-requestid".into(), format!("req-{n}")),
                    ("content-type".into(), "application/x-amz-json-1.0".into()),
                ],
                json!({"UnprocessedItems": left}).to_string().into_bytes(),
            )
        }
        "GetObject" => {
            let Some((bytes, sha)) = s.objects.get(&r.key()).cloned() else {
                let key = r
                    .header("authorization")
                    .and_then(|a| a.split("Credential=").nth(1))
                    .and_then(|c| c.split('/').next())
                    .unwrap_or_default();
                // A session's inline policy decides; the key's own, the
                // fake's whole account, may list.
                return match s.policies.get(key) {
                    Some(Some(p)) if !lists_without_prefix(p) => s3_error(403, "AccessDenied"),
                    _ => s3_error(404, "NoSuchKey"),
                };
            };
            let mut headers = vec![
                ("x-amz-request-id".into(), format!("req-{n}")),
                ("content-type".into(), "application/octet-stream".into()),
            ];
            match r.header("range").and_then(|v| v.strip_prefix("bytes=")) {
                Some(range) => {
                    let (a, b) = range.split_once('-').unwrap();
                    let a: usize = a.parse().unwrap();
                    let b = b.parse::<usize>().unwrap().min(bytes.len() - 1);
                    headers.push((
                        "content-range".into(),
                        format!("bytes {a}-{b}/{}", bytes.len()),
                    ));
                    (206, headers, bytes[a..=b].to_vec())
                }
                None => {
                    if r.header("x-amz-checksum-mode") == Some("ENABLED") {
                        headers.push(("x-amz-checksum-sha256".into(), sha));
                    }
                    (200, headers, bytes)
                }
            }
        }
        "Query" => {
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            let pk = v["ExpressionAttributeValues"][":pk"]["S"].as_str().unwrap().to_string();
            let after = v["ExclusiveStartKey"]["sk"]["S"].as_str().map(String::from);
            let limit = v["Limit"].as_u64().map_or(usize::MAX, |l| l as usize);
            let page: Vec<Value> = s
                .items
                .iter()
                .filter(|((p, k), _)| *p == pk && after.as_ref().is_none_or(|a| k > a))
                .take(limit)
                .map(|(_, i)| i.clone())
                .collect();
            let mut out = json!({"Items": page, "Count": page.len()});
            if page.len() == limit {
                let last = page.last().unwrap();
                out["LastEvaluatedKey"] = json!({"pk": last["pk"], "sk": last["sk"]});
            }
            (
                200,
                vec![
                    ("x-amzn-requestid".into(), format!("req-{n}")),
                    ("content-type".into(), "application/x-amz-json-1.0".into()),
                ],
                out.to_string().into_bytes(),
            )
        }
        other => s3_error(400, &format!("TheFakeDoesNotKnow{other}")),
    }
}

// ------------------------------------------------------------------ the rig

pub(super) fn layer(fake: &Fake) -> Arc<Aws> {
    let cfg = AwsConfig {
        accounts: BTreeMap::from([(
            ACCOUNT.to_string(),
            AwsAccountConfig {
                credentials: Default::default(),
                region: "us-west-2".into(),
                regions: Vec::new(),
                endpoint: Some(fake.url.clone()),
                owner_role: Some("theseus-owner".into()),
                deployment: Some("theseus-lab".into()),
                monthly_budget_usd: None,
                daily_budget_usd: None,
                hourly_alert_usd: crate::config::default_hourly_alert_usd(),
                durability: true,
            },
        )]),
    };
    Aws::from_config(&cfg, board()).expect("an account")
}

pub(super) const PREFIX: &str = "durability/theseus-lab/";

/// A store's directory with its WAL in small segments, so a few frames seal
/// several.
struct Rig {
    _dir: tempfile::TempDir,
    store: PathBuf,
    wal: Wal,
    aws: Arc<Aws>,
    shipped: Arc<Mutex<Vec<Measure>>>,
    rows: Arc<Mutex<Vec<crate::ledger::LedgerRow>>>,
}

pub(super) fn tuning() -> Tuning {
    Tuning {
        part_bytes: 128,
        single_max: 150,
        batch_bytes: 1 << 20,
        settle: Duration::from_millis(1),
        row_tries: 4,
        row_backoff: Duration::from_millis(1),
    }
}

impl Rig {
    fn new(fake: &Fake) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let wal = Wal::open(
            &store.join("wal"),
            WalConfig {
                segment_bytes: 400,
                fsync: false,
                ..WalConfig::default()
            },
        )
        .unwrap();
        Rig {
            _dir: dir,
            store,
            wal,
            aws: layer(fake),
            shipped: Arc::default(),
            rows: Arc::default(),
        }
    }

    fn write(&self, from: u32, count: u32) {
        for i in from..from + count {
            let key = format!("sess-{}", i % 7);
            self.wal
                .append(&[NewRecord::bytes(
                    kinds::SESSION,
                    Some(&key),
                    format!("a session's record, number {i}").into_bytes(),
                )])
                .unwrap();
        }
    }

    fn segments(&self) -> Vec<u32> {
        wal::list_segments(&self.store.join("wal")).unwrap()
    }

    fn segment(&self, n: u32) -> Vec<u8> {
        std::fs::read(wal::segment_path(&self.store.join("wal"), n)).unwrap()
    }

    /// A shipper as a start opens it: from the cursor on disk.
    fn shipper(&self) -> Shipper {
        let (shipped, rows) = (self.shipped.clone(), self.rows.clone());
        let hooks = Hooks {
            ledger: Some(Arc::new(move |r| rows.lock().unwrap().push(r))),
            measure: Some(Arc::new(move |m| shipped.lock().unwrap().push(m))),
        };
        let account = self.aws.accounts().next().unwrap().clone();
        Shipper::open(account, Paths::for_store(&self.store), tuning(), hooks).unwrap()
    }

    fn blob(&self, bytes: &[u8]) -> String {
        let d = hex::encode(Sha256::digest(bytes));
        std::fs::create_dir_all(self.store.join("blobs")).unwrap();
        std::fs::write(self.store.join("blobs").join(&d), bytes).unwrap();
        d
    }
}

/// A segment's bytes as S3 holds them: the sealed object, or its tails
/// joined, each tail starting where the last ended.
fn shipped_segment(fake: &Fake, n: u32) -> Option<Vec<u8>> {
    if let Some(b) = fake.object(&format!("{PREFIX}wal/{n:09}.seg")) {
        return Some(b);
    }
    let tails = fake.keys(&format!("{PREFIX}wal/{n:09}.seg.tail/"));
    let mut out = Vec::new();
    for k in tails {
        let range = k.rsplit('/').next().unwrap();
        let (from, _) = range.split_once('-').unwrap();
        assert_eq!(
            from.parse::<usize>().unwrap(),
            out.len(),
            "a gap or an overlap at {k}"
        );
        out.extend(fake.object(&k).unwrap());
    }
    (!out.is_empty()).then_some(out)
}

/// Each key S3 was sent (a put, or a completed upload), and how many times.
pub(super) fn sends(fake: &Fake) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for r in fake
        .ops("PutObject")
        .into_iter()
        .chain(fake.ops("CompleteMultipartUpload"))
    {
        *m.entry(r.key()).or_insert(0) += 1;
    }
    m
}

// ------------------------------------------------------------------ the tests

/// Sealed segments ship whole, once each, with their SHA-256 (a single put's,
/// or each part's); the open one's tails tile it; a second pass sends
/// nothing; and each segment has its row and its `durability.shipped` row.
#[tokio::test]
async fn a_sealed_segment_is_shipped_once_with_its_checksum() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    rig.write(0, 30);
    let segs = rig.segments();
    assert!(segs.len() >= 4, "{segs:?}");
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    let open = *segs.last().unwrap();
    for &n in &segs {
        assert_eq!(
            shipped_segment(&fake, n),
            Some(rig.segment(n)),
            "segment {n}"
        );
    }
    for &n in &segs[..segs.len() - 1] {
        assert!(
            fake.object(&format!("{PREFIX}wal/{n:09}.seg")).is_some(),
            "sealed {n} whole"
        );
        assert!(
            fake.keys(&format!("{PREFIX}wal/{n:09}.seg.tail/"))
                .is_empty(),
            "no tail of sealed {n}"
        );
    }
    // Each body went with its checksum (the fake refuses one that differs),
    // and the multipart ones went in parts of 128 bytes.
    for r in fake.ops("PutObject").iter().chain(&fake.ops("UploadPart")) {
        assert_eq!(
            r.header("x-amz-checksum-sha256"),
            Some(sha_b64(&r.body).as_str())
        );
    }
    assert!(
        !fake.ops("UploadPart").is_empty(),
        "segments past 150 bytes go in parts"
    );
    assert!(fake.ops("UploadPart").iter().all(|r| r.body.len() <= 128));
    let once = sends(&fake);
    assert!(once.values().all(|n| *n == 1), "{once:?}");
    // The rows: one per sealed segment and tail, and each key's latest.
    let items = fake.state.lock().unwrap().items.clone();
    for &n in &segs[..segs.len() - 1] {
        let row = &items[&("theseus-lab#wal".to_string(), format!("{n:09}"))];
        assert_eq!(
            row["sha256"]["S"],
            hex::encode(Sha256::digest(rig.segment(n))),
            "segment {n}'s row names its SHA-256"
        );
    }
    let sess = &items[&("theseus-lab#rec#session".to_string(), "sess-1".to_string())];
    assert_eq!(
        sess["position"]["N"], "30",
        "sess-1's latest record is position 30"
    );
    assert_eq!(sess["segment"]["N"], open.to_string());
    let ledger = rig.rows.lock().unwrap().clone();
    assert_eq!(
        ledger.len(),
        segs.len() - 1,
        "a durability.shipped row per sealed segment"
    );
    assert_eq!(ledger[0].kind, "durability.shipped");
    assert_eq!(s.status().state, "caught_up");
    assert_eq!(s.status().oldest_unshipped_unix_ms, None);
    assert_eq!(s.status().shipped_to_position, 30);
    // Nothing new: a second pass sends nothing at all.
    let before = fake.state.lock().unwrap().seen.len();
    s.pass().await.unwrap();
    assert_eq!(fake.state.lock().unwrap().seen.len(), before);
    // More frames: the open segment's new bytes go as a tail, or with its
    // seal, and the earlier objects are not sent again.
    rig.write(30, 3);
    s.changed(theseus_protocol::now_unix_ms());
    assert!(s.status().oldest_unshipped_unix_ms.is_some());
    s.pass().await.unwrap();
    for &n in &rig.segments() {
        assert_eq!(
            shipped_segment(&fake, n),
            Some(rig.segment(n)),
            "segment {n}"
        );
    }
    let again = sends(&fake);
    assert!(again.values().all(|n| *n == 1), "{again:?}");
    assert!(rig
        .shipped
        .lock()
        .unwrap()
        .iter()
        .any(|m| matches!(m, Measure::CaughtUp { .. })));
}

/// A crash in the middle of a multipart upload, after S3 took a part and
/// before the cursor said so: the restart asks S3 for the upload's parts,
/// sends only the rest, and completes it once. And a crash just after a
/// whole put: the restart finds the object by its checksum, and sends
/// nothing again. No object is sent twice, and none is missing.
#[tokio::test]
async fn a_restart_mid_upload_resumes_without_a_duplicate_or_a_gap() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    rig.write(0, 30);
    let mut s = rig.shipper();
    s.crash_after_part = Some(2);
    assert!(matches!(s.pass().await, Err(Halt::Retry(_))));
    let up = fake.ops("CreateMultipartUpload");
    assert_eq!(up.len(), 1);
    let key = up[0].key();
    assert_eq!(
        fake.ops("UploadPart").len(),
        2,
        "two parts sent before the crash"
    );
    drop(s);
    // The restart: a new shipper from the cursor on disk.
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    assert_eq!(
        fake.ops("CreateMultipartUpload")
            .iter()
            .filter(|r| r.key() == key)
            .count(),
        1
    );
    let mut parts: Vec<(String, String)> = fake
        .ops("UploadPart")
        .iter()
        .filter(|r| r.key() == key)
        .map(|r| (r.query["uploadId"].clone(), r.query["partNumber"].clone()))
        .collect();
    let all = parts.len();
    parts.sort();
    parts.dedup();
    assert_eq!(parts.len(), all, "a part sent twice: {parts:?}");
    assert_eq!(
        fake.ops("ListParts").len(),
        1,
        "the restart asked S3 for the parts"
    );
    for n in rig.segments() {
        assert_eq!(
            shipped_segment(&fake, n),
            Some(rig.segment(n)),
            "segment {n}"
        );
    }
    let once = sends(&fake);
    assert!(once.values().all(|n| *n == 1), "{once:?}");

    // A crash just after a whole put (a tail of the open segment).
    rig.write(30, 1);
    s.crash_after_put = true;
    assert!(matches!(s.pass().await, Err(Halt::Retry(_))));
    drop(s);
    let puts = fake.ops("PutObject").len();
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    assert_eq!(
        fake.ops("PutObject").len(),
        puts,
        "found by its checksum, not sent again"
    );
    assert_eq!(fake.ops("HeadObject").len(), 1);
    for n in rig.segments() {
        assert_eq!(
            shipped_segment(&fake, n),
            Some(rig.segment(n)),
            "segment {n}"
        );
    }
    let once = sends(&fake);
    assert!(once.values().all(|n| *n == 1), "{once:?}");
}

/// A restart after a tail shipped but before its batch's rows and cursor
/// were written, with more frames written meanwhile: the tail is not sent
/// again, and the next starts where it ended, so the tails tile the open
/// segment with no overlap and no gap.
#[tokio::test]
async fn a_tail_shipped_before_a_crash_is_not_shipped_again_in_part() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    rig.write(0, 2);
    assert_eq!(rig.segments(), vec![1]);
    fake.state.lock().unwrap().refuse = Some("BatchWriteItem".into());
    let mut s = rig.shipper();
    assert!(matches!(s.pass().await, Err(Halt::Retry(_))));
    assert_eq!(
        fake.keys(&format!("{PREFIX}wal/000000001.seg.tail/")).len(),
        1
    );
    drop(s);
    rig.write(2, 2);
    assert_eq!(rig.segments(), vec![1], "still one open segment");
    fake.state.lock().unwrap().refuse = None;
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    let tails = fake.keys(&format!("{PREFIX}wal/000000001.seg.tail/"));
    assert_eq!(tails.len(), 2, "{tails:?}");
    assert_eq!(shipped_segment(&fake, 1), Some(rig.segment(1)));
    let once = sends(&fake);
    assert!(once.values().all(|n| *n == 1), "{once:?}");
    // The rows of both tails, and the cursor's pending rows written.
    let items = fake.state.lock().unwrap().items.clone();
    let wal_rows = items
        .keys()
        .filter(|(pk, _)| pk == "theseus-lab#wal")
        .count();
    assert_eq!(wal_rows, 2);
}

/// Each blob ships once, with its SHA-256 and its row; a new one ships
/// alone; a restart ships none again; and a file whose bytes are not its
/// name is left.
#[tokio::test]
async fn a_blob_is_shipped_once() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    let a = rig.blob(b"an image's bytes, invented");
    std::fs::write(
        rig.store.join("blobs").join("0".repeat(64)),
        b"not its name",
    )
    .unwrap();
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    assert_eq!(
        fake.object(&format!("{PREFIX}blobs/{a}")).as_deref(),
        Some(&b"an image's bytes, invented"[..])
    );
    assert!(fake
        .object(&format!("{PREFIX}blobs/{}", "0".repeat(64)))
        .is_none());
    let b = rig.blob(b"a second image, invented");
    s.pass().await.unwrap();
    drop(s);
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    let blobs: Vec<String> = fake
        .ops("PutObject")
        .iter()
        .map(Req::key)
        .filter(|k| k.contains("/blobs/"))
        .collect();
    assert_eq!(
        blobs,
        vec![format!("{PREFIX}blobs/{a}"), format!("{PREFIX}blobs/{b}")]
    );
    let items = fake.state.lock().unwrap().items.clone();
    assert!(items.contains_key(&("theseus-lab#blob".to_string(), b.clone())));
    assert_eq!(s.status().blobs, 0, "this start shipped none");
}

/// Index rows go 25 to a request, and the items DynamoDB leaves unprocessed
/// are sent again until every row is written; past the tries, the pass
/// fails, keeps its rows, and the next pass writes them.
#[tokio::test]
async fn index_rows_batch_and_retry_their_unprocessed_items() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    // 60 keys, each in its own record, in one segment.
    for i in 0..60 {
        rig.wal
            .append(&[NewRecord::bytes(
                kinds::NODE,
                Some(&format!("node-{i}")),
                vec![b'x'],
            )])
            .unwrap();
    }
    fake.state.lock().unwrap().unprocessed = 2;
    let mut s = rig.shipper();
    s.pass().await.unwrap();
    let writes = fake.ops("BatchWriteItem");
    let items = fake.state.lock().unwrap().items.clone();
    for i in 0..60 {
        assert!(
            items.contains_key(&("theseus-lab#rec#node".to_string(), format!("node-{i}"))),
            "node-{i}"
        );
    }
    let sizes: Vec<usize> = writes
        .iter()
        .map(|r| {
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            v["RequestItems"]["theseus-durability"]
                .as_array()
                .unwrap()
                .len()
        })
        .collect();
    assert!(sizes.iter().all(|n| *n <= 25), "{sizes:?}");
    // Two batches answered with two items left each, and each retried.
    assert!(
        sizes.contains(&2),
        "the unprocessed items sent again: {sizes:?}"
    );
    assert!(s.status().rows >= 60);

    // DynamoDB keeps refusing: the pass fails, and the next one writes.
    for i in 60..70 {
        rig.wal
            .append(&[NewRecord::bytes(
                kinds::NODE,
                Some(&format!("node-{i}")),
                vec![b'y'],
            )])
            .unwrap();
    }
    fake.state.lock().unwrap().unprocessed = 100;
    assert!(matches!(s.pass().await, Err(Halt::Retry(e)) if e.contains("unprocessed")));
    assert_eq!(s.status().state, "failing");
    fake.state.lock().unwrap().unprocessed = 0;
    s.pass().await.unwrap();
    let items = fake.state.lock().unwrap().items.clone();
    for i in 60..70 {
        assert!(
            items.contains_key(&("theseus-lab#rec#node".to_string(), format!("node-{i}"))),
            "node-{i}"
        );
    }
}

/// What AWS refuses is health's to say, with the oldest unshipped record's
/// time, and nothing moves past it; once AWS answers, the next pass ships.
#[tokio::test]
async fn a_refusal_is_said_in_health_and_the_next_pass_ships() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    rig.write(0, 3);
    fake.state.lock().unwrap().refuse = Some("PutObject".into());
    let mut s = rig.shipper();
    assert!(matches!(s.pass().await, Err(Halt::Retry(_))));
    let account = rig.aws.accounts().next().unwrap().clone();
    let line = account.status().durability.expect("a durability line");
    assert_eq!(line.state, "failing");
    assert!(
        line.error.as_deref().unwrap_or("").contains("AccessDenied"),
        "{line:?}"
    );
    assert!(line.oldest_unshipped_unix_ms.is_some());
    assert_eq!(line.bucket, format!("theseus-{ACCOUNT}-us-west-2"));
    assert_eq!(line.prefix, PREFIX);
    fake.state.lock().unwrap().refuse = None;
    s.pass().await.unwrap();
    let line = account.status().durability.unwrap();
    assert_eq!((line.state.as_str(), line.lag_ms), ("caught_up", 0));
    assert_eq!(shipped_segment(&fake, 1), Some(rig.segment(1)));
}

/// The tender's session: the foundation's bucket under its own prefix, and
/// rows of its own deployment; nothing else.
#[test]
fn the_tender_session_is_narrowed_to_its_prefix_and_its_rows() {
    let fake_cfg = AwsAccountConfig {
        credentials: Default::default(),
        region: "us-west-2".into(),
        regions: Vec::new(),
        endpoint: None,
        owner_role: Some("theseus-owner".into()),
        deployment: Some("theseus-lab".into()),
        monthly_budget_usd: None,
        daily_budget_usd: None,
        hourly_alert_usd: crate::config::default_hourly_alert_usd(),
        durability: true,
    };
    assert!(durable::policy("tender", ACCOUNT, &fake_cfg).is_none());
    let p = durable::policy(durable::TENDER, ACCOUNT, &fake_cfg).unwrap();
    let st = p["Statement"].as_array().unwrap();
    assert_eq!(st.len(), 2);
    assert_eq!(
        st[0]["Resource"],
        format!("arn:aws:s3:::theseus-{ACCOUNT}-us-west-2/durability/theseus-lab/*")
    );
    assert_eq!(
        st[1]["Resource"],
        format!("arn:aws:dynamodb:us-west-2:{ACCOUNT}:table/theseus-durability")
    );
    assert_eq!(
        st[1]["Condition"]["ForAllValues:StringLike"]["dynamodb:LeadingKeys"],
        json!(["theseus-lab#*"])
    );
    let actions: Vec<&str> = st
        .iter()
        .flat_map(|s| match &s["Action"] {
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            Value::String(a) => vec![a.as_str()],
            _ => Vec::new(),
        })
        .collect();
    assert!(
        !actions
            .iter()
            .any(|a| a.contains("Delete") || a.ends_with('*')),
        "{actions:?}"
    );
}

/// The tender's whole life: nothing is asked of AWS before `START_AFTER`
/// has passed (the daemon starts it after serving, never on the start
/// path), then the store ships, and a write wakes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tender_starts_after_its_wait_and_ships_what_the_wal_gains() {
    let fake = Fake::start();
    let rig = Rig::new(&fake);
    rig.write(0, 5);
    let account = rig.aws.accounts().next().unwrap().clone();
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let still = alive.clone();
    let task = tokio::spawn(durable::run(
        account.clone(),
        Paths::for_store(&rig.store),
        tuning(),
        Hooks::default(),
        move || still.load(std::sync::atomic::Ordering::SeqCst),
    ));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        fake.state.lock().unwrap().seen.is_empty(),
        "a request before START_AFTER"
    );
    assert_eq!(account.status().durability.unwrap().state, "waiting");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while shipped_segment(&fake, 1) != Some(rig.segment(1)) {
        assert!(std::time::Instant::now() < deadline, "never shipped");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // A write wakes it, and its bytes ship.
    rig.write(5, 2);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while rig
        .segments()
        .iter()
        .any(|n| shipped_segment(&fake, *n) != Some(rig.segment(*n)))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the write never shipped"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    alive.store(false, std::sync::atomic::Ordering::SeqCst);
    task.abort();
}
