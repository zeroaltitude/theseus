//! `theseus --spawn ask` follows what its turn left for later
//! (theseus-mqxk). The daemon it spawned ends with the run, so a job's late
//! result, or a wake the model set, would come back to nothing. The daemon is
//! told it serves one run (`theseusd --one-shot SECS`), and each turn's
//! result says what it left (`later`): while a job runs, a result waits for
//! its turn, or a wake is due within `--follow-for`, `ask` keeps the daemon
//! up and prints each turn that comes, as a turn prints, then stops when
//! none is left. Past the bound it says what still runs, and the status line
//! names what was left behind. `--follow-for 0` follows nothing.
//!
//! `--json` stays one object: the last turn's result, with every follow
//! turn in `continuations`, the ask's own in `asked`, and the spend summed
//! (`combined`). A SIGINT or SIGTERM stops the follow as it stops a turn: the
//! first stops the session's execution (a running turn ends `stopped`, with
//! its spend), the second ends the run at once.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{json, Value};
use theseus_client::render;
use theseus_client::{outcome, Conn};
use theseus_protocol::later::Later;
use theseus_protocol::{method, notify, Event, Message, TurnSubmitResult};

use crate::print::Printer;

/// `--follow-for`'s default: how long `ask` follows a turn's later results
/// and wakes. The owner's call (theseus-mqxk); one constant.
pub const FOLLOW_FOR: &str = "30m";

/// How long a turn the bound or a signal stopped has to end with its spend.
const STOP_GRACE: Duration = Duration::from_secs(30);

/// `theseusd`'s arguments for a spawned `ask`: it serves one run, followed
/// for at most `follow_for`.
pub fn daemon_args(follow_for: Duration) -> Vec<String> {
    vec!["--one-shot".into(), follow_for.as_secs().to_string()]
}

/// `--follow-for`'s value: `30m`, `90s`, `2h`, `1500ms`, bare seconds, or 0.
pub fn parse_follow_for(s: &str) -> Result<Duration, String> {
    let t = s.trim();
    let (n, unit) = t
        .find(|c: char| !c.is_ascii_digit())
        .map_or((t, ""), |i| t.split_at(i));
    let bad = || format!("`{s}` is no duration: write 30m, 90s, 2h, or 0");
    let n: u64 = n.parse().map_err(|_| bad())?;
    let ms = match unit {
        "" | "s" => n.saturating_mul(1000),
        "ms" => n,
        "m" => n.saturating_mul(60_000),
        "h" => n.saturating_mul(3_600_000),
        _ => return Err(bad()),
    };
    Ok(Duration::from_millis(ms))
}

/// The follow of one `ask`: its bound, and the run's end, which the daemon
/// fixed as the ask's turn ended (`Later.ends_at_ms`).
pub struct Follow {
    bound: Duration,
    deadline: tokio::time::Instant,
}

/// What the follow saw: every turn it printed, in order, and how it ended.
pub struct Followed {
    pub turns: Vec<(Value, TurnSubmitResult)>,
    /// A continuation that failed, or a second signal: the run's error once
    /// its result is printed.
    pub error: Option<anyhow::Error>,
    /// A signal stopped the follow while no turn ran.
    pub stopped_idle: bool,
}

impl Followed {
    /// A later turn failed: the follow ends, and the run with the error.
    fn failed(mut self, printer: &mut Printer, why: &str) -> Self {
        printer.settle();
        self.error = Some(anyhow::anyhow!("a later turn failed: {why}"));
        self
    }
}

impl Follow {
    /// A follow of `bound`, until the run's end the daemon named in the
    /// ask's result; none when the bound is 0 or it named none.
    pub fn new(bound: Duration, first: &TurnSubmitResult) -> Option<Self> {
        let ends_at_ms = first.later.as_ref()?.ends_at_ms;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let left = Duration::from_millis(ends_at_ms.saturating_sub(now_ms)).min(bound);
        (!bound.is_zero()).then(|| Self {
            bound,
            deadline: tokio::time::Instant::now() + left,
        })
    }

    /// Whether a turn may still come before the run ends, after `r`: it
    /// ended done, and left a job, a queued result, or a wake that fires.
    pub fn wants(r: &TurnSubmitResult) -> bool {
        outcome::TurnEnd::of(r) == outcome::TurnEnd::Done
            && r.later.as_ref().is_some_and(Later::comes)
    }

    /// Follow the session's turns after `first`, the ask's own, which
    /// `wants`. Each turn's reply prints through `printer` as it streams (on
    /// stdout too, under `--no-stream`, once it ends), and its status line
    /// on stderr, but the last's, which `ask` prints. The connection hears
    /// the session already: a one-run daemon watched it for the ask's turn.
    pub async fn run(
        &self,
        conn: &mut Conn,
        printer: &mut Printer,
        first: (Value, TurnSubmitResult),
        (json, stream): (bool, bool),
    ) -> Result<Followed> {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        let execution = first.1.execution_id.clone();
        let mut out = Followed {
            turns: vec![first],
            error: None,
            stopped_idle: false,
        };
        let between = |out: &Followed| {
            if !json {
                let r = &out.turns[out.turns.len() - 1].1;
                if !stream {
                    println!("{}", r.output);
                }
                eprintln!("{}", render::status_line(&without_later(r)));
                if let Some(l) = &r.later {
                    eprintln!("{}", self.following(l));
                }
            }
        };
        between(&out);
        // A turn has started and not ended.
        let mut running = false;
        // The signal that stopped it, 0 for the bound.
        let mut stop: Option<i32> = None;
        let mut deadline = self.deadline;
        loop {
            let signal = tokio::select! {
                msg = conn.next() => {
                    let Some(msg) = msg? else {
                        out.error = Some(anyhow::anyhow!("the daemon closed the connection during the follow"));
                        return Ok(out);
                    };
                    let Message::Notification(n) = msg else { continue };
                    let event = Event::from_notification(&n.method, &n.params).ok().flatten();
                    printer.on(&n.method, &n.params);
                    match event {
                        Some(Event::TurnStarted(_)) => running = true,
                        // A turn already taken (the ask's own) is not a later one.
                        Some(Event::TurnEnded(t)) if out.turns.iter().any(|(_, r)| r.turn_id == t.turn_id) => {}
                        Some(Event::TurnEnded(t)) => {
                            running = false;
                            printer.settle();
                            let ends = !Self::wants(&t) || stop.is_some();
                            out.turns.push((n.params.clone(), t));
                            if ends {
                                return Ok(out);
                            }
                            between(&out);
                        }
                        Some(Event::TurnFailed(f)) => return Ok(out.failed(printer, &f.error)),
                        _ if n.method == notify::TURN_FAILED => return Ok(out.failed(printer, "?")),
                        _ => {}
                    }
                    None
                }
                _ = sigint.recv() => Some(libc::SIGINT),
                _ = sigterm.recv() => Some(libc::SIGTERM),
                () = tokio::time::sleep_until(deadline) => {
                    printer.settle();
                    if stop.is_some() || !running {
                        if stop.is_none() {
                            let last = &out.turns[out.turns.len() - 1].1;
                            eprintln!("{}", self.reached(last.later.as_ref()));
                        }
                        return Ok(out);
                    }
                    // The bound came while a turn runs: stop it, and let it
                    // end with its spend.
                    eprintln!(
                        "theseus: --follow-for {} reached: stopping the running turn",
                        span(self.bound)
                    );
                    stop_execution(conn, execution.as_deref(), "cli:follow-for").await?;
                    stop = Some(0);
                    deadline = tokio::time::Instant::now() + STOP_GRACE;
                    None
                }
            };
            let Some(s) = signal else { continue };
            if stop.is_some_and(|by| by != 0) {
                out.error = Some(outcome::Signalled { signal: s }.into());
                return Ok(out);
            }
            printer.settle();
            eprintln!(
                "theseus: {}: stopping the session's later work; a second signal ends the run at once",
                outcome::signal_name(s)
            );
            let author = format!("cli:{}", outcome::signal_name(s));
            stop_execution(conn, execution.as_deref(), &author).await?;
            if !running {
                out.stopped_idle = true;
                return Ok(out);
            }
            stop = Some(s);
            deadline = tokio::time::Instant::now() + STOP_GRACE;
        }
    }

    /// `[following: 1 job(s) running · until it is done, or 30 min from the
    /// ask's turn]`
    fn following(&self, l: &Later) -> String {
        let mut parts = Vec::new();
        if l.jobs > 0 {
            parts.push(format!("{} job(s) running", l.jobs));
        }
        if l.queued {
            parts.push("a result waiting for its turn".into());
        }
        let due = l.wakes.iter().filter(|w| w.fires).count();
        if due > 0 {
            parts.push(format!("{due} wake(s) due"));
        }
        format!(
            "[following: {} · until it is done, or {} from the ask's turn]",
            parts.join(" · "),
            span(self.bound)
        )
    }

    /// The bound reached with work left.
    fn reached(&self, l: Option<&Later>) -> String {
        let left = l.map(render::later::words).unwrap_or_default();
        format!(
            "[still running after --follow-for {}: {}; the run ends, and its daemon stops]",
            span(self.bound),
            if left.is_empty() {
                "nothing named"
            } else {
                left.as_str()
            }
        )
    }
}

/// After the ask's turn (`first`), under `--spawn`: follow what it left for
/// later within `follow_for`, when it left anything. Returns the result to
/// print (one object), the last turn's, and the run's error, if the follow
/// ended in one: a later turn's failure, a second signal, or a first that
/// stopped the session's work while no turn ran (exit 9).
pub async fn after(
    conn: &mut Conn,
    printer: &mut Printer,
    follow_for: Option<Duration>,
    first: (Value, TurnSubmitResult),
    modes: (bool, bool),
) -> Result<(Value, TurnSubmitResult, Option<anyhow::Error>)> {
    let Some(f) = follow_for
        .and_then(|b| Follow::new(b, &first.1))
        .filter(|_| Follow::wants(&first.1))
    else {
        return Ok((first.0, first.1, None));
    };
    let followed = f.run(conn, printer, first, modes).await?;
    let value = combined(&followed.turns);
    let last = followed.turns[followed.turns.len() - 1].1.clone();
    let error = followed.error.or_else(|| {
        followed.stopped_idle.then(|| {
            outcome::Ended {
                end: outcome::TurnEnd::Stopped,
                stop_reason: "stopped".into(),
            }
            .into()
        })
    });
    Ok((value, last, error))
}

/// A bound as people say it: `30 min`, `90 s`, `2 h`.
fn span(d: Duration) -> String {
    match d.as_secs() {
        s if s >= 3600 && s % 3600 == 0 => format!("{} h", s / 3600),
        s if s >= 60 && s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

/// `execution.stop` on the session's execution, as `/stop` does: its turn
/// ends `stopped` with its spend, and its jobs are stopped. Its answer comes
/// back as a response the follow skips.
async fn stop_execution(conn: &mut Conn, execution: Option<&str>, author: &str) -> Result<()> {
    if let Some(exec) = execution {
        let params = json!({"execution_id": exec, "author": author});
        conn.send(method::EXECUTION_STOP, params).await?;
    }
    Ok(())
}

/// A turn's result as its status line shows it mid-follow: what it left
/// is being followed, not left behind.
fn without_later(r: &TurnSubmitResult) -> TurnSubmitResult {
    TurnSubmitResult {
        later: None,
        trace: None,
        ..r.clone()
    }
}

/// The one JSON object `ask --json` prints after a follow: the last turn's
/// result as the daemon sent it, its spend replaced by every turn's summed
/// (`cost_usd`, `usage`, `tool_calls`, `loops`), the ask's own turn in
/// `asked`, and each turn after it, the last included, in `continuations`.
/// A follow of no turn leaves the ask's result as it was.
pub fn combined(turns: &[(Value, TurnSubmitResult)]) -> Value {
    let Some(((last, _), rest)) = turns.split_last() else {
        return Value::Null;
    };
    if rest.is_empty() {
        return last.clone();
    }
    let mut out = last.clone();
    let brief = |(_, r): &(Value, TurnSubmitResult)| {
        json!({
            "turn_id": r.turn_id,
            "output": r.output,
            "cost_usd": r.cost_usd,
            "usage": r.usage,
            "stop_reason": r.stop_reason,
            "loops": r.loops,
            "tool_calls": r.tool_calls,
        })
    };
    let cost: Option<f64> = turns
        .iter()
        .map(|(_, r)| r.cost_usd)
        .try_fold(0.0, |sum, c| c.map(|c| sum + c));
    let mut usage = json!({});
    for (v, _) in turns {
        if let Some(u) = v["usage"].as_object() {
            for (k, n) in u {
                if let Some(n) = n.as_u64() {
                    usage[k] = json!(usage[k].as_u64().unwrap_or(0) + n);
                }
            }
        }
    }
    out["cost_usd"] = json!(cost);
    out["usage"] = usage;
    out["tool_calls"] = json!(turns.iter().map(|(_, r)| r.tool_calls).sum::<u32>());
    out["loops"] = json!(turns.iter().map(|(_, r)| r.loops).sum::<u32>());
    out["asked"] = brief(&turns[0]);
    out["continuations"] = Value::Array(turns[1..].iter().map(brief).collect());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_for_reads_as_people_write_it() {
        assert_eq!(parse_follow_for("30m"), Ok(Duration::from_secs(1800)));
        assert_eq!(parse_follow_for("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_follow_for("0"), Ok(Duration::ZERO));
        assert_eq!(parse_follow_for("1500ms"), Ok(Duration::from_millis(1500)));
        assert!(parse_follow_for("soon").is_err());
        let mut r = TurnSubmitResult::default();
        assert!(
            Follow::new(Duration::from_secs(60), &r).is_none(),
            "no run's end named"
        );
        r.later = Some(Later::default());
        assert!(Follow::new(Duration::ZERO, &r).is_none());
        assert!(Follow::new(Duration::from_secs(60), &r).is_some());
    }

    #[test]
    fn one_object_sums_the_spend_and_keeps_every_turn() {
        let turn = |id: &str, cost: f64, out: u64| {
            let r = TurnSubmitResult {
                turn_id: id.into(),
                output: format!("said in {id}"),
                cost_usd: Some(cost),
                loops: 1,
                tool_calls: 1,
                usage: theseus_protocol::Usage {
                    input_tokens: 10,
                    output_tokens: out,
                    ..Default::default()
                },
                ..Default::default()
            };
            (serde_json::to_value(&r).unwrap(), r)
        };
        let one = vec![turn("trn_a", 0.5, 3)];
        assert_eq!(combined(&one), one[0].0, "no follow: the ask's result");
        let all = combined(&[turn("trn_a", 0.5, 3), turn("trn_b", 0.25, 4)]);
        assert_eq!(all["turn_id"], "trn_b");
        assert_eq!(all["output"], "said in trn_b");
        assert_eq!(all["cost_usd"], 0.75);
        assert_eq!(all["usage"]["output_tokens"], 7);
        assert_eq!(all["usage"]["input_tokens"], 20);
        assert_eq!(all["tool_calls"], 2);
        assert_eq!(all["asked"]["turn_id"], "trn_a");
        assert_eq!(all["continuations"][0]["output"], "said in trn_b");
    }
}
