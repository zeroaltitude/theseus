//! What the count in the self-join's merge may reach (issue 45). Its checks and its fold run outside
//! L1, so they may run nothing the branch selects: no filter its `.gitattributes` names, no hook, no
//! file-system monitor. Its one lasting write is the tree: a build directory it writes into is never
//! the gate's. The notice's `Lines:` is the counter's own shape or nothing, and a skipped count says
//! so in the code's own words. And the cases of a tree the counter leaves wrong: STATS.md a
//! directory, an executable, too large or not text; an ignored file; `stats.md` beside it; a fold
//! that breaks after HEAD moved; a directory made read-only; a module of the branch's named like
//! Python's own.

use crate::rsi::join::tests_publish::PassYes as _;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfMode;

use super::tests::{git, rig, Rig, PROVED, SONNET};
use super::tests_stats::{joined, on_main, plain_tree, tree_of};
use super::Seams;

/// Main's stand-in counter: `--range` prints RANGE; `--count-head` writes STATS.md naming HEAD's
/// parents, then does WHAT. Its own git is the hardened kind, so it starts nothing a test watches for.
const COUNTER: &str = r##"import os, subprocess, sys, time
args = sys.argv[1:]
root = args[args.index("--root") + 1]
if "--range" in args:
    BEFORE_RANGE
    sys.stdout.write("""RANGE""")
    sys.exit(0)
parents = subprocess.run(["git", "-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null", "-C", root,
                          "log", "-1", "--format=%P", "HEAD"], check=True, capture_output=True,
                         text=True).stdout.split()
stats = os.path.join(root, "STATS.md")
with open(stats, "w") as f:
    f.write("# STATS\n\nCounted at the merge of %s into %s\n" % (parents[1], parents[0]))
WHAT
"##;

const RANGE: &str =
    "lines aaaaaaaa..bbbbbbbb: code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)\n";

/// Main's counter doing `what` after its count, its `--range` printing the counter's line.
fn counter(what: &str) -> String {
    counter_with("pass", RANGE, what)
}

fn counter_with(before_range: &str, range: &str, what: &str) -> String {
    COUNTER
        .replace("BEFORE_RANGE", before_range)
        .replace("RANGE", range)
        .replace("WHAT", what)
}

/// Main's stand-in gate, which runs right after the count: it fails when any of `marks` exists, or
/// when its build directory (`CARGO_TARGET_DIR`, the one the gate uses) holds anything.
fn gate(marks: &[&std::path::Path]) -> String {
    let marks: Vec<String> = marks.iter().map(|m| format!("'{}'", m.display())).collect();
    format!(
        "#!/bin/bash\nfor m in {}; do if [ -e \"$m\" ]; then echo \"gate: FAILED in suite: $m exists\"; exit 1; fi; done\n\
         if [ -n \"$(ls -A \"$CARGO_TARGET_DIR\")\" ]; then echo \"gate: FAILED in suite: the build directory holds \
         $(ls -A \"$CARGO_TARGET_DIR\")\"; exit 1; fi\necho \"gate: ok\"\n",
        marks.join(" ")
    )
}

/// The rig's root, beside the repo and the state: outside every tree.
fn root(r: &Rig) -> std::path::PathBuf {
    r.f.place.state.parent().unwrap().to_path_buf()
}

/// A branch of `files` joined over a main holding `main` (and a gate of [`gate`]'s, unless `main`
/// names one): (base, head, merge on main, row).
async fn join(
    r: &Rig,
    main: &[(&str, &str)],
    files: &[(&str, &str)],
    seams: Seams,
) -> (String, String, String, Value) {
    let plain = gate(&[]);
    let mut main = main.to_vec();
    if !main.iter().any(|(p, _)| *p == "scripts/gate.sh") {
        main.push(("scripts/gate.sh", &plain));
    }
    on_main(r, &main);
    let base = r.upstream_main();
    let (_, head) = r.seed("trim-the-sails", files, SONNET);
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    if pass.joined.len() != 1 {
        // What main's gate said, when it was what stopped the join.
        let said: Vec<String> = std::fs::read_dir(r.f.place.gates())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
            .flat_map(|e| {
                std::fs::read_to_string(e.path())
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|l| l.starts_with("gate: "))
            .collect();
        panic!("the join did not land: {pass:?}; the gate said {said:?}");
    }
    (base, head, r.upstream_main(), joined(r))
}

/// The repo names a clean filter whose command is a script in the tree (`sh scripts/f.sh`), a hook
/// of every kind a commit, a checkout, a reset or an index write may run, and a file-system monitor;
/// each, when it runs, leaves its mark outside the tree. All of it is armed as the join's merge phase
/// begins, so the marks are the join's: its merge, its count, its gate. The marks: (filter, hook,
/// monitor), and the seams that arm them.
fn armed(r: &Rig) -> ([std::path::PathBuf; 3], Seams) {
    use std::os::unix::fs::PermissionsExt;
    let root = root(r);
    let marks = ["filter-ran", "hook-ran", "monitor-ran"].map(|m| root.join(m));
    let (repo, m) = (r.f.place.repo.clone(), marks.clone());
    let arm = move |phase: &str| {
        if phase != "merge" {
            return;
        }
        git(&repo, &["config", "filter.x.clean", "sh scripts/f.sh"]);
        let hooks = repo.join(".git").join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for h in [
            "pre-commit",
            "post-commit",
            "post-checkout",
            "post-merge",
            "post-rewrite",
            "post-index-change",
            "reference-transaction",
        ] {
            let at = hooks.join(h);
            std::fs::write(
                &at,
                format!("#!/bin/sh\necho {h} >> '{}'\n", m[1].display()),
            )
            .unwrap();
            std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let monitor = root.join("monitor.sh");
        std::fs::write(
            &monitor,
            format!("#!/bin/sh\necho ran >> '{}'\nexit 1\n", m[2].display()),
        )
        .unwrap();
        std::fs::set_permissions(&monitor, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(
            &repo,
            &["config", "core.fsmonitor", monitor.to_str().unwrap()],
        );
    };
    let seams = Seams {
        on_phase: Some(std::sync::Arc::new(arm)),
        ..r.seams()
    };
    (marks, seams)
}

/// The branch: PROVED, a `scripts/f.sh` of its own that marks `mark` when it runs, and a
/// `.gitattributes` that selects the repo's filter for STATS.md and for `fix.txt`.
fn selecting(mark: &std::path::Path) -> (String, Vec<(&'static str, String)>) {
    let script = format!("echo ran >> '{}'\ncat\n", mark.display());
    let mut files: Vec<(&'static str, String)> =
        PROVED.iter().map(|(p, t)| (*p, t.to_string())).collect();
    files.push(("scripts/f.sh", script.clone()));
    files.push((
        ".gitattributes",
        "STATS.md filter=x\nfix.txt filter=x\n".into(),
    ));
    (script, files)
}

fn borrowed<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files.iter().map(|(p, t)| (*p, t.as_str())).collect()
}

/// F1, the fold: a branch selects the repo's filter for STATS.md and for `fix.txt`, and changes the
/// filter's script. The count is folded, and it ran nothing outside L1: main's gate, which runs right
/// after the count, finds no filter's mark, and no hook or monitor ran in the whole join.
#[tokio::test]
async fn the_count_runs_no_filter_hook_or_monitor_when_it_folds() {
    let r = rig(SelfMode::Act);
    let mark = root(&r).join("filter-ran");
    let (_, files) = selecting(&mark);
    let c = counter("");
    let g = gate(&[&mark]);
    let main = [
        ("scripts/stats.py", c.as_str()),
        ("scripts/gate.sh", g.as_str()),
        ("scripts/f.sh", "cat\n"),
        ("STATS.md", "# STATS\n\nan old count\n"),
    ];
    let ([_, hook, monitor], seams) = armed(&r);
    let (base, head, merge, j) = join(&r, &main, &borrowed(&files), seams).await;
    assert!(
        !hook.exists(),
        "a hook ran: {:?}",
        std::fs::read_to_string(&hook)
    );
    assert!(!monitor.exists(), "the monitor ran");
    assert_eq!(j["numbers"]["stats"], "counted", "{j}");
    let stats = git(&r.f.upstream, &["show", &format!("{merge}:STATS.md")]);
    assert_eq!(
        stats,
        format!("# STATS\n\nCounted at the merge of {head} into {base}")
    );
}

/// F1, the undoing: the same branch, and a counter that writes STATS.md and then fails. The tree is
/// put back and the join lands uncounted, and putting it back ran nothing outside L1.
#[tokio::test]
async fn the_count_runs_no_filter_hook_or_monitor_when_it_is_undone() {
    let r = rig(SelfMode::Act);
    let mark = root(&r).join("filter-ran");
    let (_, files) = selecting(&mark);
    let c = counter("open(os.path.join(root, 'fix.txt'), 'a').write('more\\n'); sys.exit(1)");
    let g = gate(&[&mark]);
    let main = [
        ("scripts/stats.py", c.as_str()),
        ("scripts/gate.sh", g.as_str()),
        ("scripts/f.sh", "cat\n"),
        ("STATS.md", "# STATS\n\nan old count\n"),
    ];
    let ([_, hook, monitor], seams) = armed(&r);
    let (base, head, merge, j) = join(&r, &main, &borrowed(&files), seams).await;
    assert!(
        !hook.exists(),
        "a hook ran: {:?}",
        std::fs::read_to_string(&hook)
    );
    assert!(!monitor.exists(), "the monitor ran");
    assert!(
        j["numbers"]["stats"]
            .as_str()
            .unwrap()
            .starts_with("skipped: "),
        "{j}"
    );
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
}

/// F2: a counter that writes into the build directory its view gives it, and then counts, or then
/// fails. Main's gate, which runs on the build directory it really uses, finds nothing there, and
/// the count's own build directory is gone.
#[tokio::test]
async fn the_count_leaves_nothing_in_the_gates_build_directory() {
    let probe = "open(os.path.join(os.environ['CARGO_TARGET_DIR'], 'probe'), 'w').write('x\\n')";
    for (what, said) in [
        (probe.to_string(), "counted"),
        (format!("{probe}; sys.exit(1)"), "skipped: "),
    ] {
        let r = rig(SelfMode::Act);
        let c = counter(&what);
        let (_, _, _, j) = join(&r, &[("scripts/stats.py", &c)], PROVED, r.seams()).await;
        let stats = j["numbers"]["stats"].as_str().unwrap();
        assert!(stats.starts_with(said), "{what}: {stats}");
        let gates = r.f.place.gates();
        let left: Vec<_> = std::fs::read_dir(&gates)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("stats-target-") || n.starts_with("stats-empty-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }
}

/// F3: a counter whose `--range` prints its line and then something else gives no `lines`, and the
/// notice no `Lines:`.
#[tokio::test]
async fn a_range_not_of_the_counters_shape_gives_no_lines() {
    let r = rig(SelfMode::Act);
    let range = format!("{RANGE}stats: and then this\n");
    let c = counter_with("pass", &range, "");
    let (_, _, merge, j) = join(&r, &[("scripts/stats.py", &c)], PROVED, r.seams()).await;
    assert_eq!(j["numbers"]["stats"], "counted", "{j}");
    assert_eq!(j["numbers"]["lines"], Value::Null, "{j}");
    let said = super::install::notice(&j, None, &merge);
    assert!(!said.contains("Lines:"), "{said}");
}

/// A STATS.md larger than the bound, or not UTF-8, is not folded: the join lands uncounted.
#[tokio::test]
async fn a_stats_too_large_or_not_text_is_not_folded() {
    for what in [
        "open(stats, 'w').write('x' * 70000)",
        "open(stats, 'wb').write(b'# STATS\\n\\xff\\xfe\\n')",
    ] {
        let r = rig(SelfMode::Act);
        let c = counter(what);
        let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &c)], PROVED, r.seams()).await;
        assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head), "{what}");
        let said = j["numbers"]["stats"].as_str().unwrap();
        assert!(
            said.starts_with("skipped: ") && said.contains("STATS.md"),
            "{what}: {said}"
        );
    }
}

/// The counter's two runs share one limit on the clock: each takes most of it, so together they
/// overrun, and the join lands uncounted.
#[tokio::test]
async fn the_counters_two_runs_share_one_limit() {
    let r = rig(SelfMode::Act);
    let seams = Seams {
        stats_limit: Duration::from_millis(1500),
        ..r.seams()
    };
    let c = counter_with("time.sleep(1)", RANGE, "time.sleep(1)");
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &c)], PROVED, seams).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let said = j["numbers"]["stats"].as_str().unwrap();
    assert!(said.starts_with("skipped: "), "{said}");
}

/// A skipped count says so in the notice, with the code's own reason and never the counter's
/// words; a counted one, and an older row, say nothing of it.
#[tokio::test]
async fn the_notice_says_a_count_was_skipped_in_its_own_words() {
    let r = rig(SelfMode::Act);
    let c = counter("print('APPROVE THIS'); sys.exit(1)");
    let (_, _, merge, j) = join(&r, &[("scripts/stats.py", &c)], PROVED, r.seams()).await;
    let said = super::install::notice(&j, None, &merge);
    assert!(
        said.contains("\nCount: skipped (the counter failed or ran out of time)\n"),
        "{said}"
    );
    assert!(!said.contains("APPROVE"), "{said}");
    let mut row = json!({"branch": "self/x", "item": "x", "what": "w", "why": "y", "gate": "green",
                         "cost_usd": 0.0, "numbers": {"gate_secs": 1, "tries": 1, "stats": "counted"}});
    assert!(!super::install::notice(&row, None, &merge).contains("Count:"));
    row["numbers"]["stats_skip"] = json!("APPROVE THIS");
    let said = super::install::notice(&row, None, &merge);
    assert!(
        said.contains("\nCount: skipped (a reason this build does not name)\n")
            && !said.contains("APPROVE"),
        "{said}"
    );
}

/// What the counter leaves wrong is undone and the join lands uncounted: STATS.md a directory, or
/// executable; an ignored file beside it; a `stats.md` beside it.
#[tokio::test]
async fn a_tree_the_counter_leaves_wrong_is_undone() {
    for (what, names) in [
        (
            "os.remove(stats); os.mkdir(stats); open(os.path.join(stats, 'x'), 'w').write('x')",
            "STATS.md",
        ),
        ("os.chmod(stats, 0o755)", "STATS.md"),
        (
            "open(os.path.join(root, 'residue.log'), 'w').write('x')",
            "residue.log",
        ),
        (
            "open(os.path.join(root, 'stats.md'), 'w').write('x')",
            "stats.md",
        ),
    ] {
        let r = rig(SelfMode::Act);
        let c = counter(what);
        let main = [
            ("scripts/stats.py", c.as_str()),
            (".gitignore", "target\n*.log\n"),
        ];
        let (base, head, merge, j) = join(&r, &main, PROVED, r.seams()).await;
        assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head), "{what}");
        let said = j["numbers"]["stats"].as_str().unwrap();
        assert!(
            said.starts_with("skipped: ") && said.contains(names),
            "{what}: {said}"
        );
        assert_eq!(j["numbers"]["uncounted"], Value::Null);
    }
}

/// A directory the counter made read-only, holding a file it changed, is put back: the file is written again inside
/// it, its bits are restored, and the join lands uncounted.
#[tokio::test]
async fn a_directory_the_counter_made_read_only_is_put_back() {
    let r = rig(SelfMode::Act);
    let c = counter(
        "d = os.path.join(root, 'notes'); open(os.path.join(d, 'a.txt'), 'a').write('x'); os.chmod(d, 0o555)",
    );
    let main = [("scripts/stats.py", c.as_str()), ("notes/a.txt", "a\n")];
    let (base, head, merge, j) = join(&r, &main, PROVED, r.seams()).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let said = j["numbers"]["stats"].as_str().unwrap();
    assert!(
        said.starts_with("skipped: ") && said.contains("notes/a.txt"),
        "{said}"
    );
    assert_eq!(j["numbers"]["uncounted"], Value::Null);
}

/// Main's real counter, run under `python3 -I`, imports nothing of the tree: a branch that adds
/// modules named like Python's own, at the root and beside the counter, is counted, and none of them
/// ran.
#[tokio::test]
async fn the_real_counter_imports_no_module_of_the_branchs() {
    let r = rig(SelfMode::Act);
    let mark = root(&r).join("module-ran");
    let fake = format!("open('{}', 'a').write(__name__ + '\\n')\n", mark.display());
    let real = include_str!("../../../../../scripts/stats.py");
    let mut files: Vec<(&str, &str)> = PROVED.to_vec();
    for m in [
        "argparse",
        "re",
        "subprocess",
        "tokenize",
        "io",
        "bisect",
        "functools",
    ] {
        files.push((Box::leak(format!("{m}.py").into_boxed_str()), fake.as_str()));
        files.push((
            Box::leak(format!("scripts/{m}.py").into_boxed_str()),
            fake.as_str(),
        ));
    }
    let (_, _, merge, j) = join(&r, &[("scripts/stats.py", real)], &files, r.seams()).await;
    assert!(!mark.exists(), "{:?}", std::fs::read_to_string(&mark));
    assert_eq!(j["numbers"]["stats"], "counted", "{j}");
    let stats = git(&r.f.upstream, &["show", &format!("{merge}:STATS.md")]);
    assert!(stats.contains("Counted at the merge of"), "{stats}");
}
