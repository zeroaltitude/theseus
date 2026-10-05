//! `policy.explain` (step 42a, theseus-ext.7) agrees with the gate: for
//! every tool the template's config offers and one MCP tool, in the CLI, a
//! private place with a floor, an owner's DM, and a shared place, with and
//! without a tightening and a hold, each row's result is the decision the
//! gate's own functions make on a real call inside the roots; and through
//! whole turns, the decision the gate recorded.

use serde_json::{json, Map, Value};
use std::path::Path;
use theseus_protocol::{ExternalText, PlaceCeiling, PolicyExplainParams, ToolExplain};
use theseus_tools::Tool;

use crate::node::Body;
use crate::places::BoundPlace;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_places::{rig_setup, rig_with, session, turn, LAB, OWNER};
use crate::toolrun::order::At;
use crate::toolrun::Turned;

/// A guild channel the bindings file binds shared.
const PIER: u64 = 141_421_356_237_309_504;

fn bind(core: &Core) {
    core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ceiling: Some(PlaceCeiling {
                posture_floor: Some("approve".into()),
                ..Default::default()
            }),
        },
        BoundPlace {
            target: format!("discord:channel:{PIER}"),
            name: "#pier".into(),
            private: false,
            guild: Some("100000000000000002".into()),
            ceiling: None,
        },
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @owner".into(),
            ..Default::default()
        },
    ]);
}

/// An MCP server's tool, offered from its stored list at the start, as
/// the board offers one before its server answers.
fn with_mcp(cfg: &mut crate::Config) {
    cfg.mcp.servers.insert(
        "fake".into(),
        crate::config::McpServerConfig {
            command: vec!["theseus-sim".into(), "fake-mcp".into()],
            env: Default::default(),
            url: None,
            auth_secret: None,
            read: vec![],
            sandbox: Default::default(),
            egress: vec![],
            frozen: None,
            external: true,
            enabled: true,
            start_timeout_secs: 30,
            call_timeout_secs: 110,
        },
    );
}

fn store_mcp_list(store: &crate::store::Store) {
    let list = crate::mcp::StoredList {
        digest: "stored".into(),
        tools: vec![serde_json::from_value(json!({
            "name": "echo", "description": "Echoes its text.",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}
        }))
        .unwrap()],
    };
    store
        .put_meta(&format!("{}fake", crate::mcp::STORED_PREFIX), &list)
        .unwrap();
}

/// An input for a tool, inside the roots and the public paths (`open/`):
/// its required properties, each from its name or its type.
fn input_for(tool: &dyn Tool, open: &Path) -> Value {
    let readme = open.join("README.md");
    // The tools whose input a name or a type cannot make valid.
    match tool.name() {
        "fs.edit" => return json!({"path": readme, "old_string": "tide", "new_string": "ebb"}),
        "fs.patch" => {
            return json!({"patch": "--- a/open/README.md\n+++ b/open/README.md\n@@ -1 +1 @@\n-the open tide table\n+the open ebb table\n"})
        }
        "task.create" => {
            return json!({"brief": "read the tide table", "arrangement":
                          {"pieces": [{"quote": "read the open tide table today", "role": "objective"}]}})
        }
        "text.diff" => return json!({"a": "tide", "b": "ebb"}),
        "extend.propose" => {
            return json!({"name": "tidecount", "dir": open, "command": ["tidecount"],
                          "description": "count the open tide table"})
        }
        "task.update" => {
            return json!({"id": "tsk_0000tide", "version": 1, "patch": {"title": "the tide table"}})
        }
        "task.split" => {
            return json!({"id": "tsk_0000tide", "version": 1, "into": ["the ebb", "the flow"]})
        }
        "task.claim" => return json!({"id": "tsk_0000tide", "version": 1}),
        "task.close" => {
            return json!({"id": "tsk_0000tide", "version": 1, "outcome": "done",
                          "evidence": [{"identity": "commit:0000tide"}]})
        }
        "wake.at" => return json!({"after": "10m", "note": "check the tide"}),
        // `argv` or `steps`, so neither is required by the schema.
        "proc.run" => return json!({"argv": ["wc", "-l"]}),
        _ => {}
    }
    let schema = tool.input_schema();
    let mut input = Map::new();
    let required: Vec<String> = schema["required"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    for name in required {
        let prop = &schema["properties"][&name];
        let v = match (name.as_str(), prop["type"].as_str()) {
            ("argv", _) => json!(["wc", "-l"]),
            ("path" | "file" | "dir" | "cwd", _) => json!(open.join("README.md")),
            ("url", _) => json!("https://example.com/"),
            ("query" | "text" | "content" | "brief" | "note" | "symbol" | "keys", _) => {
                json!("the open tide table")
            }
            ("old" | "new" | "a" | "b", _) => json!("tide"),
            (_, Some("integer" | "number")) => json!(1),
            (_, Some("boolean")) => json!(false),
            (_, Some("array")) => json!([]),
            (_, Some("object")) => json!({}),
            (_, _) => match prop["enum"].as_array().and_then(|e| e.first()) {
                Some(first) => first.clone(),
                None => json!("tide"),
            },
        };
        input.insert(name, v);
    }
    // A directory the tool would default to the working directory for.
    for name in ["path", "cwd", "dir"] {
        if schema["properties"].get(name).is_some() && !input.contains_key(name) {
            input.insert(name.into(), json!(open));
        }
    }
    Value::Object(input)
}

/// The gate's decision on `input`, from the functions `gate` calls: the
/// tool's own plan, the place's refusal, then the order, with the place and
/// the hold read as the turn's gate reads them.
fn gated(core: &Core, sid: &str, tool: &dyn Tool, input: &Value) -> Option<(String, String)> {
    let rt = &core.tools;
    let plan = tool.plan(input, &rt.ctx).ok()?;
    let view = core.runner.view_of(sid);
    let held = || crate::external::held(&core.store, sid);
    // A glide's other place is its call's (38b), which neither a probe nor
    // this input names: `policy.explain` gives the rule as a condition.
    let at = At {
        place: view,
        held: &held,
        mcp: &|| None,
        glide: None,
    };
    // The gate's own judgment: a batch's every step (theseus-7gir.3).
    match rt.judge(&at, tool, &plan, input, &mut |_, _| {}) {
        Ok((d, _)) => Some((d.posture.as_str().into(), d.reason)),
        Err(Turned::Place(why) | Turned::Invalid(why)) => Some(("refused".into(), why)),
    }
}

fn explained(core: &Core, sid: &str) -> Vec<ToolExplain> {
    let r = core
        .policy_explain(PolicyExplainParams {
            session_id: Some(sid.into()),
            tool: None,
        })
        .unwrap();
    assert_eq!(r.places.len(), 1);
    r.places.into_iter().next().unwrap().tools
}

/// The reason after its summary: the gate's words for the tool, which a
/// probe's summary and a real call's differ before.
fn tail(reason: &str, tool: &str) -> String {
    let head = format!("{tool} — ");
    reason
        .find(&head)
        .map_or_else(|| reason.to_string(), |i| reason[i..].to_string())
}

/// Every tool, in every place, agrees: its result, and the gate's words.
/// Returns how many tools were compared with a real call.
fn every_tool_agrees(core: &Core, sessions: &[(&str, &str)], open: &Path) -> usize {
    let mut compared = 0;
    let mut unplanned = Vec::new();
    for (what, sid) in sessions {
        for row in explained(core, sid) {
            let tool = core.tools.tool(&row.tool).unwrap();
            let input = input_for(tool.as_ref(), open);
            let Some((result, reason)) = gated(core, sid, tool.as_ref(), &input) else {
                unplanned.push(row.tool.clone());
                continue;
            };
            compared += 1;
            assert_eq!(
                row.result, result,
                "{what}: {} explains {} but the gate decides {result} ({reason}) for {input}",
                row.tool, row.result
            );
            match result.as_str() {
                "refused" => assert!(row.reason.ends_with(&reason), "{what}: {row:?}"),
                _ => assert_eq!(
                    tail(&row.reason, &row.tool),
                    tail(&reason, &row.tool),
                    "{what}: {}",
                    row.tool
                ),
            }
            // The last layer's posture is the result, in the gate's order.
            if row.result != "refused" {
                assert_eq!(row.layers.last().unwrap().result, row.result, "{row:?}");
            }
        }
    }
    unplanned.sort();
    unplanned.dedup();
    // `term.send` names a terminal `term.open` gave, which none did.
    assert_eq!(
        unplanned,
        ["term.send"],
        "each tool needs an input here that its plan accepts"
    );
    compared
}

/// The table: every tool of the template's config and an MCP tool, in the
/// CLI, a floored private place, the owner's DM, and a shared place; then
/// with `proc.run` and `fs.write` tightened, and then with every session
/// holding external text.
#[tokio::test]
async fn policy_explain_agrees_with_the_gate_for_every_tool_and_place() {
    let r = rig_setup(with_mcp, store_mcp_list);
    let core = r.core.clone();
    bind(&core);
    let open = r.root.join("open");
    let cli = session(&core, None);
    let lab = session(&core, Some(&format!("channel:{LAB}")));
    let dm = session(&core, Some(&format!("dm:{OWNER}")));
    let pier = session(&core, Some(&format!("channel:{PIER}")));
    let sessions = [
        ("cli", cli.as_str()),
        ("#lab", &lab),
        ("dm", &dm),
        ("#pier", &pier),
    ];
    let rows = explained(&core, &cli);
    assert!(
        rows.iter().any(|t| t.tool == "mcp:fake/echo"),
        "the MCP tool is explained: {:?}",
        rows.iter().map(|t| &t.tool).collect::<Vec<_>>()
    );
    assert!(rows.len() >= 15, "every tool: {}", rows.len());
    let n = every_tool_agrees(&core, &sessions, &open);
    assert_eq!(
        n,
        4 * (rows.len() - 1),
        "every tool but term.send, in each place"
    );

    // The floor and the shared place say so.
    let lab_rows = explained(&core, &lab);
    let proc = lab_rows.iter().find(|t| t.tool == "proc.run").unwrap();
    assert_eq!(proc.result, "approve");
    let floor = proc.layers.iter().find(|l| l.layer == "floor").unwrap();
    assert!(
        floor.raised
            && floor
                .says
                .contains("#lab's ceiling sets a floor of approve")
    );
    let pier_rows = explained(&core, &pier);
    let proc = pier_rows.iter().find(|t| t.tool == "proc.run").unwrap();
    assert_eq!(proc.result, "refused");
    assert!(!proc.offered && proc.refused.as_deref().unwrap().contains("shared place"));
    let mcp = pier_rows
        .iter()
        .find(|t| t.tool == "mcp:fake/echo")
        .unwrap();
    assert_eq!(mcp.result, "refused", "no MCP tool in a shared place");

    // Tightened.
    core.tighten("proc.run", None, "cli").unwrap();
    core.tighten("fs.write", None, "cli").unwrap();
    every_tool_agrees(&core, &sessions, &open);
    let cli_rows = explained(&core, &cli);
    let proc = cli_rows.iter().find(|t| t.tool == "proc.run").unwrap();
    assert_eq!(proc.result, "approve");
    let t = proc
        .layers
        .iter()
        .find(|l| l.layer == "tightening")
        .unwrap();
    assert!(
        t.raised && t.setting.as_deref() == Some("tightened by cli"),
        "{t:?}"
    );

    // Held: every session read a page.
    for (_, sid) in &sessions {
        let mut rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
        rec.external = Some(ExternalText {
            since_ms: 1_759_300_000_000,
            tool: "http.fetch".into(),
            url: "tides.example/today".into(),
            ..Default::default()
        });
        core.store.put_session(sid, &rec).unwrap();
    }
    every_tool_agrees(&core, &sessions, &open);
    let dm_rows = explained(&core, &dm);
    for t in dm_rows.iter().filter(|t| t.class == "read") {
        let hold = t.layers.iter().find(|l| l.layer == "hold").unwrap();
        assert!(!hold.raised, "{t:?}");
    }
    let write = dm_rows.iter().find(|t| t.tool == "fs.write").unwrap();
    assert_eq!(write.result, "approve");
    let read = dm_rows.iter().find(|t| t.tool == "fs.read").unwrap();
    let own = read.layers.iter().find(|l| l.layer == "posture").unwrap();
    assert_eq!(
        read.result, own.result,
        "a read keeps its posture: {read:?}"
    );
}

/// What the gate recorded for each call of `sid`'s turns: its tool and
/// its result (`refused`, or the decision's posture).
fn recorded(core: &Core, sid: &str) -> Vec<(String, String)> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolCall { tool, gate, .. } => {
                let g = gate?;
                let result = match (g.result.gate.as_str(), &g.decision) {
                    ("deny", _) => "refused".to_string(),
                    (_, Some(d)) => d.posture.clone().unwrap_or_default(),
                    _ => "?".into(),
                };
                Some((tool, result))
            }
            _ => None,
        })
        .collect()
}

/// Through whole turns: each call the gate recorded agrees with the
/// session's explanation of its tool, in a floored place, the owner's DM,
/// and a shared place, before and after a tightening and a hold. The calls
/// are `proc.run` (`cat notes.md`) and two `fs.read`s; the one of
/// `notes.md` in the shared place is outside its public paths, which
/// explain lists as a condition, so it is left out there.
#[tokio::test]
async fn each_call_a_turn_records_agrees_with_its_sessions_explanation() {
    let r = rig_with(|_| {});
    let core = r.core.clone();
    bind(&core);
    let lab = session(&core, Some(&format!("channel:{LAB}")));
    let dm = session(&core, Some(&format!("dm:{OWNER}")));
    let pier = session(&core, Some(&format!("channel:{PIER}")));
    let check = |sid: &str, skip_private_read: bool| {
        let rows = explained(&core, sid);
        let calls = recorded(&core, sid);
        assert!(!calls.is_empty(), "{sid}");
        for (i, (tool, result)) in calls.iter().enumerate() {
            // The calls run three to a turn: p1, p2 (notes.md), p3.
            if skip_private_read && i % 3 == 1 {
                continue;
            }
            let row = rows.iter().find(|t| &t.tool == tool).unwrap();
            assert_eq!(&row.result, result, "{sid}: {tool}: {row:?}");
        }
    };
    for sid in [&lab, &dm, &pier] {
        turn(&core, sid, "look around").await;
    }
    check(&lab, false);
    check(&dm, false);
    check(&pier, true);
    // Tightened: a session of the CLI's, whose proc.run now waits.
    core.tighten("proc.run", None, "cli").unwrap();
    let cli = session(&core, None);
    turn(&core, &cli, "look around").await;
    check(&cli, false);
    assert_eq!(
        recorded(&core, &cli)[0],
        ("proc.run".into(), "approve".into())
    );
    // Held: a session that read a page, whose acting call waits (its turn
    // waits with it; the table test holds the reads).
    core.untighten("proc.run", "cli").unwrap();
    let held = session(&core, None);
    let mut rec: SessionRecord = core.store.get_session(&held).unwrap().unwrap();
    rec.external = Some(ExternalText {
        since_ms: 1_759_300_000_000,
        tool: "http.fetch".into(),
        url: "tides.example/today".into(),
        ..Default::default()
    });
    core.store.put_session(&held, &rec).unwrap();
    turn(&core, &held, "look around").await;
    check(&held, false);
    assert_eq!(
        recorded(&core, &held)[0],
        ("proc.run".into(), "approve".into())
    );
}

/// A batch (`proc.run`'s `steps`, theseus-7gir.3) is judged as the gate
/// judges it: each step as the call it would be alone, the strictest
/// taken, its reason naming the step; and the explanation says so.
#[tokio::test]
async fn a_batch_is_judged_as_its_strictest_step() {
    let r = rig_with(|_| {});
    let core = &r.core;
    let sid = session(core, None);
    let tool = core.tools.registry.get("proc.run").unwrap().clone();
    let alone = gated(
        core,
        &sid,
        tool.as_ref(),
        &json!({"argv": ["op", "whoami"]}),
    )
    .unwrap();
    let batch = json!({"steps": [{"argv": ["wc", "-l"]}, {"argv": ["op", "whoami"]}]});
    let (posture, reason) = gated(core, &sid, tool.as_ref(), &batch).unwrap();
    assert_eq!(posture, alone.0);
    assert_eq!(posture, "approve");
    assert_eq!(reason, format!("step 2 of 2 (`op whoami`): {}", alone.1));
    let row = explained(core, &sid)
        .into_iter()
        .find(|t| t.tool == "proc.run")
        .unwrap();
    assert!(row.conditions.iter().any(|c| c.layer == "steps"), "{row:?}");
}
