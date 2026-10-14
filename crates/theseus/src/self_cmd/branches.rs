//! `theseus self branches` (theseus-pw1q.8): the self branches Theseus built
//! in its own trees, each with the gate's verdict, its frozen report, and
//! its tree while one is open.

use anyhow::Result;
use theseus_client::{render, Conn};
use theseus_protocol::method;
use theseus_protocol::rsi::{SelfBranchesParams, SelfBranchesResult};

use crate::cmd::output;

pub async fn run(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn
        .request(
            method::SELF_BRANCHES,
            serde_json::to_value(SelfBranchesParams::default())?,
        )
        .await?;
    output(json, v, |r: SelfBranchesResult| {
        for l in lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

/// Its lines: the state, where branches come from and go, then one block a
/// branch, newest first.
pub fn lines(r: &SelfBranchesResult) -> Vec<String> {
    let mut out = vec![format!(
        "Self-improvement: {}.",
        render::self_state_line(&r.state)
    )];
    let Some(repo) = &r.repo else {
        out.push("[self] repo is unset, so there are no self branches.".into());
        return out;
    };
    out.push(format!(
        "Built from {repo}'s origin/main; {}.",
        r.remote.as_deref().map_or_else(
            || "[self] remote is unset, so branches stay local".to_string(),
            |m| format!("green branches are pushed to {m}")
        )
    ));
    if r.branches.is_empty() {
        out.push("No self branch yet.".into());
    }
    for b in &r.branches {
        let head = b.head.as_deref().map_or("", |h| &h[..h.len().min(10)]);
        out.push(format!(
            "{}  {head}  gate {}{}",
            b.branch,
            b.gate.as_deref().unwrap_or("not run"),
            b.item
                .as_deref()
                .map(|i| format!("  ({i})"))
                .unwrap_or_default()
        ));
        if let Some(t) = &b.tree {
            out.push(format!("    tree: {t}"));
        }
        if let Some(p) = &b.report {
            out.push(format!("    report: {p}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::rsi::{SelfBranch, SelfState};

    #[test]
    fn the_branches_say_their_verdict_report_and_tree() {
        let mut r = SelfBranchesResult {
            state: SelfState {
                gate: "allowed".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let l = lines(&r);
        assert_eq!(l[1], "[self] repo is unset, so there are no self branches.");
        r.repo = Some("/srv/ship".into());
        r.branches.push(SelfBranch {
            branch: "self/20261014-fix-the-mast".into(),
            item: Some("fix-the-mast".into()),
            head: Some("0123456789abcdef".into()),
            gate: Some("green".into()),
            report: Some("/state/self/reports/self-20261014-fix-the-mast.md".into()),
            tree: None,
        });
        let l = lines(&r);
        assert_eq!(
            l[1],
            "Built from /srv/ship's origin/main; [self] remote is unset, so branches stay local."
        );
        assert_eq!(
            l[2],
            "self/20261014-fix-the-mast  0123456789  gate green  (fix-the-mast)"
        );
        assert_eq!(
            l[3],
            "    report: /state/self/reports/self-20261014-fix-the-mast.md"
        );
        assert_eq!(l.len(), 4);
    }
}
