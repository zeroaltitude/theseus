//! The Discord proof (theseus-9kjv): everything theseus-kl8m asked a person
//! to do on Discord, done against a real `theseusd` by a stand-in for
//! Discord, in one command (`theseus-sim discord proof`), and in the gate
//! (theseusd's `tests/discord_proof.rs`).
//!
//! The daemon runs on a scratch state dir with its own config: its Discord
//! binding's REST and gateway on `fake_discord` (no Discord, no bot token: a
//! fake `op` answers every secret with an invented value), and its model on
//! a stand-in that answers by script. Nothing leaves the machine.
//!
//! The steps: the daemon serves; the gateway identifies; the binding binds
//! `#lab` and ana's DM; the viewer check trusts `#lab`, which only ana and
//! ben can view (theseus-ck0k); ana types a message in `#lab` and its reply
//! answers it; ben, whom `#lab` doesn't list, is ignored; ana asks for a
//! write to a path on the approve list, and the call waits with its card in
//! `#lab`, naming ana; ben's press of Approve is refused, and the call keeps
//! waiting; ana's press is acknowledged, the call runs, the card says so and
//! loses its buttons, and the reply comes; the daemon stops cleanly, with no
//! binding error and no token in anything the stand-in kept. Then the
//! transcript: what the binding posted, read back from the stand-in
//! (theseus-qifw).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::fake_discord::{FakeDiscord, Guild, Msg, Pressed, Typed, BOT_ID, DEFAULT_GUILD};

/// Invented people and places.
pub const ANA: u64 = 900_000_000_000_000_101;
pub const BEN: u64 = 900_000_000_000_000_202;
pub const CY: u64 = 900_000_000_000_000_303;
pub const LAB: u64 = 900_000_000_000_000_010;

/// What every secret resolves to: the bot's token among them.
const SECRET: &str = "proof-secret-value-not-a-token";
/// The word that makes the stand-in model ask for the write.
const WRITE_WORD: &str = "PROOF-WRITE";
/// The word that makes it ask for an `fs.read` of `notes.txt` in the
/// projects directory (M4 19a's live check); its answer then begins with
/// what the read gave it, so a withheld result shows in the reply.
const READ_WORD: &str = "PROOF-READ";
const READ_ID: &str = "toolu_proof_read";
/// M4 19c's live check: `PROOF-GROW` reads `notes.txt` as `PROOF-READ` does,
/// and while it answers, opens every channel of the rig's `guild.json` to the
/// whole guild, so the channel gains a viewer mid-turn; `PROOF-SAY` answers
/// with the start of a published item its request carries (the place rule).
const GROW_WORD: &str = "PROOF-GROW";
const SAY_WORD: &str = "PROOF-SAY";
const READY_TEXT: &str = "ready";
const DONE_TEXT: &str = "Done: the proof file is written.";
const WRITTEN: &str = "written through the Discord stand-in";
/// How long one step may wait.
const STEP: Duration = Duration::from_secs(30);

pub struct Opts {
    pub theseusd: PathBuf,
    /// Work here and keep it; default: a temporary directory, removed.
    pub dir: Option<PathBuf>,
    /// Print each step as it ends.
    pub verbose: bool,
}

pub struct Step {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    pub ms: f64,
}

pub struct Report {
    pub steps: Vec<Step>,
    /// What the stand-in holds at the end: every message and answer.
    pub transcript: Vec<String>,
    pub ms: f64,
}

impl Report {
    pub fn passed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.ok)
    }

    /// Each step, then the summary.
    pub fn render(&self) -> String {
        let steps: String = self.steps.iter().map(step_line).collect();
        steps + &self.summary()
    }

    /// How many steps passed, and the transcript.
    pub fn summary(&self) -> String {
        let ok = self.steps.iter().filter(|s| s.ok).count();
        let mut out = format!(
            "{ok} of {} steps passed in {:.1} s\n\nWhat the stand-in holds (theseus-qifw):\n",
            self.steps.len(),
            self.ms / 1000.0
        );
        for l in &self.transcript {
            out.push_str(&format!("  {l}\n"));
        }
        out
    }
}

fn step_line(s: &Step) -> String {
    format!(
        "{} {} ({:.0} ms): {}\n",
        if s.ok { "PASS" } else { "FAIL" },
        s.name,
        s.ms,
        s.detail
    )
}

/// The stand-in for the Messages API: a request whose last message carries
/// a tool's result ends the turn with `DONE_TEXT` (a read's with `Read:` and
/// the start of what it got); one whose prompt holds `WRITE_WORD` asks for
/// an `fs.write` of `write` (its `path` and `content`), and `READ_WORD` for
/// an `fs.read` of `notes.txt`; anything else is answered `READY_TEXT`.
/// `theseus-sim discord model` serves it alone, for a live check.
pub struct Model {
    /// `127.0.0.1:<port>`; `[model] api_base` is its `http://` URL.
    pub addr: String,
}

impl Model {
    fn start(write: Value) -> Result<Self> {
        Self::start_on("127.0.0.1:0", write)
    }

    /// Listen on `addr`; answer until the process ends.
    pub fn start_on(addr: &str, write: Value) -> Result<Self> {
        let listener = TcpListener::bind(addr).context("binding the model stand-in")?;
        let addr = listener.local_addr()?.to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let write = write.clone();
                std::thread::spawn(move || {
                    let _ = answer_model(stream, &write);
                });
            }
        });
        Ok(Self { addr })
    }
}

/// The write the model asks for: the proof's file, under `dir`'s `outside/`.
pub fn write_input(dir: &Path) -> Value {
    let path = dir.join("outside").join("proof.txt");
    json!({"path": path.display().to_string(), "content": WRITTEN})
}

fn answer_model(mut stream: TcpStream, write: &Value) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut len = 0usize;
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
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().context("content-length")?;
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let req: Value = serde_json::from_slice(&body)?;
    let model = req["model"].as_str().unwrap_or("claude-sonnet-5-5");
    let last = req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .cloned()
        .unwrap_or_default();
    let blocks = last["content"].as_array().cloned().unwrap_or_default();
    let result = blocks.iter().find(|b| b["type"] == "tool_result");
    let events = match result {
        Some(r) if r["tool_use_id"] == READ_ID => {
            if req["messages"].to_string().contains(GROW_WORD) {
                open_the_guild(write);
            }
            let got: String = r["content"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(160)
                .collect();
            sse_text(model, &format!("Read: {got}"))
        }
        Some(_) => sse_text(model, DONE_TEXT),
        None if last.to_string().contains(WRITE_WORD) => sse_write(model, write),
        None if last.to_string().contains(READ_WORD) => sse_read(model),
        None if last.to_string().contains(GROW_WORD) => sse_read(model),
        None if last.to_string().contains(SAY_WORD) => sse_text(model, &said(&req)),
        None => sse_text(model, READY_TEXT),
    };
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
    );
    for e in events {
        out.push_str(&format!(
            "event: {}\ndata: {e}\n\n",
            e["type"].as_str().unwrap_or("")
        ));
    }
    stream.write_all(out.as_bytes())?;
    Ok(stream.flush()?)
}

/// Open every channel of the rig's `guild.json` to the whole guild (M4 19c):
/// the fake reads the file again at its next request. The rig's directory is
/// the write's, two levels up (`write_input`).
fn open_the_guild(write: &Value) {
    let Some(dir) = write["path"]
        .as_str()
        .map(Path::new)
        .and_then(|p| p.parent()?.parent())
    else {
        return;
    };
    let path = dir.join("guild.json");
    let Some(mut g) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return;
    };
    for c in g["channels"].as_array_mut().into_iter().flatten() {
        c["overwrites"] = json!([]);
    }
    let _ = std::fs::write(&path, g.to_string());
}

/// What `PROOF-SAY` answers: the start of the published item its request
/// carries, or that it carries none (the place rule's publish).
fn said(req: &Value) -> String {
    let published = req["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|b| b["text"].as_str().map(str::to_string))
        .find(|t| t.starts_with("[Published here by the owner"));
    match published {
        Some(t) => format!("Said: {}", t.chars().take(220).collect::<String>()),
        None => "Said: nothing published is in my context".into(),
    }
}

fn sse(model: &str, block: Value, delta: Value, stop: &str) -> Vec<Value> {
    vec![
        json!({"type": "message_start", "message": {"id": "msg_proof", "type": "message", "role": "assistant", "model": model, "content": [], "usage": {"input_tokens": 50, "output_tokens": 1}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": block}),
        json!({"type": "content_block_delta", "index": 0, "delta": delta}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": stop}, "usage": {"output_tokens": 12}}),
        json!({"type": "message_stop"}),
    ]
}

fn sse_text(model: &str, text: &str) -> Vec<Value> {
    sse(
        model,
        json!({"type": "text", "text": ""}),
        json!({"type": "text_delta", "text": text}),
        "end_turn",
    )
}

fn sse_write(model: &str, input: &Value) -> Vec<Value> {
    sse(
        model,
        json!({"type": "tool_use", "id": "toolu_proof_write", "name": "fs_write", "input": {}}),
        json!({"type": "input_json_delta", "partial_json": input.to_string()}),
        "tool_use",
    )
}

fn sse_read(model: &str) -> Vec<Value> {
    sse(
        model,
        json!({"type": "tool_use", "id": READ_ID, "name": "fs_read", "input": {}}),
        json!({"type": "input_json_delta", "partial_json": json!({"path": "notes.txt"}).to_string()}),
        "tool_use",
    )
}

/// The fake `op`: every reference resolves to `SECRET`.
fn fake_op() -> String {
    format!(
        "#!/bin/sh\n\
         # The Discord proof's stand-in for 1Password's op (theseus-sim): every\n\
         # reference resolves to one invented value; nothing is read.\n\
         case \"$1\" in\n\
         \x20 inject) sed -e 's/{{{{ [^}}]* }}}}/{SECRET}/g' ;;\n\
         \x20 read) printf '%s' {SECRET} ;;\n\
         \x20 *) echo \"fake op: $1 is not supported\" >&2; exit 1 ;;\n\
         esac\n"
    )
}

/// Where a scratch daemon finds its stand-ins, each `host:port`: Discord's
/// REST, its gateway, and the model.
pub struct Ends<'a> {
    pub rest: &'a str,
    pub gateway: &'a str,
    pub model: &'a str,
}

/// Lay out `dir` for a scratch daemon of `theseusd` on the stand-ins at
/// `ends`: `config.toml`, the fake `op` in `bin/`, `state/bindings.toml`,
/// `guild.json` (for `theseus-sim fake-discord --guild`), and `outside/`,
/// where the proof's write goes.
pub fn lay_out(dir: &Path, theseusd: &Path, ends: &Ends<'_>) -> Result<()> {
    for d in ["bin", "projects", "state", "outside"] {
        std::fs::create_dir_all(dir.join(d))?;
    }
    let text = config(theseusd, ends, &dir.join("projects"))?;
    std::fs::write(dir.join("config.toml"), text)?;
    std::fs::write(dir.join("state").join("bindings.toml"), bindings())?;
    std::fs::write(
        dir.join("guild.json"),
        serde_json::to_string_pretty(&guild())?,
    )?;
    let op = dir.join("bin").join("op");
    std::fs::write(&op, fake_op())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// `theseusd` on a laid-out `dir`: its config, socket, and state dir there,
/// the fake `op` first on its PATH, and none of the operator's settings.
pub fn daemon_command(theseusd: &Path, dir: &Path) -> Command {
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut c = Command::new(theseusd);
    c.arg("--config")
        .arg(dir.join("config.toml"))
        .arg("--socket")
        .arg(dir.join("sock"))
        .arg("--state-dir")
        .arg(dir.join("state"))
        .env("PATH", path)
        .env("OP_SERVICE_ACCOUNT_TOKEN", "proof-not-a-token")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_CONFIG")
        .env_remove("THESEUS_STATE_DIR")
        .env_remove("THESEUS_SOCKET");
    c
}

/// The daemon's config: the template, with every endpoint on a stand-in,
/// every secret on the fake `op`, the web UI off, and `[approval]` trusting
/// ana and ben in the CLI, a DM, and `#lab`.
fn config(theseusd: &Path, ends: &Ends<'_>, projects: &Path) -> Result<String> {
    let model = format!("http://{}", ends.model);
    let model = model.as_str();
    let out = Command::new(theseusd)
        .args(["example-config", "--plain"])
        .output()
        .with_context(|| format!("running {} example-config", theseusd.display()))?;
    let mut t: toml::Table = String::from_utf8(out.stdout)?.parse()?;
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .expect("a table")
    }
    table(&mut t, "model").insert("api_base".into(), model.into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        if let Some(p) = p.as_table_mut() {
            p.insert("api_base".into(), model.into());
        }
    }
    let secrets = table(&mut t, "secrets");
    let names: Vec<String> = secrets
        .keys()
        .filter(|k| *k != "github_token")
        .cloned()
        .collect();
    secrets.clear();
    for n in names {
        secrets.insert(n.clone(), format!("op://Proof/{n}/credential").into());
    }
    let discord = table(&mut t, "discord");
    discord.insert("enabled".into(), true.into());
    discord.insert("rest_proxy".into(), ends.rest.into());
    discord.insert(
        "gateway_proxy".into(),
        format!("ws://{}", ends.gateway).into(),
    );
    discord.insert("edit_interval_ms".into(), 250.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    table(&mut t, "tools").insert("projects_dir".into(), projects.display().to_string().into());
    // A write outside the roots takes its tool's posture (theseus-ewi), so
    // the approve list is what makes the proof's write wait for its card.
    let tools = table(&mut t, "tools");
    let mut approve: Vec<toml::Value> = tools
        .get("approve_paths")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    approve.push(
        projects
            .with_file_name("outside")
            .display()
            .to_string()
            .into(),
    );
    tools.insert("approve_paths".into(), approve.into());
    table(&mut t, "policy").insert("enforcement".into(), "notify".into());
    let approval = table(&mut t, "approval");
    approval.insert(
        "trusted_users".into(),
        vec![format!("discord:{ANA}"), format!("discord:{BEN}")].into(),
    );
    approval.insert(
        "channels".into(),
        vec![
            "cli".to_string(),
            "discord:dm".into(),
            format!("discord:{LAB}"),
        ]
        .into(),
    );
    Ok(toml::to_string(&t)?)
}

/// `#lab`, where ana may drive Theseus, and ana's DM.
pub fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// ana owns the guild; ben and cy are members; `#lab` is private to ana and
/// ben, so cy, who is not trusted, cannot view it.
pub fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA, BEN])
}

/// A spawned daemon, killed and reaped when dropped.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Rig {
    dir: PathBuf,
    fake: Arc<FakeDiscord>,
    daemon: Option<Daemon>,
    steps: Vec<Step>,
    verbose: bool,
    /// The typed message the reply answers, and the card.
    typed: String,
    card: String,
}

impl Rig {
    fn sock(&self) -> PathBuf {
        self.dir.join("sock")
    }

    fn written(&self) -> PathBuf {
        self.dir.join("outside").join("proof.txt")
    }

    /// One JSON-RPC call on the daemon's socket.
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let s = UnixStream::connect(self.sock())?;
        s.set_read_timeout(Some(Duration::from_secs(30)))?;
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line?)?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => bail!("{method}: {e}"),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        bail!("{method}: the connection closed")
    }

    fn health(&self) -> Value {
        self.call("health", Value::Null).unwrap_or_default()
    }

    fn binding(&self) -> Value {
        self.health()["bindings"][0].clone()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self
            .call("ledger.tail", json!({"n": 2000, "kind": kind}))
            .unwrap_or_default();
        t["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    fn waiting(&self) -> usize {
        let l = self.call("confirm.list", json!({})).unwrap_or_default();
        l["confirms"]
            .as_array()
            .or_else(|| l.as_array())
            .map_or(0, Vec::len)
    }

    fn posted(&self, channel: u64) -> Vec<Msg> {
        self.fake
            .messages(channel)
            .into_iter()
            .filter(|m| m.author == BOT_ID.to_string())
            .collect()
    }

    fn card_msg(&self) -> Option<Msg> {
        self.posted(LAB)
            .into_iter()
            .find(|m| m.versions[0].contains("**Approve?**"))
    }

    fn log_tail(&self) -> String {
        let log = std::fs::read_to_string(self.dir.join("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        lines[lines.len().saturating_sub(15)..].join("\n")
    }

    /// Run one step: `f` says what it saw, or why it failed.
    fn step(
        &mut self,
        name: &'static str,
        f: impl FnOnce(&mut Self) -> Result<String, String>,
    ) -> bool {
        let t0 = Instant::now();
        let r = f(self);
        let s = Step {
            name,
            ok: r.is_ok(),
            detail: r.unwrap_or_else(|e| e),
            ms: t0.elapsed().as_secs_f64() * 1000.0,
        };
        if self.verbose {
            print!("{}", step_line(&s));
        }
        let ok = s.ok;
        self.steps.push(s);
        ok
    }
}

/// Poll `f` every 25 ms until it gives a value, for up to `STEP`.
fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if t0.elapsed() > STEP {
            return Err(format!("{what}: not within {} s", STEP.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Run the proof against `o.theseusd`.
pub fn run(o: &Opts) -> Result<Report> {
    let t0 = Instant::now();
    let tmp = tempfile::tempdir()?;
    let dir = o.dir.clone().unwrap_or_else(|| tmp.path().to_path_buf());
    let fake = FakeDiscord::start_with_gateway();
    fake.set_guild(guild());
    let gateway = fake
        .gateway()
        .context("the stand-in's gateway")?
        .addr
        .clone();
    let model = Model::start(write_input(&dir))?;
    let ends = Ends {
        rest: &fake.addr,
        gateway: &gateway,
        model: &model.addr,
    };
    lay_out(&dir, &o.theseusd, &ends)?;
    let mut rig = Rig {
        dir,
        fake,
        daemon: None,
        steps: Vec::new(),
        verbose: o.verbose,
        typed: String::new(),
        card: String::new(),
    };
    let _ = steps(&mut rig, &o.theseusd);
    let transcript = transcript(&rig.fake);
    let steps = std::mem::take(&mut rig.steps);
    drop(rig);
    Ok(Report {
        steps,
        transcript,
        ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}

/// The steps, in order; the first that fails ends the run.
fn steps(r: &mut Rig, theseusd: &Path) -> Option<()> {
    r.step("the daemon serves", |r| start(r, theseusd))
        .then_some(())?;
    r.step("the gateway identifies", |r| {
        let gw = r.fake.gateway().expect("a gateway").clone();
        if gw.wait_connected(STEP) {
            Ok(format!("{:?}", gw.state().sent))
        } else {
            Err(format!("{:?}; the log ends:\n{}", gw.state(), r.log_tail()))
        }
    })
    .then_some(())?;
    r.step("the binding binds #lab and ana's DM", |r| {
        wait("two bound places", || {
            let b = r.binding();
            let bound = b["places"].as_array().map_or(0, |p| {
                p.iter()
                    .filter(|p| p["session_id"].as_str().is_some_and(|s| !s.is_empty()))
                    .count()
            });
            (b["state"] == "ready" && bound == 2).then(|| format!("state ready, {bound} places"))
        })
    })
    .then_some(())?;
    r.step("the viewer check trusts #lab (theseus-ck0k)", viewer_check)
        .then_some(())?;
    r.step(
        "ana types a message in #lab and its reply answers it",
        typed_message,
    )
    .then_some(())?;
    r.step("ben, whom #lab doesn't list, is ignored", ignored)
        .then_some(())?;
    r.step(
        "a write on the approve list waits, its card in #lab naming ana",
        card,
    )
    .then_some(())?;
    r.step(
        "ben's press of Approve is refused, and the call keeps waiting",
        refused,
    )
    .then_some(())?;
    r.step("ana's press of Approve runs the call", approved)
        .then_some(())?;
    r.step("the card is updated: approved, its buttons gone", settled)
        .then_some(())?;
    r.step("the resumed turn's reply posts", |r| {
        wait("the reply", || {
            r.posted(LAB)
                .into_iter()
                .find(|m| m.content.contains(DONE_TEXT))
                .map(|m| format!("{:?}", first_line(&m.content)))
        })
    })
    .then_some(())?;
    r.step(
        "the daemon stops cleanly, with no binding error and no token kept",
        stop,
    )
    .then_some(())
}

fn start(r: &mut Rig, theseusd: &Path) -> Result<String, String> {
    let log = std::fs::File::create(r.dir.join("theseusd.log")).map_err(|e| e.to_string())?;
    let child = daemon_command(theseusd, &r.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|e| format!("starting {}: {e}", theseusd.display()))?;
    let pid = child.id();
    r.daemon = Some(Daemon(child));
    wait("health", || {
        r.call("health", Value::Null).ok().map(|h| {
            format!(
                "pid {pid}, version {}",
                h["version"].as_str().unwrap_or("?")
            )
        })
    })
    .map_err(|e| format!("{e}; the log ends:\n{}", r.log_tail()))
}

/// The binding's check of `#lab`, as health reports it: trusted, or why not.
fn viewer_check(r: &mut Rig) -> Result<String, String> {
    let c = wait("check of #lab in health", || {
        let h = r.health();
        let c = h["approval"]["channels"]
            .as_array()?
            .iter()
            .find(|c| c["channel"] == format!("discord:{LAB}"))?
            .clone();
        (c["checked_at_ms"].as_u64() > Some(0)).then_some(c)
    })?;
    let detail = c["detail"].as_str().unwrap_or("").to_string();
    if c["state"] == "trusted" {
        Ok(detail)
    } else {
        Err(format!("#lab is {}: {detail}", c["state"]))
    }
}

fn typed_message(r: &mut Rig) -> Result<String, String> {
    r.typed = r.fake.say(&Typed {
        user: ANA,
        name: "ana",
        channel: Some(LAB),
        content: "Reply with exactly one word: ready",
    })?;
    let typed = r.typed.clone();
    let reply = wait("the reply", || {
        r.posted(LAB).into_iter().find(|m| {
            m.reply_to.as_deref() == Some(typed.as_str()) && m.content.contains(READY_TEXT)
        })
    })?;
    Ok(format!(
        "message {typed} answered by {}: {:?}",
        reply.id,
        first_line(&reply.content)
    ))
}

fn ignored(r: &mut Rig) -> Result<String, String> {
    let turns = r.ledger("discord.message.in").len();
    r.fake.say(&Typed {
        user: BEN,
        name: "ben",
        channel: Some(LAB),
        content: "Reply with exactly one word: ready",
    })?;
    // The row names the author as a string, as Discord's ids are.
    let ben = BEN.to_string();
    let seen = wait("ben's message ignored, or taken", || {
        let taken = r.ledger("discord.message.in").len();
        if taken != turns {
            return Some(Err(format!(
                "ben's message was taken: {turns} messages in before, {taken} after"
            )));
        }
        r.ledger("discord.ignored")
            .into_iter()
            .find(|d| d["author_id"] == ben)
            .map(Ok)
    })?;
    let row = seen?;
    Ok(format!("ignored: {}", row["reason"].as_str().unwrap_or("")))
}

fn card(r: &mut Rig) -> Result<String, String> {
    r.fake.say(&Typed {
        user: ANA,
        name: "ana",
        channel: Some(LAB),
        content: &format!("{WRITE_WORD}: write the proof file."),
    })?;
    wait("a waiting call", || (r.waiting() == 1).then_some(()))?;
    let card = wait("the card", || r.card_msg())?;
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    if labels != ["Approve", "Decline"] {
        return Err(format!("its buttons: {labels:?}"));
    }
    if card.mentions != [ANA.to_string()] {
        return Err(format!("it notifies {:?}, not only ana", card.mentions));
    }
    r.card = card.id.clone();
    Ok(format!(
        "{:?}, buttons {labels:?}",
        first_line(&card.content)
    ))
}

fn refused(r: &mut Rig) -> Result<String, String> {
    let i = r.fake.press(&Pressed {
        message: &r.card,
        button: "Approve",
        user: BEN,
        name: "ben",
    })?;
    let answer = wait("an answer to ben", || {
        r.fake
            .replies()
            .into_iter()
            .find(|a| a.interaction.as_deref() == Some(i.as_str()))
    })?;
    let told = answer.content.clone().unwrap_or_default();
    if !answer.ephemeral || !told.contains("Only the people this place is bound to") {
        return Err(format!("ben was answered {answer:?}"));
    }
    std::thread::sleep(Duration::from_millis(300));
    if r.waiting() != 1 || r.written().exists() {
        return Err("the call did not keep waiting".into());
    }
    Ok(format!("told only ben: {told:?}"))
}

fn approved(r: &mut Rig) -> Result<String, String> {
    let i = r.fake.press(&Pressed {
        message: &r.card,
        button: "Approve",
        user: ANA,
        name: "ana",
    })?;
    let ack = wait("the press acknowledged", || {
        r.fake
            .replies()
            .into_iter()
            .find(|a| a.interaction.as_deref() == Some(i.as_str()))
    })?;
    if ack.response_type != Some(6) {
        return Err(format!("acknowledged as {ack:?}"));
    }
    let text = wait("the file written", || {
        std::fs::read_to_string(r.written()).ok()
    })?;
    if text != WRITTEN {
        return Err(format!("the file holds {text:?}"));
    }
    let row = wait("a discord.confirm row", || {
        r.ledger("discord.confirm").into_iter().next()
    })?;
    if row["ok"] != true || row["by"] != "discord:ana" {
        return Err(format!("discord.confirm: {row}"));
    }
    Ok(format!(
        "deferred update; discord.confirm ok by discord:ana; {} holds {text:?}",
        r.written().display()
    ))
}

fn settled(r: &mut Rig) -> Result<String, String> {
    let card = wait("the card's settle", || {
        r.card_msg().filter(|c| c.buttons.is_empty())
    })
    .map_err(|e| match r.card_msg() {
        Some(c) => format!(
            "{e}; the card holds {:?} with {} buttons",
            c.content,
            c.buttons.len()
        ),
        None => e,
    })?;
    if !card.content.starts_with("✅ **Approved** by discord:ana") {
        return Err(format!("it says {:?}", card.content));
    }
    if r.waiting() != 0 {
        return Err("a call still waits".into());
    }
    Ok(format!(
        "{} versions; now {:?}",
        card.versions.len(),
        first_line(&card.content)
    ))
}

fn stop(r: &mut Rig) -> Result<String, String> {
    let b = r.binding();
    let errors = b["errors"].as_u64().unwrap_or(0);
    let counts = format!(
        "{} in, {} presses, {} ignored, {errors} errors",
        b["messages_in"], b["interactions"], b["ignored"]
    );
    let _ = r.call("shutdown", Value::Null);
    let mut d = r.daemon.take().ok_or("no daemon")?;
    let status = wait("the daemon's exit", || d.0.try_wait().ok().flatten())?;
    if !status.success() {
        return Err(format!(
            "it exited {status}; the log ends:\n{}",
            r.log_tail()
        ));
    }
    if errors != 0 {
        return Err(format!("{counts}; the last: {}", b["last_error"]));
    }
    let kept = format!(
        "{}{}{}",
        serde_json::to_string(&r.fake.seen()).unwrap_or_default(),
        serde_json::to_string(&r.fake.all_messages()).unwrap_or_default(),
        serde_json::to_string(&r.fake.gateway_state()).unwrap_or_default()
    );
    if kept.contains(SECRET) {
        return Err("the bot's token is in what the stand-in kept".into());
    }
    Ok(format!("exit 0; {counts}"))
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// A message's first line, cut at 100 characters, for the transcript.
fn clip(s: &str) -> String {
    let line = first_line(s);
    match line.char_indices().nth(100) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

/// Every message the stand-in holds, by channel, each version in order, its
/// buttons, and every answer to an interaction.
fn transcript(fake: &FakeDiscord) -> Vec<String> {
    let who = |a: &str| match a.parse::<u64>() {
        Ok(BOT_ID) => "theseus".to_string(),
        Ok(ANA) => "ana".into(),
        Ok(BEN) => "ben".into(),
        _ => a.to_string(),
    };
    let mut out = Vec::new();
    for m in fake.all_messages() {
        let place = match m.channel.parse::<u64>() {
            Ok(LAB) => "#lab".to_string(),
            Ok(c) if c == ANA + 1 => "ana's DM".into(),
            _ => format!("channel {}", m.channel),
        };
        let versions: Vec<String> = m
            .versions
            .iter()
            .map(|v| format!("{:?}", clip(v)))
            .collect();
        let buttons: Vec<&str> = m.buttons.iter().map(|b| b.label.as_str()).collect();
        out.push(format!(
            "{place} {}: {}{}",
            who(&m.author),
            versions.join(" -> "),
            if buttons.is_empty() {
                String::new()
            } else {
                format!(" {buttons:?}")
            }
        ));
    }
    for a in fake.replies() {
        out.push(format!(
            "interaction {}: {} {}{}",
            a.interaction.unwrap_or_default(),
            a.kind,
            a.response_type
                .map(|t| format!("type {t} "))
                .unwrap_or_default(),
            a.content
                .map(|c| format!(
                    "{:?}{}",
                    c,
                    if a.ephemeral {
                        " (only the presser sees it)"
                    } else {
                        ""
                    }
                ))
                .unwrap_or_default()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stand-in model asks for the write when the prompt holds the word,
    /// answers a tool's result with the done text, and anything else ready.
    #[test]
    fn the_model_answers_by_script() {
        let m = Model::start(json!({"path": "/x", "content": "y"})).unwrap();
        let post = |body: Value| -> String {
            let addr = m.addr.parse().unwrap();
            let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let b = body.to_string();
            write!(
                s,
                "POST /v1/messages HTTP/1.1\r\ncontent-length: {}\r\n\r\n{b}",
                b.len()
            )
            .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let asked = post(
            json!({"model": "m", "messages": [{"role": "user", "content": format!("{WRITE_WORD} now")}]}),
        );
        assert!(
            asked.contains("\"name\":\"fs_write\"") && asked.contains("tool_use"),
            "{asked}"
        );
        let done = post(
            json!({"model": "m", "messages": [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": "ok"}]}]}),
        );
        assert!(done.contains(DONE_TEXT), "{done}");
        let hi = post(json!({"model": "m", "messages": [{"role": "user", "content": "hello"}]}));
        assert!(hi.contains(READY_TEXT) && !hi.contains("tool_use"), "{hi}");
    }

    /// The fake op gives every reference the invented value, and reads
    /// nothing.
    #[test]
    fn the_fake_op_answers_every_reference_with_the_invented_value() {
        let d = tempfile::tempdir().unwrap();
        let op = d.path().join("op");
        std::fs::write(&op, fake_op()).unwrap();
        let read = Command::new("sh")
            .arg(&op)
            .args(["read", "op://Proof/x/credential"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(read.stdout).unwrap(), SECRET);
        let mut inject = Command::new("sh")
            .arg(&op)
            .arg("inject")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        inject
            .stdin
            .take()
            .unwrap()
            .write_all(b"a={{ op://Proof/a/credential }}\n")
            .unwrap();
        let out = inject.wait_with_output().unwrap();
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("a={SECRET}\n")
        );
    }
}
