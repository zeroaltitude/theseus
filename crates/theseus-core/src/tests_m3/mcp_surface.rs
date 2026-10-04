//! The MCP server's surface (step 41b, `crate::mcp_server`): a session an
//! MCP client opens has the principal `mcp` and the floor, its calls that
//! act wait though the config would run them, no answer from that surface
//! counts, and it may write only into its own sessions. A child of
//! `tests_m3` for its rig and helpers.

use super::*;
use crate::approval::Surface::{Cli, Mcp};

/// A session opened on `Surface::Mcp` carries the principal `mcp`, the
/// floor, and `[mcp_server]`'s limit. Its write, open by the config, waits
/// for the operator, and the card says why. An answer from the MCP surface
/// is refused (a trust, a press, an undo, and a publish take the same
/// judgment, `owner_in_private`), ledgered as `approval.refused` with
/// `via: mcp`, and the call keeps waiting. The CLI's answer counts, and the
/// write runs.
#[tokio::test]
async fn an_mcp_sessions_act_waits_and_no_answer_from_mcp_counts() {
    let r = rig_with(write_script(), |cfg| {
        cfg.policy.tools.insert("fs.write".into(), Posture::Open);
        cfg.mcp_server.spend_limit_usd = 3.0;
    });
    let mcp = || surface("mcp", Mcp);
    let opened = rpc_as(
        &r.core,
        mcp(),
        theseus_protocol::method::SESSION_OPEN,
        json!({"label": "mcp lantern-agent"}),
    )
    .await
    .unwrap();
    let sid = opened["session_id"].as_str().unwrap().to_string();
    let exec_id = opened["execution_id"].as_str().unwrap().to_string();
    let exec = r.core.kernel.execution(&exec_id).unwrap().unwrap();
    assert_eq!(exec.authority.principal, crate::mcp_server::PRINCIPAL);
    assert_eq!(
        exec.authority.ceilings[crate::mcp_server::FLOOR_CEILING],
        "approve"
    );
    assert_eq!(exec.budget.limit_micros, 3_000_000);

    let res = rpc_as(
        &r.core,
        mcp(),
        theseus_protocol::method::TURN_SUBMIT,
        json!({"session_id": sid, "input": "write out.txt"}),
    )
    .await
    .unwrap();
    let corr = res["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the write waits: {res}"))
        .to_string();
    let pending = r.core.pending_confirms(&sid).unwrap();
    assert!(
        pending[0]
            .reason
            .contains("an MCP client opened this session"),
        "{}",
        pending[0].reason
    );

    // Each owner's act from the MCP surface is refused, and recorded.
    let answer = answer_as(&r.core, mcp(), &corr, None).await;
    let e = answer.expect_err("an answer from MCP");
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert!(
        e.message.contains("never an approval surface"),
        "{}",
        e.message
    );
    assert_eq!(e.data["via"], "mcp", "{e:?}");
    let rows = ledgered(&r, "approval.refused");
    assert!(
        !rows.is_empty() && rows.iter().all(|row| row["via"] == "mcp"),
        "{rows:?}"
    );
    assert_eq!(
        r.core.kernel.action(&corr).unwrap().unwrap().state,
        theseus_kernel::ActionState::Planned
    );
    assert!(!r.root.join("out.txt").exists());

    // The operator's answer, from the CLI, counts.
    answer_as(&r.core, surface("sock#1", Cli), &corr, None)
        .await
        .unwrap();
    let cont = r.core.continue_execution(&exec_id).await.unwrap().unwrap();
    assert_eq!(cont.output, "Written.");
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "approved\n"
    );
}

/// The floor is the MCP client's alone: the same write in the operator's
/// own session runs at once, as the config says.
#[tokio::test]
async fn the_operators_own_session_has_no_floor() {
    let r = rig_with(write_script(), |cfg| {
        cfg.policy.tools.insert("fs.write".into(), Posture::Open);
    });
    let res = turn(&r.core, None, "write out.txt").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert_eq!(res.output, "Written.");
}

/// `Surface::Mcp` calls nothing but its tools' methods and the owner's acts
/// (which their judgment refuses), and sends turns only into its own
/// sessions.
#[tokio::test]
async fn an_mcp_connection_calls_only_its_tools_into_its_own_sessions() {
    let r = rig(write_script());
    let mcp = || surface("mcp", Mcp);
    let opened = rpc_as(
        &r.core,
        mcp(),
        theseus_protocol::method::SESSION_OPEN,
        json!({}),
    )
    .await
    .unwrap();
    let exec_id = opened["execution_id"].as_str().unwrap().to_string();
    let cancel = rpc_as(
        &r.core,
        mcp(),
        theseus_protocol::method::EXECUTION_CANCEL,
        json!({"execution_id": exec_id}),
    )
    .await
    .expect_err("a cancel from MCP");
    assert!(
        cancel.message.contains("may not call execution.cancel"),
        "{cancel:?}"
    );
    let own = rpc_as(
        &r.core,
        surface("sock#1", Cli),
        theseus_protocol::method::SESSION_OPEN,
        json!({}),
    )
    .await
    .unwrap();
    let into = rpc_as(
        &r.core,
        mcp(),
        theseus_protocol::method::TURN_SUBMIT,
        json!({"session_id": own["session_id"], "input": "hello"}),
    )
    .await
    .expect_err("a turn into the operator's conversation");
    assert!(
        into.message
            .contains("is not a conversation an MCP client opened"),
        "{into:?}"
    );
}
