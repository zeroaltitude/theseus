//! `drive.*` through the whole core, against the loopback fake's Drive
//! (`theseus_google::fake::drive`): what compiles to Drive's `q`, a Doc read
//! and written as markdown, who is outside text, and that a share or a trash
//! asks at every posture.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::drive::{Drive, File};
use theseus_tools::{External, Tool, ToolCtx, ToolOutput};

use super::tests::*;
use crate::policy::Posture;
use crate::tools_defer::family::of_wire;

fn tool(r: &Rig, name: &str) -> Arc<dyn Tool> {
    r.core
        .tools
        .registry
        .get(name)
        .unwrap_or_else(|| panic!("{name} is registered"))
        .clone()
}

async fn run(r: &Rig, name: &str, input: Value) -> Result<(ToolOutput, Option<External>), String> {
    let ctx = ToolCtx::for_tests(r.dir.path());
    tool(r, name)
        .run_async(&input, &ctx)
        .await
        .map_err(|f| f.message)
}

async fn ok(r: &Rig, name: &str, input: Value) -> (ToolOutput, Option<External>) {
    run(r, name, input)
        .await
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

async fn drive_rig(script: Vec<crate::provider::Scripted>) -> (Rig, Drive) {
    let r = rig(script).await;
    let d = Drive::install(&r.fake);
    (r, d)
}

fn query(r: &Rig, path: &str) -> Vec<(String, String)> {
    r.fake
        .log()
        .into_iter()
        .rev()
        .find(|l| l.path == path)
        .unwrap_or_else(|| panic!("no request to {path}"))
        .query
}

fn arg(q: &[(String, String)], key: &str) -> String {
    q.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn call_of(id: &str, wire: &str, input: Value) -> crate::provider::Scripted {
    crate::provider::Scripted::tools("", &[(id, wire, input)])
}

fn plan_of(r: &Rig, name: &str, input: Value) -> Result<theseus_tools::Plan, String> {
    tool(r, name).plan(&input, &ToolCtx::for_tests(r.dir.path()))
}

#[tokio::test]
async fn share_and_trash_always_ask_whatever_the_posture() {
    // Every posture open, in the config and on each tool: only a plan's own
    // authority can still make the call wait.
    let open = |c: &mut crate::Config| {
        c.policy.enforcement = Posture::Open;
        for t in ["drive.share", "drive.write", "drive.read", "drive.find"] {
            c.policy.tools.insert(t.into(), Posture::Open);
        }
    };
    let share = json!({"id": "doc1", "email": "collaborator@example.test", "role": "writer"});
    let trash = json!({"id": "doc1", "trash": true});
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![
            call_of("c1", "drive_share", share.clone()),
            call_of("c2", "drive_share", trash.clone()),
        ],
        open,
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let t = tool(&r, "drive.share");
    for input in [&share, &trash] {
        let plan = plan_of(&r, "drive.share", input.clone()).unwrap();
        assert!(plan.authority.is_some(), "{plan:?}");
        let d = r.core.tools.policy.decide(t.as_ref(), &plan);
        assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
        // Earned grants and a routine's grant loosen only a call the posture
        // alone makes wait: this one is not.
        assert!(!r.core.tools.policy.posture_only(t.as_ref(), &plan));
    }
    // The writes and the reads at the same posture are decided by it, open.
    for (name, input) in [
        ("drive.write", json!({"id": "doc1", "rename": "Plan B"})),
        ("drive.read", json!({"id": "doc1"})),
        ("drive.find", json!({"text": "plan"})),
    ] {
        let plan = plan_of(&r, name, input).unwrap();
        assert!(plan.authority.is_none(), "{name}");
        let tool = tool(&r, name);
        assert!(
            r.core.tools.policy.posture_only(tool.as_ref(), &plan),
            "{name}"
        );
        assert_eq!(
            r.core.tools.policy.decide(tool.as_ref(), &plan).posture,
            Posture::Open,
            "{name}"
        );
    }
    // Through whole turns: the share waits, then the trash waits, and nothing went out.
    for what in ["share it", "trash it"] {
        let res = crate::test_support::turn(&r.core, &session(&r.core), what).await;
        assert!(res.awaiting_confirm.is_some(), "{what}: {res:?}");
    }
    let sent = r.fake.log().into_iter().any(|l| {
        l.path.ends_with("/permissions") || (l.verb == "PATCH" && l.path == "/drive/v3/files/doc1")
    });
    assert!(!sent, "nothing was shared or trashed");
    assert!(!drive.file("doc1").unwrap().trashed);
    let asked = ledgered(&r.core, "tool.confirm_requested");
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(
        asked
            .iter()
            .all(|a| a["reason"].as_str().unwrap().contains("every time")),
        "{asked:?}"
    );
}

#[tokio::test]
async fn markdown_round_trips_through_a_doc() {
    let (r, drive) = drive_rig(vec![]).await;
    let md = "# Plan\n\n## Steps\n\n- first\n- second\n\n1. one\n2. two\n";
    let (made, ext) = ok(
        &r,
        "drive.write",
        json!({"name": "Plan", "content": md, "as": "doc"}),
    )
    .await;
    assert!(ext.is_none(), "the owner's own write is not outside text");
    let id = made.meta["id"].as_str().unwrap().to_string();
    // It was sent as an import: the metadata names the Doc type, the media is markdown.
    let up = r
        .fake
        .log()
        .into_iter()
        .find(|l| l.path == "/upload/drive/v3/files")
        .unwrap();
    assert_eq!(arg(&up.query, "uploadType"), "multipart");
    let f = drive.file(&id).unwrap();
    assert_eq!(f.mime, theseus_google::fake::drive::DOC);
    // And it reads back as markdown, headings and lists kept.
    let (read, ext) = ok(&r, "drive.read", json!({"id": id})).await;
    assert!(ext.is_none(), "{ext:?}");
    for line in [
        "# Plan", "## Steps", "- first", "- second", "1. one", "2. two",
    ] {
        assert!(read.text.contains(line), "{line}: {}", read.text);
    }
    assert_eq!(
        arg(
            &query(&r, &format!("/drive/v3/files/{id}/export")),
            "mimeType"
        ),
        "text/markdown"
    );
    // Replacing the content, then moving and renaming, keep the file.
    ok(
        &r,
        "drive.write",
        json!({"id": id, "content": "# Plan v2\n"}),
    )
    .await;
    ok(
        &r,
        "drive.write",
        json!({"id": id, "move_to": "folder9", "rename": "Plan B"}),
    )
    .await;
    let f = drive.file(&id).unwrap();
    assert_eq!(
        (f.name.as_str(), f.parents.as_slice()),
        ("Plan B", ["folder9".to_string()].as_slice())
    );
    let (read, _) = ok(&r, "drive.read", json!({"id": id})).await;
    assert!(read.text.contains("# Plan v2"), "{}", read.text);
    // A CSV becomes a Sheet, read back as a table.
    let (made, _) = ok(
        &r,
        "drive.write",
        json!({"name": "Numbers.csv", "content": "a,b\n1,2\n"}),
    )
    .await;
    let (read, _) = ok(&r, "drive.read", json!({"id": made.meta["id"]})).await;
    assert!(
        read.text.contains("| a | b |") && read.text.contains("| 1 | 2 |"),
        "{}",
        read.text
    );
}

#[tokio::test]
async fn a_shared_owned_file_is_external() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::doc("mine", "Notes", "# Notes\n").shared());
    drive.add(File::doc("theirs", "Offer", "# Offer\n").theirs());
    let (_, ext) = ok(&r, "drive.read", json!({"id": "mine"})).await;
    assert_eq!(ext.map(|e| e.url).as_deref(), Some("drive:mine"));
    let (out, ext) = ok(&r, "drive.read", json!({"id": "theirs"})).await;
    assert_eq!(ext.map(|e| e.url).as_deref(), Some("drive:theirs"));
    assert!(
        crate::google::private::marked(&out.meta),
        "every result is private"
    );
    // A listing with one such row is outside text too.
    let (_, ext) = ok(&r, "drive.find", json!({})).await;
    assert!(ext.is_some());
}

#[tokio::test]
async fn an_owned_unshared_file_is_not_external() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::doc("mine", "Notes", "# Notes\n"));
    let (out, ext) = ok(&r, "drive.read", json!({"id": "mine"})).await;
    assert!(ext.is_none(), "{ext:?}");
    assert!(crate::google::private::marked(&out.meta), "private even so");
    let (_, ext) = ok(&r, "drive.find", json!({"name": "Notes"})).await;
    assert!(
        ext.is_none(),
        "a listing of the owner's own unshared files: {ext:?}"
    );
    assert_eq!(r.fake.log().last().unwrap().path, "/drive/v3/files");
}

#[tokio::test]
async fn find_compiles_typed_fields_to_drive_q() {
    let (r, _) = drive_rig(vec![]).await;
    ok(
        &r,
        "drive.find",
        json!({"text": "Q3 plan", "name": "O'Brien \\ budget", "folder": "fold_1", "type": "sheet",
            "q": "starred = true", "recent": true, "max": 500}),
    )
    .await;
    let q = query(&r, "/drive/v3/files");
    assert_eq!(
        arg(&q, "q"),
        "(starred = true) and fullText contains 'Q3 plan' and name contains 'O\\'Brien \\\\ budget' \
         and 'fold_1' in parents and mimeType = 'application/vnd.google-apps.spreadsheet' \
         and trashed = false"
    );
    assert_eq!(
        (arg(&q, "pageSize").as_str(), arg(&q, "orderBy").as_str()),
        ("100", "modifiedTime desc")
    );
    // A folder that is not an id cannot smuggle a clause in.
    let e = run(&r, "drive.find", json!({"folder": "x' or 'a' in parents"}))
        .await
        .unwrap_err();
    assert!(e.contains("not a Drive file id"), "{e}");
    let e = run(&r, "drive.find", json!({"type": "spreadsheet"}))
        .await
        .unwrap_err();
    assert!(e.contains("not doc, sheet"), "{e}");
    assert_eq!(super::drive::quote("a'b\\c"), "'a\\'b\\\\c'");
}

#[tokio::test]
async fn a_large_binary_needs_save_to() {
    let (r, drive) = drive_rig(vec![]).await;
    let big = vec![7u8; (1 << 20) + 1];
    drive.add(File::binary(
        "big",
        "scan.pdf",
        "application/pdf",
        big.clone(),
    ));
    drive.add(File::binary(
        "small",
        "note.txt",
        "text/plain",
        b"hello".to_vec(),
    ));
    drive.add(File::binary(
        "blob",
        "pic.png",
        "image/png",
        vec![0xff, 0xd8, 0xff, 0x00],
    ));
    let e = run(&r, "drive.read", json!({"id": "big"}))
        .await
        .unwrap_err();
    assert!(e.contains("save_to"), "{e}");
    assert!(
        r.fake
            .log()
            .iter()
            .all(|l| l.query.iter().all(|(k, _)| k != "alt")),
        "nothing was downloaded"
    );
    // With `save_to` it is written, under the workspace, whole.
    let (out, _) = ok(
        &r,
        "drive.read",
        json!({"id": "big", "save_to": "work/scan.pdf"}),
    )
    .await;
    assert_eq!(
        std::fs::read(r.dir.path().join("work/scan.pdf")).unwrap(),
        big
    );
    assert!(out.text.contains("work/scan.pdf"), "{}", out.text);
    // A small text file is shown; a small binary is not, and says why.
    let (out, _) = ok(&r, "drive.read", json!({"id": "small"})).await;
    assert!(out.text.contains("hello"), "{}", out.text);
    let e = run(&r, "drive.read", json!({"id": "blob"}))
        .await
        .unwrap_err();
    assert!(e.contains("not text") && e.contains("save_to"), "{e}");
    // The saved file is a resource of the plan: the gate judges the path.
    let plan = plan_of(
        &r,
        "drive.read",
        json!({"id": "big", "save_to": "work/scan.pdf"}),
    )
    .unwrap();
    assert_eq!(plan.resources.len(), 1);
    assert_eq!(plan.resources[0].access, theseus_tools::Access::Write);
}

#[tokio::test]
async fn a_sheet_reads_as_tabs_and_a_table_and_slides_as_text_by_object() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::sheet(
        "sh1",
        "Budget",
        vec![
            (
                "Q3",
                vec![vec!["item", "cost"], vec!["rent | office", "900"]],
            ),
            ("Q4", vec![vec!["item", "cost"], vec!["tea", "5"]]),
        ],
    ));
    drive.add(File::slides(
        "sl1",
        "Kickoff",
        vec![
            ("p1", vec![("title1", "Welcome"), ("body1", "Agenda\n")]),
            ("p2", vec![("t2", "Thanks")]),
        ],
    ));
    let (out, _) = ok(&r, "drive.read", json!({"id": "sh1"})).await;
    for want in [
        "- Q3 (2 rows x 2 columns)",
        "- Q4",
        "| item | cost |",
        "| --- | --- |",
        "| rent \\| office | 900 |",
    ] {
        assert!(out.text.contains(want), "{want}: {}", out.text);
    }
    assert_eq!(
        arg(&query(&r, "/v4/spreadsheets/sh1/values:batchGet"), "ranges"),
        "'Q3'!1:200"
    );
    let (out, _) = ok(&r, "drive.read", json!({"id": "sh1", "range": "Q4!A1:B2"})).await;
    assert!(
        out.text.contains("| tea | 5 |") && !out.text.contains("rent"),
        "{}",
        out.text
    );
    let (out, _) = ok(&r, "drive.read", json!({"id": "sl1"})).await;
    for want in [
        "## Slide 1 (p1, layout TITLE_AND_BODY)",
        "[title1] Welcome",
        "[body1] Agenda",
        "## Slide 2 (p2",
    ] {
        assert!(out.text.contains(want), "{want}: {}", out.text);
    }
}

#[tokio::test]
async fn share_grants_and_removes_access_and_trash_moves_a_file() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    ok(&r, "drive.share", json!({"id": "doc1", "email": "collaborator@example.test", "role": "commenter", "notify": true})).await;
    let post = r
        .fake
        .log()
        .into_iter()
        .find(|l| l.verb == "POST" && l.path.ends_with("/permissions"))
        .unwrap();
    assert_eq!(
        post.body,
        json!({"type": "user", "role": "commenter", "emailAddress": "collaborator@example.test"})
    );
    assert_eq!(arg(&post.query, "sendNotificationEmail"), "true");
    assert!(drive.file("doc1").unwrap().shared);
    ok(
        &r,
        "drive.share",
        json!({"id": "doc1", "anyone_with_link": true}),
    )
    .await;
    assert_eq!(drive.file("doc1").unwrap().permissions.len(), 2);
    ok(
        &r,
        "drive.share",
        json!({"id": "doc1", "email": "Collaborator@Example.test", "remove": true}),
    )
    .await;
    let left = drive.file("doc1").unwrap().permissions;
    assert_eq!((left.len(), left[0].kind.as_str()), (1, "anyone"));
    let e = run(
        &r,
        "drive.share",
        json!({"id": "doc1", "email": "nobody@example.test", "remove": true}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("no such access"), "{e}");
    // The wrong shapes are refused before anything is sent.
    for bad in [
        json!({"id": "doc1"}),
        json!({"id": "doc1", "email": "a@example.test", "anyone_with_link": true}),
        json!({"id": "doc1", "email": "not an address"}),
        json!({"id": "doc1", "email": "a@example.test", "role": "owner"}),
        json!({"id": "doc1", "trash": true, "email": "a@example.test"}),
    ] {
        assert!(plan_of(&r, "drive.share", bad.clone()).is_err(), "{bad}");
    }
    ok(&r, "drive.share", json!({"id": "doc1", "trash": true})).await;
    assert!(drive.file("doc1").unwrap().trashed);
    ok(&r, "drive.share", json!({"id": "doc1", "trash": false})).await;
    assert!(!drive.file("doc1").unwrap().trashed);
}

#[tokio::test]
async fn every_drive_tool_is_deferred_and_small() {
    // Tokens as the request estimates them: bytes at 2.4 to the token.
    const MAX_TOKENS: usize = 220;
    let r = rig(vec![]).await;
    for name in super::drive::tools(&r.g)
        .iter()
        .map(|t| t.name().to_string())
    {
        let t = tool(&r, &name);
        let wire = name.replace('.', "_");
        assert_eq!(
            of_wire(&wire),
            "docs",
            "{wire} is deferred in the docs family"
        );
        let def =
            json!({"name": wire, "description": t.description(), "input_schema": t.input_schema()});
        let tokens = def.to_string().len() * 10 / 24;
        assert!(tokens <= MAX_TOKENS, "{wire} is {tokens} tokens");
    }
    assert!(["drive.find", "drive.read", "drive.write", "drive.share"]
        .iter()
        .all(|n| super::NAMES.contains(n)));
}

#[tokio::test]
async fn a_write_names_what_it_needs_and_a_path_it_reads() {
    let (r, _) = drive_rig(vec![]).await;
    for (bad, says) in [
        (json!({"content": "x"}), "needs a `name`"),
        (
            json!({"name": "a", "content": "x", "from_path": "a.md"}),
            "not both",
        ),
        (json!({"id": "abc"}), "give `content`"),
        (
            json!({"id": "abc", "name": "new", "content": "x"}),
            "make a new file",
        ),
        (
            json!({"name": "a", "move_to": "x1"}),
            "change an existing file",
        ),
        (
            json!({"name": "a", "content": "x", "as": "slides"}),
            ".pptx",
        ),
        (json!({"name": "a", "as": "page"}), "not doc, sheet"),
    ] {
        let e = plan_of(&r, "drive.write", bad.clone()).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
    // A file it uploads is a read resource of its plan, and its bytes are sent.
    std::fs::create_dir_all(r.dir.path().join("work")).unwrap();
    std::fs::write(r.dir.path().join("work/notes.md"), "# From a file\n").unwrap();
    let plan = plan_of(
        &r,
        "drive.write",
        json!({"name": "Notes", "from_path": "work/notes.md"}),
    )
    .unwrap();
    assert_eq!(plan.resources[0].access, theseus_tools::Access::Read);
    let (out, _) = ok(
        &r,
        "drive.write",
        json!({"name": "Notes", "from_path": "work/notes.md"}),
    )
    .await;
    assert_eq!(
        out.meta["mime"],
        theseus_google::fake::drive::DOC,
        "an .md file becomes a Doc"
    );
    let up = r
        .fake
        .log()
        .into_iter()
        .find(|l| l.path == "/upload/drive/v3/files")
        .unwrap();
    assert_eq!(up.verb, "POST");
}
