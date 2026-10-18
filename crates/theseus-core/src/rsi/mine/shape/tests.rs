//! Normalization goldens (§2.2): one table of calls and the shapes they
//! become, and the result classes. Dropping a subcommand word, a flag's sort
//! or a value's type moves a line here.

use serde_json::json;

use super::*;

fn texts(tool: &str, input: serde_json::Value, root: Option<&str>) -> Vec<String> {
    let w = Where {
        root: root.map(str::to_string),
    };
    normalize(tool, &input, &w)
        .into_iter()
        .map(|n| n.shape.text)
        .collect()
}

fn run(argv: &[&str]) -> String {
    texts("proc.run", json!({ "argv": argv }), None).remove(0)
}

#[test]
fn programs_keep_their_subcommand_words_and_flags_as_a_sorted_set() {
    let table: &[(&[&str], &str)] = &[
        (
            &["git", "pull", "--ff-only"],
            "proc.run[git pull --ff-only]",
        ),
        (
            &["cargo", "nextest", "run", "-p", "theseusd"],
            "proc.run[cargo nextest run -p $CRATE]",
        ),
        (
            &["git", "merge", "--no-ff", "self/x"],
            "proc.run[git merge --no-ff $REF]",
        ),
        // Flags sort, whatever order they came in; a valued flag keeps its slot.
        (
            &["git", "log", "-n", "5", "--oneline"],
            "proc.run[git log --oneline -n $NUM]",
        ),
        (&["git", "stash", "pop"], "proc.run[git stash pop]"),
        (&["git", "rerere", "status"], "proc.run[git rerere status]"),
        (&["gh", "pr", "view", "123"], "proc.run[gh pr view $NUM]"),
        (&["npm", "run", "build"], "proc.run[npm run build]"),
        (
            &["git", "clone", "https://example.org/team/repo.git"],
            "proc.run[git clone $URLHOST]",
        ),
        (
            &["git", "-C", "/srv/work/tree", "status"],
            "proc.run[git status -C $PATH:$ABS]",
        ),
        (
            &["/usr/bin/git", "fetch", "origin"],
            "proc.run[git fetch $REF]",
        ),
        (
            &["cargo", "build", "--release", "--workspace"],
            "proc.run[cargo build --release --workspace]",
        ),
        (
            &["git", "diff", "--", "src/lib.rs"],
            "proc.run[git diff $PATH:src/**.rs]",
        ),
        (
            &["git", "checkout", "-b", "feature/a-b"],
            "proc.run[git checkout -b $REF]",
        ),
    ];
    for (argv, want) in table {
        assert_eq!(&run(argv), want, "{argv:?}");
    }
}

#[test]
fn a_subcommand_word_is_part_of_the_shape() {
    assert_ne!(run(&["git", "merge", "x"]), run(&["git", "rebase", "x"]));
    assert_eq!(run(&["git", "merge", "a"]), run(&["git", "merge", "b-c"]));
}

#[test]
fn paths_are_typed_by_their_top_directory_and_extension() {
    let t = |p: &str, root: Option<&str>| texts("fs.read", json!({"path": p}), root).remove(0);
    assert_eq!(
        t("crates/a/src/b.rs", None),
        "fs.read[path=$PATH:crates/**.rs]"
    );
    assert_eq!(t("README.md", None), "fs.read[path=$PATH:*.md]");
    // Inside the repository, an absolute path is relative to it.
    assert_eq!(
        t("/home/me/proj/crates/a/src/b.rs", Some("/home/me/proj")),
        "fs.read[path=$PATH:crates/**.rs]"
    );
    // Outside it, it is $ABS: no constant may be one.
    assert_eq!(
        t("/etc/hosts.conf", Some("/home/me/proj")),
        "fs.read[path=$PATH:$ABS]"
    );
}

#[test]
fn a_model_written_edit_is_authored() {
    let n = normalize(
        "fs.edit",
        &json!({"path": "crates/a/src/b.rs", "old_string": "x", "new_string": "y"}),
        &Where::default(),
    );
    assert_eq!(n[0].shape.text, "fs.edit[$PATH:crates/**.rs] AUTHORED");
    assert!(n[0].shape.authored);
    let w = normalize(
        "fs.write",
        &json!({"path": "a.txt", "content": "z"}),
        &Where::default(),
    );
    assert!(w[0].shape.authored);
    // A long message is the model's writing too.
    let long = "word ".repeat(60);
    let c = normalize(
        "proc.run",
        &json!({"argv": ["git", "commit", "-m", long]}),
        &Where::default(),
    );
    assert!(c[0].shape.authored && c[0].shape.text.ends_with("AUTHORED"));
}

#[test]
fn steps_and_command_lines_are_one_shape_each() {
    let s = texts(
        "proc.run",
        json!({"steps": [{"argv": ["git", "fetch", "origin"]}, {"argv": ["cargo", "test"]}]}),
        None,
    );
    assert_eq!(s, ["proc.run[git fetch $REF]", "proc.run[cargo test]"]);
    let c = texts(
        "proc.run",
        json!({"command": "git fetch origin && cargo test -p core | tail -n 5 > /tmp/out"}),
        None,
    );
    assert_eq!(
        c,
        [
            "proc.run[git fetch $REF]",
            "proc.run[cargo test -p $CRATE]",
            "proc.run[tail -n $NUM]"
        ]
    );
    // A line a shell must read is authored.
    let sub = normalize(
        "proc.run",
        &json!({"command": "echo $(date)"}),
        &Where::default(),
    );
    assert!(sub[0].shape.authored);
}

#[test]
fn a_secret_glued_to_a_flag_is_a_value_never_a_flag_name() {
    let t = run(&["tool", "--token=sk-live-abcdefghijklmnop"]);
    assert!(!t.contains("sk-"), "{t}");
    let weird = run(&["tool", "--sk-live-abcdefghijklmnopqrstuvwxyz0123456789"]);
    assert!(!weird.contains("sk-live"), "{weird}");
}

#[test]
fn the_features_a_result_shows() {
    let cases: &[(&str, &[&str])] = &[
        (
            "CONFLICT (content): Merge conflict in a.rs\nAutomatic merge failed; fix conflicts",
            &["conflict"],
        ),
        (
            "Resolved 'a.rs' using previous resolution.\nAutomatic merge failed; fix conflicts",
            &["conflict", "rerere-resolved"],
        ),
        (
            "Recorded preimage for 'b.rs'\nCONFLICT (content): Merge conflict in b.rs",
            &["conflict", "rerere-left"],
        ),
        (
            "        FAIL [   0.1s] core tests::a\ntest result: FAILED. 1 failed",
            &["test-fail"],
        ),
        (
            "On branch main\nnothing to commit, working tree clean",
            &["nothing-to-commit"],
        ),
        (
            "fatal: Not possible to fast-forward, aborting.",
            &["non-ff"],
        ),
        (
            "remote: Authentication failed for 'https://example.org/x'",
            &["auth"],
        ),
        ("all good", &[]),
    ];
    for (text, want) in cases {
        assert_eq!(&features(text), want, "{text}");
    }
}

#[test]
fn a_result_class_is_ok_or_an_error_class_then_its_features() {
    assert_eq!(result_class("ok", false, Some(0), "fine"), "ok");
    assert_eq!(result_class("ok", false, None, "fine"), "ok");
    assert_eq!(
        result_class("ok", true, Some(1), "CONFLICT (content)"),
        "err:1+conflict"
    );
    assert_eq!(result_class("ok", true, Some(127), ""), "err:127");
    assert_eq!(result_class("error", true, None, ""), "err");
    assert_eq!(result_class("declined", false, None, ""), "declined");
    assert_eq!(result_class("ok", true, Some(137), ""), "err:signal");
}

#[test]
fn what_is_never_mined_is_named() {
    for t in [
        "channel.post",
        "aws.call",
        "extend.propose",
        "self.halt",
        "mcp:x/y",
        "task.create",
        "owner_ask",
    ] {
        assert!(never_tool(t), "{t}");
    }
    for t in ["proc.run", "fs.read", "git.diff"] {
        assert!(!never_tool(t), "{t}");
    }
    let curl = normalize(
        "proc.run",
        &json!({"argv": ["curl", "-d", "x", "https://example.org"]}),
        &Where::default(),
    );
    assert!(never_mined(&curl[0]));
}

#[test]
fn a_raw_value_prints_as_nothing() {
    let r = Raw::new("hunter2-secret");
    assert!(!format!("{r:?}").contains("hunter2"));
}
