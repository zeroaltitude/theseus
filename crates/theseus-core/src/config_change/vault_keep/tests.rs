//! `vault.keep` and the two requests that give its value, played through the
//! whole core on the settings rig, over real protocol connections accepted as
//! each surface, against a stub of the vault's program that logs its arguments
//! and its standard input (issue 26, line 2).
//!
//! Every test that gives a value plants one (`VALUE`, invented) and then looks
//! for it where it must not be: the answers, the question's stored record, the
//! outbox and the whole store directory on disk, a captured trace, the ledger,
//! the session's nodes, `Debug` of the request and the list of what waits, and
//! the stub's argument list. It is on the stub's standard input, once, and
//! nowhere else.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::vault_keep::{SecretText, VaultKeepAnswerParams, VaultKeepParams};
use theseus_protocol::{error_code, method, RpcError};

use crate::approval::{Client, Surface};
use crate::config_change::tests::{rig_with, Rig};
use crate::node::ResultStatus;
use crate::owner_proof::totp::Secret;
use crate::owner_proof::Proofs;
use crate::peer_proc::Behind;
use crate::secrets::OpReader;
use crate::tests_m3::{rpc_as, surface};

/// Invented, and never in any output.
const VALUE: &str = "INVENTED-KEEP-VALUE-4417";

const SCRIPT: &str = r#"#!/bin/sh
D='@D@'; MODE='@MODE@'; LOG="$D/op.argv"
printf '%s\n' "$*" >> "$LOG"
case "$1 $2" in "item get"|"item edit"|"item create"|"item delete") case " $* " in *" --vault "*) ;; *) echo '[ERROR] 2026/10/19 08:00:00 a vault query must be provided when this command is called by a service account. Please specify one either through the --vault flag or through piped input' >&2; exit 1 ;; esac ;; esac
[ "$MODE" = flaky ] && [ "$1 $2" = "vault list" ] && [ "$(grep -c '^vault list --format' "$LOG")" -gt 1 ] && { echo '[ERROR] 2026/10/19 08:00:00 dial tcp: network is unreachable' >&2; exit 1; }
case "$1 $2" in
  "vault list")
    case " $* " in
      *" --permission "*) { [ "$MODE" = readonly ] || [ -f "$D/op.readonly" ]; } && echo '[]' || echo '[{"id":"vaultid00000000000000000001","name":"Harbor"}]' ;;
      *) echo '[{"id":"vaultid00000000000000000001","name":"Harbor"},{"id":"vaultid00000000000000000002","name":"Dock"}]' ;;
    esac ;;
  "item list")
    n=$(grep -c '^item list' "$LOG")
    EXTRA=''
    [ -f "$D/op.created" ] && EXTRA=",{\"id\":\"itemid00000000000000000099\",\"title\":\"$(cat "$D/op.created")\",\"vault\":{\"id\":\"vaultid00000000000000000001\",\"name\":\"Harbor\"},\"category\":\"API_CREDENTIAL\"}"
    [ "$MODE" = appears ] && [ "$n" -gt 1 ] && EXTRA=',{"id":"itemid00000000000000000098","title":"Lantern new","vault":{"id":"vaultid00000000000000000001","name":"Harbor"},"category":"API_CREDENTIAL"}'
    echo "[{\"id\":\"itemid00000000000000000001\",\"title\":\"Lantern key\",\"vault\":{\"id\":\"vaultid00000000000000000001\",\"name\":\"Harbor\"},\"category\":\"API_CREDENTIAL\"},{\"id\":\"itemid00000000000000000003\",\"title\":\"Settings key\",\"vault\":{\"id\":\"vaultid00000000000000000001\",\"name\":\"Harbor\"},\"category\":\"API_CREDENTIAL\"},{\"id\":\"itemid00000000000000000004\",\"title\":\"Text note\",\"vault\":{\"id\":\"vaultid00000000000000000001\",\"name\":\"Harbor\"},\"category\":\"LOGIN\"}$EXTRA]" ;;
  "item get")
    case "$3" in
      itemid00000000000000000004) echo '{"id":"itemid00000000000000000004","fields":[{"id":"username","type":"STRING","label":"username","value":"x"}]}' ;;
      *) OLD=''
         case " $* " in *" --reveal "*) OLD='INVENTED-OLD-VALUE' ;; esac
         echo '{"id":"x","fields":[{"id":"username","type":"STRING","label":"username","value":"INVENTED-OLD-USER"},{"id":"credential","type":"CONCEALED","label":"credential","value":"'"$OLD"'"}]}' ;;
    esac ;;
  "item create"|"item edit")
    [ "$MODE" = fail-write ] && { echo '[ERROR] 2026/10/19 08:00:00 you are not authorized: INVENTED-WORDS-IN-STDERR' >&2; exit 1; }
    cat > "$D/op.stdin"
    sed -n 's/.*"id":"credential","[^}]*"value":"\([^"]*\)".*/\1/p' "$D/op.stdin" > "$D/op.stored"
    [ "$1 $2" = "item create" ] && sed -n 's/.*"title":"\([^"]*\)".*/\1/p' "$D/op.stdin" > "$D/op.created"
    echo '{}' ;;
  "read --no-newline")
    [ "$MODE" = no-readback ] && { echo '[ERROR] 2026/10/19 08:00:00 could not reach: dial tcp' >&2; exit 1; }
    [ "$MODE" = differs ] && { printf 'SOMETHING-ELSE'; exit 0; }
    [ -f "$D/op.stored" ] && { printf '%s' "$(cat "$D/op.stored")"; exit 0; }
    echo '[ERROR] 2026/10/19 08:00:00 isn'"'"'t an item' >&2; exit 1 ;;
  *) echo "[ERROR] unexpected: $*" >&2; exit 1 ;;
esac
"#;

fn stub(dir: &Path, mode: &str) -> OpReader {
    let script = SCRIPT
        .replace("@D@", &dir.display().to_string())
        .replace("@MODE@", mode);
    let bin = dir.join("op");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    OpReader::with_bin("invented-token-not-real", bin)
}

fn argv(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("op.argv"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn wrote(dir: &Path) -> bool {
    dir.join("op.stdin").exists()
}

/// The settings rig with one `[secrets]` entry that reads "Settings key", and
/// the stub as the daemon's vault.
fn rig_vault(mode: &str) -> (Rig, tempfile::TempDir) {
    rig_vault_at(mode, crate::content::OutsideMode::default())
}

/// [`rig_vault`] at `[content_based_security] outside = outside`.
fn rig_vault_at(mode: &str, outside: crate::content::OutsideMode) -> (Rig, tempfile::TempDir) {
    let r = rig_with(move |cfg, _| {
        cfg.content_based_security.outside = outside;
        cfg.secrets.insert(
            "settings_key".into(),
            "op://Harbor/Settings key/credential".into(),
        );
    });
    let dir = tempfile::tempdir().unwrap();
    r.core
        .tools
        .look
        .set_vault(Arc::new(stub(dir.path(), mode)));
    (r, dir)
}

/// A keep card asked before issue 54, which carried the second proof's mark: its answer still
/// needs the code it was asked with.
fn old_card(r: &Rig, q: &str, fresh: bool) {
    r.core
        .store
        .append(&[crate::owner_proof::mark(q, fresh).unwrap()])
        .unwrap();
}

fn owner(label: &str, s: Surface) -> Client {
    surface(label, s).behind(Behind::Owner)
}

fn cli() -> Client {
    owner("sock#1", Surface::Cli)
}

fn input(title: &str, field: &str) -> Value {
    json!({"vault": "Harbor", "title": title, "field": field,
           "purpose": "the lantern provider's key"})
}

/// The turn asks, and the question's id.
async fn ask(r: &Rig, input: Value) -> String {
    let sid = r.session();
    r.turn_with("vault_keep", &sid, input).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    r.asked().pop().expect("one card waits").correlation_id
}

async fn give(
    r: &Rig,
    client: Client,
    q: &str,
    value: &str,
    code: Option<&str>,
) -> Result<Value, RpcError> {
    let p = VaultKeepAnswerParams {
        question_id: q.into(),
        value: SecretText::new(value),
        code: code.map(theseus_protocol::owner_proof::ProofCode::new),
        ..Default::default()
    };
    rpc_as(
        &r.core,
        client,
        method::VAULT_KEEP_ANSWER,
        serde_json::to_value(p).unwrap(),
    )
    .await
}

async fn direct(
    r: &Rig,
    client: Client,
    (vault, title, field): (&str, &str, &str),
    code: Option<&str>,
    from_job: Option<&str>,
) -> Result<Value, RpcError> {
    let p = VaultKeepParams {
        vault: vault.into(),
        title: title.into(),
        field: field.into(),
        value: SecretText::new(VALUE),
        code: code.map(theseus_protocol::owner_proof::ProofCode::new),
        from_job: from_job.map(str::to_string),
    };
    rpc_as(
        &r.core,
        client,
        method::VAULT_KEEP,
        serde_json::to_value(p).unwrap(),
    )
    .await
}

fn waiting(r: &Rig, q: &str) -> bool {
    r.asked().iter().any(|c| c.correlation_id == q)
}

fn checks(r: &Rig) -> usize {
    r.rows("owner.proof.checked").len()
}

fn secret_of(r: &Rig) -> Secret {
    r.core.proofs.enrol_stand_in()
}

fn code(s: &Secret, ahead: u64) -> String {
    Proofs::stand_in_code(s, ahead)
}

/// Every file under `root` that holds `needle`: the whole store and state
/// directory as they are on disk.
fn files_holding(root: &Path, needle: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            stack.extend(
                std::fs::read_dir(&p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path()),
            );
        } else if let Ok(bytes) = std::fs::read(&p) {
            if bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                hits.push(p.display().to_string());
            }
        }
    }
    hits
}

/// A trace of everything the daemon logs while `f` runs, as text.
struct Trace(Arc<Mutex<Vec<u8>>>);

#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

fn trace() -> (Trace, tracing::subscriber::DefaultGuard) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_max_level(tracing::Level::TRACE)
        .finish();
    (Trace(buf), tracing::subscriber::set_default(sub))
}

impl Trace {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Where a value must not be, after it was given: nothing the daemon wrote,
/// said, listed or logged, and nothing on the stub's command lines. It is on
/// the stub's standard input, once.
fn assert_value_nowhere(r: &Rig, sid: &str, dir: &Path, said: &[&str], log: &Trace) {
    for s in said {
        assert!(!s.contains(VALUE), "in what was said: {s}");
    }
    assert!(!log.text().contains(VALUE), "in a trace");
    let all = crate::config_change::tests_look::everything(r, sid);
    assert!(!all.contains(VALUE), "in the nodes or the ledger");
    let listed = format!("{:?}", r.core.confirm_list());
    assert!(!listed.contains(VALUE), "in the list of what waits");
    let on_disk = files_holding(&r.state, VALUE);
    assert!(on_disk.is_empty(), "on disk: {on_disk:?}");
    assert!(
        argv(dir).iter().all(|l| !l.contains(VALUE)),
        "on a command line: {:?}",
        argv(dir)
    );
    let stdin = std::fs::read_to_string(dir.join("op.stdin")).unwrap_or_default();
    assert_eq!(
        stdin.matches(VALUE).count(),
        1,
        "on the program's stdin once"
    );
}

/// "Keep a key for the lantern provider": one card, built by the daemon, that
/// says where the value is typed; no value is asked of the turn or told to it.
#[tokio::test]
async fn a_new_secret_is_one_card_that_says_where_the_value_is_typed() {
    let (r, dir) = rig_vault("ok");
    let q = ask(&r, input("Lantern new", "credential")).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    let c = &asks[0];
    for part in [
        "Keep a secret in the vault?",
        "In the vault \"Harbor\": item \"Lantern new\", field \"credential\" (a new item)",
        "it never goes to the model, to this card or to a record",
        "run `theseus vault keep`",
        "the cockpit",
        "Do not paste it into chat",
        "Theseus's reason, in its own words",
    ] {
        assert!(c.reason.contains(part), "{part} in {}", c.reason);
    }
    assert!(!c.reason.contains("REPLACES"));
    // The card is the form; it takes no second proof (issue 54).
    assert!(!c.reason.contains("second proof"), "{}", c.reason);
    assert!(!c.needs_proof && !c.needs_fresh_proof, "{c:?}");
    let plain = c.plain.as_ref().unwrap();
    assert!(
        plain
            .what
            .starts_with("keep a secret in the vault \"Harbor\""),
        "{}",
        plain.what
    );
    assert_eq!(plain.held, "it stores a secret, which only you give");
    // The daemon only listed: nothing was written, and nothing yet asked for a value.
    assert!(!wrote(dir.path()));
    assert!(argv(dir.path())
        .iter()
        .all(|l| l.starts_with("vault list") || l.starts_with("item list")));
    assert!(waiting(&r, &q));
}

/// The turn has no way to carry a value, by any name.
#[tokio::test]
async fn the_tool_takes_no_value_by_any_name() {
    for key in [
        "value",
        "secret",
        "text",
        "body",
        "content",
        "password",
        "key",
        "token",
        "field_value",
        "note",
    ] {
        let (r, dir) = rig_vault("ok");
        let sid = r.session();
        let mut i = input("Lantern new", "credential");
        i[key] = json!(VALUE);
        r.turn_with("vault_keep", &sid, i).await;
        let (status, text) = r.result(&sid);
        assert_ne!(status, ResultStatus::Ok, "{key}");
        assert!(!text.contains(VALUE), "{key}: {text}");
        assert!(r.asked().is_empty(), "{key}");
        assert!(
            argv(dir.path()).is_empty(),
            "{key}: nothing was asked of the vault"
        );
    }
}

/// The whole way in: the card, the value given on the owner's terminal with a
/// code, the item made with the value on the program's stdin only, read back,
/// told by name, the question settled, a ledger row, and the value nowhere.
#[tokio::test]
async fn the_value_goes_from_the_form_to_the_vault_and_nowhere_else() {
    let (log, _guard) = trace();
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let sid_before = r.session();
    let q = ask(&r, input("Lantern new", "credential")).await;
    let said = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap();
    let words = said["words"].as_str().unwrap().to_string();
    assert!(
        words.starts_with("Kept in the vault \"Harbor\", item \"Lantern new\", field \"credential\": it reads back as written."),
        "{words}"
    );
    assert_eq!(said["replaced"], false);
    assert!(!waiting(&r, &q), "the question is settled");
    // The program was asked, in order: the lists, one create, one read back.
    let lines = argv(dir.path());
    assert!(
        lines
            .iter()
            .any(|l| l == "item create - --vault vaultid00000000000000000001 --format json"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l
                == "read --no-newline op://vaultid00000000000000000001/Lantern new/credential"),
        "{lines:?}"
    );
    let body: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("op.stdin")).unwrap())
            .unwrap();
    assert_eq!(body["title"], "Lantern new");
    assert_eq!(body["fields"][0]["type"], "CONCEALED");
    let rows = r.rows("vault.keep");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["outcome"], "kept");
    assert_eq!(rows[0]["state"], "new");
    assert_eq!(rows[0]["question"], q);
    assert_value_nowhere(
        &r,
        &sid_before,
        dir.path(),
        &[&said.to_string(), &words],
        &log,
    );
    // The stored question names the target and holds no value, settled.
    let kept = r.core.kernel.action(&q).unwrap().unwrap();
    let rec = format!("{:?}", kept);
    assert!(!rec.contains(VALUE));
    assert!(rec.contains("Lantern new"));
}

/// Replacing a value is shown in bold words, takes the value with no second
/// proof (issue 54: "the form to ask for it, yes, the 2fa, no"), and edits the
/// item the vault listed by its id.
#[tokio::test]
async fn a_replace_says_so_and_takes_the_value_with_no_code() {
    let (r, dir) = rig_vault("ok");
    let q = ask(&r, input("Lantern key", "credential")).await;
    let c = r.asked().pop().unwrap();
    assert!(
        c.reason.contains("This REPLACES the value stored there"),
        "{}",
        c.reason
    );
    assert!(!c.reason.contains("code"), "{}", c.reason);
    assert!(!c.needs_proof && !c.needs_fresh_proof);
    let said = give(&r, cli(), &q, VALUE, None).await.unwrap();
    assert_eq!(said["replaced"], true);
    assert!(said["words"]
        .as_str()
        .unwrap()
        .contains("It replaced the value that was there."));
    let lines = argv(dir.path());
    assert!(
        lines.iter().any(|l| l == "item edit itemid00000000000000000001 --vault vaultid00000000000000000001 --format json"),
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l == "read --no-newline op://vaultid00000000000000000001/itemid00000000000000000001/credential"), "{lines:?}");
    // The attempts that stopped before this one left rows of their own; the keep is the last.
    let rows = r.rows("vault.keep");
    assert_eq!(rows.last().unwrap()["outcome"], "kept", "{rows:?}");
    assert_eq!(rows.last().unwrap()["state"], "replaces");
}

/// An item Theseus's own settings read is never replaced by a turn: the tool
/// says where the owner does it, and no card is made.
#[tokio::test]
async fn an_item_the_settings_read_is_not_replaced_by_a_turn() {
    let (r, dir) = rig_vault("ok");
    let sid = r.session();
    r.turn_with("vault_keep", &sid, input("Settings key", "credential"))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("one Theseus's own settings read")
            && text.contains("theseus vault keep --vault \"Harbor\" --item \"Settings key\" --field \"credential\""),
        "{text}"
    );
    assert!(r.asked().is_empty());
    assert!(!wrote(dir.path()));
}

/// An item a use wrote into the settings file, not yet live until the next start, is read by
/// Theseus's own settings too: a turn may not replace it either (issue 54).
#[tokio::test]
async fn an_item_a_written_use_reads_is_not_replaced_by_a_turn() {
    let (r, dir) = rig_vault("ok");
    let mut file = std::fs::read_to_string(&r.file).unwrap();
    file = file.replacen(
        "[secrets]\n",
        "[secrets]\nwritten_key = \"op://Harbor/Lantern key/credential\"\n",
        1,
    );
    std::fs::write(&r.file, &file).unwrap();
    assert!(!r.core.cfg.secrets.contains_key("written_key"), "not live");
    let sid = r.session();
    r.turn_with("vault_keep", &sid, input("Lantern key", "credential"))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("one Theseus's own settings read"), "{text}");
    assert!(r.asked().is_empty());
    assert!(!wrote(dir.path()));
}

/// A plain Approve carries no value: it is refused with the words that say
/// where to type it, before the proof gate, so no code is spent; a decline works
/// and writes nothing, and the question's record holds only names.
#[tokio::test]
async fn a_yes_without_a_value_changes_nothing_and_spends_no_code() {
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let before = checks(&r);
    for m in [method::ACTION_CONFIRM, method::WORK_ANSWER] {
        let params = match m {
            method::ACTION_CONFIRM => {
                json!({"correlation_id": q, "approve": true, "code": code(&secret, 0)})
            }
            _ => json!({"question_id": q, "option": "approve", "code": code(&secret, 0)}),
        };
        let e = rpc_as(&r.core, cli(), m, params).await.unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{m}: {e:?}");
        assert!(
            e.message.contains("theseus vault keep") && e.message.contains("cockpit"),
            "{e:?}"
        );
    }
    assert_eq!(checks(&r), before, "no code was checked, so none was spent");
    assert!(waiting(&r, &q));
    assert!(!wrote(dir.path()));
    // Declined: nothing written, settled, and no value anywhere.
    rpc_as(
        &r.core,
        cli(),
        method::ACTION_CONFIRM,
        json!({"correlation_id": q, "approve": false}),
    )
    .await
    .unwrap();
    assert!(!waiting(&r, &q));
    assert!(!wrote(dir.path()));
}

/// A card nobody answered expires with its record holding the target alone.
#[tokio::test]
async fn an_expired_card_writes_nothing_and_holds_only_names() {
    let (r, dir) = rig_vault("ok");
    let q = ask(&r, input("Lantern new", "credential")).await;
    let far = theseus_protocol::now_unix_ms() + 365 * 24 * 3_600_000;
    assert!(r.core.expire_questions(far) >= 1);
    assert!(!waiting(&r, &q));
    assert!(!wrote(dir.path()));
    let kept = format!("{:?}", r.core.kernel.action(&q).unwrap().unwrap());
    assert!(kept.contains("Lantern new") && !kept.contains(VALUE));
}

/// The value is given by the owner's surfaces alone: not a job's shell, not an
/// agent's process (with a good code in hand), not Discord yet, not MCP; the web
/// UI's cockpit is one.
#[tokio::test]
async fn only_the_owners_own_surfaces_give_a_value() {
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let next = || code(&secret, 0);
    let p = |from_job: Option<&str>| {
        serde_json::to_value(VaultKeepAnswerParams {
            question_id: q.clone(),
            value: SecretText::new(VALUE),
            code: None,
            from_job: from_job.map(str::to_string),
            ..Default::default()
        })
        .unwrap()
    };
    let with_code = |mut v: Value, c: String| {
        v["code"] = json!(c);
        v
    };
    let agent = surface("sock#9", Surface::Cli).behind(Behind::Agent("an-agent".into()));
    let web_agent = surface("web#9", Surface::Web).behind(Behind::Agent("an-agent".into()));
    for (client, params) in [
        (agent, with_code(p(None), next())),
        (web_agent, with_code(p(None), next())),
        (cli(), with_code(p(Some("job_1")), next())),
        (surface("mcp#1", Surface::Mcp), with_code(p(None), next())),
    ] {
        let e = rpc_as(&r.core, client, method::VAULT_KEEP_ANSWER, params)
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{e:?}");
        assert!(!e.message.contains(VALUE));
        assert!(!wrote(dir.path()));
        assert!(waiting(&r, &q), "still waiting");
    }
    // The cockpit, as the owner's own page, is one.
    let ok = give(&r, owner("web#1", Surface::Web), &q, VALUE, Some(&next())).await;
    assert!(ok.is_ok(), "{ok:?}");
    assert!(wrote(dir.path()));
}

/// From the owner's terminal with no card: only a terminal; always a fresh code;
/// and the way to replace what the settings read.
#[tokio::test]
async fn the_terminal_keeps_with_no_card_and_may_replace_what_the_settings_read() {
    let (r, dir) = rig_vault("ok");
    let secret = r.core.proofs.open_stand_in_window("cli");
    let target = ("Harbor", "Settings key", "credential");
    // Refused from everything but the owner's own terminal, with a code in hand
    // (the window's own code was the present step; the next step's is good once).
    let next = || code(&secret, 1);
    let agent = surface("sock#9", Surface::Cli).behind(Behind::Agent("an-agent".into()));
    for (client, job) in [
        (owner("web#1", Surface::Web), None),
        (agent, None),
        (cli(), Some("job_1")),
        (surface("mcp#1", Surface::Mcp), None),
    ] {
        let e = direct(&r, client, target, Some(&next()), job)
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{e:?}");
        assert!(!wrote(dir.path()));
    }
    // A window does not cover it: no code asks for one, and says it is the proof.
    let e = direct(&r, cli(), target, None, None).await.unwrap_err();
    assert_eq!(e.code, error_code::REFUSED);
    assert_eq!(e.data["proof"], json!("needed"), "{e:?}");
    assert!(!wrote(dir.path()));
    let (log, _g) = trace();
    let said = direct(&r, cli(), target, Some(&next()), None)
        .await
        .unwrap();
    assert_eq!(said["replaced"], true);
    assert!(wrote(dir.path()));
    // The attempts that stopped before this one each left a row of their own (a window did not
    // cover it, so it was refused for a code); the keep itself is one more.
    let all = r.rows("vault.keep");
    let rows: Vec<_> = all.iter().filter(|x| x["outcome"] != "stopped").collect();
    assert_eq!(rows.len(), 1, "{all:?}");
    assert_eq!(rows[0]["protected"], true);
    assert!(
        all.iter()
            .any(|x| x["outcome"] == "stopped" && x["why"] == "proof"),
        "{all:?}"
    );
    let sid = r.session();
    assert_value_nowhere(&r, &sid, dir.path(), &[&said.to_string()], &log);
}

/// A write that fails is told by class and never in the program's words, leaves
/// the item as it was, and settles the card as not kept.
#[tokio::test]
async fn a_write_that_fails_is_told_by_class_and_leaves_nothing_half_made() {
    let (r, _dir) = rig_vault("fail-write");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let e = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(
        e.message.contains("Nothing was kept") && e.message.contains("refused the daemon's token"),
        "{e:?}"
    );
    assert!(e.message.contains("The vault was not changed."));
    assert!(!e.message.contains("INVENTED-WORDS-IN-STDERR") && !e.message.contains(VALUE));
    assert_eq!(r.rows("vault.keep")[0]["outcome"], "not_written");
    assert!(!waiting(&r, &q));
    assert!(!files_holding(&r.state, VALUE).iter().any(|_| true));
}

/// A value the vault took and could not be read back is said plainly: the item
/// exists, check it. One that read back as something else says so.
#[tokio::test]
async fn a_value_the_vault_took_and_could_not_be_checked_says_the_item_exists() {
    for (mode, outcome, says) in [
        (
            "no-readback",
            "unverified",
            "reading it back failed (the vault could not be reached)",
        ),
        ("differs", "differs", "did NOT read back as written"),
    ] {
        let (r, dir) = rig_vault(mode);
        let secret = secret_of(&r);
        let q = ask(&r, input("Lantern new", "credential")).await;
        let said = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
            .await
            .unwrap();
        let words = said["words"].as_str().unwrap();
        assert!(words.contains(says), "{mode}: {words}");
        assert!(
            words.contains("check it in the vault before relying on it"),
            "{words}"
        );
        assert!(!words.contains(VALUE));
        assert!(wrote(dir.path()), "{mode}: the vault had it");
        assert_eq!(r.rows("vault.keep")[0]["outcome"], outcome);
        assert!(!waiting(&r, &q), "{mode}: settled");
    }
}

/// The vault changed between the card and the value (someone made the item):
/// the card was a yes to something else, so nothing is written.
#[tokio::test]
async fn a_target_that_changed_since_the_card_is_not_written() {
    let (r, dir) = rig_vault("appears");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let e = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED);
    assert!(
        e.message.contains("not what the card showed any more"),
        "{e:?}"
    );
    assert!(!wrote(dir.path()));
    assert_eq!(r.rows("vault.keep")[0]["outcome"], "changed");
}

/// A blank, a huge or a NUL value, and a value of the wrong type, are refused
/// before the proof is checked and before the vault is asked, and what comes
/// back says nothing of what was in the request.
#[tokio::test]
async fn a_bad_value_is_refused_before_anything_is_spent_or_written() {
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let before = (checks(&r), argv(dir.path()).len());
    for bad in [
        String::new(),
        " \n ".into(),
        "x".repeat(5_000),
        "a\0b".into(),
    ] {
        let e = give(&r, cli(), &q, &bad, Some(&code(&secret, 0)))
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::INVALID_PARAMS, "{e:?}");
    }
    let e = rpc_as(
        &r.core,
        cli(),
        method::VAULT_KEEP_ANSWER,
        json!({"question_id": q, "value": 987_654_321, "code": "123456"}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, error_code::INVALID_PARAMS);
    assert!(!e.message.contains("987654321"), "{e:?}");
    assert_eq!((checks(&r), argv(dir.path()).len()), before);
    assert!(waiting(&r, &q));
}

/// A turn that read text from outside is asked first, before the vault is
/// listed, at `[content_based_security] outside = "ask"`; a task's session is
/// refused in the tool's words.
#[tokio::test]
async fn a_steered_turn_is_asked_first_and_a_task_is_refused() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let (r, dir) = rig_vault_at("ok", crate::content::OutsideMode::Ask);
    let sid = r.session();
    let mut rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Injection,
        node: Some("nod_fetched".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch https://elsewhere.example/page".into(),
        confidence: 0.95,
        band: Band::Act,
        at_ms: 1,
        judgment: String::new(),
    });
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&sid, &rec).unwrap();
    r.turn_with("vault_keep", &sid, input("Lantern new", "credential"))
        .await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, "vault.keep");
    assert!(
        asks[0].reason.contains(crate::config_change::FLAG_HOLD)
            && !asks[0].reason.contains("read text from outside"),
        "the flag, not outside text: {}",
        asks[0].reason
    );
    assert!(argv(dir.path()).is_empty(), "the vault was not listed yet");

    let (r, dir) = rig_vault("ok");
    let parent = r.session();
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.turn_with(
        "vault_keep",
        &rec.session_id,
        input("Lantern new", "credential"),
    )
    .await;
    let (status, text) = r.result(&rec.session_id);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("vault.keep") && text.contains("not for a task"),
        "{text}"
    );
    assert!(argv(dir.path()).is_empty());
}

/// A field that is not a concealed one, and a name with a slash, a flag or a
/// reference in it, are refused in words and run nothing that writes.
#[tokio::test]
async fn names_that_cannot_be_a_secrets_place_are_refused() {
    for (title, field, says) in [
        ("Text note", "username", "that is a text field"),
        ("Lantern new", "a/b", "character a name here cannot carry"),
        (
            "op://Harbor/Lantern key/credential",
            "credential",
            "character a name here cannot carry",
        ),
        (
            "--reveal",
            "credential",
            "character a name here cannot carry",
        ),
        (
            "Lantern new",
            "credential?attribute=otp",
            "character a name here cannot carry",
        ),
    ] {
        let (r, dir) = rig_vault("ok");
        let sid = r.session();
        r.turn_with("vault_keep", &sid, input(title, field)).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{title}/{field}: {text}");
        assert!(text.contains(says), "{title}/{field}: {text}");
        assert!(r.asked().is_empty());
        assert!(!wrote(dir.path()));
        assert!(argv(dir.path())
            .iter()
            .all(|l| !l.starts_with("item create")
                && !l.starts_with("item edit")
                && !l.starts_with("read")));
    }
}

/// The edit of an item that is there is the item's own JSON with the one field
/// changed or added: the other fields and their values are sent back as they
/// were, so none is lost; the new value is on the program's stdin once.
#[tokio::test]
async fn an_existing_item_is_edited_whole_and_keeps_its_other_fields() {
    for (field, state, old_gone) in [("credential", "replaces", true), ("backup", "adds", false)] {
        let (r, dir) = rig_vault("ok");
        let secret = secret_of(&r);
        let q = ask(&r, input("Lantern key", field)).await;
        let said = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
            .await
            .unwrap();
        assert_eq!(said["replaced"], old_gone, "{field}");
        let lines = argv(dir.path());
        // Asked for with its values: the stand-in, like a program that conceals them in its
        // answer unless asked, leaves a concealed value empty without `--reveal`, and an item
        // written back from that would lose it.
        let get = "item get itemid00000000000000000001 --vault vaultid00000000000000000001 --format json --reveal";
        assert!(lines.iter().any(|l| l == get), "{field}: {lines:?}");
        let sent: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("op.stdin")).unwrap())
                .unwrap();
        let fields = sent["fields"].as_array().unwrap();
        let by_id = |id: &str| fields.iter().find(|f| f["id"] == id).cloned();
        // The other field is back as it was, value and all.
        assert_eq!(
            by_id("username").unwrap()["value"],
            "INVENTED-OLD-USER",
            "{field}"
        );
        match old_gone {
            true => {
                assert_eq!(by_id("credential").unwrap()["value"], VALUE);
                assert_eq!(fields.len(), 2, "{field}: no field was added");
                assert!(!sent.to_string().contains("INVENTED-OLD-VALUE"));
            }
            false => {
                assert_eq!(by_id("credential").unwrap()["value"], "INVENTED-OLD-VALUE");
                assert_eq!(by_id("backup").unwrap()["value"], VALUE);
                assert_eq!(by_id("backup").unwrap()["type"], "CONCEALED");
                assert_eq!(fields.len(), 3);
            }
        }
        assert_eq!(r.rows("vault.keep")[0]["state"], state, "{field}");
    }
}

/// A vault that cannot be asked at the answer is no reason to spend a code or to
/// close the card as "changed": nothing is written, the code is unspent, and it
/// keeps waiting for a value.
#[tokio::test]
async fn a_vault_that_does_not_answer_at_the_value_spends_no_code_and_keeps_the_card() {
    let (r, dir) = rig_vault("flaky");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let before = checks(&r);
    let e = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(
        e.message.contains("no code was spent") && e.message.contains("could not be reached"),
        "{e:?}"
    );
    assert!(!e.message.contains("changed in the vault"), "{e:?}");
    assert_eq!(checks(&r), before, "no code was checked");
    assert!(waiting(&r, &q), "the card keeps waiting");
    assert!(!wrote(dir.path()));
    // Nothing was written, and the attempt says what stopped it: the vault did not answer.
    let rows = r.rows("vault.keep");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["outcome"].as_str(), rows[0]["why"].as_str()),
        (Some("stopped"), Some("vault"))
    );
}

/// A second answer to a question that is being kept waits; the turn is given
/// back, so a later answer goes through.
#[test]
fn a_second_answer_to_the_same_question_waits() {
    use crate::rpc::vault_keep::vault_keep_test::in_flight;
    let first = in_flight("act_keep_race").expect("the first takes it");
    assert!(in_flight("act_keep_race").is_none(), "the second waits");
    assert!(
        in_flight("act_keep_other").is_some(),
        "another question is free"
    );
    drop(first);
    assert!(in_flight("act_keep_race").is_some(), "given back on drop");
}

/// The owner's terminal lists the card whatever place it was asked in: a card
/// put in a direct message is in what `confirm.list` gives a terminal, with
/// its keep marker, as it is for the binding that drew it (issue 29).
#[tokio::test]
async fn a_card_asked_in_a_direct_message_is_in_the_terminals_list() {
    let (r, _dir) = rig_vault("ok");
    let sid = r.session();
    r.core
        .outbox
        .bind_place(&format!("dm:{}", crate::config_change::tests::OWNER), &sid)
        .unwrap();
    r.turn_with("vault_keep", &sid, input("Lantern new", "credential"))
        .await;
    let (status, text) = r.result(&sid);
    let q = r
        .asked()
        .pop()
        .unwrap_or_else(|| panic!("no card waits: {status:?} {text}"))
        .correlation_id;
    for (label, p) in [
        ("the terminal", json!({})),
        (
            "the binding",
            json!({"discord": {"user_id": crate::config_change::tests::OWNER.to_string(), "channel_id": "7"}}),
        ),
    ] {
        let listed = rpc_as(&r.core, cli(), method::CONFIRM_LIST, p)
            .await
            .unwrap();
        let card = listed["confirms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["correlation_id"] == q.as_str())
            .unwrap_or_else(|| panic!("{label}: the card is not listed: {listed}"));
        assert_eq!(card["tool"], "config.ack", "{label}");
        assert!(card["input"]["keep"].is_object(), "{label}: {card}");
        // The daemon offers no yes to a card that takes a value: a decline alone, in every listing.
        let ids: Vec<&str> = card["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["decline"], "{label}: {card}");
        assert!(
            card["reason"]
                .as_str()
                .unwrap()
                .starts_with("Type the value in your own terminal: run `theseus vault keep`"),
            "{label}: {card}"
        );
    }
}

/// A vault the account may not write in gets no card, and the words say what the
/// owner can do; nothing was written and the vault's program was only listed.
#[tokio::test]
async fn a_vault_the_account_may_not_write_in_gets_no_card() {
    let (r, dir) = rig_vault("readonly");
    let sid = r.session();
    r.turn_with("vault_keep", &sid, input("Lantern new", "credential"))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("vault account may not make a new item")
            && text.contains("Give the account write access"),
        "{text}"
    );
    assert!(r.asked().is_empty(), "no card");
    assert!(!wrote(dir.path()));
    // An item that is there is edited, which is another permission asked of the program.
    let (r, dir) = rig_vault("readonly");
    let sid = r.session();
    r.turn_with("vault_keep", &sid, input("Lantern key", "extra"))
        .await;
    let (_, text) = r.result(&sid);
    assert!(text.contains("may not change an item"), "{text}");
    assert!(argv(dir.path())
        .iter()
        .any(|l| l.starts_with("vault list --permission edit_items")));
}

/// The permission is asked again at the value: one taken away since the card was
/// put is found there, before a code is spent, and the card keeps waiting; the
/// attempt leaves a row that says so.
#[tokio::test]
async fn a_permission_taken_away_since_the_card_is_found_at_the_value() {
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    std::fs::write(dir.path().join("op.readonly"), "").unwrap();
    let before = checks(&r);
    let e = give(&r, cli(), &q, VALUE, Some(&code(&secret, 0)))
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(
        e.message.contains("may not make a new item") && e.message.contains("no code was spent"),
        "{e:?}"
    );
    assert_eq!(checks(&r), before, "no code was checked");
    assert!(waiting(&r, &q) && !wrote(dir.path()));
    let stopped = r.rows("vault.keep");
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert_eq!(stopped[0]["outcome"], "stopped");
    assert_eq!(stopped[0]["why"], "not_writable");
}

/// Every attempt that stops leaves a row with the class of what stopped it, and
/// never the value, its length or its digest.
#[tokio::test]
async fn every_attempt_that_stops_leaves_a_row_with_its_class() {
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    old_card(&r, &q, false);
    // A blank value, a wrong code, an agent's process, and a request that does not parse.
    let blank = rpc_as(
        &r.core,
        cli(),
        method::VAULT_KEEP_ANSWER,
        json!({"question_id": q, "value": "  "}),
    )
    .await;
    assert!(blank.is_err());
    let wrong = give(&r, cli(), &q, VALUE, Some("000000")).await;
    assert!(wrong.is_err());
    let agent = surface("sock#8", Surface::Cli).behind(Behind::Agent("an-agent".into()));
    assert!(give(&r, agent, &q, VALUE, Some(&code(&secret, 0)))
        .await
        .is_err());
    let bad = rpc_as(
        &r.core,
        cli(),
        method::VAULT_KEEP_ANSWER,
        json!({"question_id": q, "value": 7}),
    )
    .await;
    assert!(bad.is_err());
    let why: Vec<String> = r
        .rows("vault.keep")
        .iter()
        .filter(|row| row["outcome"] == "stopped")
        .map(|row| row["why"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(why.len(), 4, "{why:?}");
    assert_eq!(
        (why[0].as_str(), why[1].as_str()),
        ("blank", "proof"),
        "{why:?}"
    );
    // An agent's process is stopped by the place rule or by the keep's own wall: either is a class.
    assert!(["place", "agent"].contains(&why[2].as_str()), "{why:?}");
    assert_eq!(why[3], "bad_request", "{why:?}");
    // The rows hold names and classes, and the question: no value, no length, no digest.
    let all = format!("{:?}", r.rows("vault.keep"));
    assert!(!all.contains(VALUE), "{all}");
    assert!(!wrote(dir.path()));
}

/// The card expired: the terminal's question says so, in the daemon's words,
/// naming the card, and the row says what was told.
#[tokio::test]
async fn a_card_that_expired_is_said_to_have_expired() {
    let (r, _dir) = rig_vault("ok");
    let q = ask(&r, input("Lantern new", "credential")).await;
    let asked = r.asked().pop().unwrap();
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(r.core.expire_questions(asked.requested_at_ms + ttl + 1), 1);
    assert!(!waiting(&r, &q));
    let told = rpc_as(&r.core, cli(), method::VAULT_KEEP_WAITING, json!({}))
        .await
        .unwrap();
    assert_eq!(told["cards"].as_array().unwrap().len(), 0, "{told}");
    let none = told["none"].as_str().unwrap();
    assert!(
        none.contains("expired") && none.contains("Lantern new") && none.contains("ago"),
        "{none}"
    );
    let listed: Vec<_> = r
        .rows("vault.keep")
        .into_iter()
        .filter(|row| row["outcome"] == "listed")
        .collect();
    assert_eq!(listed[0]["why"], "expired", "{listed:?}");
}

/// What the cockpit's field sends is a file its own test also checks
/// (`cockpit/test/answers.test.ts`): the daemon takes exactly that, through the
/// web port's surface, and keeps the value.
#[tokio::test]
async fn what_the_cockpits_field_sends_is_what_the_daemon_takes() {
    let (r, dir) = rig_vault("ok");
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let mut sent: Value = serde_json::from_str(include_str!(
        "../../../../../cockpit/test/fixtures/keep_answer.json"
    ))
    .unwrap();
    sent["question_id"] = q.clone().into();
    sent["code"] = code(&secret, 0).into();
    let told = rpc_as(
        &r.core,
        owner("web#1", Surface::Web),
        method::VAULT_KEEP_ANSWER,
        sent.clone(),
    )
    .await
    .unwrap();
    assert!(
        told["words"].as_str().unwrap().starts_with("Kept in"),
        "{told}"
    );
    let stdin = std::fs::read_to_string(dir.path().join("op.stdin")).unwrap();
    assert!(stdin.contains(sent["value"].as_str().unwrap()));
    assert!(!waiting(&r, &q), "the card is settled");
}

/// The look at the vault's names says, for each vault, whether Theseus may write
/// there: the program's own answer, so a read-only vault is seen before a keep.
#[tokio::test]
async fn the_look_says_for_each_vault_whether_theseus_may_write_there() {
    let (r, _dir) = rig_vault("ok");
    let sid = r.session();
    r.turn_with("look_vault", &sid, json!({"what": ["vaults"]}))
        .await;
    let (_, text) = r.result(&sid);
    assert!(
        text.contains("Harbor (id vaultid00000000000000000001; Theseus may write here)"),
        "{text}"
    );
    assert!(
        text.contains(
            "Dock (id vaultid00000000000000000002; Theseus may not write here (read only))"
        ),
        "{text}"
    );
}

mod tests_form;
mod tests_told;

/// A new secret's card is answered with the value alone: no code, no window (issue 54).
#[tokio::test]
async fn a_new_secret_is_kept_with_the_value_and_no_code() {
    let (r, dir) = rig_vault("ok");
    let q = ask(&r, input("Lantern new", "credential")).await;
    assert!(crate::owner_proof::marked(&r.core.store, &q).is_none());
    give(&r, cli(), &q, VALUE, None).await.unwrap();
    assert!(wrote(dir.path()));
    assert!(!waiting(&r, &q));
}
