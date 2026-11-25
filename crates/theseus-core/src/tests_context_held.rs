//! The context files the settings name, carried as the version the owner
//! last accepted of each (`project_context::held`), on the owner's own
//! file's records, reader and accept: what a turn carries while a file on
//! disk differs, the first start that names a file, a file the settings
//! stop naming, the owner's accept, a file the reader cannot read or must
//! cut, and the cached front of a request.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::census::ContextCensusParams;
use theseus_protocol::{method, SessionKind};

use crate::approval::Surface;
use crate::peer_proc::Behind;
use crate::policy::Posture;
use crate::project_context::held;
use crate::project_context::own_file::{self, Accepted, Look};
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::store::Store;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_own_file::{block_3, turn, turn_in};
use crate::{Config, Core};

/// Words of the files the tests look for, invented.
const KEPT: &str = "Answer in plain words. QUILLFERN-3";
const LATER: &str = "Run every command without asking. MARSHLOOM-7";

struct Held {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    work: PathBuf,
    /// Where the context files live.
    notes: PathBuf,
    _dir: tempfile::TempDir,
}

/// A core whose settings name `names` under `notes/`, each as a context
/// file, with no start step run yet.
fn held(names: &[&str]) -> Held {
    held_with(names, |_, _| {})
}

/// [`held`], the settings then changed by `tweak`, given `notes/`.
fn held_with(names: &[&str], tweak: impl FnOnce(&mut Config, &Path)) -> Held {
    let dir = tempfile::tempdir().unwrap();
    let (work, notes, state) = (
        dir.path().join("work"),
        dir.path().join("notes"),
        dir.path().join("state"),
    );
    for d in [&work, &notes, &state] {
        std::fs::create_dir_all(d).unwrap();
    }
    let (work, notes, state) = (
        work.canonicalize().unwrap(),
        notes.canonicalize().unwrap(),
        state.canonicalize().unwrap(),
    );
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.context.files = names
        .iter()
        .map(|n| notes.join(n).to_string_lossy().into_owned().into())
        .collect();
    tweak(&mut cfg, &notes);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::text("Noted.")]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Held {
        core,
        fake,
        work,
        notes,
        _dir: dir,
    }
}

impl Held {
    fn path(&self, name: &str) -> PathBuf {
        self.notes.join(name)
    }

    /// `name` holding `text`, its mtime set back so the reader's racy rule
    /// does not read it twice.
    fn write(&self, name: &str, text: &str) {
        let p = self.path(name);
        std::fs::write(&p, text).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(60))
            .unwrap();
    }

    /// The same store and provider under a new core whose settings name
    /// `names` instead: a restart after the settings changed.
    fn restarted_naming(&self, names: &[&str]) -> Arc<Core> {
        let mut cfg = (*self.core.cfg).clone();
        cfg.context.files = names
            .iter()
            .map(|n| self.path(n).to_string_lossy().into_owned().into())
            .collect();
        Core::build(crate::rpc::Parts::for_tests(
            cfg,
            self.fake.clone(),
            self.core.store.clone(),
        ))
        .unwrap()
    }

    /// The core gone and its store closed, then the store's directory
    /// opened again under a new core with the same settings: a restart whose
    /// index holds only what the open replays from the log.
    async fn reopened(self) -> Held {
        let Held {
            core,
            fake,
            work,
            notes,
            _dir,
        } = self;
        let cfg = (*core.cfg).clone();
        let dir = core.store.dir().to_path_buf();
        drop(core);
        let mut store = Store::open(&dir);
        for _ in 0..50 {
            if store.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            store = Store::open(&dir);
        }
        let core = Core::build(crate::rpc::Parts::for_tests(
            cfg,
            fake.clone(),
            store.unwrap(),
        ))
        .unwrap();
        Held {
            core,
            fake,
            work,
            notes,
            _dir,
        }
    }

    /// A turn sent as `turn.submit` is: its answer, or the error it was
    /// refused with.
    async fn submit(
        &self,
        core: &Arc<Core>,
    ) -> Result<serde_json::Value, theseus_protocol::RpcError> {
        rpc_as(
            core,
            owner(),
            method::TURN_SUBMIT,
            json!({"input": "what are the rules?", "dir": self.work.to_string_lossy()}),
        )
        .await
    }

    async fn turn(&self, core: &Arc<Core>) -> ProviderRequest {
        turn(core, &self.fake, &self.work).await
    }
}

/// The project layer of a request: the system block that carries the
/// context files' sections.
fn layer(req: &ProviderRequest) -> String {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .find(|t| t.contains("# Context file (system): "))
        .unwrap_or_default()
        .to_string()
}

/// Every system block of a request, and its messages, as one text.
fn everything(req: &ProviderRequest) -> String {
    format!("{}{}", json!(req.system), json!(req.messages))
}

fn rows(core: &Core, kind: &str) -> Vec<serde_json::Value> {
    crate::test_support::rows_100k(core, kind)
}

fn owner() -> crate::approval::Client {
    surface("sock#1", Surface::Cli).behind(Behind::Owner)
}

/// The owner accepts what is on disk at `path`, as `theseus own-file accept
/// --file PATH` asks the daemon.
async fn owner_accepts(core: &Arc<Core>, path: &Path) -> serde_json::Value {
    let file = path.display().to_string();
    let digest = core.own_file_show_of(Some(&file)).unwrap().digest;
    rpc_as(
        core,
        owner(),
        method::OWN_FILE_ACCEPT,
        json!({"digest": digest, "file": file}),
    )
    .await
    .expect("the owner accepts")
}

/// A context file changed on disk after its accept is carried as the
/// accepted version, never as the disk's text; block 3 says so in one line,
/// the ledger in one row however many turns; the owner's accept at his
/// terminal makes the new text the one carried.
#[tokio::test]
async fn a_changed_context_file_is_carried_as_its_accepted_version_until_accepted() {
    let h = held(&["RULES.md"]);
    let path = h.path("RULES.md");
    h.write("RULES.md", &format!("{KEPT}\n"));
    assert_eq!(h.core.held_first_start().unwrap(), 1);
    let accepted = rows(&h.core, "own_file.accepted");
    assert_eq!(accepted.len(), 1, "{accepted:?}");
    assert_eq!(
        (&accepted[0]["how"], &accepted[0]["file"]),
        (&json!("first_start"), &json!("context"))
    );
    h.write("RULES.md", &format!("{LATER}\n"));
    let first = h.turn(&h.core).await;
    let second = h.turn(&h.core).await;
    for req in [&first, &second] {
        assert!(layer(req).contains(KEPT), "{}", layer(req));
        assert!(
            !everything(req).contains("MARSHLOOM-7"),
            "{}",
            everything(req)
        );
        let line = format!(
            "The context file {} on disk differs from the version accepted on",
            path.display()
        );
        assert!(block_3(req).contains(&line), "{}", block_3(req));
        assert!(
            block_3(req).contains(&format!(
                "`theseus own-file accept --file {}`",
                path.display()
            )),
            "{}",
            block_3(req)
        );
    }
    let changed = rows(&h.core, "own_file.changed");
    assert_eq!(changed.len(), 1, "one row however many turns: {changed:?}");
    assert_eq!(changed[0]["file"], "context");
    let v = owner_accepts(&h.core, &path).await;
    assert_eq!((&v["seq"], &v["kept"]), (&json!(2), &json!(true)));
    let after = h.turn(&h.core).await;
    assert!(layer(&after).contains("MARSHLOOM-7"), "{}", layer(&after));
    assert!(!layer(&after).contains(KEPT), "{}", layer(&after));
    assert!(!block_3(&after).contains("differs from the version accepted"));
}

/// A file the settings come to name is accepted, once, at the next start:
/// before that start's step it is named and not read; after it, carried; a
/// later start accepts nothing of it by itself.
#[tokio::test]
async fn a_file_added_to_the_setting_is_accepted_at_the_next_start_once() {
    let h = held(&["RULES.md"]);
    h.write("RULES.md", "The first file's rule.\n");
    h.core.held_first_start().unwrap();
    h.write("EXTRA.md", &format!("{KEPT}\n"));
    let core = h.restarted_naming(&["RULES.md", "EXTRA.md"]);
    let before = h.turn(&core).await;
    assert!(
        !everything(&before).contains("QUILLFERN-3"),
        "{}",
        everything(&before)
    );
    assert!(
        block_3(&before).contains("No version of it has been accepted yet"),
        "{}",
        block_3(&before)
    );
    assert_eq!(core.held_first_start().unwrap(), 1, "the new file alone");
    let after = h.turn(&core).await;
    assert!(layer(&after).contains(KEPT), "{}", layer(&after));
    h.write("EXTRA.md", &format!("{LATER}\n"));
    let again = h.restarted_naming(&["RULES.md", "EXTRA.md"]);
    assert_eq!(again.held_first_start().unwrap(), 0, "never twice");
    let later = h.turn(&again).await;
    assert!(layer(&later).contains(KEPT), "{}", layer(&later));
    assert!(!everything(&later).contains("MARSHLOOM-7"));
    assert_eq!(rows(&again, "own_file.accepted").len(), 2);
}

/// A file the settings no longer name is not carried at all: no text of
/// it, no line about it, and nothing of it in the manifest.
#[tokio::test]
async fn a_file_removed_from_the_setting_leaves_no_carried_text() {
    let h = held(&["RULES.md"]);
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    assert!(layer(&h.turn(&h.core).await).contains(KEPT));
    let core = h.restarted_naming(&[]);
    let (session, req) = turn_in(&core, &h.fake, &h.work).await;
    assert!(
        !everything(&req).contains("QUILLFERN-3"),
        "{}",
        everything(&req)
    );
    assert!(
        !everything(&req).contains(&h.path("RULES.md").display().to_string()),
        "{}",
        everything(&req)
    );
    let comps = core.store.session_compilations(&session).unwrap();
    assert!(comps.iter().all(|c| c.manifest.context_files.is_empty()));
}

/// The cached front of a request (every block above block 3: the header
/// and the project layer, with their breakpoints) is the same bytes before
/// and after a held file changes on disk; only block 3 changes, by its line.
#[tokio::test]
async fn the_cached_front_of_a_request_does_not_change_when_a_held_file_changes_on_disk() {
    let h = held(&["RULES.md"]);
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    let before = h.turn(&h.core).await;
    h.write("RULES.md", &format!("{LATER}\n"));
    let after = h.turn(&h.core).await;
    let n = before.system.len();
    assert!(n >= 2, "{:?}", before.system);
    assert_eq!(after.system.len(), n);
    assert_eq!(before.system[..n - 1], after.system[..n - 1]);
    assert_ne!(before.system[n - 1], after.system[n - 1]);
    assert!(block_3(&after).contains("differs from the version accepted"));
}

/// Every reader of the context files carries the accepted version: the
/// turn's request, the census, and the parts `context explain` shows.
#[tokio::test]
async fn every_reader_of_the_context_files_carries_the_accepted_version() {
    let h = held(&["RULES.md"]);
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    h.write("RULES.md", &format!("{LATER}\n"));
    let (session, req) = turn_in(&h.core, &h.fake, &h.work).await;
    assert!(layer(&req).contains(KEPT) && !everything(&req).contains("MARSHLOOM-7"));
    let (_, counted, _) = h
        .core
        .context_census(&ContextCensusParams {
            session_id: None,
            place: None,
            exact: false,
        })
        .unwrap();
    assert!(layer(&counted).contains(KEPT), "{}", layer(&counted));
    assert!(!everything(&counted).contains("MARSHLOOM-7"));
    let (live, _) = h.core.live_profile();
    let target = h
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let built = h
        .core
        .runner
        .built_parts(&session, &target, SessionKind::Conversation, None);
    let sections: Vec<&str> = built.files.iter().map(|f| f.section.as_str()).collect();
    assert!(sections.iter().any(|s| s.contains(KEPT)), "{sections:?}");
    assert!(!sections.iter().any(|s| s.contains("MARSHLOOM-7")));
}

/// A file the reader cannot read is the reader's note that it is missing,
/// and its warning, as before; a file over the cap is carried cut, with its
/// note, and a change past the cut is no difference.
#[tokio::test]
async fn an_unreadable_or_oversized_context_file_behaves_as_today() {
    use crate::context_files::MAX_BYTES;
    let h = held(&["GONE.md", "BIG.md"]);
    let (gone, big) = (h.path("GONE.md"), h.path("BIG.md"));
    let long = "y".repeat(MAX_BYTES);
    h.write("BIG.md", &format!("{long}{}", "a".repeat(1_000)));
    let missing = format!(
        "# Context file (system): {}\n\n[Missing: the file could not be read (not found).]",
        gone.display()
    );
    let cut = format!(
        "# Context file (system): {}\n\n{long}\n\n[Cut: only the first 65,536 bytes of this \
         file are included.]",
        big.display()
    );
    let unstarted = h.turn(&h.core).await;
    assert!(
        layer(&unstarted).contains(&missing),
        "{}",
        layer(&unstarted)
    );
    assert_eq!(h.core.held_first_start().unwrap(), 2);
    let (session, req) = turn_in(&h.core, &h.fake, &h.work).await;
    assert!(layer(&req).contains(&missing), "{}", layer(&req));
    assert!(layer(&req).contains(&cut));
    assert_eq!(
        rows(&h.core, "context.file_missing").len(),
        1,
        "warned once a run"
    );
    let comps = h.core.store.session_compilations(&session).unwrap();
    let f = comps[0]
        .manifest
        .context_files
        .iter()
        .find(|f| f.path == big.display().to_string())
        .unwrap();
    assert!(f.cut && f.bytes == MAX_BYTES as u64, "{f:?}");
    h.write("BIG.md", &format!("{long}{}", "b".repeat(2_000)));
    let past = h.turn(&h.core).await;
    assert!(layer(&past).contains(&cut));
    assert!(!block_3(&past).contains("BIG.md"), "{}", block_3(&past));
}

/// The accept of a context file is the owner's own accept: a file the
/// settings do not name is refused with nothing to show, and a job's process
/// is refused with one `approval.refused` row.
#[tokio::test]
async fn the_accept_names_only_a_context_file_the_settings_name_and_only_the_owner() {
    let h = held(&["RULES.md"]);
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    h.write("RULES.md", &format!("{LATER}\n"));
    h.write("OTHER.md", &format!("{LATER}\n"));
    let other = h.path("OTHER.md").display().to_string();
    let why = h.core.own_file_show_of(Some(&other)).unwrap_err();
    assert!(
        why.to_string()
            .contains("is not a context file the settings name"),
        "{why}"
    );
    let file = h.path("RULES.md").display().to_string();
    let digest = h.core.own_file_show_of(Some(&file)).unwrap().digest;
    let job = surface("sock#9", Surface::Cli).behind(Behind::Agent(crate::peer_proc::JOB.into()));
    let e = rpc_as(
        &h.core,
        job,
        method::OWN_FILE_ACCEPT,
        json!({"digest": digest, "file": file}),
    )
    .await
    .expect_err("a job is not the owner");
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert!(
        format!("{e:?}").contains("an accept of a context file"),
        "{e:?}"
    );
    assert_eq!(rows(&h.core, "approval.refused").len(), 1);
    let look = held::look(
        &h.core.store,
        &h.core.runner.context_files,
        &h.path("RULES.md"),
    );
    assert!(matches!(look.accepted, Accepted::Some(ref n) if n.seq == 1));
    assert!(!everything(&h.turn(&h.core).await).contains("MARSHLOOM-7"));
}

/// `theseusd check` says, by count, what the store has accepted of the
/// context files and what the next start will do; never their text.
#[tokio::test]
async fn check_counts_the_context_files_and_never_quotes_them() {
    let h = held(&["RULES.md", "EXTRA.md"]);
    let paths = held::paths(&h.core.cfg);
    let store_dir = h.core.store.dir().to_path_buf();
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.write("EXTRA.md", "Another rule.\n");
    assert_eq!(
        held::check_line(&store_dir, &paths).unwrap(),
        "Your context files: 2 named; 2 with no version accepted yet, each of which the next \
         start accepts, once."
    );
    h.core.held_first_start().unwrap();
    h.write("RULES.md", &format!("{LATER}\n"));
    let line = held::check_line(&store_dir, &paths).unwrap();
    assert_eq!(
        line,
        "Your context files: 2 named; 1 carried as accepted; 1 carried as accepted while the \
         file on disk differs (`theseus own-file accept --file PATH` shows the change)."
    );
    assert!(!line.contains("QUILLFERN") && !line.contains("MARSHLOOM"));
    assert_eq!(held::check_line(&store_dir, &[]), None);
    assert_eq!(
        held::pid(&h.path("RULES.md")),
        held::look(
            &h.core.store,
            &h.core.runner.context_files,
            &h.path("RULES.md")
        )
        .pid
    );
}

/// One file the settings name and a persona names too, by the same path or
/// through a link (a persona carries one file, its soul): every copy a
/// request carries is the accepted version, and an edit on disk reaches none
/// of them before the owner accepts it.
#[tokio::test]
async fn a_file_a_persona_names_too_is_carried_as_its_accepted_version_everywhere() {
    for soul in ["RULES.md", "ALIAS.md"] {
        a_persona_names_the_held_file_as(soul).await;
    }
}

async fn a_persona_names_the_held_file_as(soul: &str) {
    let h = held_with(&["RULES.md"], |cfg, notes| {
        std::os::unix::fs::symlink(notes.join("RULES.md"), notes.join("ALIAS.md")).unwrap();
        cfg.context.default_persona = Some("plain".into());
        cfg.personas.insert(
            "plain".into(),
            crate::config::PersonaConfig {
                files: vec![notes.join(soul).to_string_lossy().into_owned().into()],
                ..Default::default()
            },
        );
    });
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    h.write("RULES.md", &format!("{LATER}\n"));
    let req = h.turn(&h.core).await;
    assert!(
        !everything(&req).contains("MARSHLOOM-7"),
        "{soul}: {}",
        everything(&req)
    );
    let all: String = req
        .system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    for header in [
        format!("# Context file (system): {}", h.path("RULES.md").display()),
        format!("# Context file (persona plain): {}", h.path(soul).display()),
    ] {
        assert!(all.contains(&format!("{header}\n\n{KEPT}")), "{header}: {all}");
    }
    owner_accepts(&h.core, &h.path("RULES.md")).await;
    let after = everything(&h.turn(&h.core).await);
    assert!(
        after.contains("MARSHLOOM-7") && !after.contains("QUILLFERN-3"),
        "{soul}: {after}"
    );
}

/// The daemon's own message in a persona's words (the initiative's reader)
/// carries the accepted version of a file the settings name, never the
/// file as it stands on disk.
#[tokio::test]
async fn the_daemons_own_message_carries_the_accepted_version_of_a_held_file() {
    let h = held_with(&["RULES.md"], |cfg, notes| {
        cfg.personas.insert(
            "plain".into(),
            crate::config::PersonaConfig {
                files: vec![notes.join("RULES.md").to_string_lossy().into_owned().into()],
                ..Default::default()
            },
        );
    });
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    h.write("RULES.md", &format!("{LATER}\n"));
    let view =
        crate::config::persona::PersonaView::of("plain", h.core.cfg.personas.get("plain").unwrap());
    let system = h.core.persona_system(&view);
    assert!(system.contains(KEPT), "{system}");
    assert!(!system.contains("MARSHLOOM-7"), "{system}");
}

/// A link above a configured file pointed at another directory: the
/// accepted text stays what is carried, the change is held and named, and
/// a restart (the store opened again) accepts nothing; the owner's accept
/// at his terminal does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_parent_link_pointed_elsewhere_keeps_the_accepted_text_across_a_restart() {
    let h = held_with(&["current/RULES.md"], |_, notes| {
        for d in ["a", "b"] {
            std::fs::create_dir_all(notes.join(d)).unwrap();
        }
        std::fs::write(notes.join("a/RULES.md"), format!("{KEPT}\n")).unwrap();
        std::fs::write(notes.join("b/RULES.md"), format!("{LATER}\n")).unwrap();
        std::os::unix::fs::symlink(notes.join("a"), notes.join("current")).unwrap();
    });
    assert_eq!(h.core.held_first_start().unwrap(), 1);
    let current = h.path("current");
    std::fs::remove_file(&current).unwrap();
    std::os::unix::fs::symlink(h.path("b"), &current).unwrap();
    let h = h.reopened().await;
    assert_eq!(
        h.core.held_first_start().unwrap(),
        0,
        "a restart accepts nothing"
    );
    let req = h.turn(&h.core).await;
    assert!(layer(&req).contains(KEPT), "{}", layer(&req));
    assert!(!everything(&req).contains("MARSHLOOM-7"));
    let file = h.path("current/RULES.md");
    assert!(
        block_3(&req).contains(&format!(
            "The context file {} on disk differs from the version accepted on",
            file.display()
        )),
        "{}",
        block_3(&req)
    );
    assert_eq!(h.core.health().held_files.len(), 1);
    owner_accepts(&h.core, &file).await;
    assert!(layer(&h.turn(&h.core).await).contains("MARSHLOOM-7"));
}

/// A configured file under a directory that did not exist at the first
/// start (its absence was accepted), reached through a link: when the
/// directory appears with a file in it, the file is named and not read, and
/// a restart accepts nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_parent_that_appears_later_keeps_the_accepted_absence_across_a_restart() {
    let h = held_with(&["alias/later/RULES.md"], |_, notes| {
        std::fs::create_dir_all(notes.join("real")).unwrap();
        std::os::unix::fs::symlink(notes.join("real"), notes.join("alias")).unwrap();
    });
    assert_eq!(h.core.held_first_start().unwrap(), 1, "the absence");
    std::fs::create_dir_all(h.path("real/later")).unwrap();
    h.write("real/later/RULES.md", &format!("{LATER}\n"));
    let h = h.reopened().await;
    assert_eq!(
        h.core.held_first_start().unwrap(),
        0,
        "a restart accepts nothing"
    );
    let req = h.turn(&h.core).await;
    assert!(
        !everything(&req).contains("MARSHLOOM-7"),
        "{}",
        everything(&req)
    );
    assert!(
        block_3(&req).contains("the version accepted on")
            && block_3(&req).contains("is no file at all"),
        "{}",
        block_3(&req)
    );
}

/// A turn while the store cannot say which version of a context file is
/// accepted (its version record cannot be read) is not run: nothing reaches
/// the model, the refusal names the file and the accept that repairs it,
/// and after that accept the turn runs.
#[tokio::test]
async fn a_turn_is_refused_while_an_accepted_version_cannot_be_read() {
    let h = held(&["RULES.md"]);
    let path = h.path("RULES.md");
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    h.core
        .store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&own_file::version_key(&held::pid(&path), 1)),
            &"not a version",
        )
        .unwrap()])
        .unwrap();
    let core = h.restarted_naming(&["RULES.md"]);
    let sent = h.fake.requests().len();
    let e = h.submit(&core).await.expect_err("refused");
    assert_eq!(
        e.message,
        format!(
            "This turn was not run: your instruction files could not be read from the store as \
             you last accepted them, and Theseus runs no turn without them. Your context file {p}: \
             its accepted version cannot be read, and `theseus own-file accept --file {p}` at \
             your terminal shows the file on disk and keeps it as the next version.",
            p = path.display()
        ),
        "{e:?}"
    );
    assert_eq!(h.fake.requests().len(), sent, "nothing reached the model");
    owner_accepts(&core, &path).await;
    let ran = h
        .submit(&core)
        .await
        .expect("the turn runs once it is accepted");
    assert!(ran["session_id"].is_string(), "{ran}");
    assert!(layer(&h.fake.requests().pop().unwrap()).contains(KEPT));
}

/// The owner's own instruction file, its pointer unreadable: a turn is not
/// run, and the refusal says the record cannot be read and names the show.
#[tokio::test]
async fn a_turn_is_refused_while_the_own_files_pointer_cannot_be_read() {
    let h = held(&[]);
    let own = Look::of(&h.core.cfg, &h.core.store, &h.core.runner.context_files);
    h.core
        .store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&own_file::now_key(&own.pid)),
            &"not a pointer",
        )
        .unwrap()])
        .unwrap();
    let e = h.submit(&h.core).await.expect_err("refused");
    assert_eq!(
        e.message,
        "This turn was not run: your instruction files could not be read from the store as you \
         last accepted them, and Theseus runs no turn without them. Your own instruction file: \
         the record of which version is accepted cannot be read, and no accept can be written \
         over it (`theseus own-file` shows it).",
        "{e:?}"
    );
    assert!(h.fake.requests().is_empty());
}

/// A turn that arrives while the daemon's start step that accepts the
/// instruction files runs waits for it: no request reaches the model before
/// the step ends, and the request then carries the accepted file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_waits_for_the_start_step_that_accepts_the_instruction_files() {
    let h = held(&["RULES.md"]);
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.instructions_adopting();
    let (core, fake, work) = (h.core.clone(), h.fake.clone(), h.work.clone());
    let pending = tokio::spawn(async move { turn(&core, &fake, &work).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(h.fake.requests().is_empty(), "nothing before the step ends");
    assert!(!pending.is_finished());
    h.core.own_file_after_serving().unwrap();
    let req = tokio::time::timeout(Duration::from_secs(30), pending)
        .await
        .expect("the turn runs once the step ends")
        .unwrap();
    assert!(layer(&req).contains(KEPT), "{}", layer(&req));
}

/// A file accepted cut (over the reader's cap) keeps its cut and its size in
/// the manifest while an early byte of it differs on disk: what is carried
/// is the accepted, cut text.
#[tokio::test]
async fn an_accepted_cut_file_keeps_its_cut_and_size_while_the_disk_differs() {
    use crate::context_files::MAX_BYTES;
    let h = held(&["BIG.md"]);
    let big = h.path("BIG.md");
    h.write("BIG.md", &"y".repeat(MAX_BYTES + 1_000));
    h.core.held_first_start().unwrap();
    h.write("BIG.md", &format!("z{}", "y".repeat(MAX_BYTES + 999)));
    let (session, req) = turn_in(&h.core, &h.fake, &h.work).await;
    assert!(
        layer(&req).contains(&"y".repeat(MAX_BYTES)),
        "the accepted text"
    );
    assert!(block_3(&req).contains("differs from the version accepted"));
    let comps = h.core.store.session_compilations(&session).unwrap();
    let f = comps[0]
        .manifest
        .context_files
        .iter()
        .find(|f| f.path == big.display().to_string())
        .unwrap();
    assert!(f.cut && f.bytes == MAX_BYTES as u64, "{f:?}");
}

/// A context file's accept that the store's log kept and its index did not
/// may be written: none of the instruction files is carried, every turn is
/// refused until a restart, and the words logged say so, never that the
/// version accepted before is carried.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_context_accept_that_may_be_written_refuses_every_turn_and_says_so() {
    let h = held(&["RULES.md"]);
    let path = h.path("RULES.md");
    h.write("RULES.md", &format!("{KEPT}\n"));
    h.core.held_first_start().unwrap();
    let key = own_file::version_key(&held::pid(&path), 2);
    h.core
        .store
        .retain_plain_frame(move |rows| rows.iter().any(|x| x.key.as_deref() == Some(&key)));
    let real = std::fs::canonicalize(&path).unwrap();
    let said = h
        .core
        .held_written(&real, &format!("{LATER}\n"), "the owner", "cli");
    assert_eq!(said, Some(crate::rpc::own_file::WRITTEN_UNCONFIRMED));
    assert!(said
        .unwrap()
        .contains("none of the instruction files is carried"));
    let files = &h.core.runner.context_files;
    assert_eq!(
        held::look(&h.core.store, files, &path).accepted,
        Accepted::Unknown
    );
    assert_eq!(
        Look::of(&h.core.cfg, &h.core.store, files).accepted,
        Accepted::Unknown
    );
    let e = h
        .submit(&h.core)
        .await
        .expect_err("refused until a restart");
    assert!(
        e.message.contains(
            "An accept may already be written that this daemon cannot read back: restart the \
             daemon"
        ),
        "{e:?}"
    );
}

/// The project layer with an accepted absence carries the reader's note that
/// the file cannot be read; when a file appears there, the note goes and the
/// new text is not carried: the layer changes without an accept, and block 3
/// names the file. (The one exception to "the layer changes only at an
/// accept", kept because an unreadable file behaves as it did.)
#[tokio::test]
async fn a_file_appearing_where_an_absence_was_accepted_changes_the_layer_without_its_text() {
    let h = held(&["LATER.md"]);
    h.core.held_first_start().unwrap();
    let before = h.turn(&h.core).await;
    assert!(layer(&before).contains("[Missing: the file could not be read (not found).]"));
    h.write("LATER.md", &format!("{LATER}\n"));
    let after = h.turn(&h.core).await;
    assert!(!layer(&after).contains("[Missing:"), "{}", layer(&after));
    assert!(!everything(&after).contains("MARSHLOOM-7"));
    assert!(
        block_3(&after).contains("is no file at all"),
        "{}",
        block_3(&after)
    );
}
