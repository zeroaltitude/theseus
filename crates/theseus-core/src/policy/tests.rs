//! The tool gate's own tests: the decision for each posture, the floor's
//! match, the code-cluster wrapper class, the grants, and the FAST budget of
//! a millisecond a call.
//!
//! Moved out of `policy.rs` by theseus-st2wz. That file held 1,196 lines of
//! tests and sat 33 lines under its 2,550-line ceiling in
//! `scripts/long-files.txt`; its own ceiling note named this move as the
//! right fix, under the owner's answer 6 of 2026-11-10, "move the tests
//! please". The module path is unchanged (`crate::policy::tests`).

use super::*;
use serde_json::{json, Value};
use theseus_tools::{Resource, Retry, ToolClass, ToolCtx};

/// A command's arguments reach it unexpanded (no shell), so the gate
/// reads a leading `~` as a shell would have, as `shellexpand::tilde`
/// did, and leaves `$HOME` as written: the relative path the program
/// opens (review 2's consideration 5).
#[test]
fn a_path_argument_reads_its_tilde_and_not_its_variables() {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let cwd = Path::new("/invented/work");
    let args = path_args(&["cat".into(), "~/x.txt".into(), "$HOME/y.txt".into()], cwd);
    let at: Vec<&Path> = args.iter().map(|(_, p)| p.as_path()).collect();
    assert_eq!(
        at,
        [
            paths::canonical_best_effort(&Path::new(&home).join("x.txt")).as_path(),
            paths::canonical_best_effort(&cwd.join("$HOME/y.txt")).as_path(),
        ]
    );
}

/// A line is read because a CODE FLAG introduces it, whatever program is
/// in front of the flag and however the flag is spelled. The rule used to
/// ask whether the program was one of five shell names and whether a word
/// was exactly `-c`, and so held `bash -c` while missing `bash -lc`;
/// these are the spellings that one character apart used to decide.
#[test]
fn a_line_is_read_because_its_flag_says_it_is_code() {
    let argv = |w: &[&str]| -> Vec<String> { w.iter().map(|s| s.to_string()).collect() };
    for spelling in [
        vec!["bash", "-c", "cat /x"],
        vec!["bash", "-lc", "cat /x"],
        vec!["bash", "-ec", "cat /x"],
        vec!["bash", "-ic", "cat /x"],
        vec!["sh", "-lc", "cat /x"],
        vec!["/bin/sh", "-c", "cat /x"],
        vec!["zsh", "-l", "-c", "cat /x"],
        // `-e` takes the NEXT word, and `-c`'s line is still the line:
        // a flag is not an argument.
        vec!["bash", "-e", "-c", "cat /x"],
        vec!["bash", "--command", "cat /x"],
        // A wrapper in front of the interpreter, which put the shell at
        // `argv[1]` or later and so was unheld at all eleven spellings.
        vec!["env", "bash", "-c", "cat /x"],
        vec!["timeout", "5", "bash", "-c", "cat /x"],
        vec!["nice", "bash", "-c", "cat /x"],
        vec!["/usr/bin/env", "sh", "-c", "cat /x"],
        // An interpreter that is not a shell at all, and one this
        // project has never named.
        vec!["python3", "-c", "print(open('/x').read())"],
        vec!["theseus-unknown-interpreter", "-c", "cat /x"],
    ] {
        let words = argv(&spelling);
        let got = code_lines(&words);
        assert!(
            got.contains(&spelling[spelling.len() - 1]),
            "{spelling:?} carries a line and it was not read: {got:?}"
        );
    }
    // `--command=<line>` carries its own line.
    assert_eq!(
        code_lines(&argv(&["bash", "--command=cat /x"])),
        ["cat /x"],
        "a long flag's own argument"
    );
    // No code flag: the argument is a script to run, and `path_args`
    // already judges it as the path it is.
    assert_eq!(code_lines(&argv(&["bash", "/x/run.sh"])), [] as [&str; 0]);
    assert_eq!(code_lines(&argv(&["bash", "-c"])), [] as [&str; 0]);
    assert_eq!(code_lines(&[]), [] as [&str; 0]);
    // A long flag means what it says and nothing by its letters, so the
    // message of a commit is not code. This is the must-not-hold the
    // whole change is weighed against.
    assert_eq!(
        code_lines(&argv(&["git", "commit", "--message=floor /x properly"])),
        [] as [&str; 0]
    );
    assert_eq!(
        code_lines(&argv(&["git", "commit", "-m", "floor /x properly"])),
        [] as [&str; 0]
    );
    // A program whose `-c` means something else is read anyway, and that
    // costs nothing: the word is split and every piece is judged by the
    // same `path_word` an `argv` word takes, so `<archive>` is the path
    // it already was and no new notion of a path exists.
    assert_eq!(
        code_lines(&argv(&["tar", "-czf", "/x/a.tgz", "/x/src"])),
        ["/x/a.tgz"]
    );
    // The code ATTACHED to its own cluster, one SPACE from a spelling
    // already in the key and unheld until re-cut 2 (`code_attached`).
    // `getopt` gives an option its argument from the rest of the cluster,
    // so there is no next word for the rule above to find.
    for (spelling, line) in [
        (vec!["python3", "-copen('/x','a')"], "open('/x','a')"),
        (vec!["perl", "-eprint '/x'"], "print '/x'"),
        (vec!["bash", "-lcecho /x"], "echo /x"),
        (vec!["env", "-Sbash -c 'cat /x'"], "bash -c 'cat /x'"),
        (vec!["node", "--eval=cat('/x')"], "cat('/x')"),
    ] {
        assert_eq!(code_lines(&argv(&spelling)), [line], "{spelling:?}");
    }
    // A remainder that is only more flag letters is more flags, as
    // `getopt` reads it when they take no argument: `-ec` sets errexit
    // and the line is still `-c`'s next word, and `tar -czf` names an
    // archive rather than running `zf`.
    assert_eq!(code_lines(&argv(&["bash", "-ec", "cat /x"])), ["cat /x"]);
    assert_eq!(code_lines(&argv(&["head", "-c100"])), [] as [&str; 0]);
    assert_eq!(code_lines(&argv(&["ps", "-ef"])), [] as [&str; 0]);
}

/// A LINE gives its programs to the floor; a single WORD a code flag
/// happened to introduce does not. `grep -c theseusd <log>` makes
/// `theseusd` the argument of a code flag, and reading that one word as a
/// command held an ordinary health check — four of the round-2 reviewer's
/// seven false positives, none of them held by main.
///
/// The cost is the one-word spelling `bash -c op`, the cheap side of the
/// trade **as a line**: a one-word line runs its program with no arguments
/// of its own. The sentence this comment used to carry — that such a
/// program can therefore ask nothing of the vault — was FALSE, and
/// `find <dir> -exec op item get {} \;` is its counter-example: the
/// one-word line is `op` and its arguments are find's own argv words. That
/// case is answered by the floor's MATCH (`introduces_a_program`) and
/// asserted below, so the line rule carries no claim about programs it
/// cannot see. The moment a line has a second word — every line that does
/// anything — its programs are read again, which is asserted here too.
#[test]
fn a_one_word_line_is_a_word_not_a_program() {
    let got = |l: &str| shell_argvs(l);
    let none: Vec<Vec<String>> = vec![];
    assert_eq!(got("theseusd"), none, "a grep pattern is not a program");
    // `-exec`'s one-word line yields no program, and the floor holds the
    // call anyway because `op` stands in a program position of the argv.
    assert_eq!(got("op"), none, "find -exec's line is still one word");
    let words = |w: &[&str]| -> Vec<String> { w.iter().map(|s| s.to_string()).collect() };
    assert!(
        runs_program(
            &words(&["find", "/x", "-exec", "op", "item", "get", "y", ";"]),
            &words(&["op"])
        ),
        "a program an `exec` flag introduces is held by the match"
    );
    assert!(
        !runs_program(
            &words(&["grep", "-c", "theseusd", "/x/depart.log"]),
            &words(&["theseusd"])
        ),
        "a pattern a CODE flag introduces is not a program"
    );
    // A substitution expression is one word too, because a chain
    // character chains nothing in text with no whitespace in it.
    assert_eq!(got("s|/x/y.toml|/tmp/other.toml|"), none);
    // The half that matters: a line that runs something still gives up
    // every program it runs, at any position in a chain.
    assert_eq!(
        got("op item get x --reveal"),
        [["op", "item", "get", "x", "--reveal"]]
    );
    assert_eq!(
        got("echo x | op item get y"),
        [
            vec!["echo".to_string(), "x".into()],
            vec!["op".into(), "item".into(), "get".into(), "y".into()],
        ]
    );
    // And the leading assignments a shell sets for one command are still
    // dropped, so `op` is the program of `OP_ACCOUNT=x op item get`.
    assert_eq!(
        got("OP_ACCOUNT=x op item get y"),
        [["op", "item", "get", "y"]]
    );
}

/// The words a line is read as. It splits as a shell does and expands
/// nothing, which is the whole of where the floor's reach now sits: a
/// path the line OPERATES on is a word of its own and is seen; a path
/// inside a quoted string is part of one word and is not; a path a
/// variable holds is unknowable without running the line.
#[test]
fn a_line_is_read_as_the_words_a_shell_would_run_it_as() {
    let words = |l: &str| shell_words(l);
    assert_eq!(
        words("echo pwned >> /x/AGENTS.md"),
        ["echo", "pwned", "/x/AGENTS.md"]
    );
    assert_eq!(
        words("cat /a | tee /b; rm /c"),
        ["cat", "/a", "tee", "/b", "rm", "/c"]
    );
    assert_eq!(
        words("sed -i s/a/b/ \"/x/y.toml\""),
        ["sed", "-i", "s/a/b/", "/x/y.toml"]
    );
    // A substitution's own text is words too.
    assert_eq!(
        words("D=$(cat /x/y) echo $D"),
        ["D=", "cat", "/x/y", "echo", "$D"]
    );
    // One quoted word, NOT the path inside it: a message that names a
    // file is not an access to it.
    assert_eq!(
        words("git commit -m \"fix the /x/y.toml read\""),
        ["git", "commit", "-m", "fix the /x/y.toml read"]
    );
    // Nothing is expanded: only the shell knows what `$D` holds.
    assert_eq!(
        words("echo pwned >> $D/AGENTS.md"),
        ["echo", "pwned", "$D/AGENTS.md"]
    );
    // A word is offered by its comma-separated pieces AND whole, so the
    // text that is not a shell line at all (a REPL line `term.send`
    // types, an MCP argument) gives up its paths, and a path that holds
    // a comma is still offered unbroken.
    assert_eq!(words("f('/x/y',1)"), ["f", "/x/y", "1", "/x/y,1"]);
    assert_eq!(words("/a,b/c"), ["/a", "b/c", "/a,b/c"]);
    assert_eq!(words(""), [] as [&str; 0]);
}

/// A tool by name only: the gate looks at nothing else about it.
struct T(&'static str);
impl Tool for T {
    fn name(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        ""
    }
    fn input_schema(&self) -> Value {
        json!({})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
        Ok(Plan::default())
    }
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    (d, root)
}

fn policy(root: &Path, enforcement: Posture) -> ToolPolicy {
    ToolPolicy {
        roots: vec![root.to_path_buf()],
        allow_argv: vec![vec!["git".into(), "status".into()]],
        notify_argv: vec![vec!["sudo".into()]],
        enforcement,
        tools: BTreeMap::new(),
        mcp: BTreeMap::new(),
        aws: BTreeMap::new(),
        confirmer: "operator".into(),
        floor_paths: vec![root.join("state")],
        floor_argv: floor_argv(),
        private_addresses: Default::default(),
    }
}

/// The bench profile's policy (theseus-7gir.20): every tool open, no
/// approve lists. A fetch of this machine's page waits for approval under
/// the default, which no one gives in a headless trial (b5's
/// install-windows-3.11), and runs open once `private_addresses` is open;
/// a public host's fetch is open either way, and the floor still asks.
#[test]
fn a_private_address_waits_unless_the_policy_opens_it() {
    let (_d, root) = workspace();
    let bench = |private_addresses| ToolPolicy {
        allow_argv: vec![],
        notify_argv: vec![],
        private_addresses,
        ..policy(&root, Posture::Open)
    };
    let fetch = |url: &str| Plan {
        url: Some(url.into()),
        summary: format!("fetch {url}"),
        ..Default::default()
    };
    let t = T("http.fetch");
    let local = fetch("http://localhost/vnc.html");
    let asked = bench(Default::default()).decide(&t, &local);
    assert_eq!(asked.posture, Posture::Approve, "{}", asked.reason);
    assert!(
        asked.reason.contains("localhost is this machine"),
        "{}",
        asked.reason
    );
    let open = crate::web::net::PrivateAddresses::Open;
    for url in [
        "http://localhost/vnc.html",
        "http://127.0.0.1:6080/vnc.html",
        "http://10.0.0.7/",
        "http://example.com/",
    ] {
        let out = bench(open).decide(&t, &fetch(url));
        assert_eq!(out.posture, Posture::Open, "{url}: {}", out.reason);
    }
    let floor = Plan {
        resources: vec![Resource {
            path: root.join("state/store"),
            access: Access::Read,
        }],
        ..local
    };
    assert!(bench(open).decide(&t, &floor).floor);
}

fn plan(path: PathBuf, access: Access, argv: Option<Vec<&str>>) -> Plan {
    Plan {
        resources: vec![Resource { path, access }],
        argv: argv.map(|v| v.into_iter().map(String::from).collect()),
        url: None,
        summary: "the call".into(),
        ..Default::default()
    }
}

#[test]
fn each_posture_runs_notifies_or_waits_on_a_plain_call() {
    let (_d, root) = workspace();
    assert_eq!(
        Posture::ALL.map(Posture::as_str),
        ["open", "notify", "approve"],
        "three postures, and none refuses"
    );
    // No class axis: a read, a write, and a command all take the posture.
    for (tool, pl) in [
        (
            "proc.run",
            plan(root.clone(), Access::Exec, Some(vec!["cargo", "test"])),
        ),
        ("fs.write", plan(root.join("a.rs"), Access::Write, None)),
        ("fs.read", plan(root.join("a.rs"), Access::Read, None)),
    ] {
        let t = T(tool);
        let out = policy(&root, Posture::Open).decide(&t, &pl);
        assert_eq!(out.posture, Posture::Open);
        assert!(out.notify.is_none() && !out.floor);
        assert_eq!(out.reason, format!("{tool} — open (enforcement = open)"));

        let out = policy(&root, Posture::Notify).decide(&t, &pl);
        assert_eq!(out.posture, Posture::Notify);
        assert_eq!(
            out.notify,
            Some(Notice {
                kind: "notify".into(),
                setting: "enforcement = notify".into(),
                rule: format!("{tool} — notify (enforcement = notify)"),
            })
        );

        let out = policy(&root, Posture::Approve).decide(&t, &pl);
        assert_eq!(out.posture, Posture::Approve);
        assert!(out.notify.is_none() && !out.floor);
        assert_eq!(
            out.reason,
            format!("the call: {tool} — approve (enforcement = approve)")
        );
    }
}

/// theseus-ewi (the owner, 2026-09-30: one rule for writes outside the
/// roots), widened by theseus-m9fdg (the owner, 2026-10-30 12:04: "That
/// whole system of project folders should work that way generally"): an
/// access outside the roots, a read or a working directory as much as a
/// write, takes the tool's posture, whichever tool makes it, and says
/// where it went. The floor still asks, for every tool and posture.
#[test]
fn an_access_outside_the_roots_takes_the_posture_whichever_tool_makes_it() {
    let (_d, root) = workspace();
    let (_o, outside) = workspace();
    let write = plan(outside.join("scratch/config.toml"), Access::Write, None);
    let script = plan(
        root.clone(),
        Access::Exec,
        Some(vec![
            "bash",
            "-c",
            "mkdir -p /tmp/s && echo x > /tmp/s/config.toml",
        ]),
    );
    for e in Posture::ALL {
        let p = policy(&root, e);
        for (tool, pl) in [("fs.write", &write), ("proc.run", &script)] {
            let out = p.decide(&T(tool), pl);
            // `open` is raised to a notice by what a call reaches outside
            // (the script's cwd is inside, so only the write is).
            let want = if tool == "fs.write" {
                e.max(Posture::Notify)
            } else {
                e
            };
            assert_eq!((out.posture, out.floor), (want, false), "{tool} at {e:?}");
            assert!(
                out.reason.contains(if want == e {
                    "(enforcement = "
                } else {
                    "(outside [tools].roots"
                }),
                "{}",
                out.reason
            );
        }
    }
    // A write to a path that was once on the approve list is now the
    // posture's alone, outside-the-roots label and all: no path waits.
    let listed = policy(&root, Posture::Open);
    let out = listed.decide(
        &T("fs.write"),
        &plan(outside.join("keys/id"), Access::Write, None),
    );
    assert_eq!((out.posture, out.floor), (Posture::Notify, false));
    assert!(
        out.reason.contains("outside [tools].roots"),
        "{}",
        out.reason
    );
    let mut floor = policy(&root, Posture::Open);
    floor.floor_paths.push(outside.join("state"));
    let out = floor.decide(
        &T("fs.write"),
        &plan(outside.join("state/store/0.wal"), Access::Write, None),
    );
    assert_eq!((out.posture, out.floor), (Posture::Approve, true));
}

/// The owner's config shape (theseus-m9fdg): `enforcement = "notify"`
/// with the read tools set to `open` in `[policy.tools]`. A read outside
/// the roots there must still tell the operator, and the label never
/// asks: `open` is raised to `notify` and no further. A rig on default
/// postures cannot see this, which is how it once ran silent.
#[test]
fn an_open_read_tool_outside_the_roots_is_a_notice_not_silence() {
    let (_d, root) = workspace();
    let mut p = policy(&root, Posture::Notify);
    let reads = [
        "fs.read", "fs.list", "fs.glob", "fs.grep", "git.diff", "git.log",
    ];
    for t in reads {
        p.tools.insert(t.into(), Posture::Open);
    }
    let outside = plan("/etc/passwd".into(), Access::Read, None);
    let inside = plan(root.join("src/lib.rs"), Access::Read, None);
    for t in reads {
        let out = p.decide(&T(t), &outside);
        assert_eq!(out.posture, Posture::Notify, "{t}: {}", out.reason);
        assert!(!out.floor, "{t}");
        let notice = out.notify.expect("the notice names the path");
        assert_eq!(notice.setting, "outside [tools].roots", "{t}");
        for text in [&out.reason, &notice.rule] {
            assert!(
                text.contains("read outside your project folder: /etc/passwd (roots: "),
                "{t}: {text}"
            );
            assert!(
                text.contains("the config says [policy.tools]"),
                "{t}: {text}"
            );
        }
        // Inside the roots the override still runs open and silent.
        let out = p.decide(&T(t), &inside);
        assert_eq!(out.posture, Posture::Open, "{t}: {}", out.reason);
        assert!(out.notify.is_none(), "{t}");
    }
    // Never raised past notify, and nothing a path is or is under asks
    // any more: the approve list is retired (theseus-c4i8m), so an open
    // override inside the roots is open, wherever the path sits.
    let out = p.decide(
        &T("fs.read"),
        &plan(root.join("secret/key"), Access::Read, None),
    );
    assert_eq!(out.posture, Posture::Open, "{}", out.reason);
}

/// `decide_with` hands the labels on in `Decision::outside`.
#[test]
fn a_decision_carries_the_labels_of_what_it_reaches_outside_the_roots() {
    let (_d, root) = workspace();
    let (_o, outside) = workspace();
    let p = policy(&root, Posture::Approve);
    let out = p.decide(
        &T("fs.write"),
        &plan(outside.join("scratch/config.toml"), Access::Write, None),
    );
    assert_eq!(out.outside.len(), 1, "{:?}", out.outside);
    assert!(
        out.outside[0].contains("outside your project folder"),
        "{:?}",
        out.outside
    );
    let inside = p.decide(
        &T("fs.write"),
        &plan(root.join("notes.txt"), Access::Write, None),
    );
    assert!(inside.outside.is_empty(), "{:?}", inside.outside);
}

/// What a call reaches outside the roots is named in its decision, as the
/// owner's footer line: `<verb> outside your project folder: <path>
/// (roots: ...)`, in the reason at every posture and in the notice at
/// notify (theseus-m9fdg).
#[test]
fn what_a_call_reaches_outside_the_roots_is_named_in_its_decision() {
    let (_d, root) = workspace();
    let (_o, outside) = workspace();
    let write = plan(outside.join("scratch/config.toml"), Access::Write, None);
    let read = plan(outside.join("notes.txt"), Access::Read, None);
    let ls = plan(outside.clone(), Access::Exec, Some(vec!["ls"]));
    let r = root.display();
    for e in Posture::ALL {
        let p = policy(&root, e);
        for (tool, pl, says) in [
            (
                "fs.read",
                &read,
                format!(
                    "read outside your project folder: {} (roots: {r})",
                    outside.join("notes.txt").display()
                ),
            ),
            (
                "proc.run",
                &ls,
                format!(
                    "ran outside your project folder: {} (roots: {r})",
                    outside.display()
                ),
            ),
            (
                "fs.write",
                &write,
                format!(
                    "wrote outside your project folder: {} (roots: {r})",
                    outside.join("scratch/config.toml").display()
                ),
            ),
        ] {
            let out = p.decide(&T(tool), pl);
            // An `open` enforcement is raised to a notice, never higher.
            let want = e.max(Posture::Notify);
            assert_eq!((out.posture, out.floor), (want, false), "{tool} at {e:?}");
            assert!(out.reason.contains(&says), "{}", out.reason);
            if want == Posture::Notify {
                let notice = out.notify.expect("a notify names the path");
                assert!(notice.rule.contains(&says), "{}", notice.rule);
                let set = if e == Posture::Notify {
                    "enforcement = notify"
                } else {
                    "outside [tools].roots"
                };
                assert_eq!(notice.setting, set);
            }
        }
    }
    // With no roots configured, everything is outside, and says so.
    let mut bare = policy(&root, Posture::Notify);
    bare.roots.clear();
    let out = bare.decide(&T("fs.read"), &read);
    assert_eq!(out.posture, Posture::Notify, "{}", out.reason);
    assert!(
        out.reason
            .contains("(roots: none configured, set [tools].projects_dir)"),
        "{}",
        out.reason
    );
    // An argument outside the roots is labelled the same way, and the
    // allow list does not run it open.
    let out = policy(&root, Posture::Notify).decide(
        &T("proc.run"),
        &plan(
            root.clone(),
            Access::Exec,
            Some(vec!["git", "status", "/etc"]),
        ),
    );
    assert_eq!(out.posture, Posture::Notify, "{}", out.reason);
    assert!(
        out.reason.contains(&format!(
            "ran outside your project folder: /etc (roots: {r})"
        )),
        "{}",
        out.reason
    );
}

#[test]
fn a_per_tool_override_beats_the_global_posture() {
    let (_d, root) = workspace();
    let mut p = policy(&root, Posture::Notify);
    p.tools.insert("proc.run".into(), Posture::Approve);
    p.tools.insert("fs.read".into(), Posture::Open);
    let run = plan(root.clone(), Access::Exec, Some(vec!["cargo", "test"]));
    let out = p.decide(&T("proc.run"), &run);
    assert_eq!(out.posture, Posture::Approve);
    assert_eq!(
        out.reason,
        "the call: proc.run — approve ([policy.tools] \"proc.run\" = approve)"
    );
    let read = plan(root.join("a.rs"), Access::Read, None);
    let out = p.decide(&T("fs.read"), &read);
    assert_eq!((out.posture, out.notify), (Posture::Open, None));
    assert_eq!(
        p.decide(&T("fs.grep"), &read).posture,
        Posture::Notify,
        "a tool without an override inherits"
    );
    // An override lowers as well as raises.
    p.enforcement = Posture::Approve;
    assert_eq!(p.decide(&T("fs.read"), &read).posture, Posture::Open);
    let write = plan(root.join("a.rs"), Access::Write, None);
    assert_eq!(p.decide(&T("fs.write"), &write).posture, Posture::Approve);
}

/// "Should have asked" (theseus-sgh): a tightening applies after the
/// config's posture, and the stricter one wins. Under a config that
/// already asks, the decision and its reason are the config's. The floor
/// and the approve lists ask with their own reasons, and the allow list
/// still runs its commands, as it does under a config `approve`.
#[test]
fn a_tightening_is_the_stricter_posture_and_never_loosens() {
    let (_d, root) = workspace();
    let t = Tightened {
        posture: Posture::Approve,
        by: "discord:zeroaltitude",
    };
    let cmd = |argv: Vec<&str>| plan(root.clone(), Access::Exec, Some(argv));
    let run = cmd(vec!["cargo", "test"]);
    for e in Posture::ALL {
        let p = policy(&root, e);
        let out = p.decide_with(&T("proc.run"), &run, Some(t));
        assert_eq!(out.posture, Posture::Approve, "under {}", e.as_str());
        assert!(out.notify.is_none() && !out.floor);
        let want = if e == Posture::Approve {
            "the call: proc.run — approve (enforcement = approve)".to_string()
        } else {
            format!(
                "the call: proc.run — approve (tightened by discord:zeroaltitude; the config says \
                 enforcement = {})",
                e.as_str()
            )
        };
        assert_eq!(out.reason, want);
        assert_eq!(p.decide(&T("proc.run"), &run).posture, e, "untightened");
        let write = plan(root.join("a.rs"), Access::Write, None);
        assert_eq!(
            p.decide_with(&T("fs.write"), &write, None).posture,
            e,
            "another tool keeps its posture"
        );
        let allowed = p.decide_with(&T("proc.run"), &cmd(vec!["git", "status"]), Some(t));
        assert_eq!(allowed.posture, Posture::Open, "{}", allowed.reason);
        let floor = p.decide_with(&T("proc.run"), &cmd(vec!["op", "read", "x"]), Some(t));
        assert!(floor.floor, "{}", floor.reason);
        // A notify-listed program keeps the tightening's posture and
        // names the list: the tightening is the stricter of the two.
        let listed = p.decide_with(&T("proc.run"), &cmd(vec!["sudo", "ls"]), Some(t));
        assert_eq!(listed.posture, Posture::Approve, "{}", listed.reason);
        assert!(
            listed
                .reason
                .ends_with("`sudo ls` matches the notify list entry `sudo`)"),
            "{}",
            listed.reason
        );
    }
    // A per-tool override is the config's posture too.
    let mut p = policy(&root, Posture::Notify);
    p.tools.insert("proc.run".into(), Posture::Open);
    let now = p.posture_now("proc.run", Some(t));
    assert_eq!(
        (now.posture, now.config, now.tightened()),
        (Posture::Approve, Posture::Open, true)
    );
    assert_eq!(now.setting, "tightened by discord:zeroaltitude");
    assert_eq!(now.config_setting, "[policy.tools] \"proc.run\" = open");
    // Stricter wins whatever the tightening says: one to notify raises an
    // open tool to a notice, and leaves a tool that asks asking.
    let n = Tightened {
        posture: Posture::Notify,
        by: "sock#1",
    };
    let out = policy(&root, Posture::Open).decide_with(&T("proc.run"), &run, Some(n));
    assert_eq!(out.posture, Posture::Notify);
    assert_eq!(
        out.notify.map(|x| x.setting).as_deref(),
        Some("tightened by sock#1")
    );
    let out = policy(&root, Posture::Approve).decide_with(&T("proc.run"), &run, Some(n));
    assert_eq!(
        out.reason,
        "the call: proc.run — approve (enforcement = approve)"
    );
    assert!(Posture::Open < Posture::Notify && Posture::Notify < Posture::Approve);
    assert_eq!(Posture::parse("notify"), Some(Posture::Notify));
    assert_eq!(Posture::parse("deny"), None);
}

#[test]
fn an_mcp_tool_takes_server_tool_then_server_then_the_global_posture() {
    let (_d, root) = workspace();
    let call = plan(root.clone(), Access::Read, None);
    let at = |p: &ToolPolicy, name: &'static str| {
        let d = p.decide(&T(name), &call);
        (d.posture, d.reason)
    };
    let mut p = policy(&root, Posture::Notify);
    assert_eq!(
        at(&p, "mcp:x/y"),
        (
            Posture::Notify,
            "mcp:x/y — notify (enforcement = notify)".into()
        )
    );
    p.mcp.insert("x".into(), Posture::Approve);
    assert_eq!(
        at(&p, "mcp:x/y"),
        (
            Posture::Approve,
            "the call: mcp:x/y — approve ([policy.mcp] \"x\" = approve)".into()
        )
    );
    p.mcp.insert("x/y".into(), Posture::Open);
    assert_eq!(
        at(&p, "mcp:x/y"),
        (
            Posture::Open,
            "mcp:x/y — open ([policy.mcp] \"x/y\" = open)".into()
        )
    );
    assert_eq!(
        at(&p, "mcp:x/z").0,
        Posture::Approve,
        "the server's other tools take the server's"
    );
    assert_eq!(
        at(&p, "mcp:w/y").0,
        Posture::Notify,
        "another server inherits"
    );
    assert_eq!(
        at(&p, "x/y").0,
        Posture::Notify,
        "only an mcp: name consults [policy.mcp]"
    );
    p.tools.insert("mcp:x/y".into(), Posture::Approve);
    assert_eq!(
        at(&p, "mcp:x/y").0,
        Posture::Approve,
        "[policy.tools] first"
    );
}

#[test]
fn the_floor_asks_at_every_posture_and_is_never_refused_or_silent() {
    let (_d, root) = workspace();
    let store = root.join("state/store/000.wal");
    let store_arg = store.to_string_lossy().into_owned();
    // The token file lives outside the workspace, like the real one in $HOME.
    let token = tempfile::NamedTempFile::new().unwrap();
    let token = token.path().canonicalize().unwrap();
    for e in Posture::ALL {
        let mut p = policy(&root, e);
        p.floor_paths.push(token.clone());
        for (what, tool, pl) in [
            (
                "the store",
                "fs.read",
                plan(store.clone(), Access::Read, None),
            ),
            (
                "the token file",
                "fs.read",
                plan(token.clone(), Access::Read, None),
            ),
            (
                "theseusd",
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["theseusd", "config"])),
            ),
            (
                "op by path",
                "proc.run",
                plan(
                    root.clone(),
                    Access::Exec,
                    Some(vec!["/usr/bin/op", "read", "x"]),
                ),
            ),
            (
                "a path argument in the store",
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["cat", &store_arg])),
            ),
        ] {
            let out = p.decide(&T(tool), &pl);
            assert_eq!(
                out.posture,
                Posture::Approve,
                "the floor asks at {}: {what}",
                e.as_str()
            );
            assert!(out.floor && out.notify.is_none(), "{what}");
            assert!(
                out.reason
                    .starts_with(&format!("the call: {tool} — approve (floor: ")),
                "{}",
                out.reason
            );
        }
    }
    // The floor comes before the operator's notify list and overrides:
    // an overlap still waits, and is still marked as the floor.
    let mut p = policy(&root, Posture::Open);
    p.notify_argv.push(vec!["op".into()]);
    p.tools.insert("proc.run".into(), Posture::Open);
    let out = p.decide(
        &T("proc.run"),
        &plan(root.clone(), Access::Exec, Some(vec!["op", "read", "x"])),
    );
    assert!(out.floor, "{}", out.reason);
    assert_eq!(
        out.reason,
        "the call: proc.run — approve (floor: `op` is Theseus's own binary or the 1Password CLI)"
    );
    let out = p.decide(
        &T("proc.run"),
        &plan(root, Access::Exec, Some(vec!["cat", &store_arg])),
    );
    assert!(out.floor, "{}", out.reason);
    assert_eq!(
        out.reason,
        format!("the call: proc.run — approve (floor: argument `{store_arg}` is Theseus's own state or the 1Password token)")
    );
}

/// A self task's own tree (theseus-pw1q.21.9) no longer needs a rule of
/// its own: the approve list that held the state dir is gone (the owner,
/// 2026-11-03 09:01), so the tree, the trees root and the rest of the
/// state dir are the posture's. What stays is the floor, and it stays
/// through a link and through `..`. Planted reverts: each `true` below.
#[test]
fn the_state_dirs_floor_holds_through_a_link_and_a_dotdot_and_the_rest_is_open() {
    let (_d, root) = workspace();
    let state = root.join("state");
    let trees = state.join("self").join("trees");
    let (mine, other) = (trees.join("mine"), trees.join("other"));
    for d in [&mine, &other, &state.join("store")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(mine.join("a.rs"), "a").unwrap();
    std::fs::write(state.join("notes.txt"), "n").unwrap();
    // A link out of the tree, to the rest of the state dir and to the store.
    std::os::unix::fs::symlink(state.join("notes.txt"), mine.join("to-notes")).unwrap();
    std::os::unix::fs::symlink(state.join("store"), mine.join("to-store")).unwrap();
    let p = ToolPolicy {
        roots: vec![root.clone(), trees.clone()],
        floor_paths: vec![state.join("store")],
        ..policy(&root, Posture::Open)
    };
    let at = |path: PathBuf, access| plan(path, access, None);
    for (tool, access) in [("fs.read", Access::Read), ("fs.write", Access::Write)] {
        let decide = |path: PathBuf| p.decide(&T(tool), &at(path, access));
        // The tree, another task's, the trees dir and the rest of the
        // state dir are the posture's; the store is the floor, by name,
        // through a link, and through `..`.
        for (what, path, floor) in [
            ("its own tree", mine.join("a.rs"), false),
            (
                "a new file in its tree",
                mine.join("new/deeper/b.rs"),
                false,
            ),
            ("another tree", other.join("x"), false),
            ("the trees dir", trees.join("x"), false),
            ("the rest of the state dir", state.join("notes.txt"), false),
            ("a new file in the state dir", state.join("new.txt"), false),
            ("a link to the state dir", mine.join("to-notes"), false),
            ("the store", state.join("store/x"), true),
            ("a link to the store", mine.join("to-store/x"), true),
            ("a .. out to the store", mine.join("../../../store/x"), true),
        ] {
            let out = decide(path);
            let want = match floor {
                true => Posture::Approve,
                false => Posture::Open,
            };
            assert_eq!(out.posture, want, "{tool} {what}: {}", out.reason);
            assert_eq!(out.floor, floor, "{tool} {what}: {}", out.reason);
            // A floor decision says so, and an open one is silent.
            assert_eq!(
                out.reason.contains("(floor: "),
                floor,
                "{tool} {what}: {}",
                out.reason
            );
            assert_eq!(
                out.notify.map(|n| n.kind),
                None,
                "{tool} {what}: {}",
                out.reason
            );
        }
    }
    // And a self session's fail-open reads the same line: the tree and
    // the rest of the state dir open, the store never.
    assert!(
        p.fails_open(&at(mine.join("a.rs"), Access::Write)),
        "its tree"
    );
    assert!(
        p.fails_open(&at(state.join("notes.txt"), Access::Write)),
        "the rest of the state dir"
    );
    assert!(
        !p.fails_open(&at(state.join("store/x"), Access::Read)),
        "the store"
    );
    // Outside the roots the tree is labelled, as any path is, and asks for
    // nothing: the roots are no gate (theseus-m9fdg).
    let outside = ToolPolicy {
        roots: vec![root.join("elsewhere")],
        ..p
    };
    let out = outside.decide(&T("fs.read"), &at(mine.join("a.rs"), Access::Read));
    assert_eq!(out.posture, Posture::Notify, "{}", out.reason);
    assert!(
        out.reason.contains("read outside your project folder: "),
        "{}",
        out.reason
    );
}

/// A self session's soft holds open and its hard ones stay (theseus-cudar).
/// Since 1.0.4 retired the approve lists the soft set is wider: a read or
/// a write of the state dir, a read outside the roots and a once-listed
/// program all fail open. The floor, a private address and a layer-1
/// change still ask. Planted reverts: each `false` below.
#[test]
fn a_self_sessions_soft_holds_fail_open_and_the_hard_ones_stay() {
    let (_d, root) = workspace();
    let state = root.join("state");
    let keys = root.join("keys");
    let p = ToolPolicy {
        floor_paths: vec![state.join("store")],
        ..policy(&root, Posture::Open)
    };
    let outside = tempfile::tempdir().unwrap();
    let outside = outside.path().canonicalize().unwrap();
    let ok = |plan: &Plan| p.fails_open(plan);
    let read = |path: PathBuf| plan(path, Access::Read, None);
    assert!(ok(&read(state.join("self/reports/r.md"))), "a report");
    assert!(ok(&read(state.join("self/gates/g.sh"))), "a gate script");
    assert!(ok(&read(outside.join("x"))), "a read outside the roots");
    assert!(!ok(&read(state.join("store/db"))), "the store is the floor");
    // What the retired approve list used to hold open now opens.
    assert!(ok(&read(keys.join("id"))), "keys are no longer held");
    assert!(
        ok(&plan(state.join("n"), Access::Write, None)),
        "a write to the state dir"
    );
    let run = |cwd: PathBuf, argv: Vec<&str>| plan(cwd, Access::Exec, Some(argv));
    assert!(
        ok(&run(state.clone(), vec!["ls"])),
        "a run in the state dir"
    );
    assert!(
        ok(&run(root.clone(), vec!["sudo", "ls"])),
        "a notify-listed program notices and does not wait"
    );
    assert!(
        ok(&run(
            root.clone(),
            vec!["cat", &keys.join("id").display().to_string()]
        )),
        "a key as an argument"
    );
    assert!(
        ok(&run(root.clone(), vec!["cargo", "test"])),
        "an ordinary program"
    );
    // The floor still refuses to fail open, by name and as an argument.
    assert!(
        !ok(&run(root.clone(), vec!["op", "read", "x"])),
        "the 1Password CLI is the floor"
    );
    assert!(
        !ok(&run(
            root.clone(),
            vec!["cat", &state.join("store/db").display().to_string()]
        )),
        "the store as an argument is the floor"
    );
    let layer1 = Plan {
        authority: Some("abandon a task".into()),
        ..read(root.join("a"))
    };
    assert!(!ok(&layer1), "a layer-1 change");
}

#[test]
fn the_notify_list_notices_and_the_allow_list_runs_at_every_posture() {
    let (_d, root) = workspace();
    let secret = format!("{}/secret/key", root.display());
    for e in Posture::ALL {
        let mut p = policy(&root, e);
        // An override no longer meets an approve list here: nothing a path
        // is or is under waits, so `open` means open.
        p.tools.insert("fs.read".into(), Posture::Open);
        let run = |argv: Vec<&str>| {
            p.decide(
                &T("proc.run"),
                &plan(root.clone(), Access::Exec, Some(argv)),
            )
        };
        let read = |path: PathBuf| p.decide(&T("fs.read"), &plan(path, Access::Read, None));
        // A path that was on the approve list reads open now, at every
        // posture, because the override says open and nothing preempts it.
        let out = read(root.join("secret/key"));
        assert_eq!(out.posture, Posture::Open, "{}", out.reason);
        assert!(!out.floor && out.notify.is_none(), "{}", out.reason);
        // A notify-listed program runs, and is never silent: raised from
        // `open` to `notify`, never past it, and the reason names it.
        for argv in [
            vec!["/usr/bin/sudo", "ls"],
            vec!["sudo", "-u", "root", "id"],
        ] {
            let shown = argv.join(" ");
            let out = run(argv);
            let want = e.max(Posture::Notify);
            assert_eq!(out.posture, want, "at {}: {}", e.as_str(), out.reason);
            assert!(!out.floor, "{}", out.reason);
            assert_eq!(
                out.notify.is_some(),
                want == Posture::Notify,
                "{}",
                out.reason
            );
            assert!(
                out.reason
                    .contains(&format!("`{shown}` matches the notify list entry `sudo`")),
                "{}",
                out.reason
            );
        }
        // A once-approve-listed path as an argument is the posture's too.
        let out = run(vec!["cat", &secret]);
        assert_eq!(out.posture, e.max(Posture::Open), "{}", out.reason);
        assert!(!out.reason.contains("notify list"), "{}", out.reason);
        // Outside the roots is a label, not a hold: an `open` override
        // is raised to a notice, never higher, and the decision names
        // the path.
        let out = read("/etc/passwd".into());
        assert_eq!(out.posture, Posture::Notify, "{}", out.reason);
        assert!(out.notify.is_some(), "{}", out.reason);
        assert!(
            out.reason
                .contains("read outside your project folder: /etc/passwd (roots: "),
            "{}",
            out.reason
        );
        // The allow list runs a command outright, even under approve.
        for argv in [
            vec!["git", "status", "-s"],
            vec!["git", "status", "src/lib.rs"],
        ] {
            let out = run(argv);
            assert_eq!(out.posture, Posture::Open);
            assert!(out.notify.is_none());
            assert!(
                out.reason
                    .contains("matches the allow list entry `git status`"),
                "{}",
                out.reason
            );
        }
        // A path argument outside the roots, `--opt=value` included, leaves
        // the command to its tool's posture.
        for argv in [
            vec!["git", "status", "--output=/tmp/x"],
            vec!["git", "status", "/etc"],
            vec!["git", "status", ".."],
        ] {
            let out = run(argv);
            let want = e.max(Posture::Notify);
            assert_eq!(out.posture, want, "{}", out.reason);
            let setting = if want == e {
                format!("enforcement = {}", e.as_str())
            } else {
                "outside [tools].roots; the config says enforcement = open".into()
            };
            assert!(
                out.reason.contains(&format!(
                    "proc.run — {} ({setting}; ran outside your project folder: ",
                    want.as_str()
                )),
                "{}",
                out.reason
            );
        }
    }
}

/// No reason the gate writes speaks of refusing: every answer runs,
/// notifies, or waits (theseus-8az).
#[test]
fn no_reason_says_deny_or_refuse() {
    let (_d, root) = workspace();
    let secret = format!("{}/secret/key", root.display());
    let store_arg = root.join("state/x").to_string_lossy().into_owned();
    for e in Posture::ALL {
        let p = policy(&root, e);
        for (tool, pl) in [
            ("fs.read", plan("/etc/passwd".into(), Access::Read, None)),
            ("fs.read", plan(root.join("secret/k"), Access::Read, None)),
            ("fs.write", plan(root.join("a.rs"), Access::Write, None)),
            (
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["sudo", "ls"])),
            ),
            (
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["cat", &secret])),
            ),
            (
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["cat", &store_arg])),
            ),
            (
                "proc.run",
                plan(
                    root.clone(),
                    Access::Exec,
                    Some(vec!["git", "status", "/etc"]),
                ),
            ),
            (
                "proc.run",
                plan(root.clone(), Access::Exec, Some(vec!["op", "read", "x"])),
            ),
        ] {
            let r = p.decide(&T(tool), &pl).reason.to_lowercase();
            for word in ["deny", "denied", "refuse"] {
                assert!(!r.contains(word), "{word} in {r}");
            }
        }
    }
}

/// FAST: the gate is a few map lookups and path comparisons per call.
#[test]
fn deciding_stays_well_under_a_millisecond() {
    let (_d, root) = workspace();
    let mut p = policy(&root, Posture::Notify);
    p.tools.insert("fs.read".into(), Posture::Open);
    p.mcp.insert("x".into(), Posture::Approve);
    let calls = [
        (
            T("proc.run"),
            plan(
                root.clone(),
                Access::Exec,
                Some(vec!["cargo", "test", "--manifest-path", "./Cargo.toml"]),
            ),
        ),
        (
            T("fs.read"),
            plan(root.join("src/lib.rs"), Access::Read, None),
        ),
        (T("mcp:x/y"), plan(root.clone(), Access::Read, None)),
    ];
    let n = 2_000u32;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        for (t, pl) in &calls {
            std::hint::black_box(p.decide(t, pl));
        }
    }
    let per = t0.elapsed() / (n * calls.len() as u32);
    eprintln!(
        "FAST decide: {per:?} per call over {} calls (debug build)",
        n as usize * calls.len()
    );
    assert!(per < std::time::Duration::from_millis(1), "{per:?}");
}
