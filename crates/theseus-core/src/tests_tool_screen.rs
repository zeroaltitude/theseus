//! The tool screen (theseus-a1wvq.9): a tool's result joins an open request's
//! run only when `data::screen` passes it; one that fails ends the run, never
//! the session. The rig is `tests_open_request`'s: `dsflash`, a restricted
//! profile on the `zai` fake, the route pack acting, `[routing.data]
//! open_paths` on a temporary directory, and for a secret a Stripe webhook
//! shape built from parts (the scrubber redacts it and counts it).

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{Access, Plan, Resource};

use crate::data::{self, Unscreened};
use crate::node::Body;
use crate::open_view::Open;
use crate::provider::{FakeProvider, Scripted};
use crate::stub::Kind;
use crate::tests_open_request::{
    blocked, config, dm, jev, open_task, record, say, tainted, text, OPEN_A, OPEN_B,
};
use crate::tests_route::{decided, mode, rig_parts, Rig};
use crate::Core;

/// A shape the scrubber withholds, built from parts at run time.
fn secret() -> String {
    format!("{}{}", "whsec_", "Qm7xK2pLr9TzV4nB8cD1fG6hJ3sW5yAe")
}

struct Shown {
    r: Rig,
    zai: Arc<FakeProvider>,
    claude: Arc<FakeProvider>,
    _open: tempfile::TempDir,
    _notes: tempfile::TempDir,
}

/// The rig with the open paths and the tools' directory on `open`, the `zai`
/// fake and (when given) the default provider scripted, and the driver
/// taking tasks' turns.
fn rig(
    jev: &theseus_judge::fake::FakeJev,
    open: tempfile::TempDir,
    zai: Vec<Scripted>,
    claude: Option<Vec<Scripted>>,
) -> Shown {
    let notes = tempfile::tempdir().unwrap();
    std::fs::write(notes.path().join("notes.md"), "the vault code is 4417\n").unwrap();
    let root = open.path().to_string_lossy().into_owned();
    let (zai, claude_fake) = (
        Arc::new(FakeProvider::scripted(zai)),
        claude.map(|c| Arc::new(FakeProvider::scripted(c))),
    );
    let (z, c) = (zai.clone(), claude_fake.clone());
    let r = rig_parts(
        Some(jev),
        8,
        |cfg| {
            config(cfg, &notes.path().join("notes.md"));
            cfg.routing.data.open_paths = vec![root.clone()];
            cfg.tools.projects_dir = Some(root.clone());
            cfg.tools.cwd = Some(root.clone());
        },
        move |p| {
            p.providers.insert("zai".into(), z);
            if let Some(c) = c {
                let name = p.cfg.model.provider.clone();
                p.providers.insert(name, c);
            }
        },
    );
    tokio::spawn(crate::harness::drive(r.core.clone()));
    r.core.home().unwrap();
    let claude = claude_fake.unwrap_or_else(|| r.claude.clone());
    Shown {
        r,
        zai,
        claude,
        _open: open,
        _notes: notes,
    }
}

fn write(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path.to_string_lossy().into_owned()
}

fn read_of(path: &str) -> Scripted {
    Scripted::tools("Reading.", &[("f1", "fs_read", json!({"path": path}))])
}

/// The tool results of the session, in order: node id, tool, meta.
fn results(core: &Core, sid: &str) -> Vec<(String, String, Value)> {
    core.store
        .transcript(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| n.kind == Kind::ToolResult)
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { tool, meta, .. } => Some((n.id.clone(), tool.clone(), meta.clone())),
            _ => None,
        })
        .collect()
}

fn task_jev() -> theseus_judge::fake::FakeJev {
    let jev = jev(&[OPEN_A]);
    mode(&jev, "chat", 0.2);
    jev
}

async fn run_task(
    s: &Shown,
    brief: &str,
    profile: &str,
) -> (String, crate::session::SessionRecord) {
    let (started, rec) = crate::tests_task_brief::run(&s.r, &open_task(brief, profile)).await;
    (started.session_id, rec)
}

/// T19, the key test. An open task on `dsflash` reads a file under
/// `open_paths` that holds a secret (and, second, runs a program there): the
/// result node stays written, `dsflash` gets the first loop's request alone,
/// the turn goes on on the `[model]` default with `data_gate`, the session
/// records one taint naming the result, and the stored run stops short of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_tool_result_stops_a_cleared_turn() {
    for program in [false, true] {
        let jev = task_jev();
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "private.txt", &format!("token {}\n", secret()));
        let calls = |path: &str| {
            vec![
                match program {
                    false => read_of(path),
                    true => Scripted::tools(
                        "Running.",
                        &[("p1", "proc_run", json!({"argv": ["cat", path]}))],
                    ),
                },
                Scripted::text("Done."),
            ]
        };
        let s = rig(&jev, dir, calls(&file), None);
        let (sid, rec) = run_task(&s, OPEN_A, "dsflash").await;
        let rs = results(&s.r.core, &sid);
        assert_eq!(rs.len(), 1, "{rs:?}");
        let (id, tool, meta) = &rs[0];
        assert_eq!(tool, if program { "proc.run" } else { "fs.read" });
        if !program {
            assert!(meta["redactions"].as_u64().unwrap() > 0, "{meta}");
        }
        assert_eq!(s.zai.requests().len(), 1, "program {program}: loop 0 alone");
        let routed = decided(&s.r.core.store);
        assert_eq!(routed.last().unwrap()["reason"], "data_gate", "{routed:?}");
        let after = text(s.claude.requests().last().unwrap());
        assert!(after.contains("f1") || after.contains("p1"), "{after}");
        let rows = tainted(&s.r.core, &sid);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0]["why"].as_str(), rows[0]["node"].as_str()),
            (Some("tool_result"), Some(id.as_str()))
        );
        assert!(
            rec.data.unwrap().run.is_none(),
            "the run stops short of the result"
        );
        assert_eq!(blocked(&s.r.core), 0, "nothing reached the chokepoint");
    }
}

/// The screen runs only for an open request. The same two calls (the secret
/// read and a program) in a private-view DM turn on a Claude profile and in
/// an open task whose profile is Claude: the screen writes no taint (a row
/// naming a node), moves nothing, and loop 1 is sent as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_screen_runs_only_for_an_open_request() {
    let scripts = |path: &str| {
        vec![
            Scripted::tools(
                "Looking.",
                &[
                    ("f1", "fs_read", json!({"path": path})),
                    ("p1", "proc_run", json!({"argv": ["true"]})),
                ],
            ),
            Scripted::text("Done."),
        ]
    };
    // A private-view turn in the owner's DM.
    let jev = jev(&[]);
    mode(&jev, "chat", 0.95);
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "private.txt", &format!("token {}\n", secret()));
    let s = rig(&jev, dir, vec![], Some(scripts(&file)));
    let sid = dm(&s.r.core);
    let res = say(&s.r.core, &sid, OPEN_B, None).await;
    assert_eq!(s.claude.requests().len(), 2, "{:?}", res.route);
    assert!(decided(&s.r.core.store)
        .iter()
        .all(|d| d["reason"] != "data_gate"));
    assert!(tainted(&s.r.core, &sid).is_empty());
    assert!(record(&s.r.core, &sid).data.is_none());
    assert!(results(&s.r.core, &sid).len() >= 2);
    // An open task on a Claude profile.
    let jev = task_jev();
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "private.txt", &format!("token {}\n", secret()));
    let s = rig(&jev, dir, vec![], Some(scripts(&file)));
    let (sid, _) = run_task(&s, OPEN_A, "sonnet").await;
    assert_eq!(s.claude.requests().len(), 2);
    assert!(decided(&s.r.core.store)
        .iter()
        .all(|d| d["reason"] != "data_gate"));
    assert!(s.zai.requests().is_empty());
    let named: Vec<_> = tainted(&s.r.core, &sid)
        .into_iter()
        .filter(|r| !r["node"].is_null())
        .collect();
    assert!(named.is_empty(), "the screen ran: {named:?}");
}

/// A clean file under `open_paths` passes: loop 1 goes to `dsflash`, carries
/// the result under a pass for its digest, the turn stores a run through its
/// last node, and the task's next turn is admitted on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_screened_result_extends_the_run() {
    let jev = task_jev();
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "clean.txt", "The quay has nine bollards.\n");
    let s = rig(
        &jev,
        dir,
        vec![read_of(&file), Scripted::text("Nine.")],
        None,
    );
    let (sid, rec) = run_task(&s, OPEN_A, "dsflash").await;
    let sent = s.zai.requests();
    assert_eq!(sent.len(), 2, "both loops stayed");
    assert!(
        text(&sent[1]).contains("nine bollards"),
        "the result rode loop 1"
    );
    assert_eq!(blocked(&s.r.core), 0, "each request carried its pass");
    assert!(decided(&s.r.core.store)
        .iter()
        .all(|d| d["reason"] != "data_gate"));
    let d = rec.data.unwrap();
    assert!(d.taint.is_none() && tainted(&s.r.core, &sid).is_empty());
    let nodes = s.r.core.store.transcript(&sid).unwrap();
    let last = nodes.last().unwrap().1.id.clone();
    assert_eq!(
        d.run.as_ref().map(|r| r.through.clone()),
        Some(last.clone())
    );
    let again = data::admits_again(Some(&d), Some(&last));
    assert_eq!(again.map(|r| r.through), Ok(last));
}

/// In the DM an open detour's loop reads a file outside the roots: the call
/// is refused at the gate, and the screen fails it as `outside` all the same.
/// The loop leaves `dsflash`, the private-view session records no taint, and
/// the person's next open message reaches `dsflash` with a run that starts
/// at that message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_screen_in_the_dm_ends_the_run_not_the_session() {
    let jev = jev(&[OPEN_A, OPEN_B]);
    let far = tempfile::tempdir().unwrap();
    let outside = write(far.path(), "far.txt", "far away\n");
    let calls = vec![
        read_of(&outside),
        Scripted::text("Done."),
        Scripted::text("Second."),
    ];
    let s = rig(&jev, tempfile::tempdir().unwrap(), calls, None);
    let sid = dm(&s.r.core);
    let first = say(&s.r.core, &sid, OPEN_A, None).await;
    assert_eq!(s.zai.requests().len(), 1, "{:?}", first.route);
    assert_eq!(first.profile, "default");
    assert_eq!(
        decided(&s.r.core.store).last().unwrap()["reason"],
        "data_gate"
    );
    assert!(
        tainted(&s.r.core, &sid).is_empty(),
        "a private view is never tainted"
    );
    assert!(record(&s.r.core, &sid)
        .data
        .is_none_or(|d| d.taint.is_none()));
    let second = say(&s.r.core, &sid, OPEN_B, None).await;
    assert_eq!(second.profile, "dsflash", "{:?}", second.route);
    let sent = s.zai.requests();
    assert_eq!(sent.len(), 2);
    let body = text(&sent[1]);
    assert!(body.contains(OPEN_B), "{body}");
    for earlier in [OPEN_A, "far away", "f1"] {
        assert!(
            !body.contains(earlier),
            "{earlier:?} rode into the fresh run: {body}"
        );
    }
    assert_eq!(blocked(&s.r.core), 0);
}

fn plan(paths: &[&Path]) -> Plan {
    Plan {
        resources: paths
            .iter()
            .map(|p| Resource {
                path: p.to_path_buf(),
                access: Access::Read,
            })
            .collect(),
        summary: "a call".into(),
        ..Default::default()
    }
}

fn fetch(url: &str) -> Plan {
    Plan {
        url: Some(url.into()),
        summary: "a fetch".into(),
        ..Default::default()
    }
}

/// `screen`, one assertion per word and the passes.
#[test]
fn screen_matrix() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let inside = root.path().join("a.txt");
    let beyond = elsewhere.path().join("b.txt");
    std::fs::write(&inside, "a").unwrap();
    std::fs::write(&beyond, "b").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join("link")).unwrap();
    let open = Open::of(
        vec![theseus_tools::paths::canonical_best_effort(root.path())],
        root.path(),
    );
    let linked = root.path().join("link").join("b.txt");
    let screen = |tool: &str, plan: Option<&Plan>, redactions: u64| {
        data::screen(tool, plan, redactions, None, open)
    };
    assert_eq!(screen("fs.read", Some(&plan(&[&inside])), 0), Ok(()));
    assert_eq!(
        screen("fs.read", Some(&plan(&[&beyond])), 0),
        Err(Unscreened::Outside)
    );
    assert_eq!(
        screen("fs.read", Some(&plan(&[&linked])), 0),
        Err(Unscreened::Outside)
    );
    assert_eq!(
        screen("fs.grep", Some(&plan(&[&inside, &beyond])), 0),
        Err(Unscreened::Outside)
    );
    assert_eq!(screen("fs.read", None, 0), Err(Unscreened::Outside));
    assert_eq!(
        screen("git.log", Some(&plan(&[])), 0),
        Err(Unscreened::Outside)
    );
    assert_eq!(
        screen("http.fetch", Some(&fetch("https://example.com/page")), 0),
        Ok(())
    );
    assert_eq!(
        screen("http.fetch", Some(&fetch("http://127.0.0.1:8080/")), 0),
        Err(Unscreened::Outside)
    );
    assert_eq!(screen("http.fetch", None, 0), Err(Unscreened::Outside));
    assert_eq!(screen("web.search", None, 0), Ok(()));
    for tool in ["proc.run", "job.output", "term.send"] {
        assert_eq!(
            screen(tool, Some(&plan(&[&inside])), 0),
            Err(Unscreened::Program),
            "{tool}"
        );
    }
    for tool in ["task.list", "wake.at", "tools.find"] {
        assert_eq!(screen(tool, None, 0), Err(Unscreened::Tool), "{tool}");
    }
    assert_eq!(
        screen("fs.read", Some(&plan(&[&inside])), 1),
        Err(Unscreened::Secret)
    );
    for (w, s) in [
        (Unscreened::Program, "program"),
        (Unscreened::Tool, "tool"),
        (Unscreened::Secret, "secret"),
        (Unscreened::Outside, "outside"),
    ] {
        assert_eq!(w.as_str(), s);
    }
}

/// theseus-lm38z: a fetch of a public URL that redirects into the owner's
/// network fails the screen on the URL the result ended at
/// (`meta.final_url`), not the one it began at.
#[test]
fn a_fetch_that_redirects_into_the_owners_network_fails_the_screen() {
    let root = tempfile::tempdir().unwrap();
    let open = Open::of(
        vec![theseus_tools::paths::canonical_best_effort(root.path())],
        root.path(),
    );
    let plan = fetch("https://example.com/page");
    let screen = |end: Option<&str>| data::screen("http.fetch", Some(&plan), 0, end, open);
    assert_eq!(screen(None), Ok(()));
    assert_eq!(screen(Some("https://example.com/other")), Ok(()));
    for private in [
        "http://127.0.0.1:8080/admin",
        "http://169.254.169.254/latest/meta-data/",
        "http://192.168.1.10/",
    ] {
        assert_eq!(screen(Some(private)), Err(Unscreened::Outside), "{private}");
    }
}

/// theseus-lm38z: the word a failed screen gives is a stored row, not only a
/// log line: `session.data` `screened`, naming the result's node and the
/// word, readable after the turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_screen_leaves_its_word_in_the_store() {
    let jev = task_jev();
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "private.txt", &format!("token {}\n", secret()));
    let calls = vec![read_of(&file), Scripted::text("Done.")];
    let s = rig(&jev, dir, calls, None);
    let (sid, _) = run_task(&s, OPEN_A, "dsflash").await;
    let (id, _, _) = results(&s.r.core, &sid).remove(0);
    let rows: Vec<Value> = crate::tests_judge::kinds(&s.r.core.store, "session.data")
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(sid.as_str()) && r.data["row"] == "screened")
        .map(|r| r.data)
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["word"].as_str(), rows[0]["node"].as_str()),
        (Some("secret"), Some(id.as_str()))
    );
}
