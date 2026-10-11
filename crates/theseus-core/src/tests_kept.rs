//! A capped result's whole output, kept and named (theseus-v73m): a job's
//! output past the result cap is kept, scrubbed, under the session's
//! outputs, the cut line names the file and the lines it left out, and
//! `fs_read` reads them without an approval; nothing else of the state dir
//! becomes readable; the sweep honours retirement, age and the total; and
//! `fs_read` of a long file returns a contiguous window, never a hole.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_tools::Tool;

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::outputs::Outputs;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::toolrun::order::At;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The value the job prints, which the scrubber knows: it must reach no
/// kept file, no node, and no spool file.
const PLANTED: &str = "invented-secret-0f9a8b7c6d5e4f3a";

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    root: PathBuf,
    state: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().canonicalize().unwrap();
    let root = state.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.approve_paths = vec![];
    cfg.tools.proc_sync_secs = 10;
    // The gate's stops are what the read of a kept output must not meet.
    cfg.policy.enforcement = Posture::Approve;
    cfg.policy.tools.insert("proc.run".into(), Posture::Open);
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = crate::rpc::Parts::for_tests(cfg, fake.clone(), store);
    p.scrubber = Arc::new(crate::scrub::Scrubber::with_values(vec![(
        PLANTED.into(),
        "demo_secret".into(),
    )]));
    let core = Core::build(p).unwrap();
    Rig {
        core,
        fake,
        root,
        state,
        _dir: dir,
    }
}

fn session(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// Each result: its tool, status, content, whether it was cut, and the file
/// it names.
fn results(core: &Core, sid: &str) -> Vec<(String, ResultStatus, String, bool, Option<String>)> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool,
                status,
                content,
                truncated,
                full_ref,
                ..
            } => Some((tool, status, content, truncated, full_ref)),
            _ => None,
        })
        .collect()
}

/// A command that prints 4,000 numbered lines with a failure in the
/// middle, and on line 10 the planted secret, from a file in the work dir
/// (the call's own input never holds it).
fn printer(root: &Path) -> serde_json::Value {
    std::fs::write(root.join("token.txt"), PLANTED).unwrap();
    json!({"argv": ["bash", "-c",
        "for i in $(seq 1 4000); do if [ $i = 2117 ]; then echo \"line $i: FAILED tide_pools::the_middle_case\"; \
         elif [ $i = 10 ]; then echo \"line $i: token $(cat token.txt)\"; else echo \"line $i: ok, a passing case of the roster\"; fi; done"
    ]})
}

/// The cut line's offset and file: `…lines A-B of the whole output, kept at
/// P; fs_read with offset=A …`.
fn cut(text: &str) -> (usize, String) {
    let at = text.find("of the whole output, kept at ").expect(text);
    let rest = &text[at + "of the whole output, kept at ".len()..];
    let path = rest[..rest.find("; fs_read").unwrap()].to_string();
    let off = &rest[rest.find("offset=").unwrap() + 7..];
    (off[..off.find(' ').unwrap()].parse().unwrap(), path)
}

#[tokio::test]
async fn a_capped_command_keeps_its_whole_scrubbed_output_and_fs_read_reads_the_middle() {
    let r = rig(vec![Scripted::text("Ran it.")]);
    r.fake
        .script
        .lock()
        .unwrap()
        .push_front(Scripted::tools("", &[("t1", "proc_run", printer(&r.root))]));
    let sid = session(&r.core);
    assert_eq!(
        turn(&r.core, &sid, "run the roster").await.output,
        "Ran it."
    );
    let (tool, status, text, truncated, full_ref) = results(&r.core, &sid).remove(0);
    assert_eq!((tool.as_str(), status), ("proc.run", ResultStatus::Ok));
    assert!(truncated, "the cap cut it");
    assert!(
        !text.contains("FAILED"),
        "the failure is in the part not shown"
    );
    let (offset, path) = cut(&text);
    let kept = r.state.join("outputs").join(&sid).join("t1.out");
    assert_eq!(path, kept.display().to_string());
    assert_eq!(
        full_ref.as_deref(),
        Some(path.as_str()),
        "the node names it"
    );
    // The cut line names what the cap left out, counted in the whole output.
    let shown_last: usize = text[..text.find("…[").unwrap()]
        .lines()
        .last()
        .and_then(|l| l.strip_prefix("line "))
        .and_then(|l| l.split(':').next())
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(offset, shown_last + 1, "{text}");
    assert!(
        text.contains(&format!("not shown: they are lines {offset}-")),
        "{text}"
    );
    // The whole output, scrubbed.
    let whole = std::fs::read_to_string(&kept).unwrap();
    assert_eq!(whole.lines().count(), 4000);
    assert_eq!(
        whole.lines().nth(2116),
        Some("line 2117: FAILED tide_pools::the_middle_case")
    );
    assert_eq!(
        whole.lines().nth(9),
        Some("line 10: token [redacted:demo_secret]")
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&kept).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // fs_read at the cut line's offset reads the middle, with no approval.
    r.fake.script.lock().unwrap().extend([
        Scripted::tools(
            "",
            &[(
                "t2",
                "fs_read",
                json!({"path": path, "offset": 2110, "limit": 20}),
            )],
        ),
        Scripted::text("Found it."),
    ]);
    assert_eq!(
        turn(&r.core, &sid, "what failed?").await.output,
        "Found it."
    );
    let (tool, status, read, _, _) = results(&r.core, &sid).remove(1);
    assert_eq!(
        (tool.as_str(), status),
        ("fs.read", ResultStatus::Ok),
        "{read}"
    );
    assert!(
        read.contains("  2117\tline 2117: FAILED tide_pools::the_middle_case\n"),
        "{read}"
    );
    // The planted secret is in no kept file, no node, and no spool file:
    // nothing under the state dir but the work dir that holds it.
    for f in files(&r.state)
        .into_iter()
        .filter(|f| !f.starts_with(&r.root))
    {
        let bytes = std::fs::read(&f).unwrap();
        assert!(
            !bytes
                .windows(PLANTED.len())
                .any(|w| w == PLANTED.as_bytes()),
            "{} holds the secret",
            f.display()
        );
    }
}

/// Every file under `dir`.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// The gate's posture for an `fs_read` of `path` in `sid`, as the turn's
/// gate judges it.
fn posture(core: &Core, sid: &str, path: &Path) -> String {
    let tool = theseus_tools::fs::Read;
    let input = json!({"path": path});
    let plan = tool.plan(&input, &core.tools.ctx).unwrap();
    let held = || crate::external::held(&core.store, sid);
    let at = At {
        place: core.runner.view_of(sid),
        held: &held,
        mcp: &|| None,
        glide: None,
        session: Some(sid),
    };
    let (d, _) = core
        .tools
        .judge(&at, &tool, &plan, &input, &mut |_, _| {})
        .unwrap();
    d.posture.as_str().into()
}

#[tokio::test]
async fn only_the_sessions_own_outputs_become_readable() {
    let r = rig(vec![]);
    let (mine, other) = (session(&r.core), session(&r.core));
    let out = r.state.join("outputs");
    assert_eq!(
        posture(&r.core, &mine, &out.join(&mine).join("t1.out")),
        "open"
    );
    // Another session's, the outputs folder itself, a climb out of the
    // session's folder, and the store all wait, as any path outside the
    // roots does.
    for p in [
        out.join(&other).join("t1.out"),
        out.join("t1.out"),
        out.join(&mine).join("..").join(&other).join("t1.out"),
        r.state.join("store").join("index.redb"),
    ] {
        assert_eq!(posture(&r.core, &mine, &p), "approve", "{}", p.display());
    }
    // Inside the roots, as before.
    assert_eq!(posture(&r.core, &mine, &r.root.join("a.rs")), "open");
}

/// A file of `n` numbered lines of about 100 characters.
fn source(root: &Path, name: &str, n: usize) {
    let text: String = (1..=n)
        .map(|i| format!("// line {i:05} of the tide tables {}\n", "~".repeat(64)))
        .collect();
    std::fs::write(root.join(name), text).unwrap();
}

#[tokio::test]
async fn fs_read_of_a_long_file_is_a_contiguous_window_and_other_tools_keep_both_ends() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[
                ("t1", "fs_read", json!({"path": "mid.rs"})),
                ("t2", "fs_read", json!({"path": "big.rs"})),
                (
                    "t3",
                    "fs_grep",
                    json!({"pattern": "tide", "path": "big.rs", "max_results": 500}),
                ),
            ],
        ),
        Scripted::text("Read."),
    ]);
    source(&r.root, "mid.rs", 600);
    source(&r.root, "big.rs", 3_000);
    let sid = session(&r.core);
    assert_eq!(turn(&r.core, &sid, "read them").await.output, "Read.");
    let got = results(&r.core, &sid);
    let numbers = |t: &str| -> Vec<usize> {
        t.lines()
            .filter_map(|l| l.split_once('\t'))
            .filter_map(|(n, _)| n.trim().parse().ok())
            .collect()
    };
    // 60 KB: whole.
    let (_, _, mid, cut, _) = &got[0];
    assert_eq!(numbers(mid), (1..=600).collect::<Vec<_>>());
    assert!(!cut && mid.chars().count() > 60_000);
    // 300 KB: a window from line 1, contiguous, with its footer.
    let (_, _, big, cut, _) = &got[1];
    let shown = numbers(big);
    let last = *shown.last().unwrap();
    assert_eq!(shown, (1..=last).collect::<Vec<_>>(), "no hole");
    assert!(!cut && big.chars().count() <= 100_000 && last > 900);
    assert!(
        big.ends_with(&format!(
            "[showing lines 1-{last} of 3000; pass offset={} to read on]\n",
            last + 1
        )),
        "{}",
        &big[big.len() - 300..]
    );
    // Another tool keeps head and tail at 30,000, and keeps no file.
    let (tool, _, grep, cut, full_ref) = &got[2];
    assert_eq!(tool, "fs.grep");
    assert!(
        *cut && full_ref.is_none(),
        "{} {cut} {full_ref:?}: {}",
        grep.chars().count(),
        &grep[grep.len().saturating_sub(300)..]
    );
    assert!(grep.chars().count() <= 30_200, "{}", grep.chars().count());
    assert!(
        grep.contains("line 00001 ") && grep.contains("line 00500 "),
        "both ends"
    );
    assert!(grep.contains(" not shown"));
}

/// Set a file's time of last write.
fn aged(p: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[test]
fn the_sweep_honours_retirement_age_and_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let day = Duration::from_secs(24 * 60 * 60);
    // A week's keep and 100 bytes in all.
    let o = Outputs::new(dir.path().to_path_buf(), 7, 100);
    let put = |s: &str, c: &str, len: usize, age: Duration| {
        let p = o.path_for(s, c).unwrap();
        o.keep_text(&"x".repeat(len), &p).unwrap();
        aged(&p, now - age);
        p
    };
    let retired = put("ses_retired", "c1", 10, Duration::ZERO);
    let old = put("ses_live", "c1", 10, day * 8);
    let oldest = put("ses_live", "c2", 40, day * 3);
    let middle = put("ses_live", "c3", 40, day * 2);
    let newest = put("ses_other", "c1", 40, day);
    let s = o.sweep(&|sid| sid == "ses_retired", now);
    assert!(!retired.exists(), "its session retired");
    assert!(
        !retired.parent().unwrap().exists(),
        "its empty folder went too"
    );
    assert!(!old.exists(), "past the keep");
    assert!(!oldest.exists(), "the oldest went first, past the total");
    assert!(middle.exists() && newest.exists());
    assert_eq!(
        (s.removed, s.removed_bytes, s.kept, s.kept_bytes),
        (3, 60, 2, 80)
    );
}

#[tokio::test]
async fn the_cores_sweep_reads_a_retired_session_from_the_store() {
    let r = rig(vec![]);
    let (live, gone) = (session(&r.core), session(&r.core));
    let o = &r.core.tools.outputs;
    let (a, b) = (
        o.path_for(&live, "t1").unwrap(),
        o.path_for(&gone, "t1").unwrap(),
    );
    o.keep_text("kept", &a).unwrap();
    o.keep_text("kept", &b).unwrap();
    let mut rec: SessionRecord = r.core.store.get_session(&gone).unwrap().unwrap();
    rec.turns = 1;
    rec.retired = Some(theseus_protocol::SessionRetired {
        reason: theseus_protocol::RetiredReason::ByHand,
        at_ms: theseus_protocol::now_unix_ms(),
    });
    r.core.store.put_session(&gone, &rec).unwrap();
    let mut rec: SessionRecord = r.core.store.get_session(&live).unwrap().unwrap();
    rec.turns = 1;
    r.core.store.put_session(&live, &rec).unwrap();
    let s = r.core.sweep_outputs();
    assert!(a.exists() && !b.exists(), "{s:?}");
}

/// The config's two keys, with their defaults.
#[test]
fn the_outputs_keys_default_to_a_week_and_two_gib() {
    let t = crate::config::ToolsConfig::default();
    assert_eq!((t.outputs_keep_days, t.outputs_max_bytes), (7, 2 << 30));
    let cfg: crate::config::ToolsConfig =
        toml::from_str("outputs_keep_days = 2\noutputs_max_bytes = 1024\n").unwrap();
    assert_eq!((cfg.outputs_keep_days, cfg.outputs_max_bytes), (2, 1024));
}
