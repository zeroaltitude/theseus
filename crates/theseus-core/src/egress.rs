//! Egress for L1 jobs, the core's half (M4 18c; design §2.4). The proxy is
//! `theseus_sandbox::egress`, which the job wrapper runs; here are the list,
//! the gate's step for hosts beyond it, the list a proposal binds, and what a
//! job's completion says of its egress (`detail.egress`): its rows, its lines
//! in the result, and whether the result is outside text.
//!
//! - **The list.** `[sandbox] egress` is the operator's stated will, as
//!   `allow_argv` is: every L1 job may reach those hosts. Empty by default, so
//!   L1 has no network. A call may name more:
//!   `proc.run { sandbox: { egress: ["pypi.org:443"] } }`.
//! - **A host beyond the list makes the call wait** (step 2 of the order, as a
//!   path outside the roots does). Its approval reaches only the hosts it
//!   named: the job's whole list is in its proposal, so the digest a confirm
//!   binds covers it, and a confirmed call runs with that list and no other.
//! - **Outside text.** A job that connected out returns what it brought back.
//!   Its result is marked `external` (DD5's marker, its `url` the hosts it
//!   reached), so T1 holds its session (`via: egress`), and its node is
//!   untrusted, its readers still `Owner`. A job that connected nowhere keeps
//!   19a's label and holds nothing (theseus-20f, closed for L1).

use serde_json::Value;
use theseus_kernel::Action;
use theseus_protocol::Proposal;
use theseus_sandbox::egress::{Allow, Summary};
use theseus_store::NewRecord;

use crate::fact::sandbox::{SandboxEgress, SandboxEgressRefused};
use crate::fact::Rec;
use crate::node::{Body, Node};
use crate::policy::{Decision, Posture};
use crate::sandbox::Sandbox;
use crate::toolrun::TurnCtx;

/// The key of a proposal's policy context that names an L1 job's list. A
/// proposal without one (L0, or L1 with no network) digests as before 18c.
const KEY: &str = "egress";

/// A call's own hosts: `sandbox.egress` in its input. `proc.run`'s plan has
/// checked each already, so a bad entry never reaches here.
pub fn asked(input: &Value) -> Vec<String> {
    input
        .pointer("/sandbox/egress")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Each entry of a list (`[sandbox] egress`, or a call's), parsed: a
/// `host:port`, with a glob on the host, as the proxy matches it.
pub fn check(list: &[String]) -> Result<Vec<Allow>, String> {
    list.iter().map(|e| e.parse::<Allow>()).collect()
}

/// The job's list, and the call's hosts beyond the operator's: the
/// operator's entries, then each of the call's that the operator's do not
/// already cover (`Allow::covers`), each written as the proxy shows it.
pub fn list(operator: &[String], call: &[String]) -> (Vec<String>, Vec<String>) {
    let ours = check(operator).unwrap_or_default();
    let mut list: Vec<String> = ours.iter().map(ToString::to_string).collect();
    let mut beyond = Vec::new();
    for e in check(call).unwrap_or_default() {
        if ours.iter().any(|o| o.covers(&e)) {
            continue;
        }
        let e = e.to_string();
        if !list.contains(&e) {
            list.push(e.clone());
            beyond.push(e);
        }
    }
    (list, beyond)
}

/// The gate's step for a call whose hosts go beyond `[sandbox] egress`: it
/// waits for the operator, whatever L1's posture would be, and the reason
/// names those hosts.
pub fn gate(d: Decision, beyond: &[String], tool: &str, summary: &str) -> Decision {
    if beyond.is_empty() {
        return d;
    }
    let why = format!(
        "it names hosts beyond [sandbox] egress: {}; an approval lets this job reach them, and \
         no other",
        beyond.join(", ")
    );
    d.at_least(Posture::Approve, &why, "[sandbox] egress", tool, summary)
}

/// The list a proposal binds: what a confirmed call's job may reach.
pub fn bound(p: &Proposal) -> Vec<String> {
    theseus_protocol::sandbox::egress_in(&p.policy_context)
}

/// Names `list` in a proposal's policy context, when it is not empty.
pub fn bind(p: &mut Proposal, list: &[String]) {
    if list.is_empty() {
        return;
    }
    if let Some(m) = p.policy_context.as_object_mut() {
        m.insert(KEY.into(), serde_json::json!(list));
    }
}

/// A completion's `detail.egress`, as the wrapper wrote it: none for a job
/// with no list, or one from before 18c.
pub fn summary(detail: &Value) -> Option<Summary> {
    serde_json::from_value(detail.get(KEY)?.clone()).ok()
}

/// What a job's result is marked when its job connected out: DD5's
/// `external`, naming the hosts it reached. None when it reached none.
pub fn external(detail: &Value) -> Option<theseus_tools::External> {
    summary(detail)
        .filter(Summary::connected)
        .map(|s| theseus_tools::External { url: s.reached() })
}

/// What a job's result is marked: DD5's `external` when its job connected
/// out of L1, naming the hosts it reached. A job that left no completion (a
/// stop, a lost wrapper) and printed something is marked too when its
/// proposal bound a list (`bound`, read only then): it may have connected
/// out, and nothing recorded whether it did, so what it printed counts as
/// outside text.
pub fn marker(
    detail: &Value,
    printed: bool,
    bound: impl FnOnce() -> Vec<String>,
) -> Option<theseus_tools::External> {
    if let Some(e) = external(detail) {
        return Some(e);
    }
    if !detail.is_null() || !printed {
        return None;
    }
    let list = bound();
    (!list.is_empty()).then(|| theseus_tools::External {
        url: format!(
            "{} (not recorded: the job ended without its report)",
            list.join(", ")
        ),
    })
}

/// A job's egress, from its result node: its id, its tool, and its summary.
fn of_node(node: &Node) -> Option<(&str, &str, Summary)> {
    let Body::ToolResult {
        correlation_id: Some(c),
        tool,
        meta,
        ..
    } = &node.body
    else {
        return None;
    };
    Some((c, tool, summary(&meta["detail"])?))
}

/// The rows of a job's egress (`sandbox.egress` for each host it reached,
/// `sandbox.egress_refused` for each refusal), for the frame that writes its
/// result `node`.
pub(crate) fn rows(rec: &Rec<'_>, node: &Node) -> anyhow::Result<Vec<NewRecord>> {
    let Some((correlation_id, tool, s)) = of_node(node) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for reached in &s.hosts {
        out.push(rec.row(&SandboxEgress {
            correlation_id,
            tool,
            reached,
        })?);
    }
    for refused in &s.refused {
        out.push(rec.row(&SandboxEgressRefused {
            correlation_id,
            tool,
            refused,
        })?);
    }
    Ok(out)
}

/// Their sentences, and health's count, once that frame is written.
pub(crate) fn announce(rec: &Rec<'_>, sandbox: &Sandbox, node: &Node) {
    let Some((correlation_id, tool, s)) = of_node(node) else {
        return;
    };
    for reached in &s.hosts {
        rec.announce(&SandboxEgress {
            correlation_id,
            tool,
            reached,
        });
    }
    for refused in &s.refused {
        rec.announce(&SandboxEgressRefused {
            correlation_id,
            tool,
            refused,
        });
    }
    sandbox.egress_seen(&s);
}

/// A job's egress, recorded in a turn just before its result is written:
/// its rows wait for the turn's next frame, which is that result's.
pub(crate) fn record(tc: &TurnCtx<'_>, sandbox: &Sandbox, a: &Action, detail: &Value) {
    let Some(s) = summary(detail) else {
        return;
    };
    let (correlation_id, tool) = (a.correlation_id.as_str(), a.tool.as_str());
    for reached in &s.hosts {
        tc.record(&SandboxEgress {
            correlation_id,
            tool,
            reached,
        });
    }
    for refused in &s.refused {
        tc.record(&SandboxEgressRefused {
            correlation_id,
            tool,
            refused,
        });
    }
    sandbox.egress_seen(&s);
}

/// What `n` bytes are, in words: `640 B`, `12.5 KB`, `3.1 MB`.
pub fn bytes(n: u64) -> String {
    match n {
        0..1_000 => format!("{n} B"),
        1_000..1_000_000 => format!("{:.1} KB", n as f64 / 1e3),
        _ => format!("{:.1} MB", n as f64 / 1e6),
    }
}

/// One host reached, in words: `api.github.com:443 (2 connections, 410 B
/// up, 12.5 KB down)`.
pub fn reached(h: &theseus_sandbox::egress::Reached) -> String {
    format!(
        "{} ({} connection{}, {} up, {} down)",
        h.name(),
        h.connections,
        if h.connections == 1 { "" } else { "s" },
        bytes(h.up),
        bytes(h.down)
    )
}

/// The tool line's end for a job that connected out (design §2.11):
/// `reached api.github.com:443 (2 connections)`; None when it reached none.
pub fn reached_line(detail: &Value) -> Option<String> {
    let s = summary(detail).filter(Summary::connected)?;
    let hosts: Vec<String> = s
        .hosts
        .iter()
        .map(|h| {
            let n = h.connections;
            format!(
                "{} ({n} connection{})",
                h.name(),
                if n == 1 { "" } else { "s" }
            )
        })
        .collect();
    Some(format!("reached {}", hosts.join(", ")))
}

/// An L1 job's head: where it could reach, after "ran in L1, the sandbox:".
pub fn reach_words(detail: &Value) -> String {
    theseus_protocol::sandbox::reach(&summary(detail).map(|s| s.allow).unwrap_or_default())
}

/// The result's lines about the job's egress: the hosts it reached, so its
/// text is outside text; each refusal, with the proxy's reason; and a proxy
/// that did not start.
pub fn lines(detail: &Value) -> Vec<String> {
    let Some(s) = summary(detail) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(e) = detail.pointer("/egress/error").and_then(Value::as_str) {
        out.push(format!("[L1: {e}, so it had no network]"));
    }
    if s.connected() {
        let hosts: Vec<String> = s.hosts.iter().map(reached).collect();
        out.push(format!(
            "[L1: it reached {}: what it printed may hold outside text]",
            hosts.join("; ")
        ));
    } else if !s.allow.is_empty() {
        out.push("[L1: it reached none of its egress hosts]".into());
    }
    for r in &s.refused {
        let times = if r.count > 1 {
            format!(" ({} times)", r.count)
        } else {
            String::new()
        };
        out.push(format!("[L1: egress refused{times}: {}]", r.why));
    }
    if s.dropped > 0 {
        out.push(format!(
            "[L1: {} more connections were counted and not recorded]",
            s.dropped
        ));
    }
    out
}

/// Health's view of the list: `[sandbox] egress`, each entry as the proxy
/// shows it.
pub fn health_list(operator: &[String]) -> Vec<String> {
    check(operator)
        .unwrap_or_default()
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The proxy's stand-ins for a debug build's tests (DD5's `Dns.hosts`
/// pattern): `THESEUS_TEST_EGRESS_DNS="stand.test=127.0.0.1;meta.test=
/// 169.254.169.254"`, and `THESEUS_TEST_EGRESS_PUBLIC="127.0.0.1"`, the
/// addresses taken as public. A release build reads neither.
pub fn test_dns() -> Option<theseus_sandbox::egress::Resolver> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let names = std::env::var("THESEUS_TEST_EGRESS_DNS").ok()?;
    let mut r = theseus_sandbox::egress::Resolver::default();
    for pair in names.split(';').filter(|p| !p.is_empty()) {
        let (name, ips) = pair.split_once('=')?;
        let ips = ips
            .split(',')
            .map(|i| i.parse().ok())
            .collect::<Option<Vec<_>>>()?;
        r.hosts.insert(name.to_ascii_lowercase(), ips);
    }
    if let Ok(public) = std::env::var("THESEUS_TEST_EGRESS_PUBLIC") {
        r.public = public.split(',').filter_map(|i| i.parse().ok()).collect();
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// The job's list is the operator's, then the call's that it does not
    /// cover; only those wait, and only they are named.
    #[test]
    fn the_list_is_the_operators_and_the_calls_beyond_it() {
        let op = s(&["*.crates.io:443", "github.com:443"]);
        let (list, beyond) = list(&op, &s(&["index.crates.io:443", "pypi.org:443"]));
        assert_eq!(list, ["*.crates.io:443", "github.com:443", "pypi.org:443"]);
        assert_eq!(beyond, ["pypi.org:443"]);
        let (list, beyond) = super::list(&op, &[]);
        assert_eq!(list, op);
        assert!(beyond.is_empty());
        let (list, beyond) = super::list(&[], &s(&["PyPI.org.:443", "pypi.org:443"]));
        assert_eq!((list.clone(), beyond), (s(&["pypi.org:443"]), list));
        assert!(check(&s(&["github.com"])).is_err());
    }

    /// A proposal binds the list, so its digest covers it; an empty list
    /// leaves the proposal as it was.
    #[test]
    fn a_proposal_binds_the_list() {
        let mut p = Proposal {
            tool: "proc.run".into(),
            args: json!({}),
            resource: None,
            policy_context: json!({"class": "l1"}),
        };
        let before = theseus_kernel::gate::digest_proposal(&p);
        bind(&mut p, &[]);
        assert_eq!(theseus_kernel::gate::digest_proposal(&p), before);
        bind(&mut p, &s(&["api.github.com:443"]));
        assert_eq!(bound(&p), ["api.github.com:443"]);
        assert_ne!(theseus_kernel::gate::digest_proposal(&p), before);
        assert_eq!(
            asked(&json!({"sandbox": {"egress": ["a.test:443"]}})),
            ["a.test:443"]
        );
        assert!(asked(&json!({"sandbox": true})).is_empty());
    }

    /// A completion's egress, in the result's lines and its marker.
    #[test]
    fn a_job_that_connected_out_is_outside_text_and_says_so() {
        let detail = json!({"egress": {"allow": ["api.github.com:443"],
            "hosts": [{"host": "api.github.com", "port": 443, "connections": 2, "up": 410,
                "down": 12500, "ms": 80}],
            "refused": [{"host": "evil.test", "port": 443,
                "why": "evil.test:443 is not on this job's egress list", "count": 1}]}});
        assert_eq!(
            external(&detail).map(|e| e.url),
            Some("api.github.com:443".into())
        );
        let l = lines(&detail);
        assert_eq!(
            l[0],
            "[L1: it reached api.github.com:443 (2 connections, 410 B up, 12.5 KB down): what it \
             printed may hold outside text]"
        );
        assert_eq!(
            l[1],
            "[L1: egress refused: evil.test:443 is not on this job's egress list]"
        );
        assert_eq!(reach_words(&detail), "egress: api.github.com:443");
        let none = json!({"egress": {"allow": ["api.github.com:443"]}});
        assert!(external(&none).is_none());
        assert_eq!(lines(&none), ["[L1: it reached none of its egress hosts]"]);
        assert!(external(&json!({"exit_code": 0})).is_none());
        assert_eq!(reach_words(&json!({})), "no network");
    }

    /// A job that left no completion has no record of its connections: when
    /// its proposal bound a list and it printed something, its output counts
    /// as outside text; the list is read only then.
    #[test]
    fn a_job_that_left_no_report_is_outside_text_when_it_may_have_connected() {
        let list = || s(&["api.github.com:443"]);
        let marked = marker(&Value::Null, true, list).map(|e| e.url);
        assert_eq!(
            marked.as_deref(),
            Some("api.github.com:443 (not recorded: the job ended without its report)")
        );
        assert!(
            marker(&Value::Null, false, list).is_none(),
            "it printed nothing"
        );
        assert!(
            marker(&Value::Null, true, Vec::new).is_none(),
            "no list: no network"
        );
        let never = || -> Vec<String> { panic!("a completion's record is enough") };
        assert!(marker(&json!({"egress": {"allow": ["a.test:443"]}}), true, never).is_none());
        assert!(marker(&json!({"exit_code": 0}), true, never).is_none());
    }
}
