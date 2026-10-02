//! `theseus herdr sync` (theseus-l1l, design `stage2` §2.10, step 11b). herdr
//! shows a pane per program, and a Theseus session is not a program: sync gives
//! each session that needs you or is working, and each session pinned, a pane
//! of its own in herdr's `theseus` workspace, running `theseus watch <session>
//! --interactive`. That watch reports the session's state to its pane
//! (`herdr::Reporter`) and sets the pane's `$session` token, by which a later
//! sync finds the pane again.
//!
//! theseusd holds the truth, so sync can run any number of times: when nothing
//! is missing, it sends herdr only reads. It mends what a crash leaves:
//! - a pane whose watch died (its foreground process says so) has its stale
//!   state swept, since herdr's custom authority never expires;
//! - its session's watch starts again in it, if its shell waits at a prompt. A
//!   pane is never typed into while anything else runs in it, a new pane
//!   included (its shell's startup files may ask for a passphrase), and a
//!   session whose pane is busy gets no second pane.
//!
//! Only this daemon's sessions are touched: a pane whose `$session` this daemon
//! does not know is left alone. A finished session's pane (complete or
//! cancelled) closes only with `--close-finished`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;
use serde::Serialize;
use serde_json::{json, Value};
use theseus_client::Conn;
use theseus_protocol::{method, Level, SessionInfo, SessionListResult};

use crate::herdr;

/// The workspace that holds the sessions' panes.
pub const WORKSPACE: &str = "theseus";

/// How long sync waits for a new pane's shell to reach its prompt before it
/// types the watch's command, and how often it looks.
const PROMPT_WAIT: Duration = Duration::from_secs(5);
const PROMPT_LOOK: Duration = Duration::from_millis(100);

/// `theseus herdr …`.
#[derive(Subcommand, Debug)]
pub enum HerdrCmd {
    /// Give each session that needs you or is working its own pane in herdr's `theseus`
    /// workspace, running `theseus watch <session> --interactive`, which shows the session's
    /// state in herdr. Run it again at any time: with nothing missing, it changes nothing.
    Sync {
        /// Give this session a pane too, whatever its state (repeatable). SESSION is its id, or
        /// at least its last four characters.
        #[arg(long = "pin", value_name = "SESSION")]
        pin: Vec<String>,
        /// Close the panes of sessions that finished (complete or cancelled).
        #[arg(long)]
        close_finished: bool,
        /// herdr's API socket. Default: $HERDR_SOCKET_PATH, else herdr's own
        /// (~/.config/herdr/herdr.sock, or $HERDR_SESSION's).
        #[arg(long, value_name = "PATH")]
        herdr_socket: Option<PathBuf>,
    },
}

/// What runs in a pane, by herdr's view of its foreground job
/// (`pane.process_info`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Occupant {
    /// A `theseus watch`: the pane's session is watched.
    Watch,
    /// The pane's shell, waiting at its prompt: a command may be typed in.
    Shell,
    /// Anything else, by name, or nothing herdr could read: never typed into.
    Other(String),
}

/// The occupant of a pane, from `pane.process_info`'s `process_info`.
pub fn occupant(info: &Value) -> Occupant {
    let procs = info["foreground_processes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if procs.iter().any(is_watch) {
        return Occupant::Watch;
    }
    let shell = info["shell_pid"].as_u64();
    if shell.is_some() && info["foreground_process_group_id"].as_u64() == shell {
        return Occupant::Shell;
    }
    let names: Vec<&str> = procs.iter().filter_map(|p| p["name"].as_str()).collect();
    Occupant::Other(if names.is_empty() {
        "nothing herdr can read".into()
    } else {
        names.join(", ")
    })
}

/// A `theseus watch`: by its argv, or by its name alone when herdr could not
/// read the argv.
fn is_watch(p: &Value) -> bool {
    let theseus = |s: &str| Path::new(s).file_name().is_some_and(|n| n == "theseus");
    match p["argv"].as_array() {
        Some(argv) => {
            let argv: Vec<&str> = argv.iter().filter_map(Value::as_str).collect();
            argv.first().is_some_and(|a| theseus(a)) && argv.contains(&"watch")
        }
        None => p["name"].as_str() == Some("theseus"),
    }
}

/// The command typed into a pane: this binary by its whole path (a pane's PATH
/// may find another build), on this daemon's socket.
pub fn watch_command(exe: &Path, socket: &str, session_id: &str) -> String {
    format!(
        "{} --socket {} watch {} --interactive",
        quote(&exe.to_string_lossy()),
        quote(socket),
        quote(session_id)
    )
}

/// A word as a POSIX shell reads it: plain when it is, else single-quoted.
fn quote(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-:@%+=,".contains(c);
    if !s.is_empty() && s.chars().all(plain) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// The sessions `--pin` names: each by its id, or at least its last four
/// characters.
pub fn resolve_pins(pins: &[String], sessions: &[SessionInfo]) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for pin in pins {
        if let Some(s) = sessions.iter().find(|s| s.session_id == *pin) {
            out.insert(s.session_id.clone());
            continue;
        }
        if pin.chars().count() < 4 {
            bail!("--pin {pin}: give a session's id, or at least its last four characters");
        }
        let ends: Vec<&SessionInfo> = sessions
            .iter()
            .filter(|s| s.session_id.ends_with(pin.as_str()))
            .collect();
        match ends.as_slice() {
            [one] => {
                out.insert(one.session_id.clone());
            }
            [] => bail!("--pin {pin}: no session has that id"),
            many => bail!(
                "--pin {pin}: {} sessions end with it; give more of the id",
                many.len()
            ),
        }
    }
    Ok(out)
}

/// A session gets a pane when it needs you, is working, or is pinned.
pub fn wants_pane(s: &SessionInfo, pinned: &HashSet<String>) -> bool {
    pinned.contains(&s.session_id)
        || s.attention
            .as_ref()
            .is_some_and(|a| matches!(a.level, Level::NeedsYou | Level::Working))
}

/// A finished session: its execution completed or was cancelled (the
/// attention level `idle`). A failure needs you, so it is not finished.
pub fn finished(s: &SessionInfo) -> bool {
    s.attention.as_ref().is_some_and(|a| a.level == Level::Idle)
}

/// Where a new pane goes in a tab (`pane.layout`'s `layout`): a split of its
/// largest pane, along that pane's longer side. A cell is about twice as tall
/// as it is wide, so a pane at least twice as many columns as rows is wide.
pub fn split_target(layout: &Value) -> Option<(String, &'static str)> {
    let size = |p: &Value, k: &str| p["rect"][k].as_u64().unwrap_or(0);
    let largest = layout["panes"]
        .as_array()?
        .iter()
        .filter(|p| p["pane_id"].is_string())
        .max_by_key(|p| size(p, "width") * size(p, "height"))?;
    let wide = size(largest, "width") >= 2 * size(largest, "height");
    Some((
        largest["pane_id"].as_str()?.to_string(),
        if wide { "right" } else { "down" },
    ))
}

/// herdr's default API socket, as herdr resolves it: `$HERDR_SOCKET_PATH`,
/// then `$HERDR_SESSION`'s, then its own, under `$XDG_CONFIG_HOME` or
/// `~/.config`.
fn default_socket() -> Result<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    if let Some(p) = var("HERDR_SOCKET_PATH") {
        return Ok(p.into());
    }
    let config = match var("XDG_CONFIG_HOME") {
        Some(d) => PathBuf::from(d),
        None => {
            PathBuf::from(var("HOME").ok_or_else(|| anyhow!("no HOME to find herdr's socket"))?)
                .join(".config")
        }
    };
    let dir = config.join("herdr");
    Ok(match var("HERDR_SESSION") {
        Some(name) => dir.join("sessions").join(name).join("herdr.sock"),
        None => dir.join("herdr.sock"),
    })
}

/// herdr's socket, and how many requests went to it.
struct Herdr {
    socket: PathBuf,
    n: u64,
}

impl Herdr {
    async fn call(&mut self, m: &str, params: Value) -> Result<Value> {
        self.n += 1;
        let id = format!("theseus-sync:{}:{}", std::process::id(), self.n);
        herdr::request(&self.socket, &id, m, params).await
    }
}

/// A pane that holds one of this daemon's sessions, by its `$session` token.
struct Held {
    pane_id: String,
    session_id: String,
    /// herdr's agent for the pane: `theseus` while a watch's state stands,
    /// whether the watch lives or not.
    agent: Option<String>,
    occupant: Occupant,
}

/// One thing sync did, or found already done.
#[derive(Debug, Serialize)]
struct Step {
    /// `watched` (already), `made`, `restarted`, `swept`, `closed`, or `busy`.
    action: &'static str,
    pane_id: String,
    session_id: String,
    /// The session's attention: `needs you · confirm proc.run: …`.
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn step(action: &'static str, pane_id: &str, s: &SessionInfo, detail: Option<String>) -> Step {
    let state = match &s.attention {
        Some(a) => format!("{} · {}", a.level.as_str().replace('_', " "), a.label),
        None => "no turn yet".into(),
    };
    Step {
        action,
        pane_id: pane_id.to_string(),
        session_id: s.session_id.clone(),
        state,
        detail,
    }
}

/// The `theseus` workspace as sync found it, then as it left it.
struct Place {
    workspace_id: Option<String>,
    /// A pane in it, whose tab the next split goes into.
    a_pane: Option<String>,
    made: bool,
}

/// `theseus herdr sync`.
pub async fn run(
    conn: &mut Conn,
    json: bool,
    cmd: HerdrCmd,
    theseus_socket: Option<&str>,
) -> Result<()> {
    let HerdrCmd::Sync {
        pin,
        close_finished,
        herdr_socket,
    } = cmd;
    let Some(theseus_socket) = theseus_socket else {
        bail!(
            "`theseus herdr sync` needs a running theseusd, not --spawn: \
             the watches it starts connect to the daemon's socket"
        );
    };
    let theseus_socket = theseus_client::client::tilde(theseus_socket, std::env::var("HOME").ok());
    let exe = std::env::current_exe().context("finding this theseus binary")?;
    let socket = match herdr_socket {
        Some(p) => p,
        None => default_socket()?,
    };
    let mut h = Herdr { socket, n: 0 };
    let sessions = serde_json::from_value::<SessionListResult>(
        conn.request(method::SESSION_LIST, Value::Null).await?,
    )?
    .sessions;
    let pinned = resolve_pins(&pin, &sessions)?;
    let known = |sid: &str| sessions.iter().find(|s| s.session_id == sid);

    // herdr's side: the workspace, and each pane that holds one of this
    // daemon's sessions, with what runs in it.
    let workspaces = h.call("workspace.list", json!({})).await?;
    let workspace_id = workspaces["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|w| w["label"] == WORKSPACE)
        .and_then(|w| w["workspace_id"].as_str())
        .map(str::to_string);
    let panes = h.call("pane.list", json!({})).await?;
    let panes = panes["panes"].as_array().cloned().unwrap_or_default();
    let mut place = Place {
        a_pane: workspace_id.as_ref().and_then(|w| {
            panes
                .iter()
                .find(|p| p["workspace_id"] == w.as_str())
                .and_then(|p| p["pane_id"].as_str())
                .map(str::to_string)
        }),
        workspace_id,
        made: false,
    };
    let mut held = Vec::new();
    let mut foreign = 0;
    for p in &panes {
        let (Some(pane_id), Some(sid)) = (p["pane_id"].as_str(), p["tokens"]["session"].as_str())
        else {
            continue;
        };
        if known(sid).is_none() {
            foreign += 1;
            continue;
        }
        let info = h
            .call("pane.process_info", json!({ "pane_id": pane_id }))
            .await?;
        held.push(Held {
            pane_id: pane_id.to_string(),
            session_id: sid.to_string(),
            agent: p["agent"].as_str().map(str::to_string),
            occupant: occupant(&info["process_info"]),
        });
    }

    let mut steps = Vec::new();
    // The panes first: sweep a dead watch's state, and close a finished
    // session's pane when asked.
    for p in &held {
        let Some(s) = known(&p.session_id) else {
            continue;
        };
        if p.occupant != Occupant::Watch && p.agent.as_deref() == Some(herdr::AGENT) {
            h.call(
                "pane.clear_agent_authority",
                json!({"pane_id": p.pane_id, "source": herdr::SOURCE, "seq": herdr::next_seq()}),
            )
            .await?;
            let why = "its watch had stopped: its state is cleared".to_string();
            steps.push(step("swept", &p.pane_id, s, Some(why)));
        }
        if close_finished && finished(s) && !wants_pane(s, &pinned) {
            h.call("pane.close", json!({ "pane_id": p.pane_id }))
                .await?;
            steps.push(step("closed", &p.pane_id, s, None));
        } else if !wants_pane(s, &pinned) {
            // Its session wants no pane now: kept as it is, and said.
            let stopped =
                (p.occupant != Occupant::Watch).then(|| "no watch runs there".to_string());
            steps.push(step("kept", &p.pane_id, s, stopped));
        }
    }
    // Then each session that wants a pane: watched already, its pane's shell
    // waiting at a prompt, or a new pane.
    for s in sessions.iter().filter(|s| wants_pane(s, &pinned)) {
        let mine: Vec<&Held> = held
            .iter()
            .filter(|p| p.session_id == s.session_id)
            .collect();
        if let Some(p) = mine.iter().find(|p| p.occupant == Occupant::Watch) {
            steps.push(step("watched", &p.pane_id, s, None));
            continue;
        }
        let (pane_id, action) = match mine.iter().find(|p| p.occupant == Occupant::Shell) {
            Some(p) => (p.pane_id.clone(), "restarted"),
            // Its pane runs something else: never typed into, and no second
            // pane, so syncs never multiply panes.
            None if !mine.is_empty() => {
                for p in &mine {
                    if let Occupant::Other(what) = &p.occupant {
                        let why = format!(
                            "{what} runs there: nothing typed; sync starts the watch there \
                             once its shell is back at a prompt"
                        );
                        steps.push(step("busy", &p.pane_id, s, Some(why)));
                    }
                }
                continue;
            }
            None => {
                let pane = new_pane(&mut h, &mut place).await?;
                if let Some(what) = wait_for_prompt(&mut h, &pane).await? {
                    // The session's pane all the same, by its token, so a
                    // later sync starts the watch there.
                    h.call(
                        "pane.report_metadata",
                        json!({"pane_id": pane, "source": herdr::SOURCE, "agent": herdr::AGENT,
                               "tokens": {"session": s.session_id}, "seq": herdr::next_seq()}),
                    )
                    .await?;
                    let why = format!(
                        "{what} runs there, not its shell's prompt: nothing typed; \
                         sync again once it is"
                    );
                    steps.push(step("made", &pane, s, Some(why)));
                    continue;
                }
                (pane, "made")
            }
        };
        h.call(
            "pane.send_input",
            json!({"pane_id": pane_id,
                   "text": watch_command(&exe, &theseus_socket, &s.session_id),
                   "keys": ["Enter"]}),
        )
        .await?;
        steps.push(step(action, &pane_id, s, None));
    }

    if json {
        let out = json!({"herdr_socket": h.socket, "workspace_id": place.workspace_id,
                         "workspace_made": place.made, "steps": steps,
                         "panes_of_other_daemons": foreign, "herdr_requests": h.n});
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }
    if let (true, Some(w)) = (place.made, &place.workspace_id) {
        println!("made herdr's workspace `{WORKSPACE}` ({w})");
    }
    for st in &steps {
        let detail = st
            .detail
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        println!(
            "{:<9} {:<8} {}  {}{detail}",
            st.action, st.pane_id, st.session_id, st.state
        );
    }
    let count = |a: &str| steps.iter().filter(|s| s.action == a).count();
    let changed = ["made", "restarted", "swept", "closed"]
        .iter()
        .map(|a| count(a))
        .sum::<usize>();
    if steps.is_empty() {
        println!("no session needs you, works, or is pinned: nothing to show in herdr");
    } else if changed == 0 && count("busy") == 0 {
        println!("herdr is in sync: nothing changed");
    } else if changed == 0 {
        println!("nothing changed: a busy pane waits for its shell's prompt");
    }
    if foreign > 0 {
        println!(
            "{foreign} pane(s) watch sessions this daemon does not know (another daemon's?): left alone"
        );
    }
    Ok(())
}

/// Wait for a new pane's shell to reach its prompt, looking every
/// `PROMPT_LOOK` for up to `PROMPT_WAIT`: none when it does, else what runs
/// there instead (a shell's startup files may ask for a key's passphrase).
async fn wait_for_prompt(h: &mut Herdr, pane_id: &str) -> Result<Option<String>> {
    let deadline = tokio::time::Instant::now() + PROMPT_WAIT;
    loop {
        let info = h
            .call("pane.process_info", json!({ "pane_id": pane_id }))
            .await?;
        let what = match occupant(&info["process_info"]) {
            Occupant::Shell => return Ok(None),
            Occupant::Watch => "a theseus watch".to_string(),
            Occupant::Other(what) => what,
        };
        if tokio::time::Instant::now() >= deadline {
            return Ok(Some(what));
        }
        tokio::time::sleep(PROMPT_LOOK).await;
    }
}

/// A new pane in the `theseus` workspace: the workspace's first pane when sync
/// makes the workspace, else a split of its tab's largest pane.
async fn new_pane(h: &mut Herdr, place: &mut Place) -> Result<String> {
    let id = |v: &Value, what: &str| -> Result<String> {
        v.as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("herdr's answer has no {what}"))
    };
    let Some(ws) = place.workspace_id.clone() else {
        let r = h
            .call(
                "workspace.create",
                json!({"label": WORKSPACE, "focus": false}),
            )
            .await?;
        let pane = id(&r["root_pane"]["pane_id"], "root pane")?;
        place.workspace_id = Some(id(&r["workspace"]["workspace_id"], "workspace")?);
        place.a_pane = Some(pane.clone());
        place.made = true;
        return Ok(pane);
    };
    let target = match place.a_pane.clone() {
        Some(p) => {
            let l = h.call("pane.layout", json!({ "pane_id": p })).await?;
            split_target(&l["layout"])
        }
        None => None,
    };
    let mut params = json!({"workspace_id": ws, "direction": "right", "focus": false});
    if let Some((pane, direction)) = target {
        params["target_pane_id"] = json!(pane);
        params["direction"] = json!(direction);
    }
    let r = h.call("pane.split", params).await?;
    let pane = id(&r["pane"]["pane_id"], "pane")?;
    place.a_pane = Some(pane.clone());
    Ok(pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, level: Option<Level>) -> SessionInfo {
        let mut v = json!({"session_id": id, "kind": "conversation", "label": null,
                           "created_at_unix_ms": 0, "turns": 1});
        if let Some(l) = level {
            v["attention"] = json!({"level": l, "label": "x", "since_ms": 0});
        }
        serde_json::from_value(v).unwrap()
    }

    /// A watch, by its argv or (unread) its name; the shell at its prompt;
    /// anything else, never typed into.
    #[test]
    fn a_panes_occupant_comes_from_its_foreground_job() {
        let watch = json!({"shell_pid": 10, "foreground_process_group_id": 20,
            "foreground_processes": [{"pid": 20, "name": "theseus",
              "argv": ["/opt/tide/bin/theseus", "--socket", "/s", "watch", "ses_q7f3k2", "--interactive"]}]});
        assert_eq!(occupant(&watch), Occupant::Watch);
        let unread = json!({"shell_pid": 10, "foreground_process_group_id": 20,
            "foreground_processes": [{"pid": 20, "name": "theseus", "argv": null}]});
        assert_eq!(occupant(&unread), Occupant::Watch);
        let prompt = json!({"shell_pid": 10, "foreground_process_group_id": 10,
            "foreground_processes": [{"pid": 10, "name": "bash", "argv": ["-bash"]}]});
        assert_eq!(occupant(&prompt), Occupant::Shell);
        let other = json!({"shell_pid": 10, "foreground_process_group_id": 30,
            "foreground_processes": [{"pid": 30, "name": "vim", "argv": ["vim", "tide.md"]}]});
        assert_eq!(occupant(&other), Occupant::Other("vim".into()));
        let ask = json!({"shell_pid": 10, "foreground_process_group_id": 40,
            "foreground_processes": [{"pid": 40, "name": "theseus", "argv": ["theseus", "ask", "hi"]}]});
        assert_eq!(occupant(&ask), Occupant::Other("theseus".into()));
        assert_eq!(
            occupant(&json!({"pane_id": "w1:p2"})),
            Occupant::Other("nothing herdr can read".into())
        );
    }

    /// The command a pane runs: this binary's whole path and the daemon's
    /// socket, quoted for a shell when they need it.
    #[test]
    fn the_watch_command_is_quoted_for_a_shell() {
        assert_eq!(
            watch_command(
                Path::new("/opt/tide/bin/theseus"),
                "/run/t.sock",
                "ses_q7f3k2"
            ),
            "/opt/tide/bin/theseus --socket /run/t.sock watch ses_q7f3k2 --interactive"
        );
        assert_eq!(
            watch_command(
                Path::new("/opt/tide bin/theseus"),
                "/run/it's.sock",
                "ses_q7f3k2"
            ),
            r"'/opt/tide bin/theseus' --socket '/run/it'\''s.sock' watch ses_q7f3k2 --interactive"
        );
    }

    /// `--pin` takes an id, or its last four characters or more, and says
    /// why when it can't.
    #[test]
    fn pins_resolve_by_id_or_its_end() {
        let all = [session("ses_q7f3k2", None), session("ses_a1b2k2", None)];
        let pins =
            |p: &[&str]| resolve_pins(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &all);
        assert_eq!(
            pins(&["ses_q7f3k2", "b2k2"]).unwrap(),
            HashSet::from(["ses_q7f3k2".to_string(), "ses_a1b2k2".to_string()])
        );
        let err = |p: &[&str]| pins(p).unwrap_err().to_string();
        assert!(err(&["k2"]).contains("at least its last four"));
        assert!(err(&["ses_q7f3k2", "zzzz"]).contains("--pin zzzz: no session"));
        assert!(err(&["_k2"]).contains("at least its last four"));
        let shared = [session("ses_1aa3k2", None), session("ses_2aa3k2", None)];
        let e = resolve_pins(&["aa3k2".into()], &shared).unwrap_err();
        assert!(e.to_string().contains("2 sessions end with it"), "{e}");
    }

    /// Needs you and working get a pane, as does a pin; ready and idle do
    /// not. Only idle (complete or cancelled) is finished.
    #[test]
    fn which_sessions_get_a_pane_and_which_are_finished() {
        let pinned = HashSet::from(["ses_pinned".to_string()]);
        for (level, wants, done) in [
            (Some(Level::NeedsYou), true, false),
            (Some(Level::Working), true, false),
            (Some(Level::Ready), false, false),
            (Some(Level::Idle), false, true),
            (None, false, false),
        ] {
            let s = session("ses_q7f3k2", level);
            assert_eq!(wants_pane(&s, &pinned), wants, "{level:?}");
            assert_eq!(finished(&s), done, "{level:?}");
        }
        assert!(wants_pane(
            &session("ses_pinned", Some(Level::Idle)),
            &pinned
        ));
    }

    /// A new pane splits the tab's largest pane along its longer side.
    #[test]
    fn a_split_takes_the_largest_pane_along_its_longer_side() {
        let layout = |panes: Value| json!({ "panes": panes });
        let pane = |id: &str, w: u64, h: u64| json!({"pane_id": id, "rect": {"x": 0, "y": 0, "width": w, "height": h}});
        assert_eq!(
            split_target(&layout(json!([pane("w2:p1", 200, 50)]))),
            Some(("w2:p1".to_string(), "right"))
        );
        assert_eq!(
            split_target(&layout(json!([
                pane("w2:p1", 100, 50),
                pane("w2:p2", 99, 50)
            ]))),
            Some(("w2:p1".to_string(), "right"))
        );
        assert_eq!(
            split_target(&layout(json!([
                pane("w2:p1", 60, 50),
                pane("w2:p2", 100, 25)
            ]))),
            Some(("w2:p1".to_string(), "down"))
        );
        assert_eq!(split_target(&layout(json!([]))), None);
    }
}
