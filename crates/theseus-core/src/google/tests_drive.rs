//! `drive.*` through the whole core, against the loopback fake's Drive
//! (`theseus_google::fake::drive`): what compiles to Drive's `q`, a Doc read
//! and written as markdown, who is outside text, and that a share or a trash
//! asks at every posture.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::drive::{Drive, File};
use theseus_tools::ToolFailure;
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
        // A mode and a routine's grant loosen only a call the posture
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
    drive.add(File::folder("folder9", "Archive"));
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
    // Replacing a Sheet's content sends CSV, as a Doc's sends markdown: plain
    // text would land in the file as literal text (the fake refuses it).
    ok(
        &r,
        "drive.write",
        json!({"id": made.meta["id"], "content": "a,b\n3,4\n"}),
    )
    .await;
    let (read, _) = ok(&r, "drive.read", json!({"id": made.meta["id"]})).await;
    assert!(read.text.contains("| 3 | 4 |"), "{}", read.text);
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
    let (out, _) = ok(&r, "drive.read", json!({"id": "big"})).await;
    assert!(
        out.text.contains("save_to") && out.text.contains("1048577 bytes"),
        "{}",
        out.text
    );
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
    let (out, _) = ok(&r, "drive.read", json!({"id": "blob"})).await;
    assert!(
        out.text.contains("not text") && out.text.contains("save_to"),
        "{}",
        out.text
    );
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

/// A file over the inline cap, read with no `save_to`, answers from its metadata
/// alone: what it is, how big, whose, when, and how to get it (theseus-izyws).
#[tokio::test]
async fn a_large_file_without_save_to_answers_with_its_metadata() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::binary(
        "big",
        "standup.mp3",
        "audio/mpeg",
        vec![7u8; (1 << 20) + 1],
    ));
    let (out, _) = ok(&r, "drive.read", json!({"id": "big"})).await;
    for want in [
        "standup.mp3",
        "1048577 bytes",
        "owner: you",
        "2026-10-20T09:00:00Z",
        "`save_to`",
        "big",
    ] {
        assert!(out.text.contains(want), "{want}: {}", out.text);
    }
    assert!(out.text.contains("audio/mpeg"), "{}", out.text);
    assert_eq!(out.meta["inline"], json!(false));
    assert_eq!(out.meta["bytes"], json!(1_048_577));
    assert!(
        r.fake
            .log()
            .iter()
            .all(|l| l.query.iter().all(|(k, _)| k != "alt")),
        "nothing was downloaded"
    );
}

/// A Google Doc has no size in its metadata, so the read does not stop before the
/// export: the text over the inline cap is found after the download, and the
/// answer is the same metadata, with the true size.
#[tokio::test]
async fn an_exported_doc_over_1_mb_answers_with_its_metadata() {
    let (r, drive) = drive_rig(vec![]).await;
    let head = "# Notes\n";
    let md = format!("{head}{}", "a".repeat((1 << 20) + 1 - head.len()));
    assert_eq!(md.len(), (1 << 20) + 1);
    drive.add(File::doc("bigdoc", "Long Notes", &md));
    let (out, _) = ok(&r, "drive.read", json!({"id": "bigdoc"})).await;
    for want in [
        "text over 1 MB",
        "1048577 bytes",
        "Long Notes",
        "bigdoc",
        "`save_to`",
    ] {
        assert!(out.text.contains(want), "{want}: {}", out.text);
    }
    assert!(out.text.len() < 2_000, "the body is not shown");
    assert_eq!(out.meta["inline"], json!(false));
    assert_eq!(out.meta["bytes"], json!(1_048_577));
    assert!(
        r.fake
            .log()
            .iter()
            .any(|l| l.path == "/drive/v3/files/bigdoc/export"),
        "the export happened: this is the arm after the download"
    );
}

#[tokio::test]
async fn an_exported_doc_that_is_not_text_says_so() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::binary(
        "rawdoc",
        "Odd Doc",
        theseus_google::fake::drive::DOC,
        vec![0xff, 0xfe, 0x00, 0x80],
    ));
    let (out, _) = ok(&r, "drive.read", json!({"id": "rawdoc"})).await;
    assert!(
        out.text.contains("not text") && out.text.contains("`save_to`"),
        "{}",
        out.text
    );
    assert_eq!(out.meta["inline"], json!(false));
    assert_eq!(out.meta["bytes"], json!(4));
    assert!(
        r.fake
            .log()
            .iter()
            .any(|l| l.path == "/drive/v3/files/rawdoc/export"),
        "the export happened"
    );
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

/// The tool's own failure, meta and all (`run` keeps only the message).
async fn failure(r: &Rig, name: &str, input: Value) -> ToolFailure {
    let ctx = ToolCtx::for_tests(r.dir.path());
    match tool(r, name).run_async(&input, &ctx).await {
        Ok(_) => panic!("{name} should have failed"),
        Err(f) => f,
    }
}

fn methods(meta: &Value) -> Vec<String> {
    let also = meta["google_called_also"].as_array().into_iter().flatten();
    also.chain(meta.get("google_called"))
        .map(|row| row["method"].as_str().unwrap_or("?").to_string())
        .collect()
}

#[tokio::test]
async fn a_sheet_read_keeps_every_calls_row() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::sheet(
        "sh1",
        "Tides",
        vec![("Sheet1", vec![vec!["port", "depth"], vec!["Arden", "7"]])],
    ));
    let (out, _) = ok(&r, "drive.read", json!({"id": "sh1"})).await;
    assert_eq!(
        methods(&out.meta),
        [
            "drive.files.get",
            "sheets.spreadsheets.get",
            "sheets.spreadsheets.values.batchGet"
        ]
    );
    crate::fact::google::of_result(&r.core.rec(None), &out.meta);
    let rows = ledgered(&r.core, "google.called");
    let said: Vec<_> = rows.iter().map(|r| r["method"].as_str().unwrap()).collect();
    assert_eq!(
        said,
        [
            "drive.files.get",
            "sheets.spreadsheets.get",
            "sheets.spreadsheets.values.batchGet"
        ],
        "{rows:?}"
    );
}

#[tokio::test]
async fn an_error_after_a_call_keeps_its_row() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::folder("fold1", "Archive"));
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let f = failure(&r, "drive.read", json!({"id": "fold1"})).await;
    assert!(f.message.contains("is a folder"), "{}", f.message);
    assert_eq!(methods(&f.meta), ["drive.files.get"], "{:?}", f.meta);
    assert_eq!(f.meta["private"], true);
    // A revoke of an address with no access: the list went through, the delete never ran.
    let f = failure(
        &r,
        "drive.share",
        json!({"id": "doc1", "email": "nobody@example.test", "remove": true}),
    )
    .await;
    assert!(f.message.contains("no such access"), "{}", f.message);
    assert_eq!(methods(&f.meta), ["drive.permissions.list"], "{:?}", f.meta);
}

#[tokio::test]
async fn a_move_keeps_the_get_and_the_patch() {
    let (r, drive) = drive_rig(vec![]).await;
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    drive.add(File::folder("fold2", "Archive"));
    let (out, _) = ok(&r, "drive.write", json!({"id": "doc1", "move_to": "fold2"})).await;
    // The folder's sharing, the file's parents, then the move itself.
    assert_eq!(
        methods(&out.meta),
        ["drive.files.get", "drive.files.get", "drive.files.update"],
        "{:?}",
        out.meta
    );
    assert_eq!(drive.file("doc1").unwrap().parents, ["fold2".to_string()]);
}

#[tokio::test]
async fn a_write_into_a_shared_folder_asks_first() {
    let open = |c: &mut crate::Config| {
        c.policy.enforcement = Posture::Open;
        c.policy.tools.insert("drive.write".into(), Posture::Open);
    };
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![],
        open,
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::folder("team1", "Team").shared());
    drive.add(File::folder("mine1", "Mine"));
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let writes = |r: &Rig| {
        r.fake
            .log()
            .into_iter()
            .filter(|l| l.verb == "POST" || l.verb == "PATCH")
            .count()
    };
    let create = json!({"name": "Notes", "content": "hello", "folder": "team1"});
    let mv = json!({"id": "doc1", "move_to": "team1"});
    for input in [&create, &mv] {
        let f = failure(&r, "drive.write", input.clone()).await;
        assert!(f.message.contains("into_shared"), "{}", f.message);
        assert!(f.message.contains("shared"), "{}", f.message);
        assert_eq!(methods(&f.meta), ["drive.files.get"], "{:?}", f.meta);
    }
    assert_eq!(writes(&r), 0, "nothing was created or moved");
    assert_eq!(drive.file("doc1").unwrap().parents, ["root".to_string()]);
    // A shared drive's folder counts too, though the folder itself is not shared.
    r.fake.route("GET", "/drive/v3/files/sd1", |_| {
        theseus_google::fake::Reply::json(
            200,
            json!({"id": "sd1", "mimeType": "application/vnd.google-apps.folder", "driveId": "drv1"}),
        )
    });
    let f = failure(
        &r,
        "drive.write",
        json!({"name": "Notes", "content": "hello", "folder": "sd1"}),
    )
    .await;
    assert!(f.message.contains("into_shared"), "{}", f.message);
    // An unshared folder writes as before.
    ok(
        &r,
        "drive.write",
        json!({"name": "Notes", "content": "hello", "folder": "mine1"}),
    )
    .await;
    assert_eq!(writes(&r), 1);
    // `into_shared` makes the plan the owner's, at every posture, and the run then writes.
    let asked =
        json!({"name": "Notes", "content": "hello", "folder": "team1", "into_shared": true});
    let plain = plan_of(&r, "drive.write", create.clone()).unwrap();
    assert!(plain.authority.is_none(), "{plain:?}");
    let plan = plan_of(&r, "drive.write", asked.clone()).unwrap();
    assert!(plan.authority.is_some(), "{plan:?}");
    assert_eq!(plan.class, Some(theseus_tools::ToolClass::Write));
    let t = tool(&r, "drive.write");
    let d = r.core.tools.policy.decide(t.as_ref(), &plan);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(!r.core.tools.policy.posture_only(t.as_ref(), &plan));
    ok(&r, "drive.write", asked).await;
    assert_eq!(writes(&r), 2, "the file was created in the shared folder");
}

#[test]
fn a_raw_q_must_balance() {
    use super::drive::compile_q;
    let q = |q| compile_q(Some(q), None, None, None, None);
    let e = q("a) or (b").unwrap_err();
    assert!(e.contains("unbalanced"), "{e}");
    assert!(q("(a").is_err());
    assert!(q("name = 'x").is_err());
    assert_eq!(
        q("name contains '(draft'").unwrap(),
        "(name contains '(draft') and trashed = false"
    );
    assert_eq!(
        q("name = 'it\\'s )'").unwrap(),
        "(name = 'it\\'s )') and trashed = false"
    );
    assert_eq!(q("(a or b)").unwrap(), "((a or b)) and trashed = false");
}

#[test]
fn a_double_quote_delimits_a_drive_query() {
    use super::drive::compile_q;
    let q = |q| compile_q(Some(q), None, None, None, None);
    assert_eq!(
        q(r#"name = "a(b""#).unwrap(),
        r#"(name = "a(b") and trashed = false"#
    );
    assert!(q(r#"name = "a"#).unwrap_err().contains("unbalanced"));
    assert!(q(r#"name = "it\"s""#).is_ok());
    assert!(
        q(r#"name = "it's""#).is_ok(),
        "a single quote inside double ones"
    );
}
