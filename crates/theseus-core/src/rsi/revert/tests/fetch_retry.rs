//! A red whose pass could not fetch is not spent (theseus-38sq7): the watch's
//! `known` set records a red only once its fetch has answered, so the next pass
//! acts on it. A stand-in `git` ahead of the real one fails `fetch` while a
//! flag file exists, so none of it needs a network.

use std::ffi::OsString;
use std::path::PathBuf;

use super::*;

/// A place `PATH` whose `git` fails the next `fetch` after `fail_next` is
/// called, once, and is the real one for everything else.
pub(super) struct Flaky {
    pub(super) flag: PathBuf,
    _dir: tempfile::TempDir,
}

impl Flaky {
    fn fail_next(&self) {
        std::fs::write(&self.flag, "").unwrap();
    }
}

pub(super) fn flaky_git(r: &Rig) -> Flaky {
    flaky_git_over(r, &std::env::var("PATH").unwrap())
}

/// `flaky_git` over the `PATH` `base`. The stand-in finds "the real git" with `command -v git`, and where the first
/// `git` on `PATH` is itself a wrapper (a shell script that strips its own directory from `PATH` and runs the next
/// `git`) that is the wrapper: it then looked for `git` on a `PATH` that began with the stand-in, found the stand-in,
/// which ran the wrapper again, and so on until the fetch's 60 s bound killed the loop (issue 14: four tests, one host).
/// The stand-in now runs the real git with `base` as its `PATH`, which does not hold the stand-in's directory, so the
/// chain ends whatever the real git is.
pub(super) fn flaky_git_over(r: &Rig, base: &str) -> Flaky {
    let dir = crate::scratch::tempdir();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let flag = dir.path().join("fail-next-fetch");
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .env("PATH", base)
        .output()
        .unwrap();
    let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = fetch ] && [ -e {f} ]; then\n    rm -f {f}\n    echo 'fatal: unable to access the remote (a stand-in)' >&2\n    exit 128\n  fi\ndone\nPATH='{base}' exec {real} \"$@\"\n",
        f = flag.display(),
        base = base.replace('\'', "'\\''")
    );
    let path = bin.join("git");
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = hermetic();
    env.push((
        "PATH".into(),
        OsString::from(format!("{}:{base}", bin.display())),
    ));
    r.core.tools.self_pr.set_env(env);
    Flaky { flag, _dir: dir }
}

/// A first `git` on `PATH` that is a wrapper, as on a host whose `PATH` begins with one: it strips its own directory
/// from `PATH` and runs the next `git`. With the stand-in first on `PATH` that used to loop forever (issue 14); the
/// fetch below must finish in seconds, not at the bound, on every host.
#[tokio::test]
async fn a_wrapper_git_first_on_the_path_does_not_loop_the_stand_in() {
    let r = rig(SelfMode::Act);
    let shim = crate::scratch::tempdir();
    let wrapper = shim.path().join("git");
    std::fs::write(
        &wrapper,
        "#!/bin/sh\nme=${0%/*}\nrest=\nold=$PATH\nwhile :; do\n  e=${old%%:*}\n  [ \"$e\" = \"$me\" ] || rest=${rest:+$rest:}$e\n  case $old in *:*) old=${old#*:} ;; *) break ;; esac\ndone\nPATH=$rest exec git \"$@\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let base = format!(
        "{}:{}",
        shim.path().display(),
        std::env::var("PATH").unwrap()
    );
    let _flaky = flaky_git_over(&r, &base);
    let place = r.core.tools.self_pr.place(r.f.place.repo.to_str().unwrap());
    let began = std::time::Instant::now();
    crate::rsi::join::git::fetch_within(&place, "origin", std::time::Duration::from_secs(20))
        .expect("a fetch through a wrapper git finishes");
    assert!(
        began.elapsed() < std::time::Duration::from_secs(15),
        "it finished only at the bound: {:?}",
        began.elapsed()
    );
}

/// An outer-loop red after a self-join: a pass acts on it by leaving it, with
/// the reason, in `left`.
fn outer_red(r: &Rig) -> String {
    let mine = r.land("tidy-deck", &[("deck.txt", "tidy\n")]);
    r.joined("tidy-deck", &mine);
    let outer = r.land("outer-new", &[("broken.txt", "from the outer loop\n")]);
    r.red("gate", &outer);
    outer
}

/// KEY: a pass whose fetch fails returns the error, and the next pass, with the
/// same memory and a fetch that answers, still acts on the red.
#[tokio::test]
async fn a_red_whose_fetch_failed_is_acted_on_by_the_next_pass() {
    let r = rig(SelfMode::Act);
    let flaky = flaky_git(&r);
    outer_red(&r);
    let mut known = Known::new();
    flaky.fail_next();
    let first = r.core.self_revert_pass(&r.seams(), &mut known).await;
    let e = first.expect_err("the fetch failed, so the pass did");
    assert!(format!("{e:#}").contains("stand-in"), "{e:#}");
    assert!(
        known.is_empty(),
        "a red whose fetch failed is not recorded: {known:?}"
    );
    let second = r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .unwrap();
    assert_eq!(second.left.len(), 1, "the red was acted on: {second:?}");
    assert!(second.left[0].1.contains("outer-loop"), "{second:?}");
}

/// A red that was acted on is still skipped on a later pass: no fetch, no
/// gate, nothing left, and it is acted on exactly once across the passes.
#[tokio::test]
async fn a_red_already_acted_on_is_still_skipped() {
    let r = rig(SelfMode::Act);
    let flaky = flaky_git(&r);
    outer_red(&r);
    let mut known = Known::new();
    let first = r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .unwrap();
    assert_eq!(first.left.len(), 1, "{first:?}");
    let asked = r.probed.asked.lock().unwrap().len();
    // A fetch that would fail is never reached: the red is known.
    flaky.fail_next();
    let second = r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .expect("a known red does not fetch");
    assert!(second.left.is_empty(), "{second:?}");
    assert_eq!(
        r.probed.asked.lock().unwrap().len(),
        asked,
        "no gate ran again"
    );
    assert!(flaky.flag.exists(), "no fetch was tried");
}

/// A fetch error in a batch loses none of its reds: the pass that failed
/// recorded none of them, and the next acts on every one.
#[tokio::test]
async fn a_failed_fetch_loses_none_of_the_batch() {
    let r = rig(SelfMode::Act);
    let flaky = flaky_git(&r);
    let outer = outer_red(&r);
    r.red("bench", &outer);
    let mut known = Known::new();
    flaky.fail_next();
    assert!(r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .is_err());
    let second = r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .unwrap();
    assert_eq!(second.left.len(), 2, "both reds were acted on: {second:?}");
}
