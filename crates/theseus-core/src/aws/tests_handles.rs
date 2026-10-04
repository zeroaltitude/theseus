//! Secret-bearing handles (AWS design §3.5; C3 = 14c): a secret an AWS call
//! returns goes onto the secrets board under its handle, and its value
//! never reaches the tool's text, a node, the WAL, the ledger, or a span:
//! the scrubber's test pattern, every file under the store searched for it.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::tests::{board, core, layer, ledgered, tool, turn, Fake, Reply, Seen, ACCOUNT};
use super::tests_c3::{json_reply, target};
use crate::node::Body;
use crate::provider::Scripted;

/// The secret the fake's Secrets Manager keeps.
const VALUE: &str = "hunter2-correct-horse-0042";
/// The parameter the fake's SSM keeps, decrypted.
const PARAM: &str = "param-secret-value-77";

fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return super::tests::sts(ACCOUNT, n);
    }
    match target(s) {
        Some("GetSecretValue") => json_reply(
            n,
            json!({
                "ARN": format!("arn:aws:secretsmanager:us-west-2:{ACCOUNT}:secret:app/db-AbCdEf"),
                "Name": "app/db",
                "VersionId": "v-1",
                "SecretString": format!("{{\"user\":\"app\",\"password\":\"{VALUE}\"}}"),
                "CreatedDate": 1_791_028_800.0
            }),
        ),
        Some("GetParameter") => json_reply(
            n,
            json!({"Parameter": {"Name": "/app/token", "Type": "SecureString", "Value": PARAM, "Version": 3}}),
        ),
        _ => (400, vec![], "{}".into()),
    }
}

/// Every file under `dir` that holds `needle`'s bytes.
fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    fn walk(p: &Path, needle: &[u8], out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&path, needle, out),
                Ok(t)
                    if t.is_file()
                        && std::fs::read(&path)
                            .is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle)) =>
                {
                    out.push(path);
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, needle.as_bytes(), &mut out);
    out
}

/// The tool itself, before any scrubber: its text and meta hold the
/// handle and the secret's shape, never the value, and the board holds it.
#[tokio::test]
async fn a_secret_becomes_a_handle_before_the_scrubber_sees_it() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    let ctx = super::tests::plain_ctx();
    for (input, handle, value) in [
        (
            json!({"service": "secretsmanager", "operation": "GetSecretValue", "input": {"SecretId": "app/db"}}),
            "aws-secret:app/db#SecretString",
            format!("{{\"user\":\"app\",\"password\":\"{VALUE}\"}}"),
        ),
        (
            json!({"service": "ssm", "operation": "GetParameter", "input": {"Name": "/app/token", "WithDecryption": true}}),
            "aws-secret:/app/token#Parameter.Value",
            PARAM.to_string(),
        ),
    ] {
        t.plan(&input, &ctx).expect("a secret-bearing read plans");
        let (out, _) = t.run_async(&input, &ctx).await.expect("it runs");
        let said = format!("{} {}", out.text, out.meta);
        assert!(!said.contains(VALUE) && !said.contains(PARAM), "{said}");
        assert!(out.text.contains(handle), "{}", out.text);
        assert_eq!(out.meta["secrets"], json!([handle]));
        assert_eq!(b.get(handle).expect("on the board").expose(), value);
    }
    // A JSON secret shows its keys, masked.
    let (out, _) = t
        .run_async(
            &json!({"service": "secretsmanager", "operation": "GetSecretValue", "input": {"SecretId": "app/db"}}),
            &ctx,
        )
        .await
        .unwrap();
    assert!(
        out.text.contains("\"password\": \"<masked>\""),
        "{}",
        out.text
    );
}

/// Through the whole core: the model reads a secret, and its value reaches
/// no node, no ledger row, no span, and no file of the store (the WAL, its
/// index); the board holds it, so the scrubber withholds it from a later
/// result that would carry it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handles_value_never_reaches_a_node_the_wal_or_the_ledger() {
    let fake = Fake::start(answers);
    let dir = tempfile::tempdir().unwrap();
    let read = json!({"service": "secretsmanager", "operation": "GetSecretValue", "input": {"SecretId": "app/db"}});
    let core = core(
        &fake,
        dir.path(),
        vec![
            Scripted::tools("", &[("t_secret", "aws_call", read)]),
            Scripted::text("The secret is held as aws-secret:app/db#SecretString."),
        ],
    );
    let r = turn(&core, "read the database's secret").await;
    assert_eq!(r.tool_calls, 1);
    let result = core
        .store
        .session_nodes(&r.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult { tool, content, .. } if tool == "aws.call" => Some(content.clone()),
            _ => None,
        })
        .expect("the call's result");
    assert!(
        result.contains("aws-secret:app/db#SecretString"),
        "{result}"
    );
    assert!(!result.contains(VALUE), "{result}");
    assert!(
        core.secrets.get("aws-secret:app/db#SecretString").is_some(),
        "the board holds the handle's value"
    );
    for kind in ["aws.called", "tool.ended", "tool.started"] {
        for row in ledgered(&core, kind) {
            assert!(!row.to_string().contains(VALUE), "{kind}: {row}");
        }
    }
    let spans: Value = serde_json::to_value(&r.trace).unwrap();
    assert!(!spans.to_string().contains(VALUE));
    let held = files_holding(dir.path(), VALUE);
    assert!(held.is_empty(), "the value is in {held:?}");
    // The scrubber knows it now: a text that carries it is withheld.
    let whole = format!("{{\"user\":\"app\",\"password\":\"{VALUE}\"}}");
    let scrubbed = crate::scrub::Scrubber::from_board(core.secrets.clone())
        .scrub(&format!("echo {whole}"))
        .0;
    assert_eq!(scrubbed, "echo [redacted:aws-secret:app/db#SecretString]");
}
