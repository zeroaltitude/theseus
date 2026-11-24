//! The count in the self-join's own merge (issue 45): main's counter, frozen from the join's base,
//! writes STATS.md into the merge before main's gate; the merge that is gated, signed, pushed and
//! written on the row is the one that holds it. Anything else the counter does is undone, the
//! undoing is checked, and the join goes on uncounted. These rigs have no jail, so main's stand-in
//! counter runs at L0, as the stand-in gate does; `theseusd`'s `jail_stop` runs the real counter in L1.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfMode;

use super::tests::{git, rig, Rig, PROVED, SONNET};
use super::Seams;

/// Main's stand-in counter. `--count-head` writes STATS.md naming HEAD's parents (as the real one
/// names a made merge), and does what `STATS_STUB` asks besides; `--range` prints one line.
const COUNTER: &str = r##"import os, subprocess, sys, time
args = sys.argv[1:]
root = args[args.index("--root") + 1]
if "--range" in args:
    print("lines aaaaaaaa..bbbbbbbb: code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)")
    sys.exit(0)
parents = subprocess.run(["git", "-C", root, "log", "-1", "--format=%P", "HEAD"], check=True,
                         capture_output=True, text=True).stdout.split()
stats = os.path.join(root, "STATS.md")
with open(stats, "w") as f:
    f.write("# STATS\n\nCounted at the merge of %s into %s\n" % (parents[1], parents[0]))
WHAT
"##;

/// Main's stand-in gate, which passes only a merge whose own commit holds STATS.md.
const GATE_SEES_STATS: &str = r#"#!/bin/bash
cd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"
if ! git show HEAD:STATS.md 2>/dev/null | grep -q "Counted at"; then echo "gate: FAILED in suite: no count in the merge"; exit 1; fi
echo "gate: ok"
"#;

/// Main's counter, doing `what` (Python, after STATS.md is written) besides its count.
fn counter(what: &str) -> String {
    COUNTER.replace("WHAT", what)
}

/// Main moves to hold `files`, and the repo has fetched it.
pub(super) fn on_main(r: &Rig, files: &[(&str, &str)]) {
    let root = r.f.place.state.parent().unwrap().to_path_buf();
    let w = root.join(format!("main-edit-{}", files.len()));
    let up = r.f.upstream.to_str().unwrap();
    git(&root, &["clone", "-q", up, w.to_str().unwrap()]);
    for (path, text) in files {
        let p = w.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "main"]);
    git(&w, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    std::fs::remove_dir_all(&w).unwrap();
    git(&r.f.place.repo, &["fetch", "-q", "origin"]);
}

/// The tree a plain merge of `head` into `base` has (what the join made before any count).
pub(super) fn plain_tree(r: &Rig, base: &str, head: &str) -> String {
    let out = git(&r.f.place.repo, &["merge-tree", "--write-tree", base, head]);
    out.lines().next().unwrap().to_string()
}

/// A commit's tree, read in the repo (which holds the join's unsigned merges too).
pub(super) fn tree_of(r: &Rig, commit: &str) -> String {
    git(
        &r.f.place.repo,
        &["rev-parse", &format!("{commit}^{{tree}}")],
    )
}

pub(super) fn joined(r: &Rig) -> Value {
    let rows = r.rows("self.joined");
    assert_eq!(rows.len(), 1, "one join: {rows:?}");
    rows[0].clone()
}

/// A branch of `files` joined over a main holding `main`: (base, head, signed merge, row).
async fn join(
    r: &Rig,
    main: &[(&str, &str)],
    files: &[(&str, &str)],
    seams: Seams,
) -> (String, String, String, Value) {
    if !main.is_empty() {
        on_main(r, main);
    }
    let base = r.upstream_main();
    let (_, head) = r.seed("trim-the-sails", files, SONNET);
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    (base, head, r.upstream_main(), joined(r))
}

/// The count is folded into the unsigned merge before main's gate: main's STATS.md (an old count) is
/// rewritten; the gate (which passes only a merge whose own commit holds a STATS.md other than
/// main's) is green, the merge on main has the counter's STATS.md,
/// its parents are (base, head), it is signed, and the row's `unsigned` is the folded merge, of the
/// same tree, with the merge before the fold beside it as `uncounted`.
#[tokio::test]
async fn a_join_folds_the_count_into_its_unsigned_merge_before_the_gate() {
    let r = rig(SelfMode::Act);
    let c = counter("");
    let main = [
        ("scripts/stats.py", c.as_str()),
        ("scripts/gate.sh", GATE_SEES_STATS),
        ("STATS.md", "# STATS\n\nan old count\n"),
    ];
    let (base, head, merge, j) = join(&r, &main, PROVED, r.seams()).await;
    let stats = git(&r.f.upstream, &["show", &format!("{merge}:STATS.md")]);
    assert_eq!(
        stats,
        format!("# STATS\n\nCounted at the merge of {head} into {base}")
    );
    let parents = git(&r.f.upstream, &["rev-list", "--parents", "-n", "1", &merge]);
    assert_eq!(parents, format!("{merge} {base} {head}"));
    let raw = git(&r.f.upstream, &["cat-file", "commit", &merge]);
    assert!(
        raw.contains("gpgsig -----BEGIN PGP SIGNATURE-----"),
        "{raw}"
    );
    let n = &j["numbers"];
    assert_eq!(n["stats"], "counted", "{n}");
    let (unsigned, uncounted) = (
        n["unsigned"].as_str().unwrap(),
        n["uncounted"].as_str().unwrap(),
    );
    assert_eq!(
        tree_of(&r, unsigned),
        tree_of(&r, &merge),
        "the gated tree is the signed one"
    );
    assert_eq!(tree_of(&r, uncounted), plain_tree(&r, &base, &head));
    assert_ne!(unsigned, uncounted);
    // The note's change in lines (issue 45, line 2), from the same counter, without its ids.
    assert_eq!(
        n["lines"],
        "code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)"
    );
}

/// The counter is main's, pinned at the join's base: a branch that rewrites `scripts/stats.py` to
/// write "forged" gets main's count all the same.
#[tokio::test]
async fn the_count_runs_mains_counter_not_the_branchs() {
    let r = rig(SelfMode::Act);
    let forged = "import os, sys\nroot = sys.argv[sys.argv.index('--root') + 1]\n\
                  open(os.path.join(root, 'STATS.md'), 'w').write('forged\\n')\n";
    let mut files = PROVED.to_vec();
    files.push(("scripts/stats.py", forged));
    let main = counter("");
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], &files, r.seams()).await;
    let stats = git(&r.f.upstream, &["show", &format!("{merge}:STATS.md")]);
    assert_eq!(
        stats,
        format!("# STATS\n\nCounted at the merge of {head} into {base}")
    );
    assert_eq!(j["numbers"]["stats"], "counted");
    // And the branch's counter is what lands, as the branch changed it.
    let landed = git(
        &r.f.upstream,
        &["show", &format!("{merge}:scripts/stats.py")],
    );
    assert_eq!(landed, forged.trim_end());
}

/// Containment is the whole tree: a counter that also edits `fix.txt`, a file the branch itself
/// changed (so no new path appears), is undone, and the join lands the plain merge, uncounted.
#[tokio::test]
async fn a_counter_that_touches_another_file_is_undone_and_the_join_goes_on_uncounted() {
    let r = rig(SelfMode::Act);
    let main = counter("open(os.path.join(root, 'fix.txt'), 'a').write('more\\n')");
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], PROVED, r.seams()).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let n = &j["numbers"];
    assert!(n["stats"].as_str().unwrap().starts_with("skipped: "), "{n}");
    assert!(n["stats"].as_str().unwrap().contains("fix.txt"), "{n}");
    assert_eq!(n["uncounted"], Value::Null);
    assert_eq!(
        tree_of(&r, n["unsigned"].as_str().unwrap()),
        tree_of(&r, &merge)
    );
}

/// A counter that fails, or runs past its limit, leaves the merge as it was, and the join lands.
#[tokio::test]
async fn a_counter_that_fails_or_overruns_leaves_the_merge_as_it_was() {
    for (what, why) in [
        (
            "open(os.path.join(root, 'junk.txt'), 'w').write('junk\\n'); sys.exit(1)",
            "exit 1",
        ),
        ("time.sleep(30)", "limit"),
    ] {
        let r = rig(SelfMode::Act);
        let seams = Seams {
            stats_limit: Duration::from_secs(1),
            ..r.seams()
        };
        let main = counter(what);
        let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], PROVED, seams).await;
        assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head), "{why}");
        let said = j["numbers"]["stats"].as_str().unwrap().to_string();
        assert!(said.starts_with("skipped: "), "{why}: {said}");
    }
}

/// A STATS.md the counter made a symbolic link is refused: the join lands uncounted.
#[tokio::test]
async fn a_counter_that_makes_stats_a_symlink_is_refused() {
    let r = rig(SelfMode::Act);
    let main = counter("os.remove(stats); os.symlink('README.md', stats)");
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], PROVED, r.seams()).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let said = j["numbers"]["stats"].as_str().unwrap();
    assert!(
        said.starts_with("skipped: ") && said.contains("STATS.md"),
        "{said}"
    );
}

/// Main moved before the first push: the second try merges onto the new main and counts again, so
/// the STATS.md that lands names the new base.
#[tokio::test]
async fn a_retry_after_main_moved_recounts_on_the_new_base() {
    let r = rig(SelfMode::Act);
    let main = counter("");
    on_main(&r, &[("scripts/stats.py", &main)]);
    let moved = Arc::new(Mutex::new(None::<String>));
    let (m, root, up) = (
        moved.clone(),
        r.f.place.state.parent().unwrap().to_path_buf(),
        r.f.upstream.clone(),
    );
    let seams = Seams {
        before_push: Some(Arc::new(move |attempt| {
            if attempt == 1 {
                let other = root.join("mover");
                git(
                    &root,
                    &["clone", "-q", up.to_str().unwrap(), other.to_str().unwrap()],
                );
                std::fs::write(other.join("elsewhere.txt"), "elsewhere\n").unwrap();
                git(&other, &["add", "-A"]);
                git(&other, &["commit", "-q", "-m", "elsewhere"]);
                git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
                *m.lock().unwrap() = Some(git(&other, &["rev-parse", "HEAD"]));
            }
        })),
        ..r.seams()
    };
    let (_, head, merge, j) = join(&r, &[], PROVED, seams).await;
    let new_main = moved.lock().unwrap().clone().unwrap();
    assert_eq!(j["numbers"]["tries"], 2);
    let stats = git(&r.f.upstream, &["show", &format!("{merge}:STATS.md")]);
    assert_eq!(
        stats,
        format!("# STATS\n\nCounted at the merge of {head} into {new_main}")
    );
    assert_eq!(j["numbers"]["stats"], "counted");
}

/// A main with no counter joins exactly as before: the plain merge, no STATS.md, and the row says why.
#[tokio::test]
async fn a_main_with_no_counter_joins_as_before() {
    let r = rig(SelfMode::Act);
    let (base, head, merge, j) = join(&r, &[], PROVED, r.seams()).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let n = &j["numbers"];
    assert_eq!(n["stats"], "skipped: main has no scripts/stats.py");
    assert_eq!(n["uncounted"], Value::Null);
    assert_eq!(n["lines"], Value::Null);
    let listed = git(&r.f.upstream, &["ls-tree", &merge, "--", "STATS.md"]);
    assert_eq!(listed, "", "no STATS.md");
}

/// The "Theseus changed itself" notice states the change in lines when the row has it, and says
/// nothing of lines when it has not.
#[test]
fn the_installed_notice_states_the_change_in_lines() {
    let mut j = json!({"branch": "self/x", "item": "x", "what": "w", "why": "y", "gate": "green",
                       "cost_usd": 0.0, "numbers": {"gate_secs": 1, "tries": 1,
                       "lines": "code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)"}});
    let said = super::install::notice(&j, None, "0123456789abcdef");
    assert!(
        said.contains("\nLines: code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)\n"),
        "{said}"
    );
    j["numbers"]["lines"] = Value::Null;
    assert!(!super::install::notice(&j, None, "0123456789abcdef").contains("Lines:"));
}

/// A fold that breaks after it moved HEAD and the index (here, a file appears in the tree) is undone:
/// the tree, HEAD and the index are the merge's again, and the join lands uncounted.
#[tokio::test]
async fn a_fold_that_breaks_after_head_moved_is_undone() {
    let r = rig(SelfMode::Act);
    let seams = Seams {
        after_fold: Some(Arc::new(|dir: &std::path::Path| {
            std::fs::write(dir.join("late.txt"), "late\n").unwrap()
        })),
        ..r.seams()
    };
    let main = counter("");
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], PROVED, seams).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let n = &j["numbers"];
    assert!(n["stats"].as_str().unwrap().starts_with("skipped: "), "{n}");
    assert_eq!(n["stats_skip"], "unfolded", "{n}");
    assert_eq!(n["uncounted"], Value::Null);
}

/// A rig whose vault's board holds `value` as `db_password`, and whose runtime scrubber reads that
/// board, as the daemon's does.
fn rig_knowing(value: &'static str) -> Rig {
    super::tests::rig_parts(
        SelfMode::Act,
        |_| {},
        |p| {
            let b = crate::secrets::SecretBoard::new(
                ["db_password".to_string()],
                std::time::Instant::now(),
            );
            let values = [(
                "db_password".to_string(),
                Ok(crate::secrets::Secret::new(value.to_string())),
            )];
            b.publish(values.into_iter().collect(), "test");
            p.scrubber = Arc::new(crate::scrub::Scrubber::from_board(b.clone()));
            p.secrets = b;
        },
    )
}

/// A counter that fails on a last line whose known value starts before character 200 and ends
/// after it: the row's stored `stats` says how it exited and stops before the value, keeping no
/// piece of it. The runtime's scrubber reads the vault's board, as the daemon's does.
#[tokio::test]
async fn a_failed_counters_words_stop_before_a_known_value() {
    const VALUE: &str = "vault-value-9d2f4e1a7c";
    let r = rig_knowing(VALUE);
    let lead = "word ".repeat(38);
    assert!(lead.len() < 200 && lead.len() + VALUE.len() > 200);
    let last = format!("{lead}{VALUE} and the end");
    let main = counter(&format!("print({last:?}); sys.exit(1)"));
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], PROVED, r.seams()).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let said = j["numbers"]["stats"].as_str().unwrap();
    assert!(said.starts_with("skipped: "), "{said}");
    assert!(said.contains("exited 1: word word"), "{said}");
    for piece in [
        &VALUE[..8],
        &VALUE[VALUE.len() - 8..],
        "vault-v",
        "and the end",
    ] {
        assert!(!said.contains(piece), "{piece} is kept: {said}");
    }
    assert_eq!(j["numbers"]["stats_skip"], "failed", "{}", j["numbers"]);
}

/// A counter that fails printing a known value that runs over a line break: its last line holds
/// only the value's second half, so the row's stored `stats` says how it exited and keeps neither
/// half.
#[tokio::test]
async fn a_failed_counters_words_keep_no_line_a_known_value_runs_into() {
    const VALUE: &str = "kelp-one-0417\nferry-two-0417";
    let r = rig_knowing(VALUE);
    let out = format!("note: {VALUE} later");
    let main = counter(&format!("print({out:?}); sys.exit(1)"));
    let (base, head, merge, j) = join(&r, &[("scripts/stats.py", &main)], PROVED, r.seams()).await;
    assert_eq!(tree_of(&r, &merge), plain_tree(&r, &base, &head));
    let said = j["numbers"]["stats"].as_str().unwrap();
    assert!(said.starts_with("skipped: "), "{said}");
    assert!(said.contains("main's counter (--count-head) exited 1"), "{said}");
    for piece in ["kelp-one", "ferry-two", "0417", "later"] {
        assert!(!said.contains(piece), "{piece} is kept: {said}");
    }
    assert_eq!(j["numbers"]["stats_skip"], "failed", "{}", j["numbers"]);
}
