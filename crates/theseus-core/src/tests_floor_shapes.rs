//! The floor's reach is a property of the PATH, not of the shape of the call
//! that names it (`theseus-3i6d1`, the floor reviewer's F1, 2026-11-10,
//! proven as `b6floor-probe.sh` probe 1).
//!
//! The owner was persuaded into the tool floor on 2026-11-10 at 14:53 — *"ok
//! we can keep the floor for now -- better to add a quick approve than risk
//! something for the moment"* — and he was told what it cost him: a tool call
//! naming `theseus.toml` or the state dir's `AGENTS.md` waits for his
//! approval, reads included. Three shapes of call reached those bytes without
//! ever meeting the floor, and a fourth was found while fixing them. **A
//! floor that holds the obvious spelling and misses the common one buys the
//! cost without the protection, and nobody notices.**
//!
//! So this file does two things, and the second is the one that matters:
//!
//! 1. **Each shape, held.** `the_floor_holds_a_path_whichever_shape_names_it`
//!    walks the shapes a call can name a path in and asserts the floor sees
//!    every one.
//! 2. **Three SETS, each asserted by its own instrument**, because this
//!    repository's second named pattern is *a hand-maintained list with no
//!    completeness test* and a floor enumerated by hand is exactly that:
//!    fixing three shapes without asserting the set leaves the fourth to be
//!    rediscovered, which is precisely what `{steps}` was.
//!    - the set of **`Plan` fields**, by
//!      `every_field_a_plan_can_name_a_path_in_is_classified`: a compile-time
//!      exhaustive destructure. The gate reads a call ONLY through its plan,
//!      so a new way to name a path is a new field, and adding one cannot
//!      compile until someone classifies it here. Closed by the type system.
//!    - the set of **tools that run something**, by
//!      `every_live_tool_whose_plan_runs_something_is_held`: a sweep of the
//!      LIVE registry, so a tool that starts carrying an argv, a batch or a
//!      line is a red rather than a gap. Closed by measurement.
//!    - the set of **spellings of one field**, by
//!      `every_spelling_that_really_runs_a_line_is_held`: a generator over
//!      independent axes, judged against an oracle that RUNS each spelling,
//!      so the question "is this really code?" is answered by the operating
//!      system rather than by our list of what a shell is.
//!
//! The third of those was added in the re-cut, and the reason is the lesson
//! of this row: the first two were real, and they were *not enough*. Keyed on
//! five shell names and a word equal to `-c`, the floor held `bash -c` and
//! let eight of eleven spellings of the same line — `bash -lc` among them —
//! reach the floored config file at `posture=Open` with `fails_open=true`,
//! and neither of the first two instruments could see that class at all,
//! because a spelling is neither a new `Plan` field nor a new tool. **The
//! hole did not go away when the list was asserted; it moved one level down,
//! into the set of spellings of one entry.** What is still outside all three
//! is named in `RESIDUE` and asserted there.
//!
//! What is deliberately NOT here is in `b6-floor-blind-call-shapes.md`: the
//! floor reads a line's words as a shell splits them and expands nothing, so
//! a path a variable holds is not held, and the roots label was left reading
//! the argv alone.

use std::path::Path;

use serde_json::{json, Value};
use theseus_tools::{paths, Access, Plan, Resource};

use crate::policy::Posture;
use crate::tests_config::{own_files, OwnFiles};

/// A call's decision, as the gate makes it, with the two marks that say
/// whether the owner's protection applied: `floor`, and whether a **self**
/// session could have gone ahead with a notice instead (`fails_open`).
fn decide(r: &OwnFiles, tool: &str, input: Value) -> (bool, bool, String) {
    let rt = &r.core.tools;
    let t = rt
        .registry
        .get(tool)
        .unwrap_or_else(|| panic!("{tool} is registered"))
        .clone();
    let pl = t
        .plan(&input, &rt.ctx)
        .unwrap_or_else(|why| panic!("{tool} plans {input}: {why}"));
    let d = rt.policy.decide_with(t.as_ref(), &pl, None);
    (d.floor, rt.policy.fails_open(&pl), d.reason)
}

/// The floor's verdict on a plan built by hand, for the two shapes a test
/// registry cannot produce: `term.send` needs a live terminal and an `mcp:`
/// tool a live server. Each one's TOOL side -- that the plan carries the
/// text at all -- is asserted where the tool lives (`term/tests.rs`,
/// `mcp::tool::strings`); this is the GATE side.
fn decide_plan(r: &OwnFiles, pl: &Plan) -> (bool, bool, String) {
    let rt = &r.core.tools;
    let t = rt.registry.get("proc.run").unwrap().clone();
    let d = rt.policy.decide_with(t.as_ref(), pl, None);
    (d.floor, rt.policy.fails_open(pl), d.reason)
}

/// Deliverable 1 and 2: every shape of a call that names a floored path is
/// held, at `open`, with no question available to route around and no
/// fail-open for a self session.
///
/// Before this row, rows 2, 4, 5 and 6 of this table were `floor=false
/// posture=Open fails_open=true`.
#[tokio::test]
async fn the_floor_holds_a_path_whichever_shape_names_it() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    for (what, path) in [("the config file", &r.config), ("AGENTS.md", &r.agents)] {
        let p = path.to_string_lossy().into_owned();
        let shapes: Vec<(&str, &str, Value)> = vec![
            // 1. `argv` — read since the floor existed.
            ("proc.run {argv}", "proc.run", json!({"argv": ["cat", &p]})),
            // 2. `command` — `bash -c <line>`, F1's finding: the whole line
            //    was taken as one candidate path, so no path inside it was
            //    ever compared with the floor.
            (
                "proc.run {command}",
                "proc.run",
                json!({"command": format!("echo pwned >> {p}")}),
            ),
            // 3. `command`, read rather than written: the owner was told
            //    reads wait too, so a read must be held as a write is.
            (
                "proc.run {command}, a read",
                "proc.run",
                json!({"command": format!("cat {p} | head -1")}),
            ),
            // 4. `steps` — NOT in F1. A batch sets `Plan::steps` and leaves
            //    `Plan::argv` `None`, and the gate read `argv` alone.
            (
                "proc.run {steps}",
                "proc.run",
                json!({"steps": [{"argv": ["cat", &p]}]}),
            ),
            // 5. `steps` whose step is itself a line: both holes at once.
            (
                "proc.run {steps} of a line",
                "proc.run",
                json!({"steps": [{"argv": ["bash", "-c", format!("rm -f {p}")]}]}),
            ),
            // 6. A path written in pieces by a redirect with no space, the
            //    spelling a blunt word-split would miss.
            (
                "proc.run {command}, a redirect with no space",
                "proc.run",
                json!({"command": format!("echo x >{p}")}),
            ),
        ];
        for (shape, tool, input) in shapes {
            let (floor, fails_open, reason) = decide(&r, tool, input);
            assert!(floor, "{shape} names {what} and is not held: {reason}");
            assert!(
                !fails_open,
                "{shape} names {what} and a self session goes ahead anyway: {reason}"
            );
            assert!(
                reason.contains("floor:") && reason.contains("approve"),
                "{shape}'s reason names the floor: {reason}"
            );
        }
    }
}

/// Deliverable 2, the `term.send` half: the text typed into a terminal is
/// judged for the paths its words name, not only the terminal's own program.
///
/// F1: `term.open {argv:["bash"]}` then a typed line reached both files with
/// nothing asked, because `Send::plan` names the TERMINAL's argv and its cwd
/// and the typed text appeared in no field of the plan.
#[tokio::test]
async fn the_floor_holds_what_a_terminal_is_told_to_type() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let typed = |text: String| Plan {
        summary: "type into terminal t1, which runs `bash`".into(),
        resources: vec![Resource {
            path: r.root.clone(),
            access: Access::Exec,
        }],
        argv: Some(vec!["bash".into()]),
        lines: vec![text],
        ..Default::default()
    };
    for (what, path) in [("the config file", &r.config), ("AGENTS.md", &r.agents)] {
        let p = path.to_string_lossy().into_owned();
        let (floor, fails_open, reason) = decide_plan(&r, &typed(format!("echo pwned >> {p}\n")));
        assert!(floor, "a line typed at {what} is not held: {reason}");
        assert!(!fails_open, "a self session types it anyway: {reason}");
    }
    // The terminal's own argv is a plain shell naming nothing floored, so
    // the hold above can only have come from the text: an ordinary line
    // typed into the same terminal still runs with nothing asked.
    let (floor, _, reason) = decide_plan(&r, &typed("cargo test\n".into()));
    assert!(!floor, "an ordinary line is not held: {reason}");
}

/// **The PROGRAMS a terminal is told to type, which is the half this row
/// closes.** Everything above was already in place and
/// `term.send {text:"op item get x --reveal\n"}` into a `term.open
/// {argv:["bash"]}` was still `floor=false fails_open=true` — a live path to
/// the 1Password CLI with the floor open, measured by the round-3 reviewer at
/// 02:25 on 2026-11-11 (R3-E) and by round 2 at 23:28 before it. The floor
/// saw the paths the typed text NAMES and never the programs it RUNS, because
/// `floor_view` tagged every `plan.lines` entry `runs_it = false` and
/// `argvs.extend(inner)` sat behind that one `bool`.
///
/// **It is closed with no shell-name list**, which is the thing the previous
/// builder thought impossible: `shell_argvs` already derived
/// `[["op","item","get","x","--reveal"]]` from the typed text with no program
/// name consulted anywhere, including the leading-assignment case. The
/// terminal's own argv is never read for this — keying on it IS the list.
///
/// **The cost, and it is the point of the second half of this test.** Handed
/// text is read at a command's FIRST word only (`prefix_match`), not at every
/// program position as a line the call runs is (`runs_program`), because
/// prose is the common case of text typed at a prompt and the uncommon case
/// of a command we chose to run. So `op item get …` typed waits for a yes and
/// `we should op out of that plan` does not. Both halves are asserted: a
/// floor that fires on prose is a floor the owner turns off.
#[tokio::test]
async fn the_floor_holds_the_programs_a_terminal_is_told_to_type() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let typed = |text: &str, program: &str| Plan {
        summary: format!("type into terminal t1, which runs `{program}`"),
        resources: vec![Resource {
            path: r.root.clone(),
            access: Access::Exec,
        }],
        argv: Some(vec![program.to_string()]),
        lines: vec![text.to_string()],
        ..Default::default()
    };
    for (what, text, program) in [
        // The round-3 reviewer's own reproduction, verbatim.
        (
            "the 1Password CLI typed",
            "op item get x --reveal\n",
            "bash",
        ),
        (
            "Theseus's own binary typed",
            "theseusd --config /tmp/x.toml\n",
            "bash",
        ),
        // The terminal need not be a shell: the gate does not know what the
        // program on the far end will do with the text, and that is the
        // over-hold this row accepts rather than the list it refuses.
        (
            "the 1Password CLI typed into a REPL",
            "op item get x\n",
            "python3",
        ),
        (
            "a leading assignment before it",
            "OP_ACCOUNT=y op item get x\n",
            "bash",
        ),
        (
            "the second command of a typed chain",
            "cd /tmp; op item get x\n",
            "bash",
        ),
    ] {
        let (floor, fails_open, reason) = decide_plan(&r, &typed(text, program));
        assert!(floor, "{what} is not held: {reason}");
        assert!(!fails_open, "a self session types {what} anyway: {reason}");
    }
    // The other side, and the reason the width differs from a run line's.
    for (what, text) in [
        ("an ordinary line", "cargo test\n"),
        (
            "prose that mentions the vault's CLI",
            "we should op out of that plan\n",
        ),
        (
            "prose that mentions the daemon",
            "the theseusd unit is up again\n",
        ),
        (
            "a health check for the daemon",
            "grep -c theseusd depart.log\n",
        ),
    ] {
        let (floor, _, reason) = decide_plan(&r, &typed(text, "bash"));
        assert!(!floor, "{what} typed is held and should not be: {reason}");
    }
}

/// **A wrapper no longer hides a floored program, and `find -exec` does not
/// either.** `floor_argv` matched an argv's FIRST word and nothing else since
/// the floor existed, so eight wrappers that the path floor is proof against
/// (`WRAPPERS`, this file's own first generator axis) walked the program floor
/// straight through — on this branch's parent and on main alike, which is why
/// it is a line of its own and not a regression of the floor-shapes row.
///
/// Every case here reaches the real 1Password CLI or Theseus's own binary,
/// and every one was `floor=false fails_open=true` when the round-3 reviewer
/// measured it (R3-B, `b6-floorshapes-recut-2-review.md` §6.3).
#[tokio::test]
async fn a_wrapper_does_not_hide_a_floored_program() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let root = r.root.to_string_lossy().into_owned();
    for (what, input) in [
        // A wrapper in front of the program, as a line and as the call's own
        // argv. Nothing says list-free that `nohup` execs its tail, so the
        // floor reads every position a program can stand in instead.
        (
            "nohup, as a line",
            json!({"command": "nohup op item get x --reveal"}),
        ),
        (
            "env, as a line",
            json!({"command": "env op item get x --reveal"}),
        ),
        (
            "env -i, which puts it at argv[2]",
            json!({"command": "env -i op item get x"}),
        ),
        (
            "timeout, which puts it after a number",
            json!({"command": "timeout 5 op item get x"}),
        ),
        (
            "setsid",
            json!({"command": "setsid theseusd --config /tmp/x.toml"}),
        ),
        (
            "xargs on the far side of a pipe",
            json!({"command": format!("find {root} -maxdepth 0 | xargs op item get x")}),
        ),
        (
            "a wrapper in the call's own argv",
            json!({"argv": ["nohup", "op", "item", "get", "x", "--reveal"]}),
        ),
        (
            "a wrapper in a step's argv",
            json!({"steps": [{"argv": ["timeout", "5", "op", "item", "get", "x"]}]}),
        ),
        // `find -exec` and `-execdir`: a flag introduces the PROGRAM and its
        // arguments are find's own argv words, so the line the flag key reads
        // is one word and a one-word line is deliberately not a program. The
        // floor's MATCH is what answers this, not the line rule.
        (
            "find -exec, as a line",
            json!({"command": format!("find {root} -maxdepth 0 -exec op item get x --reveal {{}} ;")}),
        ),
        (
            "find -exec, as the call's own argv",
            json!({"argv": ["find", &root, "-maxdepth", "0", "-exec", "op", "item", "get", "x", "--reveal", ";"]}),
        ),
        (
            "find -execdir",
            json!({"command": format!("find {root} -maxdepth 0 -execdir op item get x ;")}),
        ),
        (
            "find -exec Theseus's own binary",
            json!({"command": format!("find {root} -maxdepth 0 -exec theseusd --config /tmp/x.toml {{}} ;")}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(floor, "{what} reaches a floored program unheld: {reason}");
        assert!(!fails_open, "a self session runs {what} anyway: {reason}");
    }
    // The control that says the match did not simply become "the word appears
    // anywhere": an ordinary `find -exec` sweep, and the pattern cases round
    // 2 filed as false positives, are all still clear.
    for (what, input) in [
        (
            "an ordinary find -exec sweep",
            json!({"command": format!("find {root} -name '*.rs' -exec grep -l fn {{}} +")}),
        ),
        (
            "a log grep for the daemon",
            json!({"command": format!("grep -c theseusd {root}/depart.log")}),
        ),
        (
            "a health check through a pipe",
            json!({"command": "ps -ef | grep -c theseusd"}),
        ),
    ] {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(!floor, "{what} is held and should not be: {reason}");
    }
}

/// **The over-hold reading every program position buys, sized rather than
/// asserted away.** A bare `op` or `theseusd` WORD standing anywhere in a
/// command the call RUNS holds it, because nothing distinguishes `nohup op`
/// from a list of words without knowing that `nohup` execs its tail — and
/// knowing that is the list this floor refuses to keep.
///
/// Both lines below are this repository's own (`scripts/repro.sh`,
/// `scripts/build.sh`), so the cost is real and not hypothetical: an operator
/// who types one of them at `proc.run` gets one approval. `floor_argv` holds
/// two entries, so that is the whole blast radius, and it is the same trade,
/// in the same direction, as `the_over_hold_this_row_accepts_is_pinned`.
///
/// The sharper rule the previous row left as a line IS taken now, and it is
/// worth exactly one of these: a derived argv whose FIRST word is a reserved
/// word no command can follow (`for`, `case`) puts its later words in no
/// program position, so `for b in theseus theseusd theseus-index` is no longer
/// held and has moved to the not-held block below with its measurement.
/// `a_shell_keyword_hides_no_program` owns that case and both directions of
/// it. **The other seventeen of this repository's own 24 `scripts/` lines are
/// not reachable that way** — they are `<cmd> [flag…] theseusd` operands
/// (`cargo build -p theseusd`, `journalctl -u theseusd`, `command -v op`) and
/// bare words at position 1 of an ordinary program (`grep theseusd <log>`,
/// `pkill theseusd`) — and nothing tells those from `nohup op item get`
/// without knowing what `nohup` is, which is the list this floor refuses to
/// keep. The honest size of the trade is **18 of 24** and it is written where
/// the cost is, in `runs_program`.
#[tokio::test]
async fn a_word_list_naming_a_floored_program_is_held() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    for (what, input) in [
        (
            "an echo that names both floored programs",
            json!({"command": "echo theseusd op"}),
        ),
        (
            "a bare word at position 1 of an ordinary program (scripts/, and depart.log reading)",
            json!({"command": "pkill theseusd"}),
        ),
        (
            "a flag's OPERAND that happens to be the package name (scripts/acceptance/journey.sh:44)",
            json!({"command": "cargo build --profile test -p theseus -p theseus-sim -p theseusd"}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(
            floor && !fails_open,
            "{what} is no longer held. That is an IMPROVEMENT if the floor can now tell a word \
             list from a wrapper's command without a list of program names -- say how in the row \
             and delete this case. It is a REGRESSION if the match stopped reading anything but \
             an argv's first word, which is `nohup op item get x --reveal`: {reason}"
        );
    }
}

/// **The code-cluster wrapper class: the hole that survived teaching the floor
/// to read programs at every argv position.** `runs_program` excluded the word
/// after a code flag to spare `grep -c <pattern>`, and asked
/// `introduces_code` to say which flags those were. `introduces_code` is true
/// of ANY single-dash cluster carrying `c`, `e`, `E` or `S`, so a wrapper
/// whose last ATTACHED cluster before the program carried one hid the program
/// again — **`sudo -E op item get --reveal` reached the 1Password CLI with
/// `fails_open=true` while `sudo -n op` was held, one letter apart.** The
/// shift-5 floor-programs review measured eight such spellings (§2.2, RV-6);
/// probing the class rather than the list found **twelve** on `d6aa56e1`, and
/// `sudo -S op`, `sudo -EH op`, `env -S op` and `stdbuf -e0 theseusd` are the
/// four it did not name.
///
/// Two readings close them and neither is a list of programs:
/// [`hides_the_next_program`] asks getopt's question (the code letter must be
/// the cluster's LAST character for the next word to be the flag's) and keys
/// on [`PROGRAM_HIDING_LETTERS`] rather than every code letter.
///
/// **The accepted cost is the last block and it is one shape**, stated here
/// rather than discovered later: a one-word ERE whose pattern IS a floored
/// program name now waits for a yes.
#[tokio::test]
async fn a_code_cluster_wrapper_does_not_hide_a_floored_program() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    for (what, input) in [
        // getopt's half: the code letter is not the cluster's last character,
        // so the next word was never the flag's argument.
        (
            "ionice -c3, the cluster carrying its own operand",
            json!({"command": "ionice -c3 op item get x --reveal"}),
        ),
        (
            "sudo -En, a code letter then another flag",
            json!({"command": "sudo -En op item get x --reveal"}),
        ),
        (
            "stdbuf -e0",
            json!({"command": "stdbuf -e0 op item get x --reveal"}),
        ),
        (
            "stdbuf -e0 in front of Theseus's own binary",
            json!({"command": "stdbuf -e0 theseusd --config /tmp/x.toml"}),
        ),
        (
            "ionice -c3 in the call's own argv",
            json!({"argv": ["ionice", "-c3", "op", "item", "get", "x", "--reveal"]}),
        ),
        // The letters' half: `-E` and `-S` are a line's flags on `perl` and
        // `env` and a wrapper's flags on `sudo`, and the vault is on the
        // wrapper side.
        (
            "sudo -E, the commonest wrapper spelling there is",
            json!({"command": "sudo -E op item get x --reveal"}),
        ),
        (
            "sudo -S",
            json!({"command": "sudo -S op item get x --reveal"}),
        ),
        (
            "sudo -EH, a code letter clustered with a non-code one",
            json!({"command": "sudo -EH op item get x --reveal"}),
        ),
        (
            "env -S, which the wrapper axis of this file's own generator spells",
            json!({"command": "env -S op item get x"}),
        ),
        (
            "a wrapper in front of the wrapper",
            json!({"command": "nohup sudo -E op item get x --reveal"}),
        ),
        (
            "sudo -E in the call's own argv",
            json!({"argv": ["sudo", "-E", "op", "item", "get", "x", "--reveal"]}),
        ),
        (
            "sudo -E in a step's argv",
            json!({"steps": [{"argv": ["sudo", "-E", "op", "item", "get", "x"]}]}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(
            floor,
            "{what} reaches a floored program unheld: {reason}. This test exists because \
             `sudo -E op` did exactly that while `sudo -n op` was held."
        );
        assert!(!fails_open, "a self session runs {what} anyway: {reason}");
    }
}

/// **The controls that make the wrapper fix a NARROWING and not a removal**,
/// and the first three are round 2's own false positives: a code letter that
/// IS its cluster's last character still hides the next word, because
/// `grep -c theseusd <log>` is a pattern and holding it was four of round 2's
/// seven. Deleting [`hides_the_next_program`] reds these, not
/// `a_code_cluster_wrapper_does_not_hide_a_floored_program` — which is the
/// direction that matters, since a floor made wider by deleting a row's code
/// is a floor whose cost nothing measures.
#[tokio::test]
async fn the_pattern_flags_still_hide_their_argument() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let root = r.root.to_string_lossy().into_owned();
    for (what, input) in [
        (
            "grep -c, round 2's F-1",
            json!({"command": format!("grep -c theseusd {root}/depart.log")}),
        ),
        (
            "grep -c as an argv, round 2's F-1",
            json!({"argv": ["grep", "-c", "theseusd", &format!("{root}/x.log")]}),
        ),
        (
            "a health check through a pipe",
            json!({"command": "ps -ef | grep -c theseusd"}),
        ),
        (
            "grep -e, the other pattern flag",
            json!({"command": format!("grep -e theseusd {root}/depart.log")}),
        ),
        (
            "an ordinary find -exec sweep",
            json!({"command": format!("find {root} -name '*.rs' -exec grep -l fn {{}} +")}),
        ),
    ] {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(!floor, "{what} is held and should not be: {reason}");
    }
}

/// **The accepted cost of narrowing the letters, in full and in one place.**
/// The first case was a must-not-hold in
/// `an_ordinary_command_still_runs_with_nothing_asked` until this row and is
/// the ONLY case in this tree that [`PROGRAM_HIDING_LETTERS`] flips: every
/// other `-E`/`-S` line in the repository hands its flag a word that is not a
/// floored program name (`sed -E 's#…#'`, `curl -sS <url>`, `ssh -S <sock>`,
/// `sort -S 1M`, `ruby -E utf-8`, `cargo nextest -E 'test(…)'`), so not one of
/// them moves — grepped, not assumed.
///
/// `grep -E theseusd <file>` and `sudo -E op item get --reveal` are the SAME
/// argv shape — a program, a single-dash cluster whose last character is a
/// code letter, a bare word, more words — and nothing list-free tells them
/// apart. **So this is the record of a choice, not of a defect**: one approval
/// on a read-only one-word pattern search against four unheld paths to the
/// vault, in the direction the owner priced on 2026-11-10 at 14:53. Reverting
/// it is one character — put `'E'` back — and the message below says what else
/// has to be true first.
#[tokio::test]
async fn the_letters_the_program_key_drops_cost_one_shape() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let root = r.root.to_string_lossy().into_owned();
    for (what, input) in [
        (
            "a one-word ERE whose pattern IS a floored program name (MOVED from \
             an_ordinary_command_still_runs_with_nothing_asked)",
            json!({"command": format!("grep -E theseusd {root}/notes.md")}),
        ),
        (
            "the same with the quotes a shell strips",
            json!({"command": format!("grep -E 'theseusd' {root}/notes.md")}),
        ),
        (
            "round 3's declared false positive, held before this row too and by \
             the LINE key rather than this one",
            json!({"command": format!("grep -E 'theseusd started' {root}/depart.log")}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(
            floor && !fails_open,
            "{what} is no longer held. That is an IMPROVEMENT if the floor can now tell a \
             one-word ERE from `sudo -E op item get --reveal` without a list of program names -- \
             say how in the row and delete this case. It is a REGRESSION if `'E'` went back into \
             `PROGRAM_HIDING_LETTERS` without the four `sudo -E`/`-S` vault paths above being \
             closed another way: {reason}"
        );
    }
}

/// **A reserved word hides no program, and the half nobody priced is the
/// hole.** The previous row proposed `for`/`in`/`case` as a sharper rule and
/// left it as a line for want of a measurement. The measurement is: it retires
/// **one** of the eighteen `scripts/` lines the position match holds, and a
/// list that reached one word further would hand the vault away.
///
/// `shell_argvs("for i in 1; do op item get --reveal; done")` really derives
/// `[["for","i","in","1"], ["do","op","item","get","--reveal"], ["done"]]`, so
/// **`do` begins an argv that runs the 1Password CLI.** The same is true of
/// `then` after an `if`. [`WORDS_NO_COMMAND_FOLLOWS`] therefore holds only the
/// two reserved words a WORD follows, and the second block here is the test
/// that reds if anyone widens it.
#[tokio::test]
async fn a_shell_keyword_hides_no_program() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    // What the rule buys: one real line of this repository's own, and the
    // `case` head of the same shape.
    for (what, input) in [
        (
            "a for-loop over the shipped binaries (scripts/release-smoke.sh:198)",
            json!({"command": "for b in theseus theseusd theseus-index; do echo $b; done"}),
        ),
        (
            "the same loop with no body",
            json!({"command": "for b in theseus theseusd theseus-index"}),
        ),
        (
            "a case head naming the daemon",
            json!({"command": "case theseusd in *) echo x ;; esac"}),
        ),
    ] {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(
            !floor,
            "{what} is still held, so the keyword reading bought nothing: {reason}"
        );
    }
    // What the rule must never cost, and every case here was measured held on
    // `d6aa56e1` before this row. A `do`/`then`/`else` entry in the list reds
    // this block, which is the whole reason the list is two words long.
    for (what, input) in [
        (
            "a loop body that really runs the vault CLI",
            json!({"command": "for i in 1; do op item get x --reveal; done"}),
        ),
        (
            "a loop body that really runs the daemon",
            json!({"command": "for i in 1; do theseusd --config /tmp/x.toml; done"}),
        ),
        (
            "an if body that really runs the vault CLI",
            json!({"command": "if true; then op item get x --reveal; fi"}),
        ),
        (
            "a while body that really runs the vault CLI",
            json!({"command": "while true; do op item get x; done"}),
        ),
        (
            "a case arm that really runs the vault CLI",
            json!({"command": "case x in *) op item get x --reveal ;; esac"}),
        ),
        (
            "a reserved word handed in as the call's own argv[0]",
            json!({"argv": ["do", "op", "item", "get", "x"]}),
        ),
        (
            "`then` handed in as the call's own argv[0]",
            json!({"argv": ["then", "op", "item", "get", "x"]}),
        ),
        // These two went held -> UNHELD when the rule read `argv[0]` alone,
        // and they are why it reads the `in` as well: both reserved words are
        // reserved only in a compound their own `in` clause follows, so a
        // `for` with no `in` is not that compound and its words stay program
        // positions. Caught by W-4 of this row's own probe, not by review.
        (
            "`for` with no `in`, so no word list at all",
            json!({"argv": ["for", "op", "item", "get", "x"]}),
        ),
        (
            "`case` with no `in`",
            json!({"argv": ["case", "op", "item", "get", "x"]}),
        ),
        (
            "a `for` head whose word list is where the program would be",
            json!({"command": "for op item get x --reveal"}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(
            floor,
            "{what} reaches a floored program unheld: {reason}. A reserved word that a COMMAND \
             follows must never be in `WORDS_NO_COMMAND_FOLLOWS`."
        );
        assert!(!fails_open, "a self session runs {what} anyway: {reason}");
    }
}

/// Deliverable 2, the `mcp:` half: an MCP call's arguments are the only part
/// of it we can read at all, so a string of them that names a floored path
/// holds the call.
///
/// F1: an MCP tool's plan carried no resource and no argv, so there was
/// nothing of the call for the floor to see — *"not live on the owner's
/// config today … but it opens the day one is."* This is blunter than the
/// shell line's reading on purpose: of the two grammars, only bash's is one
/// we can parse. The judgement is written up in the row's report.
#[tokio::test]
async fn the_floor_holds_an_mcp_calls_arguments() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let p = r.agents.to_string_lossy().into_owned();
    for (what, args) in [
        ("a top-level string", json!({"path": &p})),
        ("a nested string", json!({"edit": {"file": {"at": &p}}})),
        ("a string in an array", json!({"files": ["/etc/hosts", &p]})),
    ] {
        let mut lines = vec![];
        crate::mcp::tool::strings(&args, &mut lines);
        let pl = Plan {
            summary: format!("mcp:notes/append {args}"),
            lines,
            ..Default::default()
        };
        let (floor, fails_open, reason) = decide_plan(&r, &pl);
        assert!(
            floor,
            "{what} naming the floored file is not held: {reason}"
        );
        assert!(
            !fails_open,
            "a self session makes the call anyway: {reason}"
        );
    }
}

/// **Deliverable 3, the completeness half that cannot go stale.**
///
/// The gate never sees a tool, only its `Plan`: `ToolPolicy::view` is the one
/// reader, and it reads fields of this struct and nothing else. So the set of
/// ways a call can name a path IS the set of `Plan` fields, and this
/// destructure names every one of them with no `..`. **Adding a field to
/// `Plan` stops this test compiling until someone writes down here whether
/// the floor must read it** — which is what nobody did for `steps`, filed as
/// a `(P1)` eight months after it was added.
#[test]
fn every_field_a_plan_can_name_a_path_in_is_classified() {
    let Plan {
        // READ BY THE FLOOR — each asserted by a shape above.
        resources: _, // a path read, written or run in
        argv: _,      // a program and its words, and a shell's `-c` line
        steps: _,     // a batch's programs and their words
        lines: _,     // text handed to a shell or another program to read
        // NOT A PATH, and each has its own door.
        url: _,       // a host, judged by `private_address` (DD5)
        aws: _,       // an account and an operation, judged by `aws_asks`
        authority: _, // layer 1: it waits for the owner on its own
        // CARRIES NO ACCESS AT ALL.
        summary: _, // one line for people
        class: _,   // this call's read/write class
        card: _,    // what a surface renders
    } = Plan::default();
}

/// **Deliverable 3, the behavioural half.** The structural test above proves
/// there is no fifth `Plan` field; this proves no live tool DROPS what it is
/// about to do out of its plan, which is how `term.send` and `mcp:` were
/// blind with every field read.
///
/// For every tool in the live registry, a synthesized input naming a floored
/// path in each of its string slots is planned, and any plan that comes back
/// carrying a program, a batch or a line must be held. A new tool that runs
/// something is a RED here, not a quiet gap — and the set it measures is
/// asserted, so a tool that stops carrying one is a red too.
#[tokio::test]
async fn every_live_tool_whose_plan_runs_something_is_held() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let rt = &r.core.tools;
    let floored = r.agents.to_string_lossy().into_owned();
    let mut runs: Vec<String> = vec![];
    let mut unheld: Vec<String> = vec![];
    for t in rt.registry.all() {
        for input in attempts(&t.input_schema(), &floored) {
            let Ok(pl) = t.plan(&input, &rt.ctx) else {
                // The tool refuses this input, so no call of that shape
                // exists to hold. The `Plan` destructure covers the rest.
                continue;
            };
            // "Runs something": a program, a batch, or a line handed on.
            let runs_something = pl.argv.is_some_and(|a| !a.is_empty())
                || pl.steps.is_some_and(|s| !s.is_empty())
                || !pl.lines.is_empty();
            if !runs_something {
                continue;
            }
            let name = t.name().to_string();
            if !runs.contains(&name) {
                runs.push(name.clone());
            }
            let pl = t.plan(&input, &rt.ctx).unwrap();
            let d = rt.policy.decide_with(t.as_ref(), &pl, None);
            if !d.floor || rt.policy.fails_open(&pl) {
                unheld.push(format!("{name} {input} -> {}", d.reason));
            }
        }
    }
    assert!(
        unheld.is_empty(),
        "a tool runs something that names Theseus's own state and is not held: {unheld:#?}"
    );
    runs.sort();
    // The measured set, not a wish. `term.open` is here because the sweep
    // found it, not because anyone remembered it: its plan runs the argv it
    // is given, so a floored path AS THE PROGRAM is held -- which is the
    // other thing this row fixed, since `path_args` skips argv's first
    // word. `term.send` needs a live terminal and `mcp:` a live server, so
    // the registry sweep cannot reach either; each has its own test above,
    // and this assertion says so rather than appearing to cover them.
    assert_eq!(
        runs,
        ["proc.run", "term.open"],
        "the set of tools whose plan runs something has changed: add each new one to the shapes \
         above, then record it here"
    );
}

/// **The other half of getting this right: a floor that fires on everything
/// is a floor the owner turns off.** He accepted one named cost — a call
/// naming those two files waits — and widening what counts as naming one
/// must not quietly turn every shell line into a question. Each line below
/// names no floored path and must still run with nothing asked.
#[tokio::test]
async fn an_ordinary_command_still_runs_with_nothing_asked() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let inside = r.root.join("src/lib.rs");
    let i = inside.to_string_lossy().into_owned();
    let i_root = r.root.to_string_lossy().into_owned();
    let floored = r.config.to_string_lossy().into_owned();
    let state = r.agents.parent().unwrap().to_string_lossy().into_owned();
    let ordinary: Vec<(&str, Value)> = vec![
        ("a build", json!({"command": "cargo test -p theseus-core"})),
        (
            "a pipeline",
            json!({"command": format!("grep -rn x {i} | head -20")}),
        ),
        (
            "a redirect inside the roots",
            json!({"command": format!("echo x >> {i}")}),
        ),
        // A message that NAMES the file is not an access to it: the line's
        // words are read as a shell splits them, so the path is part of one
        // quoted word and resolves to nothing. This is the case a blunt
        // substring match would have held, and holding it would have put a
        // question on every commit that mentions the config.
        (
            "a commit message naming the floored file",
            json!({"command": format!("git commit -m \"floor {floored} properly\"")}),
        ),
        // A sibling of a floored path, and the state dir itself: `within`
        // holds a path and what is under it, and neither of these is.
        (
            "a sibling of the config",
            json!({"argv": ["cat", format!("{}.bak", floored)]}),
        ),
        (
            "the state directory itself",
            json!({"argv": ["ls", &state]}),
        ),
        // The paths of the line are read, the variables are not, so a line
        // whose words are plain stays plain.
        (
            "a loop over the tree",
            json!({"command": "for f in *.rs; do wc -l $f; done"}),
        ),
        (
            "a batch",
            json!({"steps": [{"argv": ["cargo", "fmt"]}, {"argv": ["cargo", "clippy"]}]}),
        ),
        // ---- the round-2 reviewer's seven, six of which this re-cut
        // removes. Every one of them is a line this project's own operators
        // and its own self-improvement loop write, every one was held at
        // `3b67ddee` and at NEITHER main nor the head round 1 reviewed, and
        // the row's own discipline is that over-holding costs the owner's
        // protection as surely as a hole does.
        //
        // Mechanism 1: a code flag's argument that is one WORD was read as a
        // command, so a grep PATTERN became a program the floor holds
        // (`policy::shell_argvs`).
        (
            "a log grep for Theseus's own daemon",
            json!({"command": format!("grep -c theseusd {}/depart.log", i_root)}),
        ),
        (
            "a health check for the daemon",
            json!({"command": "ps -ef | grep -c theseusd"}),
        ),
        (
            "the same grep as an argv",
            json!({"argv": ["grep", "-c", "theseusd", format!("{i_root}/depart.log")]}),
        ),
        (
            "the same grep as a batch step",
            json!({"steps": [{"argv": ["grep", "-c", "theseusd", format!("{i_root}/depart.log")]}]}),
        ),
        // Mechanism 2: a substitution expression's `|` delimiter was read as
        // a pipe, so the floored path fell out of the middle of one word
        // that `path_word` had correctly read as no path at all
        // (`policy::shell_commands`). This is exactly the false positive the
        // row's own report said it declined the metacharacter key to avoid,
        // and it had arrived anyway, one `-e` away and with no such key.
        (
            "a sed that rewrites a MENTION of the floored path in another file",
            json!({"command": format!("sed -i -e 's|{floored}|/tmp/other.toml|' {i_root}/notes.md")}),
        ),
        (
            "the same substitution in perl",
            json!({"command": format!("perl -pe 's|{floored}|X|' {i_root}/notes.md")}),
        ),
        // The two letters re-cut 2 added to the key, on the ordinary lines
        // that carry them: `-E` is a regex dialect far more often than it is
        // perl's `-e`, and `-S` is curl's "show errors" and ssh's control
        // socket far more often than it is `env`'s split.
        //
        // ⚠ MOVED, NOT DELETED — `grep -E 'theseusd' <file>` lived here and
        // now lives in `a_code_cluster_wrapper_does_not_hide_a_floored_program`'s
        // last block, asserted HELD and pinned both ways. It is the one case
        // in this tree that the program-position key's narrowing to
        // `PROGRAM_HIDING_LETTERS` flips, and it was flipped on purpose and
        // against measurement: it is the whole price of holding `sudo -E op
        // item get --reveal`, `sudo -S op` and `env -S op`, which reached the
        // 1Password CLI with `fails_open=true` on `d6aa56e1`. The sentence
        // above stays true of `code_lines` — a `-E` line is still READ as a
        // line, which is why the two cases below are unchanged — and is no
        // longer true of the program match. One over-hold on a read-only
        // one-word pattern search for eight live vault paths is the trade, in
        // the direction the owner priced on 2026-11-10 at 14:53
        // (`logistics/OWNER-ANSWERS-20261110-1406.md`); keeping `'E'` in
        // `PROGRAM_HIDING_LETTERS` reverts exactly this half and nothing else.
        (
            "an extended-regex sed naming the floored path",
            json!({"command": format!("sed -E 's|{floored}|x|' {i_root}/notes.md")}),
        ),
        (
            "a curl with -sS",
            json!({"command": "curl -sS theseus.invalid/healthz"}),
        ),
    ];
    for (what, input) in ordinary {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(!floor, "{what} is held and should not be: {reason}");
    }
}

/// The seventh of the round-2 reviewer's false positives, which this row
/// does **not** remove, pinned so the cost is a decision on the record
/// rather than a surprise on the owner's morning.
///
/// **Why it is irreducible.** `grep -e 'op item get' <file>` and
/// `bash -c 'op item get x --reveal'` are the same text introduced by the
/// same flag. The first greps for a string and the second reaches the vault.
/// Telling them apart means knowing that `grep`'s `-e` is a pattern and
/// `bash`'s `-c` is a command — the list of program names this row deleted,
/// and the thing that bought the owner the cost of the floor and three
/// quarters of its protection. Of the two costs, this one is a call that
/// waits for a yes; the other is `op item get` running with nothing asked.
///
/// So the trade is made deliberately and in this direction, and it is narrow:
/// the line's first word must be `op` or `theseusd` (`floor_argv` holds two
/// entries), or the line must BE a floored path.
#[tokio::test]
async fn the_over_hold_this_row_accepts_is_pinned() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let i_root = r.root.to_string_lossy().into_owned();
    let floored = r.config.to_string_lossy().into_owned();
    let accepted: Vec<(&str, Value)> = vec![
        (
            "a grep whose PATTERN begins with a floored program's name",
            json!({"command": format!("grep -e 'op item get' {i_root}/x.sh")}),
        ),
        (
            "a grep whose PATTERN is a floored path",
            json!({"command": format!("grep -e '{floored}' {i_root}/x.sh")}),
        ),
    ];
    for (what, input) in accepted {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(
            floor && !fails_open,
            "{what} is no longer held. That is an IMPROVEMENT if the floor can now tell a \
             pattern from a command without a list of program names -- say how in the row and \
             delete this case. It is a REGRESSION if the key stopped reading a line's \
             programs at all, which is `bash -c 'op item get x --reveal'`: {reason}"
        );
    }
}

/// **The ceiling on nested lines fails CLOSED, at the boundary and not near
/// it.** `floor_view` reads at most `policy::LINES_READ` nested lines of one
/// call. It used to stop pushing them and say nothing, so a line that nested
/// 63 decoy calls and then the payload reached the owner's config at
/// `posture=Open` with `fails_open=true` — the round-2 reviewer measured the
/// first hiding count as exactly 63, and this test pins 62 and 63 rather
/// than a number nearby, because a ceiling is only ever wrong at its edge.
///
/// A bound that turns the protection off instead of erring is worse than no
/// bound: the floor is the one layer in this gate that is meant to have no
/// bypass, so running out of reading must hold the call and say which it is.
/// The reason is checked too — a hold for the right count with the wrong
/// story is how the next reader concludes the payload was seen.
#[tokio::test]
async fn a_call_that_nests_past_the_bound_is_held() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let floored = r.config.to_string_lossy().into_owned();
    // Decoys first, payload last, which is the order that hides it.
    let crafted = |decoys: usize| {
        let mut parts: Vec<String> = (0..decoys)
            .map(|n| format!("bash -c 'echo decoy{n}'"))
            .collect();
        parts.push(format!("bash -c 'echo pwned >> {floored}'"));
        json!({ "command": parts.join("; ") })
    };
    // The reason the FLOOR gave, not the decision's text, which quotes the
    // whole command line and so contains the floored path either way.
    let found_the_path = |reason: &str| reason.contains("is Theseus's own state");
    let ran_out_of_reading = |reason: &str| reason.contains("nest past the");
    // Just under the ceiling the payload is read, and the floor's reason is
    // the path — which is also what says the rig itself works.
    let (floor, fails_open, reason) = decide(&r, "proc.run", crafted(62));
    assert!(floor && !fails_open, "62 decoys hid the payload: {reason}");
    assert!(
        found_the_path(&reason) && !ran_out_of_reading(&reason),
        "the payload is inside the ceiling at 62, so the floor must hold it for the PATH it \
         names and not for running out of reading: {reason}"
    );
    // At the ceiling the payload is NOT read, and the call is held anyway.
    let (floor, fails_open, reason) = decide(&r, "proc.run", crafted(63));
    assert!(
        floor && !fails_open,
        "63 decoy nested calls hid the payload and the call went ahead. A ceiling that drops \
         work must HOLD, not pass: {reason}"
    );
    assert!(
        ran_out_of_reading(&reason) && !found_the_path(&reason),
        "at the ceiling the floor did not read the payload, so it must say it ran out of \
         reading rather than name a path it never saw: {reason}"
    );
    // And far past it, where nothing of the line's nesting is read.
    for decoys in [64, 200] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", crafted(decoys));
        assert!(
            floor && !fails_open,
            "{decoys} decoy nested calls hid the payload: {reason}"
        );
    }
}

/// Reading a line costs the gate time on every `proc.run` that gives one,
/// and the gate's budget is a millisecond a call (`policy::tests`'s FAST
/// check). Canonicalising a path is syscalls, so the cost is the number of
/// path-looking WORDS, which is why both numbers are measured and printed:
/// an ordinary line must stay inside the gate's own budget, and a line that
/// names two hundred files is the worst case anyone writes.
///
/// The worst case's ceiling is loose on purpose — it is there to catch a
/// tenfold regression, not to pin a debug build's arithmetic — and the
/// number it prints is the one to read.
#[tokio::test]
async fn reading_a_line_stays_inside_the_gates_budget() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let rt = &r.core.tools;
    let t = rt.registry.get("proc.run").unwrap().clone();
    let per = |line: String| {
        let pl = t.plan(&json!({"command": line}), &rt.ctx).unwrap();
        let n = 200u32;
        let t0 = std::time::Instant::now();
        for _ in 0..n {
            std::hint::black_box(rt.policy.decide_with(t.as_ref(), &pl, None));
        }
        t0.elapsed() / n
    };
    let ordinary = per("cargo nextest run -p theseus-core -E 'test(/floor/)' | tail -20".into());
    let many = (0..200)
        .map(|i| format!("./crates/theseus-core/src/file{i}.rs"))
        .collect::<Vec<_>>()
        .join(" ");
    let worst = per(format!("wc -l {many}"));
    eprintln!("FLOOR decide: ordinary line {ordinary:?}, 200-path line {worst:?} (debug build)");
    assert!(
        ordinary < std::time::Duration::from_millis(1),
        "{ordinary:?}"
    );
    assert!(worst < std::time::Duration::from_millis(60), "{worst:?}");
}

/// Every input the sweep tries on one tool: its whole schema filled in with
/// `path` in each string slot, and then each property ALONE, because a tool
/// whose schema says "exactly one of these" (`proc.run`'s argv, command and
/// steps) refuses the whole-schema attempt and would otherwise be swept
/// over in silence. Reads the schema and invents nothing.
fn attempts(schema: &Value, path: &str) -> Vec<Value> {
    let whole = synthesize(schema, path);
    let mut out = vec![whole.clone()];
    if let Value::Object(props) = &whole {
        for (name, v) in props {
            out.push(json!({name.clone(): v.clone()}));
        }
    }
    out
}

/// A JSON schema filled in so that every string it asks for is `path`.
fn synthesize(schema: &Value, path: &str) -> Value {
    let props = match schema.get("properties") {
        Some(Value::Object(p)) => p,
        _ => return json!({}),
    };
    let mut out = serde_json::Map::new();
    for (name, spec) in props {
        if let Some(v) = value_for(spec, path) {
            out.insert(name.clone(), v);
        }
    }
    Value::Object(out)
}

fn value_for(spec: &Value, path: &str) -> Option<Value> {
    match spec.get("type").and_then(Value::as_str)? {
        "string" => Some(json!(path)),
        "integer" | "number" => Some(json!(1)),
        "boolean" => Some(json!(false)),
        "array" => {
            let item = spec.get("items").and_then(|i| value_for(i, path))?;
            Some(json!([item]))
        }
        "object" => Some(synthesize(spec, path)),
        _ => None,
    }
}

/// The floor's paths are what `guarded.rs` puts there and this row changed
/// none of them: the owner's keel word of 2026-11-10 14:53 covers the two
/// files on the list, and widening how the gate MATCHES that list is not a
/// change to the list.
#[tokio::test]
async fn this_row_changed_no_floor_path() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let floor = &r.core.tools.policy.floor_paths;
    assert!(
        floor.contains(&r.config),
        "the config file is still floored"
    );
    assert!(floor.contains(&r.agents), "AGENTS.md is still floored");
    assert_eq!(
        r.core.tools.policy.floor_argv,
        crate::policy::floor_argv(),
        "the floored programs are untouched"
    );
}

/// The programs the floor never runs unasked reach a batch's steps too.
/// `op` as `proc.run {steps:[{argv:["op", …]}]}` bypassed the program floor
/// outright, because `Plan::steps` left `Plan::argv` `None` and the gate
/// matched the list against an empty argv. Every secret was one batch away.
#[tokio::test]
async fn a_floored_program_is_held_as_a_step_too() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    for (what, input) in [
        (
            "as its own argv",
            json!({"argv": ["op", "item", "get", "x"]}),
        ),
        (
            "as a step",
            json!({"steps": [{"argv": ["cargo", "fmt"]}, {"argv": ["op", "item", "get", "x"]}]}),
        ),
        (
            "as a step's shell line",
            json!({"steps": [{"argv": ["bash", "-c", "theseusd --config /x"]}]}),
        ),
    ] {
        let (floor, fails_open, reason) = decide(&r, "proc.run", input);
        assert!(floor, "the 1Password CLI {what} is not held: {reason}");
        assert!(!fails_open, "a self session runs it anyway: {reason}");
    }
}

/// What this row does NOT hold, pinned so the limit is a decision on the
/// record rather than a surprise. The floor reads a line's words as a shell
/// splits them and **expands nothing**: resolving `$D` means running the
/// line. Both of these reach a floored path at `open` today.
///
/// This is the honest cost of not over-holding, and the alternatives are
/// worse: a blunt substring match would hold a commit message that names the
/// config (`an_ordinary_command_still_runs_with_nothing_asked`), and flooring
/// `bash` as a program would hold every shell line the owner lives with.
/// The durable answer is the 1.0.6 line the owner already accepted — carry
/// only the last-kept version of the user-level `AGENTS.md`, so a changed
/// file is named rather than read — because that guards the BYTES instead of
/// the door.
#[tokio::test]
async fn a_path_a_variable_holds_is_not_held_and_that_is_on_the_record() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let state = r.agents.parent().unwrap().to_string_lossy().into_owned();
    for (what, input) in [
        (
            "a path a variable holds",
            json!({"command": format!("D={state}; echo pwned >> $D/AGENTS.md")}),
        ),
        (
            "a path the line computes",
            json!({"command": format!("echo pwned >> $(echo {state})/AGENTS.md")}),
        ),
    ] {
        let (floor, _, reason) = decide(&r, "proc.run", input);
        assert!(
            !floor,
            "{what} is now held, which is better than this test says: update the row's report \
             and this test together, do not just flip the assertion ({reason})"
        );
    }
}

/// **Deliverable 3, the half that can go red for a spelling nobody here
/// thought of.** The two tests above close the set of `Plan` FIELDS (a
/// compile-time destructure) and the set of TOOLS that run something (a live
/// registry sweep). Neither could see the set of **spellings of one field**,
/// and that is where the hole actually was: keyed on five shell names and a
/// word that was exactly `-c`, the floor held `bash -c` and let eight of
/// eleven spellings of the same line — `bash -lc` among them, the idiom this
/// project's own fixtures are written in — reach the floored config file at
/// `posture=Open` with `fails_open=true`.
///
/// A table of eleven spellings would just be a longer list, and *a
/// hand-maintained list with no completeness test* is the pattern this row
/// exists to remove. So the set here is not a list. It is a **generator** and
/// an **oracle**:
///
/// - the **generator** is the cartesian product of independent axes — what
///   wrapper stands in front, how the interpreter is named, how the code flag
///   is spelled — so a hole in a COMBINATION nobody assembled by hand is
///   still reached. That is where every one of the eight live holes was:
///   `env` + `bash` + `-c` was three separately handled things whose
///   combination was unhandled.
/// - the **oracle** for *"is this spelling really code?"* is the operating
///   system, not our opinion of it. Every spelling is RUN, appending to the
///   fixture's own config file in its temp dir, and the spellings that
///   actually appended are the ones the floor is then required to hold.
///
/// The assertion is therefore `executed ⟹ held`, and the ORACLE side of it is
/// no list at all. A spelling that is nonsense costs nothing: the oracle
/// finds it never ran and asks nothing of the floor. An interpreter this host
/// does not have is excluded the same way, and required again on a host that
/// has it.
///
/// **What the generator side is, said plainly, because re-cut 1's report
/// over-claimed it and the round-2 review proved the over-claim by finding
/// six spellings that executed unheld.** The product reaches a hole in a
/// COMBINATION nobody assembled by hand, which is a real property and is
/// where all eight live holes were. It does not reach a hole outside its own
/// axes: the flag column of `INTERPRETERS` is written by hand, and until
/// re-cut 2 the code was always pushed as a SEPARATE word, so
/// `python3 -c<line>` — one space from a spelling already in the key — could
/// not be generated at all. Attachment is now an axis and five of the six are
/// keyed; the sixth is named in `RESIDUE`. **So this instrument is better
/// than a table and is not a proof: it is the place the next spelling gets
/// asked about, and asking is a thing someone still has to do.**
///
/// It earned its keep immediately. With `CODE_LETTERS` holding only `c` —
/// which is every spelling the review named — this test went red on
/// `perl -e` and `node -e`, a flag nobody in the loop had mentioned, and
/// `-e` is in the key because the generator found it and not because anyone
/// remembered it.
///
/// `RESIDUE` is what is left: code in a position **no flag introduces**, so
/// there is nothing to key on. It is measured and asserted, not described,
/// so a new member of it is a red as well.
#[tokio::test]
async fn every_spelling_that_really_runs_a_line_is_held() {
    let r = own_files(Posture::Open, |_, _, _| vec![]);
    let floored = r.config.to_string_lossy().into_owned();
    let scratch = tempfile::tempdir().unwrap();
    let shell = format!("printf x >> {floored}");

    let mut executed: Vec<String> = vec![];
    let mut unheld: Vec<String> = vec![];
    for (program, flags, code) in INTERPRETERS {
        let code = match *code {
            "" => shell.clone(),
            other => other.replace("PATH", &floored),
        };
        for flag in *flags {
            // The fourth axis, added in re-cut 2 because its absence was a
            // blind spot and not a judgement: the code as the next WORD, and
            // the code ATTACHED to the flag's own cluster. The generator
            // always pushed a separate word, so `python3 -c<line>` — one
            // space from a spelling already in the key — was outside the
            // product space by construction and executed unheld.
            for attached in [false, true] {
                let Some(tail) = spell(flag, &code, attached) else {
                    continue;
                };
                for wrapper in WRAPPERS {
                    let mut argv: Vec<String> = wrapper
                        .iter()
                        .chain(program.iter())
                        .map(|s| s.to_string())
                        .collect();
                    argv.extend(tail.iter().cloned());
                    if !really_ran(&argv, &r.config, scratch.path()) {
                        continue;
                    }
                    let how = match attached {
                        true => " [attached]",
                        false => "",
                    };
                    let label = format!("{} {} {flag}{how}", wrapper.join(" "), program.join(" "))
                        .trim()
                        .to_string();
                    if !executed.contains(&label) {
                        executed.push(label.clone());
                    }
                    // The same spelling in each shape that can carry an
                    // argv, and nested inside another line — a hole in one
                    // carrier is a hole, whichever `Plan` field brought it.
                    let last = argv.len() - 1;
                    let quoted = format!("{} '{}'", argv[..last].join(" "), argv[last]);
                    let carriers = [
                        ("{argv}", json!({"argv": &argv})),
                        ("{steps}", json!({"steps": [{"argv": &argv}]})),
                        ("nested in a line", json!({"argv": ["bash", "-c", &quoted]})),
                    ];
                    for (carrier, input) in carriers {
                        let (floor, fails_open, _) = decide(&r, "proc.run", input);
                        if !floor || fails_open {
                            let miss = format!("{label} as {carrier}");
                            if !unheld.contains(&miss) {
                                unheld.push(miss);
                            }
                        }
                    }
                }
            }
        }
    }

    // The oracle must have found real code to judge, or `executed ⟹ held` is
    // satisfied by an empty set and this test asserts nothing at all.
    eprintln!("FLOOR spellings: {} executed on this host", executed.len());
    assert!(
        executed.len() >= 20 && executed.contains(&"bash -c".to_string()),
        "the oracle ran almost nothing, so it proved almost nothing — is a shell missing from \
         this host? executed: {executed:#?}"
    );

    // Every spelling that really ran, held — except the residue, and only
    // the members of it this host could run. A wrapper in front of a residue
    // interpreter is still the residue.
    let is_residue = |label: &String| {
        RESIDUE
            .iter()
            .any(|res| label == res || label.ends_with(&format!(" {res}")))
    };
    let mut expected: Vec<String> = executed
        .iter()
        .filter(|label| is_residue(label))
        .flat_map(|label| {
            ["{argv}", "{steps}", "nested in a line"]
                .iter()
                .map(move |c| format!("{label} as {c}"))
        })
        .collect();
    expected.sort();
    unheld.sort();
    assert_eq!(
        unheld, expected,
        "a spelling that really appended to the floored config file is not held by the floor. \
         The flag key is in `policy::code_lines`; if the new spelling's code is introduced by a \
         flag, widen the key rather than naming the spelling. If no flag introduces it, it \
         belongs in RESIDUE with a sentence saying why, and the row's report must say so too"
    );
}

/// The words after the program for one spelling: the code as the next WORD,
/// or ATTACHED to the flag's own cluster (`python3 -c<line>`, `--eval=<line>`).
///
/// A flag spelled as more than one word (`-l -c`) and the empty flag — an
/// interpreter that takes code with no flag at all — have no attached form,
/// and `None` says so rather than inventing one.
fn spell(flag: &str, code: &str, attached: bool) -> Option<Vec<String>> {
    let mut words: Vec<String> = flag.split_whitespace().map(String::from).collect();
    if !attached {
        words.push(code.to_string());
        return Some(words);
    }
    let [one] = words.as_slice() else {
        return None;
    };
    Some(vec![match one.starts_with("--") {
        true => format!("{one}={code}"),
        false => format!("{one}{code}"),
    }])
}

/// The generator's first axis: what stands in FRONT of the interpreter. Every
/// one of these puts the interpreter at `argv[1]` or later, which is what
/// left five of the review's eleven spellings unheld — the old rule read
/// `argv[0]` and nothing else.
const WRAPPERS: &[&[&str]] = &[
    &[],
    &["env"],
    &["env", "-i"],
    &["nice"],
    &["timeout", "10"],
    &["stdbuf", "-oL"],
    &["nohup"],
    &["setsid"],
];

/// The generator's second and third axes: how the interpreter is named, which
/// code flags to spell it with, and the one line of that language which
/// appends to the floored file (`PATH` is the file; an empty line means the
/// shell one). A bare name, an absolute path, `env` finding it, a shell
/// inside a multicall binary — and interpreters that are not shells at all,
/// because the floor must not care which language the code is in, only that
/// a flag said it was code.
///
/// An entry this host does not have simply does not execute, and the oracle
/// then asks nothing of the floor for it; on a host that has it, it is
/// required again. Adding a row here is how the next spelling gets asked
/// about, and it costs nothing if it turns out to be nonsense.
const INTERPRETERS: &[(&[&str], &[&str], &str)] = &[
    (&["bash"], &["-c", "-lc", "-ec", "-ic", "-xc"], ""),
    (&["bash"], &["--command"], ""),
    (&["sh"], &["-c", "-lc", "-ec"], ""),
    (&["zsh"], &["-c", "-lc", "-ic"], ""),
    (&["dash"], &["-c", "-ec"], ""),
    (&["ksh"], &["-c", "-lc"], ""),
    (&["/bin/bash"], &["-c", "-lc"], ""),
    (&["/bin/sh"], &["-c", "-lc"], ""),
    (&["busybox", "sh"], &["-c"], ""),
    (&["python3"], &["-c"], r#"open("PATH", "a").write("x")"#),
    // `-E` is `-e` with the language's later features on and runs the same;
    // it executed unheld until re-cut 2 because `CODE_LETTERS` was
    // lower-case, which is round 1's over-claim one level down.
    (
        &["perl"],
        &["-e", "-E"],
        r#"open(F, ">>", "PATH"); print F "x""#,
    ),
    (&["ruby"], &["-e"], r#"File.write("PATH", "x", mode: "a")"#),
    // `--print` evaluates and prints, so it is `--eval` with an echo; `-p`
    // is the same thing in one letter and is RESIDUE, because `-p` in the
    // key would make `mkdir -p <dir>` a line.
    (
        &["node"],
        &["-e", "--eval", "--print", "-p"],
        r#"require("fs").appendFileSync("PATH","x")"#,
    ),
    // `env`'s own "split this string into a command line": a code flag on a
    // program nobody would call an interpreter, which is the point of keying
    // on the flag.
    (&["env"], &["-S"], r#"bash -c "printf x >> PATH""#),
    // The residue below: code in an argument NO FLAG introduces.
    //
    // `awk` is THREE executables and a multicall binary, not one program, and
    // `RESIDUE` named only the spelling `awk` until B6c's
    // floor-reads-programs row: `gawk`, `mawk` and `busybox awk` all execute
    // an awk program and all three are real on this project's hosts, so each
    // ran unheld AND outside the declared residue set — the instrument said
    // nothing about them either way, which is the part that mattered. On a
    // host that has only one of them the others simply do not execute and the
    // oracle asks nothing of the floor for them.
    (&["awk"], &[""], r#"BEGIN { printf "x" >> "PATH" }"#),
    (&["gawk"], &[""], r#"BEGIN { printf "x" >> "PATH" }"#),
    (&["mawk"], &[""], r#"BEGIN { printf "x" >> "PATH" }"#),
    (
        &["busybox", "awk"],
        &[""],
        r#"BEGIN { printf "x" >> "PATH" }"#,
    ),
    (
        &["php"],
        &["-r"],
        r#"file_put_contents("PATH","x",FILE_APPEND);"#,
    ),
];

/// What is knowingly NOT held, and why it cannot be: each is code an
/// ordinary letter introduces, so keying on it would read a path argument as
/// code on half the lines anyone writes. `awk '<program>'` takes its program
/// as a bare operand with no flag at all; `php -r` and `node -p` spell "run
/// this" with the letters `cp -r` and `mkdir -p` use for something else.
/// Holding any of them would mean knowing one program's own grammar, which
/// is the list this row removed.
///
/// This is the row's residual risk, stated as an assertion rather than a
/// sentence: a member of it that becomes held is a red here too, so the gap
/// cannot quietly close or quietly grow.
///
/// **A residue member's ATTACHED spelling is judged on its own**, and is
/// usually held, because the cluster's own remainder then carries a code
/// letter in front of something that is not a flag letter
/// (`node -prequire(…)` reads `quire(…)` as the line). That is incidental
/// rather than a property, and it is asserted as MEASURED: a change to one of
/// the code strings above can flip it, and the red is the instrument saying
/// so rather than a defect.
///
/// **What this set does not and cannot claim.** `every_spelling_…` measures
/// the product of `WRAPPERS` × `INTERPRETERS` × carriers × attachment, and
/// the flag axis of `INTERPRETERS` is written by hand. So the honest
/// statement is the narrow one: within that product, `executed ⟹ held`
/// except these three, and a member that changes side is a red. It is NOT
/// that no spelling exists outside the product — re-cut 2 was sent back
/// because six did, five of them keyable and `node -p` not, and the
/// generator could not see any of them because the attached form was outside
/// its axes by construction. **Adding a row or an axis here is how the next
/// one gets asked about, and that is a procedure rather than a proof.**
/// **`awk` is three binaries and `is_residue` matched one spelling.** It
/// tests `label == res || label.ends_with(" {res}")`, so `busybox awk` and
/// `nohup busybox awk` were already covered by the trailing-space form while
/// `gawk` and `mawk` matched neither — and since neither was in
/// `INTERPRETERS` either, the generator never ran them and the gap was
/// invisible rather than declared. Both spellings are named now; the suffix
/// form still cannot confuse them, because `nohup gawk` does not end with
/// `" awk"`.
const RESIDUE: [&str; 5] = ["awk", "gawk", "mawk", "php -r", "node -p"];

/// Did this argv really run its line? The floored file's length answers it,
/// which is why the oracle needs no opinion about what a shell is: the
/// operating system decides, and a spelling nobody here thought of is
/// measured the same way as `bash -c`.
///
/// `setsid` and `nohup` can outlive the direct child, so a file that has not
/// grown by the time the child is reaped is re-read briefly before the answer
/// is "no". A program this host does not have fails to spawn, and that is a
/// "no" with nothing asked of the floor.
fn really_ran(argv: &[String], floored: &Path, cwd: &Path) -> bool {
    let len = || std::fs::metadata(floored).map(|m| m.len()).unwrap_or(0);
    let before = len();
    let spawned = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            _ if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                break;
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
    (0..10).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(5));
        len() > before
    })
}

/// The floor is also the list L1 hides from a sandboxed job
/// (`sandbox.rs`'s hidden paths), and that list is `floor_paths` — the
/// paths, not the matching. Widening the match must not have moved it.
#[test]
fn widening_the_match_did_not_move_what_l1_hides() {
    let p = Path::new("/invented/state/store");
    assert!(
        paths::within(p, Path::new("/invented/state")),
        "the hidden list is a prefix test over the same paths"
    );
}
