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
    rig_tools(jev, open, zai, claude, vec![])
}

/// [`rig`], with stand-in tools registered beside the built-in ones.
fn rig_tools(
    jev: &theseus_judge::fake::FakeJev,
    open: tempfile::TempDir,
    zai: Vec<Scripted>,
    claude: Option<Vec<Scripted>>,
    tools: Vec<Arc<dyn theseus_tools::Tool>>,
) -> Shown {
    rig_named(jev, open, zai, claude, tools, &[])
}

/// [`rig_tools`], with `[privacy] sensitive_names` (theseus-ouqru).
fn rig_named(
    jev: &theseus_judge::fake::FakeJev,
    open: tempfile::TempDir,
    zai: Vec<Scripted>,
    claude: Option<Vec<Scripted>>,
    tools: Vec<Arc<dyn theseus_tools::Tool>>,
    names: &[&str],
) -> Shown {
    let notes = tempfile::tempdir().unwrap();
    std::fs::write(notes.path().join("notes.md"), "the vault code is 4417\n").unwrap();
    let root = open.path().to_string_lossy().into_owned();
    let sensitive: Vec<String> = names.iter().map(|n| (*n).to_string()).collect();
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
            cfg.privacy.sensitive_names = sensitive;
        },
        move |p| {
            p.toollets = tools;
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

/// No sensitive name configured, for the screens that do not test one.
fn unnamed() -> theseus_judge::names::Names {
    theseus_judge::names::Names::default()
}

/// `data::screen` over one result, with no name configured and no content.
fn screen_of(
    tool: &str,
    plan: Option<&Plan>,
    redactions: u64,
    open: &'static Open,
) -> Result<(), Unscreened> {
    data::screen(&data::Screening {
        tool,
        plan,
        redactions,
        final_url: None,
        content: "",
        names: &unnamed(),
        open,
    })
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
    let screen =
        |tool: &str, plan: Option<&Plan>, redactions: u64| screen_of(tool, plan, redactions, open);
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
    // theseus-ouqru: a program is no longer refused by its kind. It passes
    // when its own call ran under the open roots and named nothing outside
    // them, and a job tool or a terminal, with no command to read, does not.
    assert_eq!(
        screen("proc.run", Some(&run(&inside, &["cat", "a.txt"])), 0),
        Ok(())
    );
    for tool in ["job.output", "term.send"] {
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
        (Unscreened::Name, "name"),
        (Unscreened::Outside, "outside"),
    ] {
        assert_eq!(w.as_str(), s);
    }
}

/// A `proc.run` plan as the tool builds one: the working directory its only
/// resource, `Access::Exec`, with the argv it runs.
fn run(cwd: &Path, argv: &[&str]) -> Plan {
    Plan {
        resources: vec![Resource {
            path: cwd.to_path_buf(),
            access: Access::Exec,
        }],
        argv: Some(argv.iter().map(|a| (*a).to_string()).collect()),
        summary: "a run".into(),
        ..Default::default()
    }
}

/// theseus-ouqru, the reach check unit by unit: what a program call may name
/// and still screen, and what refuses. A command that names only paths under
/// the open roots passes; one that names a path outside them is `outside`;
/// one whose path this screen cannot resolve, and one with no command to
/// read, are `program`. The shell form (`bash -c <line>`) is read word by
/// word, so a path in the line is judged as an argv's would be.
#[test]
fn a_programs_reach_is_read_from_its_command() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let cwd = root.path();
    let beyond = elsewhere.path().join("notes.txt");
    std::fs::write(&beyond, "the owner's notes\n").unwrap();
    std::fs::create_dir_all(cwd.join("src")).unwrap();
    let open = Open::of(vec![theseus_tools::paths::canonical_best_effort(cwd)], cwd);
    let far = beyond.to_string_lossy().into_owned();
    let screen = |argv: &[&str]| screen_of("proc.run", Some(&run(cwd, argv)), 0, open);
    let line = |l: &str| screen_of("proc.run", Some(&run(cwd, &["bash", "-c", l])), 0, open);
    // Open work: no path at all, a relative path under the roots, a redirect.
    for argv in [
        vec!["cargo", "test", "-p", "theseus-core"],
        vec!["git", "status", "--short"],
        vec!["ls", "src/"],
        vec!["cat", "./src/../src"],
    ] {
        assert_eq!(screen(&argv), Ok(()), "{argv:?}");
    }
    for l in [
        "cargo build 2>&1 | tail -20",
        "cd src && cargo test",
        "awk '{print $1}' src/x",
    ] {
        assert_eq!(line(l), Ok(()), "{l}");
    }
    // A path outside the open roots, named either way, and `..` out of them.
    assert_eq!(screen(&["cat", &far]), Err(Unscreened::Outside));
    assert_eq!(line(&format!("cat {far}")), Err(Unscreened::Outside));
    assert_eq!(screen(&["ls", "/etc"]), Err(Unscreened::Outside));
    assert_eq!(
        line("cat ../../../etc/passwd"),
        Err(Unscreened::Outside),
        "a relative escape resolves outside"
    );
    // A path this screen cannot resolve is refused, not guessed at.
    for l in [
        "cat $HOME/private/notes",
        "cat ${HOME}/private/notes",
        "cat $(cat list)/x",
        "cat `cat list`/x",
    ] {
        assert_eq!(line(l), Err(Unscreened::Program), "{l}");
    }
    assert_eq!(screen(&["cat", "$PRIVATE"]), Err(Unscreened::Program));
    // A working directory outside the open roots, and a call with no command.
    let out = run(elsewhere.path(), &["ls"]);
    assert_eq!(
        screen_of("proc.run", Some(&out), 0, open),
        Err(Unscreened::Outside)
    );
    // A command with no working directory of its own: nothing bounds where it
    // ran, so it is outside, not unreadable.
    let no_dir = Plan {
        argv: Some(vec!["ls".into()]),
        ..plan(&[cwd])
    };
    assert_eq!(
        screen_of("proc.run", Some(&no_dir), 0, open),
        Err(Unscreened::Outside),
        "no working directory planned"
    );
    // A call with no command at all, and no plan at all: unreadable.
    assert_eq!(
        screen_of("proc.run", Some(&plan(&[cwd])), 0, open),
        Err(Unscreened::Program),
        "no command to read"
    );
    assert_eq!(
        screen_of("proc.run", None, 0, open),
        Err(Unscreened::Program)
    );
    // Every step of a batch is read, and one naming an outside path refuses.
    let steps = |ss: &[&[&str]]| Plan {
        resources: vec![Resource {
            path: cwd.to_path_buf(),
            access: Access::Exec,
        }],
        steps: Some(
            ss.iter()
                .map(|s| s.iter().map(|a| (*a).to_string()).collect())
                .collect(),
        ),
        summary: "a batch".into(),
        ..Default::default()
    };
    assert_eq!(
        screen_of(
            "proc.run",
            Some(&steps(&[&["cargo", "fmt"], &["ls", "src/"]])),
            0,
            open
        ),
        Ok(())
    );
    assert_eq!(
        screen_of(
            "proc.run",
            Some(&steps(&[&["cargo", "fmt"], &["cat", &far]])),
            0,
            open
        ),
        Err(Unscreened::Outside),
        "a later step's reach counts"
    );
}

/// theseus-ouqru: a sensitive name in a result's content fails the screen,
/// whatever tool returned it, and the content screened is the one the model
/// would read. Matching is `[privacy]`'s: case-insensitive, whole words.
#[test]
fn a_sensitive_name_in_a_results_content_fails_the_screen() {
    let root = tempfile::tempdir().unwrap();
    let inside = root.path().join("a.txt");
    std::fs::write(&inside, "a").unwrap();
    let open = Open::of(
        vec![theseus_tools::paths::canonical_best_effort(root.path())],
        root.path(),
    );
    let names = theseus_judge::names::Names::new(&["Examplecorp".to_string(), "Acme".to_string()]);
    let screen = |tool: &str, plan: Option<&Plan>, content: &str| {
        data::screen(&data::Screening {
            tool,
            plan,
            redactions: 0,
            final_url: None,
            content,
            names: &names,
            open,
        })
    };
    let program = run(root.path(), &["git", "log", "--oneline", "-1"]);
    assert_eq!(
        screen("proc.run", Some(&program), "9fa1 fix the quay\n"),
        Ok(())
    );
    for held in [
        "9fa1 Examplecorp: fix the quay\n",
        "9fa1 acme loads the frame\n",
        "a line\nand ACME on the next\n",
    ] {
        assert_eq!(
            screen("proc.run", Some(&program), held),
            Err(Unscreened::Name),
            "{held:?}"
        );
    }
    // Every listed tool alike, not programs only.
    assert_eq!(
        screen("fs.read", Some(&plan(&[&inside])), "notes on acme"),
        Err(Unscreened::Name)
    );
    assert_eq!(
        screen("web.search", None, "a page about Examplecorp"),
        Err(Unscreened::Name)
    );
    // A name the list does not hold, and a word that only contains one.
    assert_eq!(screen("web.search", None, "a page about hats"), Ok(()));
    assert_eq!(screen("web.search", None, "examplecorpinc ships"), Ok(()));
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
    let screen = |end: Option<&str>| {
        data::screen(&data::Screening {
            tool: "http.fetch",
            plan: Some(&plan),
            redactions: 0,
            final_url: end,
            content: "",
            names: &unnamed(),
            open,
        })
    };
    assert_eq!(screen(None), Ok(()));
    assert_eq!(screen(Some("https://example.com/other")), Ok(()));
    for private in [
        "http://127.0.0.1:8080/admin",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.10/",
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

/// A stand-in for `http.fetch` that needs no network: it plans a public URL
/// and answers a result whose `meta.final_url` is where a redirect ended.
struct Redirected;

impl theseus_tools::Tool for Redirected {
    fn name(&self) -> &str {
        "http.fetch"
    }
    fn description(&self) -> &str {
        "a fetch that ends elsewhere"
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]})
    }
    fn class(&self) -> theseus_tools::ToolClass {
        theseus_tools::ToolClass::Read
    }
    fn backend(&self) -> theseus_tools::Backend {
        theseus_tools::Backend::Async
    }
    fn retry(&self) -> theseus_tools::Retry {
        theseus_tools::Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &theseus_tools::ToolCtx) -> Result<Plan, String> {
        Ok(fetch(input["url"].as_str().unwrap_or_default()))
    }
    fn run_async(&self, input: &Value, _ctx: &theseus_tools::ToolCtx) -> theseus_tools::AsyncRun {
        let url = input["url"].clone();
        Box::pin(std::future::ready(Ok((
            theseus_tools::ToolOutput {
                text: "A page of tide tables.".into(),
                meta: json!({"url": url, "final_url": "http://10.0.0.10/tides", "status": 200}),
            },
            None,
        ))))
    }
}

/// theseus-atbgl: the turn passes the result's `meta.final_url` to the
/// screen, not only `data::screen` taking it. A fetch of a public URL that a
/// redirect ended at a private address fails the screen as `outside`, taints
/// the open task, and is not screened open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_fetch_that_redirected_into_the_owners_network_is_not_screened_open() {
    let jev = task_jev();
    let calls = vec![
        Scripted::tools(
            "Fetching.",
            &[(
                "f1",
                "http_fetch",
                json!({"url": "https://example.com/tides"}),
            )],
        ),
        Scripted::text("Done."),
    ];
    let s = rig_tools(
        &jev,
        tempfile::tempdir().unwrap(),
        calls,
        None,
        vec![Arc::new(Redirected)],
    );
    let (sid, rec) = run_task(&s, OPEN_A, "dsflash").await;
    let (id, tool, meta) = results(&s.r.core, &sid).remove(0);
    assert_eq!(tool, "http.fetch");
    assert_eq!(meta["final_url"], "http://10.0.0.10/tides", "{meta}");
    let rows: Vec<Value> = crate::tests_judge::kinds(&s.r.core.store, "session.data")
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(sid.as_str()) && r.data["row"] == "screened")
        .map(|r| r.data)
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["word"].as_str(), rows[0]["node"].as_str()),
        (Some("outside"), Some(id.as_str()))
    );
    let taint = rec.data.unwrap().taint.expect("the task is tainted");
    assert_eq!(taint.node.as_deref(), Some(id.as_str()));
}

// ------------------------------------------------------ theseus-ouqru, live in a turn

/// The `session.data` `screened` rows of `sid`: the word and the node of each
/// result that failed the screen.
fn screened(core: &Core, sid: &str) -> Vec<(String, String)> {
    crate::tests_judge::kinds(&core.store, "session.data")
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(sid) && r.data["row"] == "screened")
        .map(|r| {
            (
                r.data["word"].as_str().unwrap_or_default().to_string(),
                r.data["node"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// A `proc_run` call of one line in bash, as the model writes one.
fn command_of(line: &str) -> Scripted {
    Scripted::tools("Running.", &[("p1", "proc_run", json!({"command": line}))])
}

/// **theseus-ouqru, the key test, red before the fix.** An open task on
/// `dsflash` runs a command under `open_paths` whose output holds nothing
/// private: the result passes the screen, so loop 1 goes to `dsflash` too,
/// carries the command's output under a pass for its own digest, nothing
/// reaches the chokepoint, no taint is written, and the stored run reaches
/// the turn's last node. Before the fix the result was refused by its kind
/// (`Unscreened::Program`) and loop 1 left `dsflash` with `data_gate`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_program_result_extends_the_run() {
    let jev = task_jev();
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "quay.txt", "The quay has nine bollards.\n");
    let s = rig(
        &jev,
        dir,
        vec![
            Scripted::tools(
                "Running.",
                &[("p1", "proc_run", json!({"argv": ["cat", "quay.txt"]}))],
            ),
            Scripted::text("Nine."),
        ],
        None,
    );
    let (sid, rec) = run_task(&s, OPEN_A, "dsflash").await;
    let rs = results(&s.r.core, &sid);
    assert_eq!(rs.len(), 1, "{rs:?}");
    assert_eq!(rs[0].1, "proc.run");
    let sent = s.zai.requests();
    assert_eq!(sent.len(), 2, "both loops stayed on dsflash");
    assert!(
        text(&sent[1]).contains("nine bollards"),
        "the command's output rode loop 1: {}",
        text(&sent[1])
    );
    assert_eq!(blocked(&s.r.core), 0, "each request carried its pass");
    assert!(decided(&s.r.core.store)
        .iter()
        .all(|d| d["reason"] != "data_gate"));
    assert!(screened(&s.r.core, &sid).is_empty(), "nothing failed");
    let d = rec.data.unwrap();
    assert!(d.taint.is_none() && tainted(&s.r.core, &sid).is_empty());
    let nodes = s.r.core.store.transcript(&sid).unwrap();
    let last = nodes.last().unwrap().1.id.clone();
    assert_eq!(d.run.as_ref().map(|r| r.through.clone()), Some(last));
}

/// theseus-ouqru: the mid-trial failure, as a test. A loop of commands —
/// the shape of a bench trial, which is agentic software work and so runs
/// them — stays on the restricted model the whole way: three loops, three
/// requests to `dsflash`, each command's output riding the next, no refusal
/// at the chokepoint and no move to the `[model]` default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_loop_of_commands_stays_on_a_restricted_model() {
    let jev = task_jev();
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "first.txt", "nine bollards\n");
    write(dir.path(), "second.txt", "four lamps\n");
    let s = rig(
        &jev,
        dir,
        vec![
            command_of("cat first.txt"),
            Scripted::tools(
                "And again.",
                &[("p2", "proc_run", json!({"argv": ["cat", "second.txt"]}))],
            ),
            Scripted::text("Nine and four."),
        ],
        None,
    );
    let (sid, rec) = run_task(&s, OPEN_A, "dsflash").await;
    let sent = s.zai.requests();
    assert_eq!(
        sent.len(),
        3,
        "every loop stayed: {:?}",
        screened(&s.r.core, &sid)
    );
    assert!(
        text(&sent[1]).contains("nine bollards"),
        "{}",
        text(&sent[1])
    );
    let third = text(&sent[2]);
    for held in ["nine bollards", "four lamps"] {
        assert!(
            third.contains(held),
            "{held:?} missing from loop 2: {third}"
        );
    }
    assert_eq!(blocked(&s.r.core), 0);
    assert!(decided(&s.r.core.store)
        .iter()
        .all(|d| d["reason"] != "data_gate"));
    assert!(screened(&s.r.core, &sid).is_empty());
    let d = rec.data.unwrap();
    assert!(d.taint.is_none());
    let nodes = s.r.core.store.transcript(&sid).unwrap();
    assert_eq!(
        d.run.map(|r| r.through),
        Some(nodes.last().unwrap().1.id.clone())
    );
}

/// theseus-ouqru, the keel half: the content screen still refuses a program
/// result that holds something private. Three commands, each under the open
/// roots and each judged by what it holds or what it named: a secret the
/// scrubber withheld (`secret`), a `[privacy] sensitive_names` name in the
/// output (`name`), and the content of a read outside the open roots —
/// a private read, which the gate does not hold and only this screen catches
/// (`outside`). In every case loop 1 leaves `dsflash` with `data_gate`,
/// nothing reaches the chokepoint, and the task is tainted at that node.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_private_program_result_still_refuses_the_clearance() {
    let name = "acme";
    for (what, word) in [
        ("secret", "secret"),
        ("name", "name"),
        ("read", "outside"),
        ("expansion", "program"),
    ] {
        let jev = task_jev();
        let dir = tempfile::tempdir().unwrap();
        let far = tempfile::tempdir().unwrap();
        let private = write(far.path(), "notes.md", "the hedge is nine feet high\n");
        write(dir.path(), "held.txt", &format!("token {}\n", secret()));
        write(
            dir.path(),
            "named.txt",
            &format!("{name} loads the frame\n"),
        );
        let script = match what {
            "secret" => command_of("cat held.txt"),
            "name" => command_of("cat named.txt"),
            "read" => command_of(&format!("cat {private}")),
            _ => command_of("cat $HOME/notes.md"),
        };
        let s = rig_named(
            &jev,
            dir,
            vec![script, Scripted::text("Done.")],
            None,
            vec![],
            &["Examplecorp", name],
        );
        let (sid, rec) = run_task(&s, OPEN_A, "dsflash").await;
        let rs = results(&s.r.core, &sid);
        assert_eq!(rs.len(), 1, "{what}: {rs:?}");
        let (id, tool, _) = &rs[0];
        assert_eq!(tool, "proc.run", "{what}");
        assert_eq!(
            screened(&s.r.core, &sid),
            vec![(word.to_string(), id.clone())],
            "{what}"
        );
        assert_eq!(s.zai.requests().len(), 1, "{what}: loop 0 alone");
        let routed = decided(&s.r.core.store);
        assert_eq!(routed.last().unwrap()["reason"], "data_gate", "{what}");
        assert_eq!(blocked(&s.r.core), 0, "{what}: nothing was sent");
        let d = rec.data.unwrap();
        assert_eq!(
            d.taint.as_ref().and_then(|t| t.node.clone()),
            Some(id.clone()),
            "{what}"
        );
        assert!(d.run.is_none(), "{what}: the run stops short of the result");
        // The private line itself never reached the restricted model.
        let held = match what {
            "secret" => secret(),
            "name" => name.to_string(),
            _ => "nine feet high".to_string(),
        };
        let sent = text(&s.zai.requests()[0]);
        assert!(!sent.contains(&held), "{what}: {sent}");
    }
}
