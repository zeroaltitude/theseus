//! A fake Jev on 127.0.0.1 (design §2.1, feature `fake`), as the simulator's
//! fake model and fake Discord are: a real HTTP server, so the client's own
//! path runs end to end. Answers are scripted per question id and come back
//! in the verified wire shape; the modes give each failure the client must
//! classify:
//!
//! | mode | what the client sees |
//! |---|---|
//! | `Up` | scripted answers (a valid default for anything unscripted) |
//! | `Down` | the connection closes at once: `network` |
//! | `Slow(d)` | the answer after `d`: a `total` timeout, usage unknown |
//! | `Malformed` | 200 with an answer missing: `malformed` |
//! | `RateLimited` | 429 with `retry-after`: `rate_limited` |
//! | `Status(n)` | any other status with a JSON error body |
//! | `EchoAuth` | 401 whose body repeats the bearer key, as a careless server might |
//! | `Held` | the answer once the test calls [`FakeJev::release`]: lateness by order, not by a delay |
//!
//! It never keeps the key: only whether a bearer token came, and its length.
//! A `HEAD`, the client's warm-up (theseus-otny), is answered 405 as Jev's
//! edge answers it, in every mode but `Down`, and counted apart from calls
//! ([`FakeJev::warmups`]).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum FakeMode {
    Up,
    Down,
    Slow(Duration),
    Malformed,
    RateLimited {
        retry_after_secs: u64,
    },
    Status(u16),
    EchoAuth,
    /// Each call's answer waits for the next [`FakeJev::release`] after it
    /// arrived, then comes as `Up`'s (theseus-biy3).
    Held,
}

/// A scripted answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Scripted {
    /// The option gets `confidence`; the others share the rest.
    Choice {
        option: String,
        confidence: f64,
    },
    /// The level (0-based) gets `confidence`; the others share the rest.
    Score {
        level: usize,
        confidence: f64,
    },
    Noul(f64),
}

/// What the fake saw of one request.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub body: Value,
    /// A bearer token came, of this many characters (never its value).
    pub bearer_len: Option<usize>,
}

/// A per-state script (`FakeJev::script_when`).
#[derive(Debug, Clone)]
struct Rule {
    state_has: String,
    criteria_has: Option<String>,
    question: String,
    answer: Scripted,
}

#[derive(Debug)]
struct Shared {
    mode: FakeMode,
    script: BTreeMap<String, Scripted>,
    rules: Vec<Rule>,
    answer_as: Option<String>,
    seen: Vec<Seen>,
    connections: usize,
    warmups: usize,
    /// [`FakeJev::release`]'s calls so far.
    releases: u64,
}

#[derive(Clone)]
pub struct FakeJev {
    addr: SocketAddr,
    shared: Arc<Mutex<Shared>>,
}

impl FakeJev {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("binding the fake Jev")?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Mutex::new(Shared {
            mode: FakeMode::Up,
            script: BTreeMap::new(),
            rules: Vec::new(),
            answer_as: None,
            seen: Vec::new(),
            connections: 0,
            warmups: 0,
            releases: 0,
        }));
        let s = shared.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let s = s.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &s);
                });
            }
        });
        Ok(Self { addr, shared })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The `api_base` to give the client.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn set_mode(&self, mode: FakeMode) {
        self.lock().mode = mode;
    }

    /// Scripts a question, by its full id (`loop.v1/work_state`) or its own
    /// (`work_state`, which also covers a per-item Noul's `still_member.2`).
    pub fn script(&self, question: &str, answer: Scripted) {
        self.lock().script.insert(question.to_string(), answer);
    }

    /// Scripts a question for some states only (M5 25d's replay tests): a
    /// request whose state's JSON holds `state_has`, and, when given, whose
    /// question's criteria hold `criteria_has`, gets `answer` for
    /// `question` (matched as [`FakeJev::script`] matches). The first rule
    /// that matches wins, before the plain script.
    pub fn script_when(
        &self,
        state_has: &str,
        criteria_has: Option<&str>,
        question: &str,
        answer: Scripted,
    ) {
        self.lock().rules.push(Rule {
            state_has: state_has.to_string(),
            criteria_has: criteria_has.map(str::to_string),
            question: question.to_string(),
            answer,
        });
    }

    /// Answer as another model (to test `model_drift`), or as asked.
    pub fn answer_as(&self, model: Option<&str>) {
        self.lock().answer_as = model.map(str::to_string);
    }

    /// Connections accepted for calls, every mode included; a warm-up's
    /// `HEAD` is counted apart ([`FakeJev::warmups`]).
    pub fn connections(&self) -> usize {
        self.lock().connections
    }

    /// The warm-ups' `HEAD`s received (theseus-otny), every mode included.
    pub fn warmups(&self) -> usize {
        self.lock().warmups
    }

    /// The requests read so far.
    pub fn seen(&self) -> Vec<Seen> {
        self.lock().seen.clone()
    }

    /// Lets every call `Held` holds now answer.
    pub fn release(&self) {
        self.lock().releases += 1;
    }
}

/// The request line, read and counted: a warm-up's `HEAD` apart from calls.
/// Returns whether it is a warm-up, and the mode the request finds.
fn counted(r: &mut BufReader<TcpStream>, shared: &Mutex<Shared>) -> Result<(bool, FakeMode)> {
    let mut first = String::new();
    r.read_line(&mut first)?;
    let warmup = first.starts_with("HEAD ");
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    match warmup {
        true => s.warmups += 1,
        false => s.connections += 1,
    }
    Ok((warmup, s.mode.clone()))
}

/// A warm-up's answer, as Jev's edge gives it: 405, its headers read first
/// so the close sends no reset over the answer.
fn answer_warmup(r: &mut BufReader<TcpStream>, stream: &mut TcpStream) -> Result<()> {
    let mut line = String::new();
    while r.read_line(&mut line)? > 0 && !line.trim_end().is_empty() {
        line.clear();
    }
    let _ = stream.write_all(
        b"HTTP/1.1 405 Method Not Allowed\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
    );
    Ok(())
}

fn serve(mut stream: TcpStream, shared: &Mutex<Shared>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let (warmup, mode) = counted(&mut r, shared)?;
    if mode == FakeMode::Down {
        // Close without a word.
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return Ok(());
    }
    if warmup {
        return answer_warmup(&mut r, &mut stream);
    }
    let mut len = 0usize;
    let mut bearer: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().context("content-length")?;
        }
        if lower.starts_with("authorization:") {
            bearer = l
                .split_once(':')
                .and_then(|(_, v)| v.trim().strip_prefix("Bearer "))
                .map(str::to_string);
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let req: Value = serde_json::from_slice(&body).context("request body")?;
    let (script, answer_as, held) = {
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.seen.push(Seen {
            body: req.clone(),
            bearer_len: bearer.as_ref().map(String::len),
        });
        (
            Script {
                plain: s.script.clone(),
                rules: s.rules.clone(),
            },
            s.answer_as.clone(),
            s.releases,
        )
    };
    if mode == FakeMode::Held {
        while shared.lock().unwrap_or_else(|e| e.into_inner()).releases == held {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let (status, extra, out) = match &mode {
        FakeMode::Up | FakeMode::Down | FakeMode::Held => {
            (200, String::new(), answers(&req, &script, answer_as))
        }
        FakeMode::Slow(d) => {
            std::thread::sleep(*d);
            (200, String::new(), answers(&req, &script, answer_as))
        }
        FakeMode::Malformed => {
            let mut v = answers(&req, &script, answer_as);
            if let Some(a) = v["answers"].as_object_mut() {
                let first = a.keys().next().cloned();
                if let Some(k) = first {
                    a.remove(&k);
                }
            }
            (200, String::new(), v)
        }
        FakeMode::RateLimited { retry_after_secs } => (
            429,
            format!("retry-after: {retry_after_secs}\r\nx-ratelimit-remaining: 0\r\n"),
            json!({"error": {"message": "rate limit exceeded"}}),
        ),
        FakeMode::Status(n) => (
            *n,
            String::new(),
            json!({"error": {"message": format!("status {n}")}}),
        ),
        FakeMode::EchoAuth => (
            401,
            String::new(),
            json!({"error": {"message": format!("invalid key: Bearer {}", bearer.unwrap_or_default())}}),
        ),
    };
    let text = out.to_string();
    let head = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-ratelimit-limit: 600\r\n{extra}connection: close\r\n\r\n",
        if status == 200 { "OK" } else { "Error" },
        text.len()
    );
    // A slow answer may find the client gone; that is its business.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.flush();
    Ok(())
}

/// Everything scripted, read once a request.
struct Script {
    plain: BTreeMap<String, Scripted>,
    rules: Vec<Rule>,
}

/// Whether a scripted key names a question id: the full id, the pack's own
/// id, or that id without a per-item suffix.
fn names(key: &str, id: &str) -> bool {
    let own = id.rsplit('/').next().unwrap_or(id);
    let def = own.split('.').next().unwrap_or(own);
    key == id || key == own || key == def
}

/// The script's answer for a question: the first per-state rule that
/// matches, then the full id, then the pack's own id, then that id without
/// a per-item suffix.
fn scripted<'a>(id: &str, q: &Value, state: &str, script: &'a Script) -> Option<&'a Scripted> {
    let criteria = q["criteria"].to_string();
    if let Some(r) = script.rules.iter().find(|r| {
        names(&r.question, id)
            && state.contains(&r.state_has)
            && r.criteria_has
                .as_deref()
                .is_none_or(|c| criteria.contains(c))
    }) {
        return Some(&r.answer);
    }
    let own = id.rsplit('/').next().unwrap_or(id);
    let def = own.split('.').next().unwrap_or(own);
    script
        .plain
        .get(id)
        .or_else(|| script.plain.get(own))
        .or_else(|| script.plain.get(def))
}

fn spread(n: usize, top: usize, confidence: f64) -> Vec<f64> {
    let c = confidence.clamp(0.0, 1.0);
    let rest = if n > 1 {
        (1.0 - c) / (n - 1) as f64
    } else {
        0.0
    };
    (0..n).map(|i| if i == top { c } else { rest }).collect()
}

/// A valid response, in the verified shape, to every question asked.
fn answers(req: &Value, script: &Script, answer_as: Option<String>) -> Value {
    let mut out = Map::new();
    let questions = req["questions"].as_object().cloned().unwrap_or_default();
    let state = match &req["state"] {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    for (id, q) in &questions {
        let s = scripted(id, q, &state, script);
        let a = match q["type"].as_str() {
            Some("choice") => {
                let options: Vec<String> = q["criteria"]
                    .as_object()
                    .map(|c| c.keys().cloned().collect())
                    .unwrap_or_default();
                let (top, conf) = match s {
                    Some(Scripted::Choice { option, confidence }) => (
                        options.iter().position(|o| o == option).unwrap_or(0),
                        *confidence,
                    ),
                    _ => (0, 0.5),
                };
                let ps = spread(options.len(), top, conf);
                let probabilities: Map<String, Value> = options
                    .iter()
                    .cloned()
                    .zip(ps.iter().map(|p| json!(p)))
                    .collect();
                json!({"type": "choice", "choice": options.get(top).cloned().unwrap_or_default(), "confidence": conf, "probabilities": probabilities})
            }
            Some("score") => {
                let levels: Vec<String> = q["criteria"]
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let (top, conf) = match s {
                    Some(Scripted::Score { level, confidence }) => (*level, *confidence),
                    _ => (0, 0.5),
                };
                let ps = spread(levels.len(), top, conf);
                let score: f64 = ps.iter().enumerate().map(|(i, p)| i as f64 * p).sum();
                let legend: Map<String, Value> = levels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| (i.to_string(), json!(l)))
                    .collect();
                let probabilities: Map<String, Value> = ps
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (i.to_string(), json!(p)))
                    .collect();
                json!({"type": "score", "score": (score * 100.0).round() / 100.0, "confidence": conf, "legend": legend, "probabilities": probabilities})
            }
            _ => {
                let p = match s {
                    Some(Scripted::Noul(p)) => *p,
                    _ => 0.5,
                };
                json!({"type": "noul", "noul": p})
            }
        };
        out.insert(id.clone(), a);
    }
    let n = questions.len() as u64;
    let state_tokens = req["state"].to_string().len().div_ceil(4) as u64;
    json!({
        "model": answer_as.unwrap_or_else(|| req["model"].as_str().unwrap_or_default().to_string()),
        "answers": out,
        "usage": {"input_tokens": n * (state_tokens + 40), "output_tokens": n * 30},
    })
}
