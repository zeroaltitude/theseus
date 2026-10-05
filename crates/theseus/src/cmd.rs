//! One function per `theseus` subcommand (theseus-0g4, finding 11). Each
//! makes its requests and prints the answer: as the daemon sent it under
//! `--json`, and as lines otherwise (`output`). `run` in `main.rs` only
//! matches; what the lines say is the library's `render`, and `print.rs`
//! writes them.

use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use theseus_client::render::{self, Frame};
use theseus_client::{outcome, CallError, Conn};
use theseus_protocol::judge::{JudgeGetParams, JudgeGetResult, JudgeListParams, JudgeListResult};
use theseus_protocol::{
    method, notify, ActionConfirmParams, ActionConfirmResult, CatalogListResult, ConfirmListResult,
    Event, HealthResult, LedgerTailParams, LedgerTailResult, Message, ProfileListResult,
    ProfileUseParams, SessionHistoryParams, SessionHistoryResult, SessionListResult,
    SessionOpenParams, SessionRecompileParams, SessionRef, ToolListResult, TurnSubmitParams,
    TurnSubmitResult,
};

use crate::print::{self, Mode, Printer};
use crate::{
    AskArgs, AwsCmd, ConfirmArgs, ExecutionsCmd, IndexCmd, JudgeCmd, MemoryCmd, PolicyCmd,
    ProfileCmd, SessionsCmd,
};

/// The answer as the daemon sent it under `--json`; else `lines`, given it
/// decoded.
pub(crate) fn output<T: DeserializeOwned>(
    json: bool,
    v: Value,
    lines: impl FnOnce(T) -> Result<()>,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    lines(serde_json::from_value(v)?)
}

/// `theseus ask`: one turn, streamed (theseus-9g2 for `--attach`). A turn
/// the model did not end returns how it ended as an `outcome::Ended`, which
/// the process's exit code says (theseus-n88g.2). `spawned`: the daemon is
/// this process's own (`--spawn`), so a signal stops the turn
/// (`submit_stoppable`).
pub async fn ask(
    conn: &mut Conn,
    json: bool,
    no_stream: bool,
    a: AskArgs,
    spawned: bool,
) -> Result<()> {
    let attachments = a
        .attach
        .iter()
        .map(|p| attachment_for(p))
        .collect::<Result<Vec<_>>>()?;
    let prompt = match a.prompt.as_deref() {
        None | Some("-") => read_stdin_prompt()?,
        Some(p) => p.to_string(),
    };
    let params = serde_json::to_value(TurnSubmitParams {
        carried: false,
        prompt: None,
        session_id: a.session,
        input: prompt,
        profile: a.profile,
        provider: a.provider,
        model: a.model,
        author: None,
        attachments,
        reply_to: None,
        // Inside a job, its session (theseus-b5cl).
        opened_from: theseus_client::client::job_session(),
    })?;
    stream_turn(conn, json, no_stream, params, a.thinking, a.trace, spawned).await
}

/// One `turn.submit`, streamed as `ask` prints it: the reply as it comes (or
/// as JSON, or quiet), the status line, the trace, and the exit code that
/// says how the turn ended. `theseus prompt` shares it.
pub(crate) async fn stream_turn(
    conn: &mut Conn,
    json: bool,
    no_stream: bool,
    params: Value,
    thinking: bool,
    trace: bool,
    spawned: bool,
) -> Result<()> {
    let stream = !no_stream && !json;
    let mode = match (json, stream) {
        (true, _) => Mode::Json,
        (false, true) => Mode::Text,
        (false, false) => Mode::Quiet,
    };
    let mut printer = Printer::new(mode, thinking);
    let call = if spawned {
        submit_stoppable(conn, params, &mut printer).await
    } else {
        conn.call(method::TURN_SUBMIT, params, |m, p| printer.on(m, p))
            .await
    };
    printer.settle();
    let result = match call {
        Ok(v) => v,
        Err(err) => {
            if trace {
                failure_trace(&err);
            }
            return Err(err);
        }
    };
    let r: TurnSubmitResult = serde_json::from_value(result.clone())?;
    if json {
        println!("{}", serde_json::to_string(&result)?);
    } else if stream {
        eprintln!("{}", render::status_line(&r));
    } else {
        println!("{}", r.output);
        eprintln!("{}", render::status_line(&r));
    }
    if let (Some(corr), false) = (&r.awaiting_confirm, json) {
        eprintln!(
            "[parked: waiting for your answer on {corr} · `theseus confirm {corr}` or `--decline`; the turn resumes on its own]"
        );
    }
    if trace && !json {
        if let Some(t) = &r.trace {
            eprintln!("--- trace ({} total)", render::fmt_us(t.duration_us()));
            print::lines(
                &mut io::stderr(),
                &render::span_lines(t, 0, &Frame::turn(t)),
            )?;
        }
    }
    match outcome::TurnEnd::of(&r) {
        outcome::TurnEnd::Done => Ok(()),
        end => Err(outcome::Ended {
            end,
            stop_reason: r.stop_reason,
        }
        .into()),
    }
}

/// `turn.submit` under `--spawn`, whose daemon ends with this process
/// (theseus-n88g.2): the first SIGINT or SIGTERM stops the turn as `/stop`
/// does (`execution.stop`, once `turn.started` has named its execution). The
/// turn then ends `stopped`, with what it spent, and nothing is left for the
/// store's next open to resume. A second signal ends the run at once
/// (`outcome::Signalled`); the daemon still gets its clean stop after.
async fn submit_stoppable(conn: &mut Conn, params: Value, printer: &mut Printer) -> Result<Value> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let id = conn.send(method::TURN_SUBMIT, params).await?;
    let mut execution: Option<String> = None;
    // The signal that asked for the stop, and whether it was sent.
    let mut stop: Option<(i32, bool)> = None;
    loop {
        let signal = tokio::select! {
            msg = conn.next() => {
                match msg? {
                    None => return Err(anyhow!("connection closed before response")),
                    Some(Message::Response(r)) if r.id == id => {
                        return theseus_client::client::answer(r);
                    }
                    Some(Message::Notification(n)) => {
                        if n.method == notify::TURN_STARTED && execution.is_none() {
                            execution = n.params["execution_id"].as_str().map(str::to_string);
                        }
                        printer.on(&n.method, &n.params);
                    }
                    Some(_) => {}
                }
                None
            }
            _ = sigint.recv() => Some(libc::SIGINT),
            _ = sigterm.recv() => Some(libc::SIGTERM),
        };
        if let Some(s) = signal {
            if stop.is_some() {
                return Err(outcome::Signalled { signal: s }.into());
            }
            eprintln!(
                "theseus: {}: stopping the turn; a second signal ends the run at once",
                outcome::signal_name(s)
            );
            stop = Some((s, false));
        }
        if let (Some((s, false)), Some(exec)) = (stop, &execution) {
            let author = format!("cli:{}", outcome::signal_name(s));
            let params = serde_json::json!({"execution_id": exec, "author": author});
            conn.send(method::EXECUTION_STOP, params).await?;
            stop = Some((s, true));
        }
    }
}

/// A failed turn's trace, from its error's data, when the daemon sent one.
fn failure_trace(err: &anyhow::Error) {
    let Some(ce) = err.downcast_ref::<CallError>() else {
        return;
    };
    if let Ok(t) = serde_json::from_value::<theseus_protocol::Span>(
        ce.data.get("trace").cloned().unwrap_or(Value::Null),
    ) {
        eprintln!(
            "--- trace up to the failure ({} total)",
            render::fmt_us(t.duration_us())
        );
        let _ = print::lines(
            &mut io::stderr(),
            &render::span_lines(&t, 0, &Frame::turn(&t)),
        );
    }
}

/// `theseus history`: a session's transcript, and what waits in it.
pub async fn history(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    n: Option<usize>,
    full: bool,
) -> Result<()> {
    let session_id = resolve_session(conn, session).await?;
    let v = conn
        .request(
            method::SESSION_HISTORY,
            SessionHistoryParams { session_id, n },
        )
        .await?;
    output(json, v, |h: SessionHistoryResult| {
        let mut out = io::stdout().lock();
        writeln!(out, "{}", render::session_header(&h.session))?;
        for node in &h.nodes {
            print::lines(&mut out, &render::node_lines(node, full))?;
        }
        for c in &h.pending_confirms {
            print::lines(&mut out, &render::confirm_lines(c))?;
        }
        Ok(())
    })
}

/// `theseus watch`: a session's events as they come, until the connection
/// closes.
pub async fn watch(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    thinking: bool,
) -> Result<()> {
    let sid = resolve_session(conn, session).await?;
    conn.request(
        method::SESSION_WATCH,
        SessionRef {
            session_id: sid.clone(),
        },
    )
    .await?;
    eprintln!("watching {sid} (Ctrl-C to stop)");
    let mut printer = Printer::new(Mode::Watch, thinking);
    while let Some(msg) = conn.next().await? {
        if let Message::Notification(n) = msg {
            if json {
                println!("{}", serde_json::to_string(&n)?);
            } else if n.method == notify::EVENTS_LOST {
                // This terminal fell behind (theseus-in3): what it missed is
                // in the history; the watch goes on.
                let lost: theseus_protocol::EventsLost = serde_json::from_value(n.params)?;
                printer.settle();
                eprintln!(
                    "{} · `theseus history {sid}` shows what happened",
                    render::lost_line(&lost)
                );
            } else {
                printer.on(&n.method, &n.params);
            }
        }
    }
    Ok(())
}

/// The snapshot's limit for a read of every view.
const EVERY: u32 = u32::MAX;

/// `theseus watch --all` (theseus-in3): the board's snapshot, then each
/// change. A view is printed only if its position is greater than the last
/// one printed for its execution, so an event that came before the snapshot,
/// or one the snapshot already holds, prints once.
pub async fn watch_all(conn: &mut Conn, json: bool) -> Result<()> {
    let mut early: Vec<(String, Value)> = Vec::new();
    let v = conn
        .call(method::EXECUTIONS_WATCH, serde_json::json!({}), |m, p| {
            early.push((m.to_string(), p.clone()))
        })
        .await?;
    let mut seen: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let show =
        |m: &str, p: &Value, seen: &mut std::collections::HashMap<String, u64>| -> Result<()> {
            if json {
                println!("{}", serde_json::json!({"method": m, "params": p}));
                return Ok(());
            }
            match theseus_protocol::Event::from_notification(m, p)? {
                Some(theseus_protocol::Event::ExecutionChanged(view)) => {
                    let last = seen.entry(view.execution_id.clone()).or_default();
                    if view.position > *last {
                        *last = view.position;
                        println!("{}", render::view_line(&view).text);
                    }
                }
                Some(e) => {
                    if let Some(line) = render::question_line(&e) {
                        println!("{line}");
                    }
                }
                None => {}
            }
            Ok(())
        };
    let snap: theseus_protocol::ExecutionsWatchResult = serde_json::from_value(v.clone())?;
    if json {
        println!("{}", serde_json::json!({"result": v}));
    } else {
        eprintln!(
            "watching every session's executions: {} of {} at position {}, {} question(s) waiting (Ctrl-C to stop)",
            snap.executions.len(),
            snap.total,
            snap.position,
            snap.confirms.len()
        );
    }
    for view in &snap.executions {
        let last = seen.entry(view.execution_id.clone()).or_default();
        if view.position > *last {
            *last = view.position;
            if !json {
                println!("{}", render::view_line(view).text);
            }
        }
    }
    for (m, p) in early {
        show(&m, &p, &mut seen)?;
    }
    while let Some(msg) = conn.next().await? {
        let Message::Notification(n) = msg else {
            continue;
        };
        show(&n.method, &n.params, &mut seen)?;
        // This terminal fell behind (theseus-in3): say so, and read the board
        // again; the position rule prints only what changed.
        if n.method == notify::EVENTS_LOST && !json {
            let lost: theseus_protocol::EventsLost = serde_json::from_value(n.params.clone())?;
            eprintln!("{}", render::lost_line(&lost));
            // Every view this time, not the default 200: a session that
            // changed while this terminal was behind prints, however idle.
            let v = conn
                .call(
                    method::EXECUTIONS_WATCH,
                    serde_json::json!({"limit": EVERY}),
                    |m, p| {
                        let _ = show(m, p, &mut seen);
                    },
                )
                .await?;
            let snap: theseus_protocol::ExecutionsWatchResult = serde_json::from_value(v)?;
            for view in &snap.executions {
                let last = seen.entry(view.execution_id.clone()).or_default();
                if view.position > *last {
                    *last = view.position;
                    println!("{}", render::view_line(view).text);
                }
            }
        }
    }
    Ok(())
}

/// `theseus confirm`: everything waiting, or an answer, then the turn it
/// resumes.
pub async fn confirm(conn: &mut Conn, json: bool, a: ConfirmArgs) -> Result<()> {
    let Some(correlation_id) = a.correlation_id else {
        return confirm_list(conn, json).await;
    };
    let mode = if json { Mode::Json } else { Mode::Text };
    let mut printer = Printer::new(mode, false);
    let v = conn
        .call(
            method::ACTION_CONFIRM,
            serde_json::to_value(ActionConfirmParams {
                correlation_id,
                approve: !a.decline,
                note: a.note,
                watch: !a.no_wait,
                author: None,
                discord: None,
                trust: a.trust,
            })?,
            |m, p| printer.on(m, p),
        )
        .await?;
    let r: ActionConfirmResult = serde_json::from_value(v.clone())?;
    if !r.resumes && r.approved {
        // A proposed extension's ack (M7 43a): nothing loads, and no turn
        // resumes.
        return output(json, v, |r: ActionConfirmResult| {
            println!(
                "approved {} · session {} · nothing resumes: an extension's ack loads nothing \
                 in this build",
                r.correlation_id, r.session_id
            );
            Ok(())
        });
    }
    if !r.resumes {
        // A declined budget question: nothing resumes until a new message.
        return output(json, v, |r: ActionConfirmResult| {
            println!(
                "declined {} · session {} keeps waiting on its budget; a new message asks again",
                r.correlation_id, r.session_id
            );
            Ok(())
        });
    }
    if a.no_wait {
        return output(json, v, |r: ActionConfirmResult| {
            println!(
                "{} {} · session {}",
                if r.approved { "approved" } else { "declined" },
                r.correlation_id,
                r.session_id
            );
            Ok(())
        });
    }
    eprintln!(
        "{} {} · following the resumed turn in {}",
        if r.approved { "approved" } else { "declined" },
        r.correlation_id,
        r.session_id
    );
    follow_resumed(conn, json, &mut printer, &r.session_id).await
}

/// Everything waiting for an answer, across sessions (`confirm.list`).
async fn confirm_list(conn: &mut Conn, json: bool) -> Result<()> {
    let waiting = serde_json::from_value::<ConfirmListResult>(
        conn.request(method::CONFIRM_LIST, Value::Null).await?,
    )?
    .confirms;
    if json {
        println!("{}", serde_json::to_string(&waiting)?);
    } else if waiting.is_empty() {
        println!("nothing is waiting for confirmation");
    } else {
        let mut out = io::stdout().lock();
        for c in &waiting {
            print::lines(&mut out, &render::confirm_lines(c))?;
        }
    }
    Ok(())
}

/// The turn an answer resumed: the server subscribed this connection before
/// waking the execution, so read until the continuation turn ends, fails,
/// or parks on another confirm, for 15 minutes at most.
async fn follow_resumed(
    conn: &mut Conn,
    json: bool,
    printer: &mut Printer,
    session_id: &str,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(900);
    loop {
        let msg = match tokio::time::timeout_at(deadline, conn.next()).await {
            Ok(m) => m?,
            Err(_) => {
                printer.settle();
                eprintln!(
                    "[still running after 15 min · follow it with `theseus watch {session_id}`]"
                );
                return Ok(());
            }
        };
        let Some(Message::Notification(n)) = msg else {
            if msg.is_none() {
                return Ok(());
            }
            continue;
        };
        let event = Event::from_notification(&n.method, &n.params)
            .ok()
            .flatten();
        if let Some(e) = &event {
            printer.on_event(e);
        }
        match event {
            Some(Event::TurnEnded(t)) => {
                printer.settle();
                if json {
                    println!("{}", serde_json::to_string(&t)?);
                } else {
                    eprintln!("{}", render::status_line(&t));
                    if let Some(c) = &t.awaiting_confirm {
                        eprintln!("[parked again: `theseus confirm {c}` or `--decline`]");
                    }
                }
                return Ok(());
            }
            Some(Event::TurnFailed(f)) => {
                printer.settle();
                anyhow::bail!("the resumed turn failed: {}", f.error);
            }
            // An end this build cannot read still ends the turn.
            _ if n.method == notify::TURN_ENDED => {
                printer.settle();
                return Ok(());
            }
            _ if n.method == notify::TURN_FAILED => {
                printer.settle();
                anyhow::bail!("the resumed turn failed: ?");
            }
            _ => {}
        }
    }
}

/// `theseus tools`: the toollets, their postures, and calls so far.
pub async fn tools(conn: &mut Conn, json: bool, verbose: bool) -> Result<()> {
    let v = conn.request(method::TOOL_LIST, Value::Null).await?;
    output(json, v, |l: ToolListResult| {
        println!(
            "{:<12} {:<12} {:<6} {:<7} {:<8} {:>6}",
            "tool", "wire name", "class", "backend", "posture", "calls"
        );
        for t in &l.tools {
            // `*`: a "should have asked" press set it (theseus policy list).
            let posture = match &t.tightened {
                Some(_) if t.policy != t.config_posture => format!("{}*", t.policy),
                _ => t.policy.clone(),
            };
            println!(
                "{:<12} {:<12} {:<6} {:<7} {:<8} {:>6}",
                t.name, t.wire_name, t.class, t.backend, posture, t.calls
            );
            if verbose {
                println!("    {}", t.description);
                println!("    input: {}", serde_json::to_string(&t.input_schema)?);
            }
        }
        eprintln!(
            "[roots: {} · {} call(s) · shell-fallback ratio {:.0}% (proc.run / all calls)]",
            l.roots.join(", "),
            l.calls_total,
            l.shell_fallback_ratio * 100.0
        );
        Ok(())
    })
}

/// `theseus aws bootstrap` (AWS design §5, C2): the plan, read-only; then,
/// on a terminal and when the plan changes something, the question, and the
/// apply of exactly that plan, bound to its digest.
pub async fn aws(conn: &mut Conn, json: bool, cmd: AwsCmd) -> Result<()> {
    use std::io::IsTerminal;
    let (account, alert_email, trail_key, apply, plan_only) = match cmd {
        AwsCmd::Bootstrap {
            account,
            alert_email,
            trail_key,
            apply,
            plan_only,
        } => (account, alert_email, trail_key, apply, plan_only),
        AwsCmd::ConfirmAlerts { token, account } => {
            return confirm_alerts(conn, json, token, account).await
        }
    };
    let params = |apply: Option<String>| theseus_protocol::AwsBootstrapParams {
        account: account.clone(),
        alert_email: alert_email.clone(),
        trail_key: trail_key.clone(),
        apply,
    };
    let applying = apply.is_some();
    let v = conn.request(method::AWS_BOOTSTRAP, params(apply)).await?;
    let r: theseus_protocol::AwsBootstrapResult = if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    } else {
        serde_json::from_value(v)?
    };
    for line in render::bootstrap_lines(&r) {
        println!("{line}");
    }
    if applying || plan_only || !r.changes || !io::stdin().is_terminal() {
        return Ok(());
    }
    print!(
        "Apply this plan to account {}? It writes the stacks above. [y/N] ",
        r.account
    );
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("Nothing was applied.");
        return Ok(());
    }
    println!(
        "Applying plan {}: each stack takes a few minutes.",
        r.digest
    );
    let v = conn
        .request(method::AWS_BOOTSTRAP, params(Some(r.digest.clone())))
        .await?;
    let done: theseus_protocol::AwsBootstrapResult = serde_json::from_value(v)?;
    for line in render::bootstrap_lines(&done) {
        println!("{line}");
    }
    Ok(())
}

/// `theseus aws confirm-alerts` (theseus-9p40): the token from SNS's
/// confirmation email (or its link), from the command line or stdin, sent to
/// the daemon, which confirms the subscription authenticated on
/// unsubscribe. The token is never printed.
async fn confirm_alerts(
    conn: &mut Conn,
    json: bool,
    token: Option<String>,
    account: Option<String>,
) -> Result<()> {
    use std::io::IsTerminal;
    let token = match token {
        Some(t) => t,
        None => {
            if io::stdin().is_terminal() {
                eprint!("Paste the confirmation link's address, or its Token=… value: ");
                io::stderr().flush()?;
            }
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            line
        }
    };
    let v = conn
        .request(
            method::AWS_CONFIRM_ALERTS,
            theseus_protocol::AwsConfirmAlertsParams { account, token },
        )
        .await?;
    output(json, v, |r: theseus_protocol::AwsConfirmAlertsResult| {
        println!(
            "Confirmed {}'s subscription to {} ({}).",
            r.endpoint.as_deref().unwrap_or("the alert address"),
            r.topic,
            r.subscription
        );
        if r.authenticated {
            println!(
                "SNS says ConfirmationWasAuthenticated = true: only the account can unsubscribe \
                 it, so an alert's unsubscribe link, or a scanner that follows it, cannot."
            );
            Ok(())
        } else {
            Err(anyhow!(
                "SNS says ConfirmationWasAuthenticated = false: the subscription was confirmed \
                 before without authentication (its link was opened), so an alert's unsubscribe \
                 link still works. Subscribe the address again and confirm the new token here."
            ))
        }
    })
}

/// `theseus policy`: the postures, a tightening and its undo, and a trust.
pub async fn policy(conn: &mut Conn, json: bool, cmd: PolicyCmd) -> Result<()> {
    match cmd {
        PolicyCmd::List => policy_list(conn, json).await,
        PolicyCmd::Explain { session, tool } => {
            crate::policy_explain::explain(conn, json, session, tool).await
        }
        PolicyCmd::Tighten { tool, call } => {
            let v = conn
                .request(
                    method::POLICY_TIGHTEN,
                    theseus_protocol::PolicyTightenParams {
                        tool,
                        correlation_id: call,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, |r| {
                println!("{}", render::tightened_line(&r, true));
                Ok(())
            })
        }
        PolicyCmd::Untighten { tool } => {
            let v = conn
                .request(
                    method::POLICY_UNTIGHTEN,
                    theseus_protocol::PolicyUntightenParams {
                        tool,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, |r| {
                println!("{}", render::tightened_line(&r, false));
                Ok(())
            })
        }
        PolicyCmd::Trust { session } => {
            // A short name is looked for among the sessions that hold
            // external text (theseus-9bp), which health lists.
            let h: HealthResult =
                serde_json::from_value(conn.request(method::HEALTH, Value::Null).await?)?;
            let session_id = trust_target(&h.external_text, &session)?;
            let v = conn
                .request(
                    method::POLICY_TRUST,
                    theseus_protocol::PolicyTrustParams {
                        session_id,
                        author: None,
                        discord: None,
                    },
                )
                .await?;
            output(json, v, |r| {
                println!("{}", render::trusted_line(&r));
                Ok(())
            })
        }
    }
}

/// Every tool's posture now and what set it, with the tightenings, and the
/// sessions that hold external text.
async fn policy_list(conn: &mut Conn, json: bool) -> Result<()> {
    let tools = conn.request(method::TOOL_LIST, Value::Null).await?;
    let health = conn.request(method::HEALTH, Value::Null).await?;
    if json {
        println!(
            "{}",
            serde_json::json!({"tools": tools["tools"], "tightenings": health["tightenings"]})
        );
        return Ok(());
    }
    let l: ToolListResult = serde_json::from_value(tools)?;
    let h: HealthResult = serde_json::from_value(health)?;
    print!("{}", render::policy_list(&l, &h.tightenings));
    if let Some(line) = render::external_line(&h.external_text) {
        println!("{line}");
    }
    Ok(())
}

/// `theseus catalog`: models, windows, and prices, the code's unless the
/// config's `[catalog]` tables change them, and a line for each such table.
pub async fn catalog(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn.request(method::CATALOG_LIST, Value::Null).await?;
    output(json, v, |l: CatalogListResult| {
        println!(
            "{:<24} {:<10} {:>9} {:>8} {:>7} {:>7} {:>7} {:>7}  {:<9} profiles",
            "model", "provider", "window", "max out", "$in", "$out", "$c.rd", "$c.wr", "thinking"
        );
        for m in &l.models {
            let e = &m.entry;
            let num = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            println!(
                "{:<24} {:<10} {:>9} {:>8} {:>7} {:>7} {:>7} {:>7}  {:<9} {}",
                m.model,
                e.get("provider").and_then(Value::as_str).unwrap_or("?"),
                render::fmt_tokens(num("context_window") as u64),
                render::fmt_tokens(num("max_output_tokens") as u64),
                render::fmt_price(num("input_per_mtok")),
                render::fmt_price(num("output_per_mtok")),
                render::fmt_price(num("cache_read_per_mtok")),
                render::fmt_price(num("cache_write_per_mtok")),
                e.get("thinking").and_then(Value::as_str).unwrap_or("?"),
                m.profiles.join(",")
            );
        }
        eprintln!(
            "[catalog {} · prices are USD per million tokens]",
            l.version
        );
        for line in render::catalog_config_lines(&l) {
            eprintln!("[{line}]");
        }
        Ok(())
    })
}

/// `theseus health`.
pub async fn health(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn.request(method::HEALTH, Value::Null).await?;
    output(json, v, |h: HealthResult| {
        print::lines(
            &mut io::stdout().lock(),
            &render::health_lines(&h, theseus_protocol::now_unix_ms()),
        )?;
        Ok(())
    })
}

/// `theseus index status` and `theseus index search` (roadmap row 51): the
/// core answers each by asking its index tender.
pub async fn index(conn: &mut Conn, json: bool, cmd: IndexCmd) -> Result<()> {
    use theseus_protocol::index::{IndexHealth, IndexQueryParams, IndexQueryResult};
    match cmd {
        IndexCmd::Status => {
            let v = conn.request(method::INDEX_STATUS, Value::Null).await?;
            output(json, v, |h: IndexHealth| {
                print::lines(&mut io::stdout().lock(), &render::index_status_lines(&h))?;
                Ok(())
            })
        }
        IndexCmd::Search {
            query,
            k,
            as_of,
            sources,
        } => {
            let text = query.join(" ");
            let mut p = IndexQueryParams::new(&text);
            p.k = k;
            p.as_of = as_of;
            p.sources = sources;
            let v = conn
                .request(method::INDEX_QUERY, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: IndexQueryResult| {
                print::lines(
                    &mut io::stdout().lock(),
                    &render::index_hits_lines(&text, &r),
                )?;
                Ok(())
            })
        }
    }
}

/// `theseus memory search` and `theseus memory recalled` (M6 step 30a):
/// recall's pipeline for a query, and a session's recalls in shadow.
pub async fn memory(conn: &mut Conn, json: bool, cmd: MemoryCmd) -> Result<()> {
    use theseus_protocol::memory::{
        MemoryLabelParams, MemoryLabelResult, MemoryRecallsParams, MemoryRecallsResult,
        MemorySearchParams, RecallManifest,
    };
    match cmd {
        MemoryCmd::Search { query, session, k } => {
            let p = MemorySearchParams {
                query: query.join(" "),
                session_id: session,
                k: Some(k),
            };
            let v = conn
                .request(method::MEMORY_SEARCH, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |m: RecallManifest| {
                print::lines(&mut io::stdout().lock(), &render::recall_lines(&m))?;
                Ok(())
            })
        }
        MemoryCmd::Recalled { session, limit } => {
            let p = MemoryRecallsParams {
                session_id: session,
                limit: Some(limit),
            };
            let v = conn
                .request(method::MEMORY_RECALLS, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: MemoryRecallsResult| {
                print::lines(&mut io::stdout().lock(), &render::recalls_lines(&r))?;
                Ok(())
            })
        }
        MemoryCmd::Label {
            node,
            label,
            recall,
            note,
        } => {
            let p = MemoryLabelParams {
                node_id: node,
                label,
                recall_id: recall,
                note,
            };
            let v = conn
                .request(method::MEMORY_LABEL, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: MemoryLabelResult| {
                let then = if r.excluded {
                    "recall leaves it out from the next turn on"
                } else {
                    "recall may offer it"
                };
                println!("{} labeled {}: {then}", r.node_id, r.label);
                Ok(())
            })
        }
    }
}

/// `theseus sessions`: list, open, or recompile.
pub async fn sessions(conn: &mut Conn, json: bool, cmd: SessionsCmd) -> Result<()> {
    match cmd {
        SessionsCmd::List => {
            let v = conn.request(method::SESSION_LIST, Value::Null).await?;
            output(json, v, |l: SessionListResult| {
                for s in l.sessions {
                    println!("{}", render::session_row(&s).text);
                }
                Ok(())
            })
        }
        SessionsCmd::Open { label } => {
            let v = conn
                .request(
                    method::SESSION_OPEN,
                    SessionOpenParams {
                        kind: None,
                        label,
                        opened_from: theseus_client::client::job_session(),
                    },
                )
                .await?;
            output(json, v, |v: Value| {
                println!(
                    "{}",
                    v.get("session_id").and_then(Value::as_str).unwrap_or("")
                );
                Ok(())
            })
        }
        SessionsCmd::Recompile { session, strategy } => {
            let v = conn
                .request(
                    method::SESSION_RECOMPILE,
                    SessionRecompileParams {
                        session_id: session.clone(),
                        strategy: strategy.clone(),
                    },
                )
                .await?;
            output(json, v, |_: Value| {
                println!("{session}: the next turn recompiles ({strategy})");
                Ok(())
            })
        }
    }
}

/// `theseus executions`: list, or cancel one.
pub async fn executions(conn: &mut Conn, json: bool, cmd: ExecutionsCmd) -> Result<()> {
    match cmd {
        ExecutionsCmd::List => {
            let v = conn.request(method::EXECUTION_LIST, Value::Null).await?;
            output(json, v, |l: theseus_protocol::ExecutionListResult| {
                if l.executions.is_empty() {
                    println!("no executions");
                }
                for e in l.executions {
                    println!("{}", render::execution_row(&e).text);
                }
                Ok(())
            })
        }
        ExecutionsCmd::Explain { id } => explain(conn, json, &id).await,
        ExecutionsCmd::Cancel { execution_id } => {
            let v = conn
                .request(
                    method::EXECUTION_CANCEL,
                    theseus_protocol::ExecutionCancelParams {
                        execution_id,
                        author: None,
                    },
                )
                .await?;
            output(json, v, |r: theseus_protocol::ExecutionCancelResult| {
                println!(
                    "{} {} · {} action(s) asked to stop{}",
                    r.execution.execution_id,
                    r.execution.state,
                    r.cancelled_actions.len(),
                    r.execution
                        .ended_reason
                        .map(|x| format!(" · {x}"))
                        .unwrap_or_default()
                );
                for line in render::verdict_lines("cancelled", &r.verdicts) {
                    println!("{line}");
                }
                Ok(())
            })
        }
    }
}

/// `theseus executions explain ID` (theseus-in3): one execution in full,
/// from four reads.
async fn explain(conn: &mut Conn, json: bool, id: &str) -> Result<()> {
    let l: theseus_protocol::ExecutionListResult =
        serde_json::from_value(conn.request(method::EXECUTION_LIST, Value::Null).await?)?;
    let e = explain_target(&l.executions, id)?;
    let confirms: ConfirmListResult =
        serde_json::from_value(conn.request(method::CONFIRM_LIST, Value::Null).await?)?;
    let wakes: theseus_protocol::WakeListResult = serde_json::from_value(
        conn.request(
            method::WAKE_LIST,
            theseus_protocol::WakeListParams {
                session_id: Some(e.session_id.clone()),
                target: None,
            },
        )
        .await?,
    )?;
    let rows: LedgerTailResult = serde_json::from_value(
        conn.request(
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(8),
                kind: None,
                session_id: Some(e.session_id.clone()),
                after: None,
                ..Default::default()
            },
        )
        .await?,
    )?;
    let asks: Vec<_> = confirms
        .confirms
        .into_iter()
        .filter(|c| c.execution_id == e.execution_id)
        .collect();
    if json {
        // Its actions, each a job's with its completion's egress (18c).
        let params = theseus_protocol::ActionListParams {
            execution_id: Some(e.execution_id.clone()),
            n: None,
        };
        let actions = conn.request(method::ACTION_LIST, params).await?;
        println!(
            "{}",
            serde_json::json!({"execution": e, "confirms": asks, "wakes": wakes.wakes, "rows": rows.rows,
                "actions": actions["actions"]})
        );
        return Ok(());
    }
    let now = theseus_protocol::now_unix_ms();
    let mut out = io::stdout().lock();
    print::lines(&mut out, &render::explain_lines(&e, &wakes.wakes, now))?;
    if asks.is_empty() {
        writeln!(out, "questions: none")?;
    } else {
        writeln!(out, "questions:")?;
        for c in &asks {
            print::lines(&mut out, &render::confirm_lines(c))?;
        }
    }
    writeln!(out, "last rows of its session:")?;
    for r in &rows.rows {
        writeln!(out, "  {}", render::ledger_row(r)?)?;
    }
    Ok(())
}

/// The execution `explain` names: an execution's or a session's id, or at
/// least the last four characters of either.
fn explain_target(
    execs: &[theseus_protocol::ExecutionInfo],
    id: &str,
) -> Result<theseus_protocol::ExecutionInfo> {
    let s = id.trim().trim_start_matches('…');
    if s.len() < 4 {
        anyhow::bail!("`{id}` is too short: give at least four characters of an id");
    }
    let exact: Vec<_> = execs
        .iter()
        .filter(|e| e.execution_id == s || e.session_id == s)
        .collect();
    let found: Vec<_> = if exact.is_empty() {
        execs
            .iter()
            .filter(|e| e.execution_id.ends_with(s) || e.session_id.ends_with(s))
            .collect()
    } else {
        exact
    };
    match found.as_slice() {
        [] => anyhow::bail!("no execution or session is named `{id}`"),
        [one] => Ok((*one).clone()),
        many => anyhow::bail!(
            "`{id}` names {} executions: give more of its id",
            many.len()
        ),
    }
}

/// `theseus wait SESSION` (theseus-in3): `session.wait`, then the state it
/// reached. A timeout exits 4.
pub async fn wait(
    conn: &mut Conn,
    json: bool,
    session: String,
    until: String,
    after: Option<u64>,
    timeout: Option<String>,
) -> Result<()> {
    let l: theseus_protocol::ExecutionListResult =
        serde_json::from_value(conn.request(method::EXECUTION_LIST, Value::Null).await?)?;
    let session_id = explain_target(&l.executions, &session)?.session_id;
    let timeout_ms = timeout.as_deref().map(parse_duration_ms).transpose()?;
    let until = match until.as_str() {
        "blocked" => theseus_protocol::WaitUntil::Blocked,
        "terminal" => theseus_protocol::WaitUntil::Terminal,
        _ => theseus_protocol::WaitUntil::Settled,
    };
    let v = conn
        .request(
            method::SESSION_WAIT,
            theseus_protocol::SessionWaitParams {
                session_id,
                until,
                after_position: after,
                timeout_ms,
            },
        )
        .await?;
    let r: theseus_protocol::SessionWaitResult = serde_json::from_value(v.clone())?;
    if json {
        println!("{v}");
    } else {
        println!("{}", render::waited_line(&r).text);
        let mut out = io::stdout().lock();
        for c in &r.confirms {
            print::lines(&mut out, &render::confirm_lines(c))?;
        }
    }
    if r.reached == "timeout" {
        // Exit 4: the wait timed out (0 ok, 1 error, 2 usage, 3 cannot connect).
        std::process::exit(4);
    }
    Ok(())
}

/// `90s`, `10m`, `2h`, `1500ms`, or bare seconds, in milliseconds.
fn parse_duration_ms(s: &str) -> Result<u64> {
    let t = s.trim();
    let (n, unit) = t
        .find(|c: char| !c.is_ascii_digit())
        .map_or((t, ""), |i| t.split_at(i));
    let n: u64 = n
        .parse()
        .map_err(|_| anyhow!("`{s}` is no duration: write 90s, 10m, or 2h"))?;
    Ok(match unit {
        "" | "s" => n * 1000,
        "ms" => n,
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        _ => anyhow::bail!("`{s}` is no duration: write 90s, 10m, or 2h"),
    })
}

/// `theseus stop SESSION`: the session's execution, as Discord's `/stop`
/// (W1).
pub async fn stop(conn: &mut Conn, json: bool, session: String) -> Result<()> {
    // The session's open execution: one per session (W1).
    let l: theseus_protocol::ExecutionListResult =
        serde_json::from_value(conn.request(method::EXECUTION_LIST, Value::Null).await?)?;
    let execution_id = stop_target(&l.executions, &session)?;
    let v = conn
        .request(
            method::EXECUTION_STOP,
            theseus_protocol::ExecutionStopParams {
                execution_id,
                author: None,
            },
        )
        .await?;
    output(json, v, |r: theseus_protocol::ExecutionStopResult| {
        println!("{}", render::stop_line(&r));
        for line in render::verdict_lines("stopped", &r.verdicts) {
            println!("{line}");
        }
        Ok(())
    })
}

/// `theseus tasks`.
pub async fn tasks(conn: &mut Conn, json: bool, session: Option<String>) -> Result<()> {
    let v = conn
        .request(
            method::TASK_LIST,
            theseus_protocol::TaskListParams {
                session_id: session,
                target: None,
            },
        )
        .await?;
    output(json, v, |l: theseus_protocol::TaskListResult| {
        if l.tasks.is_empty() && l.records.is_empty() {
            println!("no tasks");
        }
        let now = theseus_protocol::now_unix_ms();
        for t in l.tasks {
            println!("{}", render::task_line(&t, now).text);
            for p in render::task_pieces(&t) {
                println!("{p}");
            }
            // A check's basis (M5 28a).
            for c in render::task_check(&t) {
                println!("{c}");
            }
        }
        // The task graph (39a): every record, as a tree.
        for line in render::task_tree_lines(&l.records) {
            println!("{line}");
        }
        Ok(())
    })
}

/// `theseus wakes`.
pub async fn wakes(conn: &mut Conn, json: bool, session: Option<String>) -> Result<()> {
    let v = conn
        .request(
            method::WAKE_LIST,
            theseus_protocol::WakeListParams {
                session_id: session,
                target: None,
            },
        )
        .await?;
    output(json, v, |l: theseus_protocol::WakeListResult| {
        if l.wakes.is_empty() {
            println!("no pending wakes");
        }
        let now = theseus_protocol::now_unix_ms();
        for w in l.wakes {
            println!("{}", render::wake_line(&w, now));
        }
        Ok(())
    })
}

/// `theseus reach NODE` (theseus-n4m, step 12a): where a node went.
pub async fn reach(
    conn: &mut Conn,
    json: bool,
    node: String,
    generations: Option<u32>,
) -> Result<()> {
    let v = conn
        .request(
            method::NODE_REACH,
            theseus_protocol::NodeReachParams {
                node_id: node,
                max_generations: generations,
            },
        )
        .await?;
    output(json, v, |r: theseus_protocol::NodeReachResult| {
        print::lines(&mut io::stdout().lock(), &render::reach_lines(&r))?;
        Ok(())
    })
}

/// `theseus publish NODE|FILE --to PLACE` (the place rule): `place.publish`.
/// A FILE is an argument that starts with `/`, `~`, or `.`, or names a file
/// here; anything else is a node's id. `--text` publishes a message.
pub async fn publish(
    conn: &mut Conn,
    json: bool,
    what: Option<String>,
    to: String,
    text: Option<String>,
    note: Option<String>,
) -> Result<()> {
    let (node_id, path) = match what {
        None => (None, None),
        Some(w) if w.starts_with('/') || w.starts_with('~') => (None, Some(w)),
        Some(w) if w.starts_with('.') || std::path::Path::new(&w).is_file() => {
            let abs = std::env::current_dir()?.join(&w);
            (None, Some(abs.display().to_string()))
        }
        Some(w) => (Some(w), None),
    };
    let v = conn
        .request(
            method::PLACE_PUBLISH,
            theseus_protocol::PlacePublishParams {
                node_id,
                path,
                text,
                to,
                note,
                author: None,
                discord: None,
            },
        )
        .await?;
    output(json, v, |r: theseus_protocol::PublishResult| {
        println!(
            "Published {} into {} ({}): node {} in session {}, {} bytes, digest {}.",
            r.what,
            r.name,
            r.class.as_str(),
            r.node_id,
            r.session_id,
            r.bytes,
            r.digest
        );
        Ok(())
    })
}

/// `theseus places` (the place rule, theseus-nbsh): health's places, a
/// line each.
pub async fn places(conn: &mut Conn, json: bool) -> Result<()> {
    let h: HealthResult = serde_json::from_value(conn.request(method::HEALTH, Value::Null).await?)?;
    let places = h.places.unwrap_or_default();
    if json {
        println!("{}", serde_json::to_string(&places)?);
        return Ok(());
    }
    for line in render::places_lines(&places) {
        println!("{line}");
    }
    Ok(())
}

/// `theseus judge log` (M5 23a, on `judge.list` since 23b): the newest
/// judgments; `theseus judge show <id>` (23b): one, with its state.
pub async fn judge(conn: &mut Conn, json: bool, cmd: JudgeCmd) -> Result<()> {
    match cmd {
        JudgeCmd::Log { n, session, pack } => {
            let v = conn
                .request(
                    method::JUDGE_LIST,
                    JudgeListParams {
                        pack,
                        session_id: session,
                        since: None,
                        limit: Some(n as u64),
                    },
                )
                .await?;
            output(json, v, |r: JudgeListResult| {
                for line in render::judge_log_lines(&r.judgments) {
                    println!("{line}");
                }
                if r.matched > r.judgments.len() as u64 {
                    println!(
                        "({} of {} judgments in {}; `--n` shows more)",
                        r.judgments.len(),
                        r.matched,
                        r.scopes.join(", ")
                    );
                }
                Ok(())
            })
        }
        JudgeCmd::Show { id } => {
            let v = conn
                .request(method::JUDGE_GET, JudgeGetParams { id })
                .await?;
            output(json, v, |r: JudgeGetResult| {
                for line in render::judge_show_lines(&r) {
                    println!("{line}");
                }
                Ok(())
            })
        }
        JudgeCmd::Label {
            id,
            label,
            question,
            note,
        } => {
            use theseus_protocol::learning::{JudgeLabelParams, JudgeLabelResult};
            let p = JudgeLabelParams {
                judgment: id,
                question,
                label: Value::String(label),
                note,
                discord: None,
            };
            let v = conn
                .request(method::JUDGE_LABEL, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: JudgeLabelResult| {
                println!("{}", render::judge_label_line(&r));
                Ok(())
            })
        }
        JudgeCmd::Report { pack, date } => {
            use theseus_protocol::learning::{LearningReport, LearningReportParams};
            let v = conn
                .request(
                    method::LEARNING_REPORT,
                    serde_json::to_value(LearningReportParams { pack, date })?,
                )
                .await?;
            output(json, v, |r: LearningReport| {
                for line in render::learning_report_lines(&r) {
                    println!("{line}");
                }
                Ok(())
            })
        }
    }
}

/// `theseus cancel ID`: a pending wake first (DD8), then a task. Each is
/// named by the end of its id, and the daemon refuses a name that means
/// both.
pub async fn cancel(conn: &mut Conn, json: bool, name: String) -> Result<()> {
    let wake = conn
        .request(
            method::WAKE_CANCEL,
            theseus_protocol::WakeCancelParams {
                wake: name.clone(),
                author: None,
            },
        )
        .await;
    let no_wake = match wake {
        Ok(v) => {
            return output(json, v, |r: theseus_protocol::WakeCancelResult| {
                println!(
                    "wake {} cancelled: it was due {}: {}",
                    r.wake.short, r.wake.due_local, r.wake.note
                );
                Ok(())
            });
        }
        Err(e) if not_found(&e) => call_message(&e),
        Err(e) => return Err(e),
    };
    let task = conn
        .request(
            method::TASK_CANCEL,
            theseus_protocol::TaskCancelParams {
                task: name.clone(),
                author: None,
            },
        )
        .await;
    let v = match task {
        Ok(v) => v,
        Err(e) if not_found(&e) => {
            // Why each lookup failed, unless it only says so.
            let why: Vec<String> = [call_message(&e), no_wake]
                .into_iter()
                .filter(|m| {
                    !m.starts_with("no task is named") && !m.starts_with("no pending wake is named")
                })
                .collect();
            let why = match why.as_slice() {
                [] => String::new(),
                w => format!(": {}", w.join("; ")),
            };
            return Err(anyhow!("`{name}` names no task and no pending wake{why}"));
        }
        Err(e) => return Err(e),
    };
    output(json, v, |r: theseus_protocol::TaskCancelResult| {
        println!(
            "task {} {} · {} action(s) asked to stop{}",
            r.task.short,
            r.task.state,
            r.cancelled_actions.len(),
            r.task
                .ended_reason
                .map(|x| format!(" · {x}"))
                .unwrap_or_default()
        );
        for line in render::verdict_lines("cancelled", &r.verdicts) {
            println!("{line}");
        }
        Ok(())
    })
}

/// `theseus profile`: list, or switch the live one.
pub async fn profile(conn: &mut Conn, json: bool, cmd: ProfileCmd) -> Result<()> {
    match cmd {
        ProfileCmd::List => {
            let v = conn.request(method::PROFILE_LIST, Value::Null).await?;
            output(json, v, |l: ProfileListResult| {
                for p in l.profiles {
                    println!(
                        "{} {:<12} {:<10} {:<28} max_output_tokens={}{}",
                        if p.live { "*" } else { " " },
                        p.name,
                        p.provider,
                        p.model,
                        p.max_output_tokens,
                        if p.has_system { "  +system" } else { "" }
                    );
                }
                eprintln!("[live: {} (from {})]", l.live, l.live_source);
                Ok(())
            })
        }
        ProfileCmd::Use { name } => {
            let v = conn
                .request(method::PROFILE_USE, ProfileUseParams { name })
                .await?;
            output(json, v, |v: Value| {
                println!(
                    "live profile: {} (was {})",
                    v.get("live").and_then(Value::as_str).unwrap_or("?"),
                    v.get("previous").and_then(Value::as_str).unwrap_or("?")
                );
                Ok(())
            })
        }
    }
}

/// `theseus ledger`: the newest rows.
pub async fn ledger(
    conn: &mut Conn,
    json: bool,
    n: usize,
    kind: Option<String>,
    session: Option<String>,
) -> Result<()> {
    let v = conn
        .request(
            method::LEDGER_TAIL,
            LedgerTailParams {
                n: Some(n),
                kind,
                session_id: session,
                after: None,
                ..Default::default()
            },
        )
        .await?;
    output(json, v, |t: LedgerTailResult| {
        for r in t.rows {
            println!("{}", render::ledger_row(&r)?);
        }
        eprintln!("[{} rows total]", t.total);
        Ok(())
    })
}

/// `theseus rpc METHOD [PARAMS]`: one raw request; its notifications echo
/// to stderr, and its result is printed pretty.
pub async fn rpc(
    conn: &mut Conn,
    json: bool,
    method: String,
    params: Option<String>,
) -> Result<()> {
    let params: Value = match params {
        Some(p) => serde_json::from_str(&p).context("PARAMS must be JSON")?,
        None => Value::Null,
    };
    let v = conn
        .call(&method, params, |m, p| {
            if !json {
                eprintln!("<- {m} {p}");
            }
        })
        .await?;
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

/// `theseus shutdown`: the daemon stops cleanly.
pub async fn shutdown(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn.request(method::SHUTDOWN, Value::Null).await?;
    if json {
        println!("{}", serde_json::to_string(&v)?);
    }
    Ok(())
}

/// `theseus tui` (theseus-7yx, step 10f): become `theseus-tui`, the way git
/// runs its subcommands. It is looked for beside this binary, then on PATH,
/// and runs with this command's `--socket` and every further argument. This
/// returns only when the exec fails; a `theseus-tui` found nowhere exits 2,
/// saying where it looked and how to install it.
pub fn tui(socket: &str, spawn: bool, args: &[String]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;

    const TUI: &str = "theseus-tui";
    if spawn {
        eprintln!(
            "theseus: tui shows a running theseusd's sessions over its socket: it has no --spawn"
        );
        std::process::exit(2);
    }
    let here = std::env::current_exe().map(|exe| exe.with_file_name(TUI));
    let beside = here.as_ref().ok().filter(|p| {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    });
    // Beside this binary, by its path; else by its name, which the exec looks
    // up on PATH.
    let program = beside.cloned().unwrap_or_else(|| TUI.into());
    let err = std::process::Command::new(&program)
        .arg("--socket")
        .arg(socket)
        .args(args)
        .exec();
    if beside.is_none() && err.kind() == io::ErrorKind::NotFound {
        let (looked, dir) = match &here {
            Ok(p) => (
                p.display().to_string(),
                p.parent()
                    .map_or_else(|| ".".into(), |d| d.display().to_string()),
            ),
            Err(e) => (format!("unknown: {e}"), "~/.local/bin".into()),
        };
        eprintln!(
            "theseus: {TUI} is not installed: not beside this binary ({looked}), and not on PATH.\n\
             Install it beside theseus, from a checkout of Theseus:\n  \
             cargo build --release -p {TUI}\n  \
             install -m 755 target/release/{TUI} {dir}/"
        );
        std::process::exit(2);
    }
    Err(err).with_context(|| format!("running {}", program.display()))
}

/// The session a command means: the one named, or the most recently active.
async fn resolve_session(conn: &mut Conn, session: Option<String>) -> Result<String> {
    if let Some(s) = session {
        return Ok(s);
    }
    let l: SessionListResult =
        serde_json::from_value(conn.request(method::SESSION_LIST, Value::Null).await?)?;
    l.sessions
        .first()
        .map(|s| s.session_id.clone())
        .ok_or_else(|| anyhow!("there are no sessions yet"))
}

/// The daemon said no such thing (`NOT_FOUND`).
fn not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<CallError>()
        .is_some_and(|c| c.code == theseus_protocol::error_code::NOT_FOUND)
}

/// What the daemon said, without its code.
fn call_message(e: &anyhow::Error) -> String {
    e.downcast_ref::<CallError>()
        .map_or_else(|| e.to_string(), |c| c.message.clone())
}

/// The execution `theseus stop SESSION` stops (W1): the one of the session
/// named by its id, or by the end of it (four characters at least), when
/// exactly one session matches.
fn stop_target(execs: &[theseus_protocol::ExecutionInfo], session: &str) -> Result<String> {
    let s = session.trim().trim_start_matches('…');
    if s.len() < 4 {
        anyhow::bail!(
            "`{session}` is too short to name a session: give at least four characters of its id"
        );
    }
    let exact: Vec<_> = execs.iter().filter(|e| e.session_id == s).collect();
    let found = if exact.is_empty() {
        execs.iter().filter(|e| e.session_id.ends_with(s)).collect()
    } else {
        exact
    };
    let mut sessions: Vec<&str> = found.iter().map(|e| e.session_id.as_str()).collect();
    sessions.dedup();
    match (found.as_slice(), sessions.len()) {
        ([], _) => anyhow::bail!("no session is named `{session}`"),
        (_, 1) => Ok(found
            .iter()
            .find(|e| e.kind != "task")
            .map_or_else(|| found[0].execution_id.clone(), |e| e.execution_id.clone())),
        (_, n) => anyhow::bail!("`{session}` names {n} sessions: give more of its id"),
    }
}

/// The session `policy trust` names (theseus-9bp): its whole id, or at least
/// its last four characters among the sessions that hold external text.
fn trust_target(held: &[theseus_protocol::ExternalTextInfo], session: &str) -> Result<String> {
    let s = session.trim().trim_start_matches('…');
    if s.len() < 4 {
        anyhow::bail!(
            "`{session}` is too short to name a session: give at least four characters of its id"
        );
    }
    let found: Vec<&str> = held
        .iter()
        .map(|i| i.session_id.as_str())
        .filter(|id| *id == s || id.ends_with(s))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.to_string()),
        [] if s.starts_with("ses_") => Ok(s.to_string()),
        [] => anyhow::bail!(
            "no session that holds external text is named `{session}` (theseus health lists them)"
        ),
        more => anyhow::bail!(
            "`{session}` names {} sessions: give more of its id",
            more.len()
        ),
    }
}

/// The largest file `ask --attach` sends; the daemon caps text further, at
/// `[tools].max_read_bytes`.
const MAX_ATTACH_BYTES: u64 = 16 * 1024 * 1024;

/// One `--attach` file as `turn.submit` carries it (theseus-9g2): a text
/// file's text, or the file listed with the reason it was not read. A file
/// that cannot be opened stops the command, before anything is sent.
fn attachment_for(path: &std::path::Path) -> Result<theseus_protocol::Attachment> {
    let meta = std::fs::metadata(path).with_context(|| format!("--attach {}", path.display()))?;
    if meta.is_dir() {
        return Err(anyhow!("--attach {}: is a directory", path.display()));
    }
    let mut a = theseus_protocol::Attachment {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        size: meta.len(),
        ..Default::default()
    };
    if meta.len() > MAX_ATTACH_BYTES {
        a.not_read = Some("over the 16 MiB limit for --attach".into());
        return Ok(a);
    }
    let bytes = std::fs::read(path).with_context(|| format!("--attach {}", path.display()))?;
    let bytes = match String::from_utf8(bytes) {
        Ok(text) if !text.as_bytes().iter().take(8192).any(|b| *b == 0) => {
            a.media_type = "text/plain".into();
            a.text = Some(text);
            return Ok(a);
        }
        Ok(text) => text.into_bytes(),
        Err(e) => e.into_bytes(),
    };
    // Not text: its bytes, when an image could be this small. The daemon
    // reads the type from the bytes and keeps only an image.
    if bytes.len() as u64 <= theseus_protocol::MAX_IMAGE_BYTES {
        use base64::Engine as _;
        a.data = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
    } else {
        a.media_type = "application/octet-stream".into();
        a.not_read = Some("not a text file, and over the 5 MiB limit for images".into());
    }
    Ok(a)
}

fn read_stdin_prompt() -> Result<String> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    let s = s.trim().to_string();
    if s.is_empty() {
        eprintln!("theseus: no prompt given (pass PROMPT or pipe it on stdin)");
        std::process::exit(2);
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_client::render::*;

    /// `theseus stop SESSION` (W1): the session named by its id or its end,
    /// its conversation's execution, and what the line says.
    #[test]
    fn stop_names_a_session_by_its_end_and_says_what_goes_on() {
        let e = |exe: &str, ses: &str, kind: &str| theseus_protocol::ExecutionInfo {
            execution_id: exe.into(),
            session_id: ses.into(),
            kind: kind.into(),
            state: "waiting".into(),
            turns: 1,
            interrupted: 0,
            outstanding: 0,
            queued_results: 0,
            budget: Default::default(),
            wake: Value::Null,
            waiting_on: None,
            reports_to: None,
            ended_reason: None,
            created_at_ms: 0,
            updated_at_ms: 0,
            attention: None,
        };
        let execs = [
            e("exe_a1b2c3", "ses_a1b2c3", "conversation"),
            e("exe_d4e5f6", "ses_d4e5f6", "task"),
            e("exe_99e5f6", "ses_99e5f6", "conversation"),
        ];
        assert_eq!(stop_target(&execs, "a1b2c3").unwrap(), "exe_a1b2c3");
        assert_eq!(stop_target(&execs, "ses_99e5f6").unwrap(), "exe_99e5f6");
        let many = stop_target(&execs, "e5f6").unwrap_err().to_string();
        assert!(many.contains("names 2 sessions"), "{many}");
        assert!(stop_target(&execs, "zzzz").is_err());
        assert!(stop_target(&execs, "c3").is_err(), "too short");
        let r = theseus_protocol::ExecutionStopResult {
            execution: execs[0].clone(),
            stopped: true,
            stopped_actions: vec!["act_1".into()],
            declined: vec![],
            turn_running: false,
            tasks_running: 1,
            wakes_pending: 0,
            verdicts: vec![],
        };
        assert_eq!(
            stop_line(&r),
            "stopped session ses_a1b2c3's work · 1 action(s) told to stop · 0 declined · the \
             conversation goes on · 1 task(s) and 0 wake(s) go on (theseus cancel <id>)"
        );
    }

    /// The sessions that hold external text (theseus-9bp): health's line
    /// names each with what it read, and `policy trust` takes a short name
    /// only among them, refusing one too short or naming two.
    #[test]
    fn external_text_lines_and_the_trust_target() {
        let held =
            |sid: &str, task: Option<&str>, via: Option<&str>| theseus_protocol::ExternalTextInfo {
                session_id: sid.into(),
                title: None,
                task: task.map(str::to_string),
                held: theseus_protocol::ExternalText {
                    since_ms: 0,
                    tool: "http.fetch".into(),
                    url: "https://example.test/a".into(),
                    node_id: "trs_1".into(),
                    from_session: via.map(|_| "ses_parent".into()),
                    via: via.map(str::to_string),
                    query: None,
                },
                since_local: String::new(),
            };
        assert_eq!(external_line(&[]), None);
        let two = [
            held("ses_0000aa1111", None, None),
            held("ses_0000bb1111", Some("bb1111"), Some("task.create")),
        ];
        // From a daemon that sends no local time: UTC, as before.
        assert_eq!(
            external_line(&two).unwrap(),
            "external text: 2 sessions read it, so their calls that act wait: …aa1111 since \
             00:00:00.000Z (http.fetch https://example.test/a); task bb1111 since 00:00:00.000Z \
             (http.fetch https://example.test/a, from the session that started it) · trust one \
             again: theseus policy trust <session>"
        );
        // A search names its query, in the daemon's local time (theseus-qiy).
        let mut search = held("ses_0000cc2222", None, None);
        search.held.tool = "web.search".into();
        search.held.url = "https://search.example.test/res?q=lantern+tide+tables".into();
        search.held.query = Some("lantern tide tables".into());
        search.since_local = "12:55:01".into();
        assert_eq!(
            external_line(std::slice::from_ref(&search)).unwrap(),
            "external text: 1 session read it, so their calls that act wait: …cc2222 since \
             12:55:01 (web.search \"lantern tide tables\") · trust one again: theseus policy \
             trust <session>"
        );
        let trusted = theseus_protocol::TrustResult {
            session_id: search.session_id.clone(),
            by: "the CLI".into(),
            how: "action.confirm".into(),
            held: search.held.clone(),
            since_local: "12:55:01".into(),
            ..Default::default()
        };
        assert_eq!(
            trusted_line(&trusted),
            "trusted session ses_0000cc2222 again (by the CLI) · it had read web.search \"lantern \
             tide tables\" since 12:55:01 · its calls that act run at their postures again, until \
             it reads external text again"
        );
        let mut long = search;
        long.held.query = Some("q".repeat(100));
        let line = external_line(std::slice::from_ref(&long)).unwrap();
        assert!(
            line.contains(&format!("(web.search \"{}…)", "q".repeat(59))),
            "{line}"
        );
        assert_eq!(trust_target(&two, "aa1111").unwrap(), "ses_0000aa1111");
        assert_eq!(trust_target(&two, "…bb1111").unwrap(), "ses_0000bb1111");
        assert!(trust_target(&two, "111")
            .unwrap_err()
            .to_string()
            .contains("too short"));
        assert!(trust_target(&two, "1111")
            .unwrap_err()
            .to_string()
            .contains("names 2 sessions"));
        assert!(trust_target(&two, "cccc")
            .unwrap_err()
            .to_string()
            .contains("no session that holds external text"));
        assert_eq!(trust_target(&two, "ses_other").unwrap(), "ses_other");
    }

    #[test]
    fn attach_sends_a_text_files_text_and_lists_anything_else() {
        let dir = std::env::temp_dir().join(format!("theseus-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "The fact is 42.\n").unwrap();
        let a = attachment_for(&notes).unwrap();
        assert_eq!(
            (a.name.as_str(), a.media_type.as_str(), a.size),
            ("notes.txt", "text/plain", 16)
        );
        assert_eq!(a.text.as_deref(), Some("The fact is 42.\n"));
        assert!(a.not_read.is_none());

        // Not text: its bytes go, and the daemon keeps them only if they are an image.
        let shot = dir.join("shot.png");
        std::fs::write(&shot, b"\x89PNG\r\n\x1a\n").unwrap();
        let b = attachment_for(&shot).unwrap();
        assert_eq!(b.data.as_deref(), Some("iVBORw0KGgo="));
        assert!(b.text.is_none() && b.not_read.is_none());

        assert!(attachment_for(&dir.join("missing.txt")).is_err());
        assert!(attachment_for(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
