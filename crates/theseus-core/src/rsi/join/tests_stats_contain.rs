//! What the count in the self-join's merge may reach (issue 45). Its checks and its fold run outside
//! L1, so they may run nothing the branch selects: no filter its `.gitattributes` names, no hook, no
//! file-system monitor. Its one lasting write is the tree: a build directory it writes into is never
//! the gate's.

use serde_json::Value;
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
    let pass = r.core.self_join_pass(&seams).await.unwrap();
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
