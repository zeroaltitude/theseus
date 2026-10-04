//! The `hand` role (AWS design §3.3): the job wrapper's contract, inside
//! AWS. `theseusd hand` runs in the hand image, as the main container's
//! command on Fargate and as Lambda's bootstrap, and for one spec it:
//! 1. runs the job's argv in a fresh directory under its own deadline, its
//!    process group stopped (SIGTERM, a grace, SIGKILL) when it passes;
//! 2. streams its output to CloudWatch Logs, `hands/<group>/<index>`;
//! 3. uploads the result (`result.json`: the exit code and what was
//!    uploaded; `output.txt`: the output's tail; and every file under `out/`)
//!    to `s3://<bucket>/hands/<correlation id>/`;
//! 4. sends the completion envelope, signed with its own key, to the queue.
//!
//! The job's environment never carries the spec, so it never sees the key.
//! What it is given: `THESEUS_HAND_INDEX`, `THESEUS_HAND_INPUT`,
//! `THESEUS_HAND_GROUP`, and `THESEUS_HAND_OUT`; and `{index}` and
//! `{input}` in its argv are replaced by them.
//!
//! Its credentials are the role's that runs it: Lambda's in the environment,
//! Fargate's from the container credentials endpoint. It never has the
//! daemon's.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_aws::{Attribution, Call, Client, ClientConfig, Credentials};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use super::envelope::{Envelope, HandSpec, TAIL_BYTES, VERSION};

/// The environment variable a Fargate hand's spec rides in.
pub const SPEC_ENV: &str = "THESEUS_HAND";

/// Where a hand's work directory goes unless `THESEUS_HAND_DIR` says.
const WORK_BASE: &str = "/tmp";

/// The output a hand keeps in memory for `output.txt`.
const KEPT_OUTPUT: usize = 256 * 1024;

/// A file under `out/` bigger than this is named in `result.json`, not
/// uploaded (part 1: one `PutObject` each, read whole).
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// The most files under `out/` it uploads.
const MAX_FILES: usize = 1000;

/// The grace between a deadline's SIGTERM and its SIGKILL.
const GRACE: Duration = Duration::from_secs(5);

/// How long a role's credentials are used before they are fetched again.
const REFRESH: Duration = Duration::from_secs(600);

/// How often its output goes to CloudWatch Logs.
const LOG_EVERY: Duration = Duration::from_secs(1);

/// The role's entry: Lambda's runtime loop when `AWS_LAMBDA_RUNTIME_API` is
/// set, else the one spec in `THESEUS_HAND` (Fargate). Its exit code: 0 once
/// the envelope is sent, else 1.
pub fn main() -> i32 {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("theseus hand: no runtime: {e}");
            return 1;
        }
    };
    rt.block_on(async {
        if let Ok(api) = std::env::var("AWS_LAMBDA_RUNTIME_API") {
            return lambda_loop(&api).await;
        }
        let spec: HandSpec = match std::env::var(SPEC_ENV)
            .map_err(|_| format!("{SPEC_ENV} is not set"))
            .and_then(|s| serde_json::from_str(&s).map_err(|e| format!("{SPEC_ENV}: {e}")))
        {
            Ok(s) => s,
            Err(why) => {
                eprintln!("theseus hand: {why}");
                return 1;
            }
        };
        let task = ecs_task_arn().await;
        match run(&spec, task, &Creds::from_env()).await {
            Ok(e) => {
                eprintln!("theseus hand: {} {}", e.correlation_id, e.outcome);
                0
            }
            Err(why) => {
                eprintln!("theseus hand: {why}");
                1
            }
        }
    })
}

/// Lambda's custom runtime: each invocation's event is a spec, and its
/// answer the envelope's outcome. A hand that could not send its envelope
/// reports an error, so the async invoke's failure destination (the
/// completion queue) says so.
async fn lambda_loop(api: &str) -> i32 {
    let http = match reqwest::Client::builder().no_proxy().build() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("theseus hand: {e}");
            return 1;
        }
    };
    let base = format!("http://{api}/2018-06-01/runtime");
    loop {
        let next = match http.get(format!("{base}/invocation/next")).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("theseus hand: the runtime API: {e}");
                return 1;
            }
        };
        let id = next
            .headers()
            .get("lambda-runtime-aws-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = next.bytes().await.unwrap_or_default();
        let answer = match serde_json::from_slice::<HandSpec>(&body) {
            Err(e) => Err(format!("the event is not a hand's spec: {e}")),
            Ok(spec) => run(&spec, Some(id.clone()), &Creds::from_env()).await,
        };
        let sent = match answer {
            Ok(e) => {
                http.post(format!("{base}/invocation/{id}/response"))
                    .json(&json!({"correlation_id": e.correlation_id, "outcome": e.outcome, "exit_code": e.exit_code}))
                    .send()
                    .await
            }
            Err(why) => {
                http.post(format!("{base}/invocation/{id}/error"))
                    .json(&json!({"errorMessage": why, "errorType": "HandFailed"}))
                    .send()
                    .await
            }
        };
        if let Err(e) = sent {
            eprintln!("theseus hand: the runtime API: {e}");
        }
    }
}

/// The ECS task's ARN, from its metadata endpoint, when it runs in one.
async fn ecs_task_arn() -> Option<String> {
    let uri = std::env::var("ECS_CONTAINER_METADATA_URI_V4").ok()?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let v: Value = http
        .get(format!("{uri}/task"))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    v["TaskARN"].as_str().map(String::from)
}

/// Where a hand's credentials come from: its role's, as AWS hands them to
/// it, refreshed before they expire.
pub struct Creds {
    source: Source,
    cached: Mutex<Option<(Credentials, Instant)>>,
}

enum Source {
    /// Lambda, or a test: the environment's keys.
    Env,
    /// ECS's container credentials endpoint, and its token if any.
    Endpoint(String, Option<String>),
    /// Given: a test's.
    Fixed(Credentials),
}

impl Creds {
    /// ECS's endpoint when the environment names one, else its keys.
    pub fn from_env() -> Creds {
        let source = if let Ok(rel) = std::env::var("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI") {
            Source::Endpoint(format!("http://169.254.170.2{rel}"), None)
        } else if let Ok(full) = std::env::var("AWS_CONTAINER_CREDENTIALS_FULL_URI") {
            Source::Endpoint(
                full,
                std::env::var("AWS_CONTAINER_AUTHORIZATION_TOKEN").ok(),
            )
        } else {
            Source::Env
        };
        Creds {
            source,
            cached: Mutex::default(),
        }
    }

    /// These, always.
    pub fn fixed(c: Credentials) -> Creds {
        Creds {
            source: Source::Fixed(c),
            cached: Mutex::default(),
        }
    }

    async fn get(&self) -> Result<Credentials, String> {
        // A role's credentials last an hour at least: fetched again after
        // ten minutes, they never expire in a hand's hands.
        if let Some((c, at)) = &*self.cached.lock().unwrap() {
            if at.elapsed() < REFRESH {
                return Ok(c.clone());
            }
        }
        let got = match &self.source {
            Source::Fixed(c) => c.clone(),
            Source::Env => {
                let var = |k: &str| std::env::var(k).map_err(|_| format!("{k} is not set"));
                Credentials::new(
                    var("AWS_ACCESS_KEY_ID")?,
                    var("AWS_SECRET_ACCESS_KEY")?,
                    std::env::var("AWS_SESSION_TOKEN").ok(),
                    None,
                )
            }
            Source::Endpoint(url, token) => {
                let http = reqwest::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .map_err(|e| e.to_string())?;
                let mut req = http.get(url);
                if let Some(t) = token {
                    req = req.header("authorization", t);
                }
                let v: Value = req
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|e| format!("the container's credentials: {e}"))?
                    .json()
                    .await
                    .map_err(|e| format!("the container's credentials: {e}"))?;
                let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
                Credentials::new(
                    s("AccessKeyId"),
                    s("SecretAccessKey"),
                    Some(s("Token")),
                    None,
                )
            }
        };
        *self.cached.lock().unwrap() = Some((got.clone(), Instant::now()));
        Ok(got)
    }
}

/// One hand's AWS: its client and credentials, each request attributed to
/// its correlation id.
struct HandAws<'a> {
    client: Client,
    creds: &'a Creds,
    attribution: Attribution,
    region: String,
}

impl HandAws<'_> {
    async fn call(&self, service: &str, operation: &str, input: Value) -> Result<Value, String> {
        let creds = self.creds.get().await?;
        let call = Call {
            service,
            operation,
            input: &input,
            region: Some(&self.region),
            pages: 1,
            attribution: &self.attribution,
        };
        self.client
            .call(&call, &creds)
            .await
            .map(|o| o.body)
            .map_err(|e| format!("{service}:{operation}: {e}"))
    }
}

/// The job's output as it comes: kept for the tail, and batched for the
/// logs.
#[derive(Default)]
struct Output {
    kept: Vec<u8>,
    /// Bytes seen in all.
    total: u64,
    /// Lines not yet in the logs, with their times.
    pending: Vec<(u64, String)>,
    log_failures: u32,
}

impl Output {
    fn take_line(&mut self, line: String) {
        self.total += line.len() as u64 + 1;
        self.kept.extend_from_slice(line.as_bytes());
        self.kept.push(b'\n');
        if self.kept.len() > KEPT_OUTPUT {
            let cut = self.kept.len() - KEPT_OUTPUT;
            self.kept.drain(..cut);
        }
        self.pending.push((theseus_protocol::now_unix_ms(), line));
    }

    /// The last `n` bytes, from a character's start.
    fn tail(&self, n: usize) -> String {
        let from = self.kept.len().saturating_sub(n);
        let skip = self.kept[from..]
            .iter()
            .take(3)
            .take_while(|&&c| c & 0xC0 == 0x80)
            .count();
        String::from_utf8_lossy(&self.kept[from + skip..]).into_owned()
    }
}

/// Ship what is pending to the log stream. A failure is counted, never
/// fatal: the result in S3 holds the output's tail either way.
async fn ship_logs(aws: &HandAws<'_>, spec: &HandSpec, out: &Mutex<Output>) {
    let lines = std::mem::take(&mut out.lock().unwrap().pending);
    if lines.is_empty() {
        return;
    }
    // PutLogEvents takes at most 10,000 events and 1 MiB a batch.
    for chunk in lines.chunks(5_000) {
        let events: Vec<Value> = chunk
            .iter()
            .map(|(t, m)| {
                let mut m = m.clone();
                if m.len() > 200_000 {
                    let mut at = 200_000;
                    while !m.is_char_boundary(at) {
                        at -= 1;
                    }
                    m.truncate(at);
                }
                json!({"timestamp": t, "message": if m.is_empty() { " ".into() } else { m }})
            })
            .collect();
        let r = aws
            .call(
                "logs",
                "PutLogEvents",
                json!({"logGroupName": spec.log_group, "logStreamName": spec.log_stream(), "logEvents": events}),
            )
            .await;
        if r.is_err() {
            out.lock().unwrap().log_failures += 1;
        }
    }
}

/// How the job ended.
struct Ended {
    exit_code: Option<i32>,
    timed_out: bool,
    /// Why it never ran, when it did not.
    spawn_error: Option<String>,
}

/// `{index}` and `{input}` replaced in each word.
fn argv_for(spec: &HandSpec) -> Vec<String> {
    let input = input_text(&spec.input);
    spec.argv
        .iter()
        .map(|w| {
            w.replace("{index}", &spec.index.to_string())
                .replace("{input}", &input)
        })
        .collect()
}

/// A string input as itself; anything else as its JSON; null as nothing.
fn input_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Run the job under its deadline, its output to `out` (and the logs).
#[expect(
    clippy::too_many_lines,
    reason = "the spawn, the pipes, and the deadline's loop: split it"
)]
async fn run_job(spec: &HandSpec, dir: &Path, aws: &HandAws<'_>, out: Arc<Mutex<Output>>) -> Ended {
    let argv = argv_for(spec);
    let mut cmd = tokio::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(dir)
        .env_remove(SPEC_ENV)
        .env("THESEUS_HAND_INDEX", spec.index.to_string())
        .env("THESEUS_HAND_INPUT", input_text(&spec.input))
        .env("THESEUS_HAND_GROUP", &spec.group)
        .env("THESEUS_HAND_OUT", dir.join("out"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let why = format!("could not start {:?}: {e}", argv[0]);
            out.lock().unwrap().take_line(why.clone());
            return Ended {
                exit_code: None,
                timed_out: false,
                spawn_error: Some(why),
            };
        }
    };
    let pid = child.id().map(|p| p as i32);
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(spec.deadline_secs);
    let mut tick = tokio::time::interval(LOG_EVERY);
    let mut timed_out = false;
    let mut status = None;
    let mut pipes_open = true;
    loop {
        tokio::select! {
            line = rx.recv(), if pipes_open => match line {
                Some(l) => out.lock().unwrap().take_line(l),
                None => pipes_open = false,
            },
            s = child.wait(), if status.is_none() => {
                status = Some(s);
            }
            _ = tokio::time::sleep_until(deadline), if status.is_none() && !timed_out => {
                timed_out = true;
                if let Some(p) = pid {
                    // Its whole group: what it started goes with it.
                    unsafe { libc::kill(-p, libc::SIGTERM) };
                    let p2 = p;
                    tokio::spawn(async move {
                        tokio::time::sleep(GRACE).await;
                        unsafe { libc::kill(-p2, libc::SIGKILL) };
                    });
                }
            }
            _ = tick.tick() => ship_logs(aws, spec, &out).await,
        }
        if status.is_some() && !pipes_open {
            break;
        }
        // A job that has exited but left a child holding its pipes: the
        // pipes are not waited for past its group's stop.
        if status.is_some() && timed_out {
            break;
        }
    }
    ship_logs(aws, spec, &out).await;
    let code = match status {
        Some(Ok(s)) => s.code().or_else(|| {
            use std::os::unix::process::ExitStatusExt;
            s.signal().map(|sig| 128 + sig)
        }),
        _ => None,
    };
    Ended {
        exit_code: code,
        timed_out,
        spawn_error: None,
    }
}

/// The files under `out/`, relative, sorted, at most `MAX_FILES`.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => {
                    if let Ok(rel) = p.strip_prefix(root) {
                        found.push(rel.to_path_buf());
                    }
                }
                _ => {}
            }
        }
    }
    found.sort();
    found.truncate(MAX_FILES);
    found
}

/// Upload the result: every file under `out/`, the output, and
/// `result.json` last. What failed, if anything.
async fn upload(
    aws: &HandAws<'_>,
    spec: &HandSpec,
    dir: &Path,
    ended: &Ended,
    output: &Output,
    ms: u64,
) -> Result<(), String> {
    let put = |key: String, body: Value| async move {
        aws.call(
            "s3",
            "PutObject",
            json!({"Bucket": spec.bucket, "Key": key, "Body": body}),
        )
        .await
    };
    let b64 = |b: &[u8]| {
        use base64::Engine as _;
        json!({"base64": base64::engine::general_purpose::STANDARD.encode(b)})
    };
    let (mut files, mut skipped, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    let out_dir = dir.join("out");
    for rel in files_under(&out_dir) {
        let path = out_dir.join(&rel);
        let name = rel.to_string_lossy().into_owned();
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_FILE {
            skipped.push(json!({"file": name, "bytes": size, "why": "over 64 MiB"}));
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => match put(format!("{}out/{name}", spec.prefix()), b64(&bytes)).await {
                Ok(_) => files.push(json!({"file": name, "bytes": size})),
                Err(e) => failed.push(e),
            },
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    if let Err(e) = put(format!("{}output.txt", spec.prefix()), b64(&output.kept)).await {
        failed.push(e);
    }
    let result = json!({
        "correlation_id": spec.correlation_id,
        "group": spec.group,
        "index": spec.index,
        "exit_code": ended.exit_code,
        "timed_out": ended.timed_out,
        "spawn_error": ended.spawn_error,
        "duration_ms": ms,
        "output_bytes": output.total,
        "output_kept_bytes": output.kept.len(),
        "files": files,
        "skipped": skipped,
        "log_failures": output.log_failures,
    });
    if let Err(e) = put(
        format!("{}result.json", spec.prefix()),
        Value::String(result.to_string()),
    )
    .await
    {
        failed.push(e);
    }
    match failed.is_empty() {
        true => Ok(()),
        false => Err(format!("the result's upload failed: {}", failed.join("; "))),
    }
}

/// One hand, start to envelope: the job, its logs, its result, and its
/// signed completion on the queue. The envelope sent, or why none was.
pub async fn run(
    spec: &HandSpec,
    external_op_id: Option<String>,
    creds: &Creds,
) -> Result<Envelope, String> {
    if spec.v != VERSION {
        return Err(format!("a spec of version {}, not {VERSION}", spec.v));
    }
    if spec.argv.is_empty() {
        return Err("the spec has no argv".into());
    }
    let key = spec.key_bytes()?;
    let mut cfg = ClientConfig::new(spec.region.clone());
    cfg.product = format!("theseus-hand/{}", crate::VERSION);
    cfg.endpoint_override = spec.endpoint.clone();
    let aws = HandAws {
        client: Client::new(cfg),
        creds,
        attribution: Attribution {
            execution: None,
            call: Some(spec.correlation_id.clone()),
        },
        region: spec.region.clone(),
    };
    let base = std::env::var("THESEUS_HAND_DIR").unwrap_or_else(|_| WORK_BASE.into());
    let dir = Path::new(&base).join(format!("hand-{}", spec.correlation_id));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Its stream; one that exists already (a retry) is used as it is.
    let _ = aws
        .call(
            "logs",
            "CreateLogStream",
            json!({"logGroupName": spec.log_group, "logStreamName": spec.log_stream()}),
        )
        .await;
    let started_at_ms = theseus_protocol::now_unix_ms();
    let t0 = Instant::now();
    let output = Arc::new(Mutex::new(Output::default()));
    let ended = run_job(spec, &dir, &aws, output.clone()).await;
    let ms = t0.elapsed().as_millis() as u64;
    let finished_at_ms = theseus_protocol::now_unix_ms();
    let out = std::mem::take(&mut *output.lock().unwrap());
    let uploaded = upload(&aws, spec, &dir, &ended, &out, ms).await;
    let _ = std::fs::remove_dir_all(&dir);
    let ok = ended.exit_code == Some(0) && !ended.timed_out;
    let mut e = Envelope {
        v: VERSION,
        correlation_id: spec.correlation_id.clone(),
        group: spec.group.clone(),
        index: spec.index,
        outcome: if ok { "succeeded" } else { "failed" }.into(),
        exit_code: ended.exit_code,
        timed_out: ended.timed_out,
        result_ref: uploaded
            .is_ok()
            .then(|| format!("s3://{}/{}", spec.bucket, spec.prefix())),
        external_op_id,
        started_at_ms,
        finished_at_ms,
        producer: format!("hand:{}", spec.backend),
        tail: out.tail(TAIL_BYTES),
        note: uploaded.err().or(ended.spawn_error),
        signature: String::new(),
    };
    e.sign(&key);
    let body = serde_json::to_string(&e).map_err(|x| x.to_string())?;
    let mut last = String::new();
    for attempt in 0..3u32 {
        match aws
            .call(
                "sqs",
                "SendMessage",
                json!({"QueueUrl": spec.queue_url, "MessageBody": body}),
            )
            .await
        {
            Ok(_) => return Ok(e),
            Err(why) => last = why,
        }
        tokio::time::sleep(Duration::from_millis(500 << attempt)).await;
    }
    Err(format!("the envelope was not sent: {last}"))
}
