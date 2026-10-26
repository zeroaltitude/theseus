//! A program's result passes the tool screen on provenance, never on its
//! command line (theseus-ouqru, design §9). A result passes only when the
//! system that ran it recorded a read reach with nothing private in it: an
//! enforced L1 job's own view (`detail.sandbox.view`, one per step of a
//! batch), or a sealed benchmark container ([`crate::seal`]).
//!
//! The command-line forms the first design misread (quoted paths, redirects,
//! an attached option, a symlink, a glob, `$HOME`, a build whose code reads a
//! private file, `..`, a batch's second directory) are here as plans with no
//! provenance, each refused; and with a clean recorded view they pass, since
//! the view's mount namespace holds nothing private for any of them to read.
//! `/tmp/private` below is the private tree, and the open root a directory
//! beside it.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use theseus_protocol::{Access, Plan, Resource};

use crate::data::{self, Proof, Screening, Unscreened};
use crate::open_view::Open;
use crate::seal::Seal;

/// An open root and a private tree beside it, a public view of the first, and
/// what a screen reads it against.
struct Fx {
    open_dir: tempfile::TempDir,
    private_dir: tempfile::TempDir,
    open: &'static Open,
    public_reach: Vec<PathBuf>,
    cover: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let open_dir = tempfile::tempdir().unwrap();
        let private_dir = tempfile::tempdir().unwrap();
        let root = theseus_tools::paths::canonical_best_effort(open_dir.path());
        std::fs::write(private_dir.path().join("notes"), "private line\n").unwrap();
        std::os::unix::fs::symlink(private_dir.path(), root.join("link")).unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        let open = Open::of(vec![root.clone()], &root);
        Self {
            open_dir,
            private_dir,
            open,
            public_reach: data::public_reach(&[]),
            // The daemon's config directory, standing in for `/etc/theseus`.
            cover: theseus_tools::paths::canonical_best_effort(Path::new("/etc/theseus")),
        }
    }

    fn root(&self) -> PathBuf {
        theseus_tools::paths::canonical_best_effort(self.open_dir.path())
    }

    fn private(&self) -> PathBuf {
        theseus_tools::paths::canonical_best_effort(self.private_dir.path())
    }

    /// The view an enforced L1 job over the open root records.
    fn view(&self) -> Value {
        json!({
            "system": theseus_sandbox::SYSTEM,
            "workspace": [self.root()],
            "ro_paths": [],
            "rw": [],
            "cwd": self.root(),
            "home_tmpfs": true,
            "hidden": [self.cover, "/home/u/.theseus-state"],
            "egress": [],
            "granted": [],
        })
    }

    fn sandbox(&self) -> Value {
        json!({"class": "l1", "view": self.view(), "start_us": 800, "setup_us": 4000})
    }

    /// A job's completion detail as a clean L1 run leaves it.
    fn detail(&self) -> Value {
        json!({"exit_code": 0, "sandbox": self.sandbox()})
    }

    /// `cwd`'s plan of one command line, as `proc.run` plans it.
    fn plan(&self, argv: &[&str]) -> Plan {
        Plan {
            resources: vec![Resource {
                path: self.root(),
                access: Access::Exec,
            }],
            argv: Some(argv.iter().map(|s| s.to_string()).collect()),
            summary: "run".into(),
            ..Default::default()
        }
    }

    /// `screen` of the result `r` describes, naming `Acme` as sensitive.
    fn screen(&self, r: Result_<'_>) -> Result<(), Unscreened> {
        let names = theseus_judge::names::Names::new(&["Acme".to_string()]);
        data::screen(&Screening {
            tool: r.tool,
            plan: Some(r.plan),
            redactions: r.redactions,
            final_url: None,
            content: r.content,
            names: &names,
            proof: Proof {
                detail: r.detail,
                steps: r.steps,
                seal: r.seal,
                public_reach: &self.public_reach,
                must_cover: std::slice::from_ref(&self.cover),
            },
            open: self.open,
        })
    }

    /// The common case: a clean content, no steps, an unsealed machine.
    fn run(&self, plan: &Plan, detail: Option<&Value>) -> Result<(), Unscreened> {
        self.screen(Result_ {
            detail,
            ..Result_::of(plan, &unsealed())
        })
    }

    fn batch(&self, steps: &Value, last: &Value) -> Result<(), Unscreened> {
        self.screen(Result_ {
            detail: Some(last),
            steps: Some(steps),
            ..Result_::of(&self.plan(&["true"]), &unsealed())
        })
    }
}

/// One tool result as the screen is handed it.
struct Result_<'a> {
    tool: &'a str,
    plan: &'a Plan,
    detail: Option<&'a Value>,
    steps: Option<&'a Value>,
    seal: &'a Seal,
    content: &'a str,
    redactions: u64,
}

impl<'a> Result_<'a> {
    /// A clean `proc.run` result with no record, on a machine with `seal`.
    fn of(plan: &'a Plan, seal: &'a Seal) -> Self {
        Self {
            tool: "proc.run",
            plan,
            detail: None,
            steps: None,
            seal,
            content: "ok\n",
            redactions: 0,
        }
    }
}

fn unsealed() -> Seal {
    Seal::Unsealed("/ is ext4, not an overlay".into())
}

/// Every command line the first design misread, as plans: none is read.
fn counterexamples(fx: &Fx) -> Vec<(&'static str, Plan)> {
    let private = fx.private().join("notes").display().to_string();
    let quoted = fx.plan(&["bash", "-c", &format!("cat \"{private}\"")]);
    let line = |s: &str| fx.plan(&["bash", "-c", s]);
    // A batch: step 1 `true` in `sub`, step 2 `cat ../private/notes` in the root
    // (one directory above where step 1 ran): the two steps' directories differ.
    let mut batch = fx.plan(&["true"]);
    batch.argv = None;
    batch.steps = Some(vec![
        vec!["true".into()],
        vec!["cat".into(), "../private/notes".into()],
    ]);
    batch.resources = [fx.root().join("sub"), fx.root()]
        .into_iter()
        .map(|path| Resource {
            path,
            access: Access::Exec,
        })
        .collect();
    vec![
        ("a batch whose steps run in different directories", batch),
        ("a quoted absolute path", quoted),
        ("a quoted parameter", line("cat \"$PRIVATE\"")),
        ("a plain symlink operand", fx.plan(&["cat", "link"])),
        ("a glob", line("cat *")),
        (
            "an attached redirect",
            line(&format!("cat<{}", fx.private().join("notes").display())),
        ),
        (
            "an attached short option",
            fx.plan(&[
                "grep",
                &format!("-f{}", fx.private().join("patterns").display()),
                "public.txt",
            ]),
        ),
        (
            "cargo test whose code reads a private config",
            fx.plan(&["cargo", "test"]),
        ),
        ("a symlink then a parent", line("cat ./link/../notes")),
        ("$HOME", line("cat $HOME/private/notes")),
        (
            "a bare parent escape",
            fx.plan(&["cat", "../private/notes"]),
        ),
        ("a command substitution", line("cat $(cat list)")),
        ("an absolute operand", fx.plan(&["cat", &private])),
    ]
}

/// Every counterexample, with no provenance (an L0 job: no sandbox record,
/// no seal): refused. Red against a `provenance` that passes anything.
#[test]
fn a_program_result_without_provenance_refuses() {
    let fx = Fx::new();
    let l0 = json!({"exit_code": 0});
    for (what, plan) in counterexamples(&fx) {
        assert_eq!(fx.run(&plan, None), Err(Unscreened::Program), "{what}");
        assert_eq!(fx.run(&plan, Some(&l0)), Err(Unscreened::Program), "{what}");
    }
}

/// With a clean recorded view the same plans pass: the screen never reads a
/// command, because the namespace holds nothing for it to read past.
#[test]
fn a_clean_recorded_view_screens_every_command_line_alike() {
    let fx = Fx::new();
    let detail = fx.detail();
    for (what, plan) in counterexamples(&fx) {
        assert_eq!(fx.run(&plan, Some(&detail)), Ok(()), "{what}");
    }
}

/// The two positives at the screen: a clean L1 view, and a sealed machine.
#[test]
fn a_clean_l1_view_and_a_sealed_machine_both_pass() {
    let fx = Fx::new();
    let plan = fx.plan(&["cargo", "test"]);
    assert_eq!(fx.run(&plan, Some(&fx.detail())), Ok(()));
    assert_eq!(fx.screen(Result_::of(&plan, &Seal::for_test())), Ok(()));
    assert_eq!(
        data::provenance::via(&Screening {
            tool: "proc.run",
            plan: Some(&plan),
            redactions: 0,
            final_url: None,
            content: "",
            names: &theseus_judge::names::Names::new(&[]),
            proof: Proof {
                detail: Some(&fx.detail()),
                steps: None,
                seal: &unsealed(),
                public_reach: &fx.public_reach,
                must_cover: std::slice::from_ref(&fx.cover),
            },
            open: fx.open,
        }),
        Some(data::provenance::Via::L1)
    );
}

/// The evidence's own edges: each is a recorded view (or the lack of one)
/// that is not a clean L1 view, and each refuses.
#[test]
fn a_recorded_view_that_is_not_clean_refuses() {
    let fx = Fx::new();
    let plan = fx.plan(&["true"]);
    let private = fx.private().display().to_string();
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut d = fx.detail();
        f(&mut d);
        d
    };
    let view = |key: &'static str, v: Value| edit(&move |d| d["sandbox"]["view"][key] = v.clone());
    let mut system: Vec<&str> = theseus_sandbox::SYSTEM.to_vec();
    system.pop();
    let mut more = theseus_sandbox::SYSTEM.to_vec();
    more.push("/opt");
    let cases: Vec<(&str, Value)> = vec![
        ("no detail at all", json!(null)),
        ("an L0 job", json!({"exit_code": 0})),
        (
            "class is not l1",
            edit(&|d| d["sandbox"]["class"] = json!("l0")),
        ),
        (
            "the spawn failed",
            edit(&|d| d["sandbox"]["error"] = json!({"stage": "pivot_root", "error": "x"})),
        ),
        (
            "no view (an older wrapper)",
            edit(&|d| {
                d["sandbox"].as_object_mut().unwrap().remove("view");
            }),
        ),
        (
            "a workspace outside the open roots",
            view("workspace", json!([private])),
        ),
        (
            "a read-only path outside",
            view("ro_paths", json!(["/home/u/.aws"])),
        ),
        ("rw is not empty (outside)", view("rw", json!([private]))),
        (
            "rw is not empty (inside the open root)",
            view("rw", json!([fx.root()])),
        ),
        ("one system entry fewer", view("system", json!(system))),
        ("one system entry more", view("system", json!(more))),
        (
            "the config directory is not covered",
            view("hidden", json!(["/home/u/x"])),
        ),
        ("nothing is covered", view("hidden", json!([]))),
        ("home is not a tmpfs", view("home_tmpfs", json!(false))),
        (
            "home_tmpfs is absent",
            edit(&|d| {
                d["sandbox"]["view"]
                    .as_object_mut()
                    .unwrap()
                    .remove("home_tmpfs");
            }),
        ),
        (
            "egress is not empty",
            view("egress", json!(["example.org:443"])),
        ),
        (
            "a variable was granted",
            view("granted", json!(["GH_TOKEN"])),
        ),
        ("the cwd is outside", view("cwd", json!(private))),
        ("the cwd is relative", view("cwd", json!("sub"))),
        ("a workspace is a string", view("workspace", json!(private))),
        ("a field has the wrong type", view("ro_paths", json!([1]))),
    ];
    for (what, detail) in cases {
        let d = (!detail.is_null()).then_some(&detail);
        assert_eq!(fx.run(&plan, d), Err(Unscreened::Program), "{what}");
    }
}

/// A public path the owner certified (`public_reach`) may be bound read-only;
/// one they did not, or one inside the open root's link to a private tree,
/// may not.
#[test]
fn a_certified_read_only_path_may_be_bound_and_no_other() {
    let mut fx = Fx::new();
    let toolchain = tempfile::tempdir().unwrap();
    let plan = fx.plan(&["true"]);
    let mut d = fx.detail();
    d["sandbox"]["view"]["ro_paths"] = json!([toolchain.path()]);
    assert_eq!(fx.run(&plan, Some(&d)), Err(Unscreened::Program));
    fx.public_reach = data::public_reach(&[toolchain.path().display().to_string()]);
    assert_eq!(fx.run(&plan, Some(&d)), Ok(()));
    // The built-in system directories are public without being listed.
    d["sandbox"]["view"]["ro_paths"] = json!(["/usr/lib"]);
    assert_eq!(fx.run(&plan, Some(&d)), Ok(()));
    // A path that is in the open root by name and outside it by link.
    d["sandbox"]["view"]["workspace"] = json!([fx.root().join("link")]);
    assert_eq!(fx.run(&plan, Some(&d)), Err(Unscreened::Program));
}

/// The evidence is for `proc.run`: a job tool's or a terminal's result
/// carries no completion of its own to read, and stays refused.
#[test]
fn a_job_tools_result_with_a_clean_view_is_still_refused() {
    let fx = Fx::new();
    let plan = fx.plan(&["true"]);
    for tool in ["job.output", "job.wait", "term.send", "term.read"] {
        let r = fx.screen(Result_ {
            tool,
            detail: Some(&fx.detail()),
            ..Result_::of(&plan, &unsealed())
        });
        assert_eq!(r, Err(Unscreened::Program), "{tool}");
    }
}

fn row(step: usize, cwd: &Path, sandbox: Option<Value>) -> Value {
    let mut r = json!({"step": step, "argv": ["true"], "cwd": cwd, "ran": true, "exit_code": 0});
    if let Some(s) = sandbox {
        r["sandbox"] = s;
    }
    r
}

/// A batch is one job a step: each step's own recorded view is read, and no
/// working directory is worked out again (review finding 4).
#[test]
fn a_batch_passes_only_when_every_step_that_ran_holds_a_clean_view() {
    let fx = Fx::new();
    let (root, sub) = (fx.root(), fx.root().join("sub"));
    let clean = fx.sandbox();
    // Step 1 in `sub`, step 2 in the root: each with its own view.
    let mut sub_view = fx.sandbox();
    sub_view["view"]["cwd"] = json!(sub);
    let rows = json!([
        row(1, &sub, Some(sub_view.clone())),
        row(2, &root, Some(clean.clone()))
    ]);
    assert_eq!(fx.batch(&rows, &fx.detail()), Ok(()));
    // Step 2 ran and carries no record.
    let rows = json!([row(1, &sub, Some(sub_view.clone())), row(2, &root, None)]);
    assert_eq!(fx.batch(&rows, &fx.detail()), Err(Unscreened::Program));
    // Step 2's own view names a path outside the open roots, while step 1's is
    // clean and the last step's detail is clean: the review's finding 4.
    let mut outside = fx.sandbox();
    outside["view"]["workspace"] = json!([fx.private()]);
    let rows = json!([
        row(1, &sub, Some(sub_view.clone())),
        row(2, &root, Some(outside.clone())),
        row(3, &root, Some(clean.clone())),
    ]);
    assert_eq!(fx.batch(&rows, &fx.detail()), Err(Unscreened::Program));
    // The same with step 2 the last: the job's own detail is the outside one.
    let rows = json!([
        row(1, &sub, Some(sub_view.clone())),
        row(2, &root, Some(outside.clone()))
    ]);
    let mut last = fx.detail();
    last["sandbox"] = outside;
    assert_eq!(fx.batch(&rows, &last), Err(Unscreened::Program));
    // A clean row set with a last detail that has no record.
    let rows = json!([row(1, &sub, Some(sub_view)), row(2, &root, Some(clean))]);
    assert_eq!(
        fx.batch(&rows, &json!({"exit_code": 0})),
        Err(Unscreened::Program)
    );
    // Rows that are not a list.
    assert_eq!(
        fx.batch(&json!({"step": 1}), &fx.detail()),
        Err(Unscreened::Program)
    );
}

/// A step that did not run read nothing: its row needs no record. One that
/// does not say whether it ran is taken as one that did.
#[test]
fn a_step_that_did_not_run_needs_no_record_and_an_unsaid_one_does() {
    let fx = Fx::new();
    let root = fx.root();
    let mut skipped = json!({"step": 2, "argv": ["true"], "cwd": root, "ran": false});
    let rows = json!([row(1, &root, Some(fx.sandbox())), skipped.clone()]);
    assert_eq!(fx.batch(&rows, &fx.detail()), Ok(()));
    skipped.as_object_mut().unwrap().remove("ran");
    let rows = json!([row(1, &root, Some(fx.sandbox())), skipped]);
    assert_eq!(fx.batch(&rows, &fx.detail()), Err(Unscreened::Program));
}

/// The content layer stands in front of provenance, and good provenance never
/// excuses it: a withheld secret, and a sensitive name in any case.
#[test]
fn the_content_layer_still_refuses_with_good_provenance() {
    let fx = Fx::new();
    let plan = fx.plan(&["true"]);
    let d = fx.detail();
    let l1 = |content: &str, redactions: u64| {
        fx.screen(Result_ {
            detail: Some(&d),
            content,
            redactions,
            ..Result_::of(&plan, &unsealed())
        })
    };
    let sealed = |content: &str, redactions: u64| {
        fx.screen(Result_ {
            content,
            redactions,
            ..Result_::of(&plan, &Seal::for_test())
        })
    };
    for (what, got) in [
        ("secret, L1", l1("ok", 1)),
        ("secret, sealed", sealed("ok", 1)),
    ] {
        assert_eq!(got, Err(Unscreened::Secret), "{what}");
    }
    for text in ["the Acme ledger", "ACME-internal notes", "an acme_report"] {
        assert_eq!(l1(text, 0), Err(Unscreened::Name), "L1: {text}");
        assert_eq!(sealed(text, 0), Err(Unscreened::Name), "sealed: {text}");
    }
    // Both pass when clean.
    assert_eq!(l1("a clean line", 0), Ok(()));
    assert_eq!(sealed("a clean line", 0), Ok(()));
}

/// A seal is the machine's, so it vouches for every program tool, and for
/// nothing else: the other tools' rules are unchanged on a sealed machine.
#[test]
fn a_sealed_machine_vouches_for_programs_and_changes_nothing_else() {
    let fx = Fx::new();
    let plan = fx.plan(&["true"]);
    for tool in ["proc.run", "job.output", "term.send"] {
        let r = fx.screen(Result_ {
            tool,
            ..Result_::of(&plan, &Seal::for_test())
        });
        assert_eq!(r, Ok(()), "{tool}");
    }
    let far = fx.plan(&[]);
    let mut read = far.clone();
    read.resources = vec![Resource {
        path: fx.private().join("notes"),
        access: Access::Read,
    }];
    assert_eq!(
        fx.screen(Result_ {
            tool: "fs.read",
            ..Result_::of(&read, &Seal::for_test())
        }),
        Err(Unscreened::Outside)
    );
    assert_eq!(
        fx.screen(Result_ {
            tool: "task.list",
            ..Result_::of(&far, &Seal::for_test())
        }),
        Err(Unscreened::Tool)
    );
    // An unsealed machine with no record refuses the same programs.
    assert_eq!(
        fx.screen(Result_::of(&plan, &unsealed())),
        Err(Unscreened::Program)
    );
}

/// The word's text, as the ledger writes it.
#[test]
fn the_words_are_stable() {
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

/// What the daemon's own config path covers: each directory a view must hide,
/// read from the process, and each absolute.
#[test]
fn the_configs_covers_are_the_daemons_own_directories() {
    let covers = crate::sandbox::config_covers();
    assert!(covers.iter().all(|p| p.is_absolute()), "{covers:?}");
    assert!(
        covers.contains(&PathBuf::from("/etc/theseus")),
        "{covers:?}"
    );
    assert!(
        covers.iter().any(|p| p.ends_with(".theseus")),
        "the state directory under HOME: {covers:?}"
    );
}
