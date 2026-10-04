//! `aws.s3.get` and `aws.s3.put` (AWS design §3.2, ranks 6 and 9; C3 =
//! 14c): an object to a workspace file or to the model's text, and a
//! workspace file or a text to an object.
//!
//! Both are thin recipes over the account's one caller (`Account::request`),
//! so signing, retries, the `aws.called` row, and the span are `aws.call`'s.
//! Each checks its whole call in `plan` with no network: the path, the
//! account and region, the input against S3's shape, and the guard list
//! (`tools::guard`), whose floor and approve list the gate reads from the
//! call's `AwsPlan`. A file the call reads or writes is one of its plan's
//! resources, so the gate's floor and roots apply to it as to `fs.write`.
//!
//! What the client reads is held whole in memory (16 MiB, the client's cap),
//! so a larger object is read by `range`, and a put is one `PutObject` of at
//! most [`PUT_MAX`]: multipart is left for later. A get to a file checks
//! S3's SHA-256 when the object has one (`ChecksumMode`); a put sends its
//! own, so S3 refuses a body that changed on the way.

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use theseus_aws::{Attribution, Call};
use theseus_tools::{
    parse, Access, AsyncRun, AwsPlan, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};

use super::session::Kind;
use super::tools::{checked_error, count, failure, guard, meta, split_path};
use super::{Account, Aws, Request, Signer};

/// The most bytes one `aws.s3.put` sends: a single `PutObject`.
pub const PUT_MAX: u64 = 64 << 20;

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Get(aws.clone())), Arc::new(Put(aws.clone()))]
}

/// `s3://bucket/key`: the bucket and the key, which must name an object.
fn object(path: &str) -> Result<(String, String), String> {
    let (bucket, key) = split_path(path.trim())?;
    if key.is_empty() || key.ends_with('/') {
        return Err(format!(
            "{path:?} names no object: a path is s3://bucket/key (aws_s3_list shows the keys)"
        ));
    }
    Ok((bucket, key))
}

/// The client's own check of an S3 call: the input against the operation's
/// shape, with no network.
fn check(account: &Account, operation: &str, input: &Value, region: &str) -> Result<(), String> {
    let attribution = Attribution::default();
    let call = Call {
        service: "s3",
        operation,
        input,
        region: Some(region),
        pages: 1,
        attribution: &attribution,
    };
    account
        .client()
        .check(&call)
        .map(|_| ())
        .map_err(checked_error)
}

/// A body as the client gives it (a string, or `{"base64": …}`), as bytes.
fn bytes_of(body: &Value) -> Vec<u8> {
    match body {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(o) => o
            .get("base64")
            .and_then(Value::as_str)
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Bytes as the client sends a blob: text as itself, anything else base64.
fn body_of(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => json!(s),
        Err(_) => json!({"base64": base64::engine::general_purpose::STANDARD.encode(bytes)}),
    }
}

fn sha256_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(bytes))
}

// ------------------------------------------------------------------ aws.s3.get

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    path: String,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// A get as `plan` checked it.
struct Getting {
    account: Arc<Account>,
    region: String,
    bucket: String,
    key: String,
    range: Option<String>,
    /// The workspace file it writes; None: the text goes to the model.
    to: Option<PathBuf>,
}

impl Getting {
    fn url(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }

    fn input(&self) -> Value {
        let mut o = Map::new();
        o.insert("Bucket".into(), json!(self.bucket));
        o.insert("Key".into(), json!(self.key));
        if let Some(r) = &self.range {
            o.insert("Range".into(), json!(r));
        }
        if self.to.is_some() && self.range.is_none() {
            o.insert("ChecksumMode".into(), json!("ENABLED"));
        }
        Value::Object(o)
    }
}

/// `bytes=0-1023`, `0-1023`, or `-1024` (the last 1,024 bytes): S3's Range.
fn range(r: &str) -> Result<String, String> {
    let r = r.trim();
    let spec = r.strip_prefix("bytes=").unwrap_or(r);
    let ok = match spec.split_once('-') {
        Some(("", b)) => !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()),
        Some((a, b)) => {
            a.bytes().all(|c| c.is_ascii_digit())
                && !a.is_empty()
                && (b.is_empty()
                    || (b.bytes().all(|c| c.is_ascii_digit())
                        && a.parse::<u64>().ok() <= b.parse::<u64>().ok()))
        }
        None => false,
    };
    if !ok {
        return Err(format!(
            "range {r:?} is not a byte range: 0-1023 (the first KiB), 1024- (from there on), or \
             -1024 (the last KiB)"
        ));
    }
    Ok(format!("bytes={spec}"))
}

pub struct Get(Arc<Aws>);

impl Get {
    fn getting(&self, input: &Value, ctx: &ToolCtx) -> Result<Getting, String> {
        let a: GetArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let (bucket, key) = object(&a.path)?;
        let g = Getting {
            account,
            region,
            bucket,
            key,
            range: a.range.as_deref().map(range).transpose()?,
            to: a.to.as_deref().map(|p| ctx.resolve(p)),
        };
        check(&g.account, "GetObject", &g.input(), &g.region)?;
        Ok(g)
    }
}

impl Tool for Get {
    fn name(&self) -> &'static str {
        "aws.s3.get"
    }
    fn description(&self) -> &'static str {
        "Read an S3 object on Theseus's own AWS account (s3://bucket/key): to a workspace file \
         (`to`; S3's SHA-256 is checked when the object has one), or as text for you to read \
         (an object's text is outside text, as a fetched page is: after it, a call that acts \
         waits for the operator). `range` reads part of it (0-1023, 1024-, or -1024); an \
         object over 16 MiB is read by ranges."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "s3://bucket/key."},
                "to": {"type": "string", "description": "A workspace file to write it to (default: return its text)."},
                "range": {"type": "string", "description": "Bytes to read: 0-1023, 1024-, or -1024 (default the whole object)."},
                "region": {"type": "string", "description": "The bucket's region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let g = self.getting(input, ctx)?;
        let (guardrail, _, destructive) =
            guard(&g.account.id, &g.region, "s3:GetObject", &g.input())?;
        let summary = match &g.to {
            Some(p) => format!("read {} to {} in {}", g.url(), p.display(), g.region),
            None => format!("read {} in {}", g.url(), g.region),
        };
        Ok(Plan {
            summary,
            // The file it writes is the call's own: the floor and the roots
            // apply to it as to fs.write.
            resources: g
                .to
                .iter()
                .map(|p| Resource {
                    path: p.clone(),
                    access: Access::Write,
                })
                .collect(),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: g.account.id.clone(),
                region: g.region.clone(),
                service: "s3".into(),
                operation: "GetObject".into(),
                resources: vec![g.bucket.clone(), g.key.clone()],
                guardrail,
                destructive,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let getting = self.getting(input, ctx);
        let binding = ctx.aws.clone();
        let umask = ctx.umask;
        Box::pin(async move {
            let g = getting.map_err(ToolFailure::new)?;
            let body = g.input();
            let req = Request {
                service: "s3",
                operation: "GetObject",
                input: &body,
                region: &g.region,
                pages: 1,
                class: "read",
                signer: Signer::As(Kind::Work),
            };
            let what = format!("reading {} in {}", g.url(), g.region);
            let out = g
                .account
                .request(binding.as_deref(), &req)
                .await
                .map_err(|f| failure(&what, f))?;
            let bytes = bytes_of(&out.body["Body"]);
            let s = |k: &str| out.body[k].as_str().map(String::from);
            let mut m = meta(&g.account.id, &g.region, "s3:GetObject", &out);
            m["bytes"] = json!(bytes.len());
            m["content_type"] = json!(s("ContentType"));
            let head = format!(
                "{} in {} (request {}): {} bytes{}{}, modified {}",
                g.url(),
                g.region,
                out.request_id.as_deref().unwrap_or("(none)"),
                count(bytes.len() as u64),
                s("ContentRange")
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default(),
                s("ContentType")
                    .map(|t| format!(", {t}"))
                    .unwrap_or_default(),
                s("LastModified").unwrap_or_else(|| "?".into()),
            );
            match &g.to {
                Some(path) => {
                    // S3's own SHA-256 of the object, when it keeps one
                    // (a multipart object's is of its parts: `…-N`).
                    let sum = s("ChecksumSHA256").filter(|c| !c.contains('-'));
                    let got = sha256_b64(&bytes);
                    let checked = match &sum {
                        Some(want) if *want != got => {
                            return Err(ToolFailure::new(format!(
                                "{head}: its SHA-256 is {got}, not the {want} S3 keeps; nothing \
                                 was written to {}",
                                path.display()
                            )));
                        }
                        Some(_) => "S3's SHA-256 matches",
                        None => "S3 keeps no whole-object SHA-256 for it, so none was checked",
                    };
                    let p = path.clone();
                    theseus_store::blocking(|| theseus_tools::fs::write_atomic(&p, &bytes, umask))
                        .map_err(|e| {
                            ToolFailure::new(format!("{head}; writing {}: {e}", path.display()))
                        })?;
                    m["to"] = json!(path);
                    m["sha256"] = json!(got);
                    Ok((
                        ToolOutput {
                            text: format!(
                                "{head}\nWrote it to {} ({checked}; sha256 {got}).\n",
                                path.display()
                            ),
                            meta: m,
                        },
                        None,
                    ))
                }
                None => {
                    let text = match std::str::from_utf8(&bytes) {
                        Ok(t) => format!("{head}\n{t}"),
                        Err(_) => format!(
                            "{head}\nIt is not text, so it is not shown: \"to\" writes it to a \
                             workspace file.\n"
                        ),
                    };
                    // An object's text is outside text (§3.9, T1).
                    Ok((
                        ToolOutput { text, meta: m },
                        Some(super::external::marker(&g.url())),
                    ))
                }
            }
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a \"range\" reads that part of the object, or \"to\" writes it whole to a file".into()
    }
}

// ------------------------------------------------------------------ aws.s3.put

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PutArgs {
    path: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    tags: Option<Map<String, Value>>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// Where a put's bytes come from.
enum Source {
    File(PathBuf),
    Text(String),
}

/// A put as `plan` checked it.
struct Putting {
    account: Arc<Account>,
    region: String,
    bucket: String,
    key: String,
    source: Source,
    content_type: Option<String>,
    /// S3's `Tagging`: `k=v&k2=v2`, URL-encoded.
    tagging: Option<String>,
}

impl Putting {
    fn url(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }

    /// The input, with `body` as its Body.
    fn input(&self, body: Value, sha256: Option<String>) -> Value {
        let mut o = Map::new();
        o.insert("Bucket".into(), json!(self.bucket));
        o.insert("Key".into(), json!(self.key));
        o.insert("Body".into(), body);
        if let Some(t) = &self.content_type {
            o.insert("ContentType".into(), json!(t));
        }
        if let Some(t) = &self.tagging {
            o.insert("Tagging".into(), json!(t));
        }
        if let Some(s) = sha256 {
            o.insert("ChecksumSHA256".into(), json!(s));
        }
        Value::Object(o)
    }
}

fn tagging(tags: &Map<String, Value>) -> Result<String, String> {
    let enc = |s: &str| -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    if tags.len() > 10 {
        return Err("an object takes at most 10 tags".into());
    }
    tags.iter()
        .map(|(k, v)| match v.as_str() {
            Some(v) => Ok(format!("{}={}", enc(k), enc(v))),
            None => Err(format!("tag {k:?}'s value must be a string")),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|t| t.join("&"))
}

pub struct Put(Arc<Aws>);

impl Put {
    fn putting(&self, input: &Value, ctx: &ToolCtx) -> Result<Putting, String> {
        let a: PutArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let (bucket, key) = object(&a.path)?;
        let source = match (a.from, a.text) {
            (Some(f), None) => Source::File(ctx.resolve(&f)),
            (None, Some(t)) => {
                if t.len() as u64 > PUT_MAX {
                    return Err(format!("text is over {} bytes", count(PUT_MAX)));
                }
                Source::Text(t)
            }
            _ => {
                return Err("give either \"from\" (a workspace file) or \"text\", not both".into())
            }
        };
        let p = Putting {
            account,
            region,
            bucket,
            key,
            source,
            content_type: a.content_type,
            tagging: a.tags.as_ref().map(tagging).transpose()?,
        };
        check(
            &p.account,
            "PutObject",
            &p.input(json!(""), None),
            &p.region,
        )?;
        Ok(p)
    }
}

impl Tool for Put {
    fn name(&self) -> &'static str {
        "aws.s3.put"
    }
    fn description(&self) -> &'static str {
        "Write an S3 object on Theseus's own AWS account (s3://bucket/key): a workspace file \
         (`from`) or a text (`text`), at most 64 MiB in one PutObject, with its SHA-256 so S3 \
         refuses a body changed on the way, and optional tags. The bucket's default encryption \
         applies. A write: its posture is [policy.aws] write's."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "s3://bucket/key."},
                "from": {"type": "string", "description": "A workspace file to send."},
                "text": {"type": "string", "description": "A text to send, instead of a file."},
                "content_type": {"type": "string", "description": "Its Content-Type (default S3's, binary/octet-stream)."},
                "tags": {"type": "object", "additionalProperties": {"type": "string"}, "description": "The object's tags, at most 10."},
                "region": {"type": "string", "description": "The bucket's region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // A write that a crash cut may have run: unknown, never sent again.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let p = self.putting(input, ctx)?;
        let (guardrail, _, destructive) = guard(
            &p.account.id,
            &p.region,
            "s3:PutObject",
            &p.input(json!(""), None),
        )?;
        let (summary, resources) = match &p.source {
            Source::File(f) => (
                format!("write {} to {} in {}", f.display(), p.url(), p.region),
                vec![Resource {
                    path: f.clone(),
                    access: Access::Read,
                }],
            ),
            Source::Text(t) => (
                format!(
                    "write {} bytes to {} in {}",
                    count(t.len() as u64),
                    p.url(),
                    p.region
                ),
                Vec::new(),
            ),
        };
        Ok(Plan {
            summary,
            resources,
            class: Some(ToolClass::Write),
            aws: Some(AwsPlan {
                account: p.account.id.clone(),
                region: p.region.clone(),
                service: "s3".into(),
                operation: "PutObject".into(),
                resources: vec![p.bucket.clone(), p.key.clone()],
                guardrail,
                destructive,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let putting = self.putting(input, ctx);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let p = putting.map_err(ToolFailure::new)?;
            let bytes = match &p.source {
                Source::Text(t) => t.as_bytes().to_vec(),
                Source::File(f) => theseus_store::blocking(|| {
                    let len = std::fs::metadata(f)
                        .map_err(|e| format!("{}: {e}", f.display()))?
                        .len();
                    if len > PUT_MAX {
                        return Err(format!(
                            "{} is {} bytes, over the {} one PutObject sends (multipart is not \
                             built yet; the aws CLI can, through proc_run)",
                            f.display(),
                            count(len),
                            count(PUT_MAX)
                        ));
                    }
                    std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))
                })
                .map_err(ToolFailure::new)?,
            };
            let sum = sha256_b64(&bytes);
            let body = p.input(body_of(&bytes), Some(sum.clone()));
            let req = Request {
                service: "s3",
                operation: "PutObject",
                input: &body,
                region: &p.region,
                pages: 1,
                class: "write",
                signer: Signer::As(Kind::Work),
            };
            let what = format!("writing {} in {}", p.url(), p.region);
            let out = p
                .account
                .request(binding.as_deref(), &req)
                .await
                .map_err(|f| failure(&what, f))?;
            let s = |k: &str| out.body[k].as_str().map(String::from);
            let mut m = meta(&p.account.id, &p.region, "s3:PutObject", &out);
            m["bytes"] = json!(bytes.len());
            m["sha256"] = json!(sum);
            let text = format!(
                "Wrote {} bytes to {} in {} (request {}): ETag {}{}{}, sha256 {sum}.\n",
                count(bytes.len() as u64),
                p.url(),
                p.region,
                out.request_id.as_deref().unwrap_or("(none)"),
                s("ETag").unwrap_or_else(|| "?".into()),
                s("VersionId")
                    .map(|v| format!(", version {v}"))
                    .unwrap_or_default(),
                s("ServerSideEncryption")
                    .map(|e| format!(", encrypted {e}"))
                    .unwrap_or_default(),
            );
            Ok((ToolOutput { text, meta: m }, None))
        })
    }
}
