//! The commands the Theseus herdr plugin's keys and link handler run (theseus-pkaq):
//! `theseus herdr next` (focus the task that has waited longest), `stop` (ask y/N, then stop what
//! the pane shows), `pause` (prefix+p: hold the pane's task, or let it go on), `plugin` (where the plugin is, and the one command that links it), `popup`
//! (open one of the plugin's popup panes through herdr's API) and `link` (what a click on a
//! `theseus://` link opens). The answer card is `herdr_answer.rs`; the config snippet is
//! `herdr_setup.rs`. Nothing here is resident: each command starts on a key and ends.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use theseus_client::Conn;
use theseus_protocol::task_pause::{TaskPauseParams, TaskPauseResult};
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{
    method, ConfirmListResult, ConfirmRequest, WorkWatchParams, WorkWatchResult,
};

use crate::herdr;

/// The plugin's id, which `herdr plugin link` reads from its manifest and a pane open names.
pub const PLUGIN_ID: &str = "theseus";

/// The plugin's popup panes, by the name the manifest gives each.
pub const POPUPS: [&str; 6] = ["answer", "tui", "stop", "history", "new", "inspect"];

/// The pattern of the links a click may open: three paths and an id of the characters an id has.
/// The manifest's link handler carries the same pattern (a test ties the two).
#[cfg(test)]
pub const LINK_PATTERN: &str = r"^theseus://(answer|history|open)/[A-Za-z0-9_-]{1,64}$";

/// What a click on a `theseus://` link asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// The answer card for a question: never an answer itself.
    Answer(String),
    /// A session's history, paged in a popup.
    History(String),
    /// A session's pane, focused.
    Open(String),
}

/// A link, read strictly: the scheme, one of three paths and an id of letters, digits, `_` and `-`.
/// Anything else is nothing, so a click on odd text opens nothing.
pub fn parse_link(url: &str) -> Option<Link> {
    let rest = url.strip_prefix("theseus://")?;
    let (kind, id) = rest.split_once('/')?;
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok {
        return None;
    }
    match kind {
        "answer" => Some(Link::Answer(id.to_string())),
        "history" => Some(Link::History(id.to_string())),
        "open" => Some(Link::Open(id.to_string())),
        _ => None,
    }
}

/// The link under a question card: three words, each a `theseus://` link, as OSC 8 hyperlinks
/// inside herdr and plain words elsewhere is `interactive`'s concern. This is the targets.
pub fn card_links(correlation_id: &str, session_id: &str) -> [(&'static str, String); 3] {
    [
        ("answer", format!("theseus://answer/{correlation_id}")),
        ("history", format!("theseus://history/{session_id}")),
        ("open", format!("theseus://open/{session_id}")),
    ]
}

/// One OSC 8 hyperlink: the text, which a click on opens `url` (herdr hands it to the plugin whose
/// link handler matches). The URL is ours and plain; the terminator is ST.
pub fn osc8(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

/// The line of links under a question's card: `[answer] [history] [open]`. Only inside a herdr
/// pane (`HERDR_ENV=1`); anywhere else nothing is printed and the output stays what it was. A click
/// opens the answer card or a view; none of the three answers anything.
pub fn links_under(c: &ConfirmRequest, herdr_env: Option<&str>) -> Option<String> {
    if herdr_env != Some("1") {
        return None;
    }
    let words: Vec<String> = card_links(&c.correlation_id, &c.session_id)
        .iter()
        .filter(|(_, url)| parse_link(url).is_some())
        .map(|(w, url)| osc8(url, &format!("[{w}]")))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// `theseus herdr plugin`: where the plugin is, and the one command that links it. Plugins are
/// global to the user, so the owner links it; this prints and changes nothing.
pub fn plugin_lines(exe: &Path) -> Vec<String> {
    let dir = plugin_dir(exe);
    vec![
        format!("plugin: {}", dir.display()),
        format!("link it once: herdr plugin link {}", dir.display()),
        "then: theseus herdr setup   (the config snippet, and what your herdr config lacks)"
            .to_string(),
    ]
}

/// The plugin's directory: `contrib/herdr/theseus` of the checkout this binary was built from, else
/// beside the binary's own checkout, else the path the docs name.
pub fn plugin_dir(exe: &Path) -> PathBuf {
    let from_build = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contrib/herdr/theseus");
    let near_exe = exe
        .ancestors()
        .map(|a| a.join("contrib/herdr/theseus"))
        .find(|p| p.join("herdr-plugin.toml").is_file());
    let dir = near_exe.unwrap_or(from_build);
    dir.canonicalize().unwrap_or(dir)
}

/// The panes of a herdr session that are Theseus's, with their tokens.
async fn panes(socket: &Path) -> Result<Vec<Value>> {
    let v = herdr::request(socket, "theseus:keys:panes", "pane.list", json!({})).await?;
    Ok(v["panes"].as_array().cloned().unwrap_or_default())
}

/// The pane that shows a session: its `$session` token.
pub fn pane_of<'a>(panes: &'a [Value], session_id: &str) -> Option<&'a Value> {
    panes
        .iter()
        .find(|p| p["tokens"]["session"].as_str() == Some(session_id))
}

/// The question that has waited longest, among those this connection may answer.
pub fn longest_waiting(qs: &[ConfirmRequest]) -> Option<&ConfirmRequest> {
    qs.iter()
        .filter(|q| q.may_answer != Some(false))
        .min_by_key(|q| (q.requested_at_ms, q.correlation_id.clone()))
}

/// `theseus herdr next`: focus the pane of the task that has waited longest for the owner.
pub async fn next(conn: &mut Conn, herdr_socket: Option<PathBuf>) -> Result<()> {
    let qs = serde_json::from_value::<ConfirmListResult>(
        conn.request(method::CONFIRM_LIST, Value::Null).await?,
    )?
    .confirms;
    let Some(q) = longest_waiting(&qs) else {
        println!("nothing waits for you");
        return Ok(());
    };
    let socket = herdr_socket_or_default(herdr_socket)?;
    let panes = panes(&socket).await?;
    let Some(pane) = pane_of(&panes, &q.session_id).and_then(|p| p["pane_id"].as_str()) else {
        println!(
            "{} waits for you, and no herdr pane shows it: `theseus herdr sync` gives it one",
            q.correlation_id
        );
        return Ok(());
    };
    let focus = herdr::request(
        &socket,
        "theseus:keys:focus",
        "agent.focus",
        json!({ "target": pane }),
    )
    .await;
    if focus.is_err() {
        // A pane that is no named agent yet: focus it as a pane.
        herdr::request(
            &socket,
            "theseus:keys:focus",
            "pane.focus",
            json!({ "pane_id": pane }),
        )
        .await?;
    }
    println!("focused {pane} · {}", q.tool);
    Ok(())
}

/// What `stop` does for a pane of this kind: a task is cancelled by its short id, a conversation's
/// work is halted by its session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Cancel(String),
    Halt(String),
}

/// The stop a pane's tokens call for, if it is a Theseus pane.
pub fn stop_for(tokens: &Value) -> Option<Stop> {
    let session = tokens["session"].as_str()?.to_string();
    match tokens["kind"].as_str()? {
        "task" => Some(Stop::Cancel(
            tokens["task"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map_or_else(|| session.clone(), str::to_string),
        )),
        "conversation" => Some(Stop::Halt(session)),
        _ => None,
    }
}

/// `theseus herdr stop --pane P`: say what would stop, ask y/N, then stop it.
pub async fn stop(
    conn: &mut Conn,
    json: bool,
    pane: &str,
    herdr_socket: Option<PathBuf>,
    yes: bool,
) -> Result<()> {
    let socket = herdr_socket_or_default(herdr_socket)?;
    let v = herdr::request(
        &socket,
        "theseus:keys:pane",
        "pane.get",
        json!({ "pane_id": pane }),
    )
    .await?;
    let tokens = v.pointer("/pane/tokens").cloned().unwrap_or(Value::Null);
    let Some(what) = stop_for(&tokens) else {
        bail!("pane {pane} does not show a Theseus session or task: nothing to stop");
    };
    let name = tokens["task"]
        .as_str()
        .or_else(|| tokens["session"].as_str())
        .unwrap_or(pane);
    let words = match &what {
        Stop::Cancel(_) => format!("cancel task {name} and its jobs"),
        Stop::Halt(_) => format!("halt what session {name} is doing (the conversation goes on)"),
    };
    if !yes {
        print!("{words}? [y/N] ");
        use std::io::Write as _;
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("left as it is");
            return Ok(());
        }
    }
    match what {
        Stop::Cancel(id) => crate::cmd::cancel(conn, json, id).await,
        Stop::Halt(session) => crate::cmd::stop(conn, json, session, false).await,
    }
}

/// What prefix+p calls for a task: its pause when it works, its resume when it is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub method: &'static str,
    pub task: String,
    pub name: String,
}

/// The call prefix+p makes for the task a pane shows (its `kind` and `session` tokens), read from
/// the board's view of it: `paused` there says to resume, otherwise to pause. A pane that shows no
/// task, a task the board lacks and one that is over have nothing to hold, and the error says so.
pub fn hold_for(tokens: &Value, views: &[WorkView]) -> Result<Hold> {
    let Some(session) = inspect_for(tokens) else {
        bail!("this pane does not show a task: nothing to pause");
    };
    let Some(v) = views
        .iter()
        .find(|v| v.kind == WorkKind::Task && v.session_id.as_deref() == Some(session.as_str()))
    else {
        bail!("the board has no task {session}: nothing to pause");
    };
    if v.state.ended() {
        bail!("task {} is over: nothing to pause", v.title);
    }
    Ok(Hold {
        method: if v.paused {
            method::TASK_RESUME
        } else {
            method::TASK_PAUSE
        },
        task: session,
        name: v.title.clone(),
    })
}

/// `theseus herdr pause --pane P` (prefix+p): `task.pause` for the task the pane shows, or
/// `task.resume` when the board says it is held. It prints one line, which herdr shows.
pub async fn pause(conn: &mut Conn, pane: &str, herdr_socket: Option<PathBuf>) -> Result<()> {
    let socket = herdr_socket_or_default(herdr_socket)?;
    let v = herdr::request(
        &socket,
        "theseus:keys:pane",
        "pane.get",
        json!({ "pane_id": pane }),
    )
    .await?;
    let tokens = v.pointer("/pane/tokens").cloned().unwrap_or(Value::Null);
    let board: WorkWatchResult = serde_json::from_value(
        conn.request(method::WORK_WATCH, WorkWatchParams::default())
            .await?,
    )?;
    let hold = hold_for(&tokens, &board.views)?;
    let params = TaskPauseParams {
        task: hold.task.clone(),
        author: Some(crate::herdr_answer::AUTHOR.to_string()),
        discord: None,
    };
    let done: TaskPauseResult = serde_json::from_value(conn.request(hold.method, params).await?)?;
    println!(
        "{}",
        match (done.paused, done.changed) {
            (true, true) => format!("paused {}: it parks at its next step", hold.name),
            (true, false) => format!("{} was paused already", hold.name),
            (false, true) => format!("resumed {}", hold.name),
            (false, false) => format!("{} was running already", hold.name),
        }
    );
    Ok(())
}

/// herdr's socket: the one named, else `$HERDR_SOCKET_PATH`, else herdr's own.
pub fn herdr_socket_or_default(named: Option<PathBuf>) -> Result<PathBuf> {
    match named {
        Some(p) => Ok(p),
        None => crate::herdr_sync::default_socket(),
    }
}

/// What a popup pane needs to know, as environment: the pane the owner was on, a question, a
/// session. Never text from outside: ids only, checked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupEnv {
    pub pane: Option<String>,
    pub cid: Option<String>,
    pub session: Option<String>,
}

/// `plugin.pane.open`'s params for one of the plugin's popups.
pub fn popup_params(entrypoint: &str, env: &PopupEnv) -> Value {
    let mut vars = serde_json::Map::new();
    for (k, v) in [
        ("THESEUS_PANE", &env.pane),
        ("THESEUS_CID", &env.cid),
        ("THESEUS_SESSION_ARG", &env.session),
    ] {
        if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
            vars.insert(k.into(), json!(v));
        }
    }
    json!({"plugin_id": PLUGIN_ID, "entrypoint": entrypoint, "placement": "popup",
           "focus": true, "env": vars})
}

/// An id that is safe to hand a popup in its environment: letters, digits, `_`, `-`, `:`.
pub fn clean_id(s: &str) -> Option<String> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':'));
    ok.then(|| s.to_string())
}

/// The session `prefix+i` inspects: the task the focused pane shows (its `task` kind and `session`
/// tokens). A pane of anything else has nothing to inspect.
pub fn inspect_for(tokens: &Value) -> Option<String> {
    (tokens["kind"].as_str() == Some("task"))
        .then(|| tokens["session"].as_str().and_then(clean_id))
        .flatten()
}

async fn inspect_target(socket: &Path, pane: Option<&str>) -> Result<String> {
    let Some(pane) = pane else {
        bail!("inspect needs the focused pane (--pane): it shows the task to read");
    };
    let v = herdr::request(
        socket,
        "theseus:keys:pane",
        "pane.get",
        json!({ "pane_id": pane }),
    )
    .await?;
    let tokens = v.pointer("/pane/tokens").cloned().unwrap_or(Value::Null);
    inspect_for(&tokens)
        .ok_or_else(|| anyhow!("pane {pane} does not show a task: nothing to inspect"))
}

/// `theseus herdr popup NAME`: open one of the plugin's popups.
pub async fn popup(name: &str, env: PopupEnv, herdr_socket: Option<PathBuf>) -> Result<()> {
    if !POPUPS.contains(&name) {
        bail!("no popup named `{name}`: one of {}", POPUPS.join(", "));
    }
    let socket = herdr_socket_or_default(herdr_socket)?;
    let mut env = env;
    if name == "inspect" {
        env.session = Some(inspect_target(&socket, env.pane.as_deref()).await?);
    }
    herdr::request(
        &socket,
        "theseus:keys:popup",
        "plugin.pane.open",
        popup_params(name, &env),
    )
    .await
    .map_err(|e| anyhow!("{e}; is the plugin linked? `theseus herdr plugin` says how"))?;
    Ok(())
}

/// What a click opens: a popup of the plugin, or a session's pane focused. Never an answer: the
/// owner answers on the card, with a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opens {
    Popup(&'static str, PopupEnv),
    Focus(String),
}

/// The decision for a link: the answer link opens the answer card on that question, a history
/// opens paged, a session's opens its pane.
pub fn opens(l: Link, pane: Option<String>) -> Opens {
    match l {
        Link::Answer(cid) => Opens::Popup(
            "answer",
            PopupEnv {
                cid: Some(cid),
                pane,
                ..PopupEnv::default()
            },
        ),
        Link::History(session) => Opens::Popup(
            "history",
            PopupEnv {
                session: Some(session),
                ..PopupEnv::default()
            },
        ),
        Link::Open(session) => Opens::Focus(session),
    }
}

/// `theseus herdr link URL`: what a click opens.
pub async fn link(url: &str, pane: Option<String>, herdr_socket: Option<PathBuf>) -> Result<()> {
    let Some(l) = parse_link(url) else {
        bail!("not a Theseus link: {url}");
    };
    match opens(l, pane) {
        Opens::Popup(name, env) => popup(name, env, herdr_socket).await,
        Opens::Focus(session) => {
            let socket = herdr_socket_or_default(herdr_socket)?;
            let all = panes(&socket).await?;
            let Some(p) = pane_of(&all, &session).and_then(|p| p["pane_id"].as_str()) else {
                bail!("no herdr pane shows {session}: `theseus herdr sync --pin {session}` gives it one");
            };
            herdr::request(
                &socket,
                "theseus:keys:open",
                "pane.focus",
                json!({ "pane_id": p }),
            )
            .await?;
            Ok(())
        }
    }
}

/// The commands that connect nothing for themselves: the card, the plugin's path, the snippet,
/// and the popups and links, which talk to herdr alone.
pub async fn run_own(socket: &str, cmd: &crate::herdr_sync::HerdrCmd) -> Result<()> {
    use crate::herdr_sync::HerdrCmd as C;
    match cmd {
        C::Answer {
            pane,
            oldest,
            herdr_socket,
            cid,
        } => {
            crate::herdr_answer::run(
                socket,
                crate::herdr_answer::Args {
                    pane: pane.clone().filter(|p| !p.is_empty()),
                    oldest: *oldest,
                    cid: cid.clone(),
                    herdr_socket: herdr_socket.clone(),
                },
            )
            .await
        }
        C::Main { herdr_socket } => crate::herdr_main::run(socket, herdr_socket.clone()).await,
        C::Plugin => {
            for l in plugin_lines(&std::env::current_exe()?) {
                println!("{l}");
            }
            Ok(())
        }
        C::Setup { config } => crate::herdr_setup::run(config.as_deref()),
        C::Popup {
            name,
            pane,
            cid,
            session,
            herdr_socket,
        } => {
            let env = PopupEnv {
                pane: pane.as_deref().and_then(clean_id),
                cid: cid.as_deref().and_then(clean_id),
                session: session.as_deref().and_then(clean_id),
            };
            popup(name, env, herdr_socket.clone()).await
        }
        C::Link {
            url,
            pane,
            herdr_socket,
        } => {
            link(
                url,
                pane.as_deref().and_then(clean_id),
                herdr_socket.clone(),
            )
            .await
        }
        other => unreachable!("{other:?} connects first"),
    }
}

#[cfg(test)]
#[path = "herdr_keys_tests.rs"]
mod tests;
