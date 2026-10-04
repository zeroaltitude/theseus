//! `theseus policy explain [--session <id>] [--tool <name>]` (step 42a,
//! theseus-ext.7): why a call waits, from `policy.explain`. Each place, then
//! each tool's result and the layers that raised it; with `--tool`, every
//! layer in the gate's order, the conditions that depend on the call, and
//! the gate's reason. `theseus policy` stays the short list.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::{
    method, PlaceExplain, PolicyExplainParams, PolicyExplainResult, ToolExplain,
};

use crate::cmd::output;

pub async fn explain(
    conn: &mut Conn,
    json: bool,
    session_id: Option<String>,
    tool: Option<String>,
) -> Result<()> {
    let whole = tool.is_some();
    let v = conn
        .request(
            method::POLICY_EXPLAIN,
            PolicyExplainParams { session_id, tool },
        )
        .await?;
    output(json, v, |r: PolicyExplainResult| {
        print!("{}", text(&r, whole));
        Ok(())
    })
}

/// The places, each with its tools: in full when `whole`, else a line each.
pub fn text(r: &PolicyExplainResult, whole: bool) -> String {
    let mut out = String::new();
    for p in &r.places {
        out.push_str(&heading(p));
        for t in &p.tools {
            match whole {
                true => out.push_str(&full(t)),
                false => out.push_str(&line(t)),
            }
        }
    }
    out.push_str(&format!(
        "each result is for a call inside the roots ({}) that no condition matches{}\n",
        if r.roots.is_empty() {
            "none configured".to_string()
        } else {
            r.roots.join(", ")
        },
        if whole {
            ""
        } else {
            " · every layer and condition of one tool: theseus policy explain --tool <name>"
        }
    ));
    out
}

fn heading(p: &PlaceExplain) -> String {
    let class = match p.class {
        theseus_protocol::PlaceClass::Private => "private",
        theseus_protocol::PlaceClass::Shared => "shared",
    };
    let mut h = format!("{} ({class}", p.name);
    if let Some(s) = &p.session_id {
        h.push_str(&format!(", session {s}"));
    }
    h.push(')');
    if let Some(c) = &p.ceiling {
        let mut said = Vec::new();
        if let Some(f) = &c.posture_floor {
            said.push(format!("floor {f}"));
        }
        if let Some(t) = &c.tools {
            said.push(format!("tools {}", t.join(", ")));
        }
        if let Some(l) = c.spend_limit_usd {
            said.push(format!("spend ${l:.2}"));
        }
        if let Some(pr) = &c.profile {
            said.push(format!("profile {pr}"));
        }
        h.push_str(&format!(" · ceiling: {}", said.join(", ")));
    }
    if let Some(x) = &p.hold {
        h.push_str(&format!(" · holds external text ({} {})", x.tool, x.url));
    }
    h.push('\n');
    h
}

/// A tool's one line: its result, and what raised it.
fn line(t: &ToolExplain) -> String {
    let why = match &t.refused {
        Some(r) => r.clone(),
        None => {
            let raised: Vec<String> = t
                .layers
                .iter()
                .filter(|l| l.raised || l.layer == "posture")
                .map(|l| match &l.setting {
                    Some(s) => format!("{}: {s}", l.layer),
                    None => l.layer.clone(),
                })
                .collect();
            raised.join(" · ")
        }
    };
    format!("  {:<22} {:<8} {why}\n", t.tool, t.result)
}

/// A tool in full: each layer, each condition, and the gate's reason.
fn full(t: &ToolExplain) -> String {
    let mut out = format!("  {} ({}) → {}\n", t.tool, t.class, t.result);
    for l in &t.layers {
        out.push_str(&format!(
            "    {:<11} {:<8} {}{}{}\n",
            l.layer,
            l.result,
            if l.raised { "↑ " } else { "" },
            l.says,
            l.setting
                .as_deref()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default()
        ));
    }
    for c in &t.conditions {
        out.push_str(&format!(
            "    if {}: {} → {}{}\n",
            c.layer,
            c.when,
            c.then,
            if c.entries.is_empty() {
                String::new()
            } else {
                format!(": {}", c.entries.join(", "))
            }
        ));
    }
    out.push_str(&format!("    reason: {}\n", t.reason));
    out
}
