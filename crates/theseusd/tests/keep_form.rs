//! Keeping a new secret from the form on the card, in the owner's Discord direct messages
//! (issue 29): a real daemon with its Discord binding on a fake gateway, a turn that puts the keep
//! card, a press of the card's button, the form's submit with an invented value, and the stand-in
//! for the vault's program holding it.
//!
//! The value is searched for where it must not be: every message and edit and reply the fake
//! Discord saw, the daemon's log, every ledger row, every file under the state directory and
//! every command line of the vault's program. It is on that program's standard input, once.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::{table, Served};
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeDiscord, Modal, Msg, Pressed, Typed};

const ASK: &str = "keep a key for the lantern provider";
/// Invented, and never in any output.
const VALUE: &str = "INVENTED-FORM-VALUE-8841";
/// The owner's Discord user, and a stranger.
const ANA: u64 = 200_000_000_000_000_001;
const MALLORY: u64 = 200_000_000_000_000_777;
const CODE_PROMPT: &str = "Code from your authenticator app: ";

struct Rig {
    s: Served,
    fake: Arc<FakeDiscord>,
    cli: PathBuf,
    vault: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn start(keep_by_discord: bool) -> Self {
        let model = FakeModel::start(|prompt| match prompt.contains(ASK) {
            true => vec![(
                "vault_keep",
                json!({"vault": "Harbor", "title": "Lantern new", "field": "credential",
                       "purpose": "the lantern provider's key"}),
            )],
            false => vec![],
        });
        let vault = tempfile::tempdir().unwrap();
        common::vault::install(vault.path());
        let path = format!(
            "{}:{}",
            vault.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let key = vault.path().join("model-key");
        std::fs::write(&key, "test-secret-value-0000").unwrap();
        std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let fake = FakeDiscord::start_with_gateway();
        let (base, rest) = (model.base.clone(), fake.addr.clone());
        let gateway = fake.gateway().unwrap().addr.clone();
        let s = Served::start_with(
            |dir| {
                std::fs::create_dir_all(dir.join("state")).unwrap();
                std::fs::write(
                    dir.join("state/bindings.toml"),
                    format!(
                        "guild_id = \"{}\"\n[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n",
                        theseus_sim::fake_discord::DEFAULT_GUILD
                    ),
                )
                .unwrap();
            },
            move |t| {
                for (_, v) in table(t, "secrets").iter_mut() {
                    *v = format!("file:{}", key.display()).into();
                }
                table(t, "model").insert("api_base".into(), base.clone().into());
                for (_, p) in table(t, "providers").iter_mut() {
                    p.as_table_mut()
                        .unwrap()
                        .insert("api_base".into(), base.clone().into());
                }
                table(t, "policy").insert("enforcement".into(), "notify".into());
                let d = table(t, "discord");
                d.insert("enabled".into(), true.into());
                d.insert("rest_proxy".into(), rest.into());
                d.insert("gateway_proxy".into(), format!("ws://{gateway}").into());
                d.insert("ping_window_secs".into(), 0.into());
                table(t, "places").insert("owner".into(), vec![format!("discord:{ANA}")].into());
                table(t, "vault").insert("keep_by_discord".into(), keep_by_discord.into());
            },
            move |cmd| {
                cmd.env("PATH", &path);
            },
        );
        let cli = common::cli_bin();
        assert!(cli.exists(), "no {}: build the CLI", cli.display());
        Self {
            s,
            fake,
            cli,
            vault,
            _model: model,
        }
    }

    /// The owner pairs an authenticator app (at a terminal, as a person does), and ends the
    /// window that pairing opens, so the next keep asks for a code.
    fn pair(&self) {
        let xdg = self.s.path("xdg-state").display().to_string();
        let secret = self.vault.path().join("pairing").display().to_string();
        let run = |args: &[&str], steps: &[(&str, &str)]| {
            let mut argv = vec![
                self.cli.display().to_string(),
                "--socket".into(),
                self.s.path("sock").display().to_string(),
            ];
            argv.extend(args.iter().map(|a| a.to_string()));
            common::terminal::typed_at_a_terminal(
                &argv,
                &[("XDG_STATE_HOME", &xdg), ("PTY_SECRET_FILE", &secret)],
                steps,
                "",
            )
        };
        let (code, out) = run(&["owner", "proof", "enroll"], &[(CODE_PROMPT, "{code:-1}")]);
        assert_eq!(code, 0, "{out}\n{}", self.s.log());
        let (code, out) = run(&["owner", "proof", "lock"], &[]);
        assert_eq!(code, 0, "{out}");
    }

    /// The code an authenticator app shows now, from the secret the pairing printed.
    fn code_now(&self) -> String {
        use theseus_core::owner_proof::totp::{code_at_step, step_at, Secret};
        let text = std::fs::read_to_string(self.vault.path().join("pairing")).unwrap();
        let mut bits = 0u32;
        let mut have = 0;
        let mut bytes = Vec::new();
        for c in text.trim().bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'2'..=b'7' => c - b'2' + 26,
                _ => continue,
            };
            bits = (bits << 5) | u32::from(v);
            have += 5;
            if have >= 8 {
                have -= 8;
                bytes.push((bits >> have) as u8);
                bits &= (1 << have) - 1;
            }
        }
        let secret = Secret::from_bytes(bytes.try_into().expect("the secret's length"));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        code_at_step(&secret, step_at(now))
    }

    /// The owner asks for a key in their direct messages, and the card Theseus posts there.
    fn card(&self) -> Msg {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self
            .fake
            .say(&Typed {
                user: ANA,
                name: "ana",
                channel: None,
                content: ASK,
                file: None,
            })
            .is_err()
        {
            assert!(Instant::now() < deadline, "no gateway client");
            std::thread::sleep(Duration::from_millis(200));
        }
        self.wait("the keep card in the DM", || {
            self.fake
                .all_messages()
                .into_iter()
                .find(|m| m.versions.concat().contains("Keep a secret in the vault"))
        })
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                Instant::now() < deadline,
                "gave up waiting for {what}\nthe binding said: {:?}\nthe keep rows: {:?}\n{}",
                self.fake
                    .replies()
                    .iter()
                    .map(|r| (r.ephemeral, r.content.clone(), r.modal.is_some()))
                    .collect::<Vec<_>>(),
                self.ledger("vault.keep"),
                self.s.log()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn cards(&self) -> Vec<Value> {
        self.s.call("confirm.list", Value::Null).unwrap()["confirms"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.s.call("ledger.tail", json!({"n": 5000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    /// The value is nowhere it must not be: not in anything the fake Discord saw (every message,
    /// edit and reply), the daemon's log, any ledger row, any file of the state directory, or a
    /// command line of the vault's program.
    fn value_nowhere(&self) {
        let seen = format!(
            "{:?}{:?}{:?}",
            self.fake.all_messages(),
            self.fake.replies(),
            self.fake.seen()
        );
        assert!(!seen.contains(VALUE), "in what Discord was sent");
        assert!(!self.s.log().contains(VALUE), "in the daemon's log");
        let t = self.s.call("ledger.tail", json!({"n": 20000})).unwrap();
        assert!(!t.to_string().contains(VALUE), "in a ledger row");
        let mut stack = vec![self.s.path("state")];
        while let Some(p) = stack.pop() {
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                stack.extend(std::fs::read_dir(&p).unwrap().flatten().map(|e| e.path()));
            } else if let Ok(bytes) = std::fs::read(&p) {
                assert!(
                    !bytes.windows(VALUE.len()).any(|w| w == VALUE.as_bytes()),
                    "in {}",
                    p.display()
                );
            }
        }
        assert!(
            common::vault::argv(self.vault.path())
                .iter()
                .all(|l| !l.contains(VALUE)),
            "on a command line of the vault's program"
        );
    }
}

/// The card in the owner's direct messages says what the form costs, the press opens a form
/// labelled with the item, its submit keeps the value, the card settles, and the value is found
/// nowhere but the stand-in vault's write.
#[test]
fn the_form_in_the_dm_keeps_the_value_and_the_value_is_found_nowhere_else() {
    let r = Rig::start(true);
    r.pair();
    let card = r.card();
    let words = card.versions.concat();
    assert!(
        words.contains("A value typed in the form goes through Discord's servers"),
        "{words}"
    );
    assert!(words.contains("the form shows what you type"), "{words}");
    assert!(words.contains("theseus vault keep"), "{words}");
    // The press opens the form, and it is labelled with the item and its field.
    r.fake
        .press(&Pressed {
            message: &card.id,
            button: "Type the value here",
            user: ANA,
            name: "ana",
        })
        .unwrap();
    let form = r.wait("the form", || {
        r.fake.replies().into_iter().find_map(|x| x.modal)
    });
    let rendered = form.to_string();
    assert!(
        rendered.contains("Value for Lantern new, field credential"),
        "{rendered}"
    );
    let id = form["custom_id"].as_str().unwrap().to_string();
    assert!(id.starts_with("kf:act_"), "{id}");
    // The code and the value are two fields.
    assert!(
        rendered.contains("\"custom_id\":\"code\"") && rendered.contains("\"custom_id\":\"value\"")
    );
    r.fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: ANA + 1,
            custom_id: &id,
            fields: &[("code", &r.code_now()), ("value", VALUE)],
        })
        .unwrap();
    let stored = r.wait("the vault to hold the value", || {
        common::vault::stored(r.vault.path())
    });
    assert_eq!(stored.trim(), VALUE, "the value is in the vault's program");
    r.wait("the card to settle", || r.cards().is_empty().then_some(()));
    let rows: Vec<Value> = r
        .ledger("vault.keep")
        .into_iter()
        .filter(|x| x["outcome"] == "kept")
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    // The card is edited to its outcome: its buttons are gone.
    let after = r.wait("the card's edit", || {
        r.fake
            .all_messages()
            .into_iter()
            .find(|m| m.id == card.id && m.components == 0)
    });
    assert!(!after.versions.concat().contains(VALUE));
    r.value_nowhere();
}

/// With the setting off the card is exactly the terminal's and the cockpit's: a Decline, and no
/// line about the chat service; a submit sent anyway is refused and nothing is written.
#[test]
fn with_the_setting_off_the_card_has_no_form_and_a_submit_is_refused() {
    let r = Rig::start(false);
    r.pair();
    let card = r.card();
    let words = card.versions.concat();
    assert!(
        words.contains("Type the value in your own terminal: run")
            && words.contains("offers only Decline"),
        "{words}"
    );
    assert!(!words.contains("Discord's servers"), "{words}");
    assert!(
        r.fake
            .press(&Pressed {
                message: &card.id,
                button: "Type the value here",
                user: ANA,
                name: "ana",
            })
            .is_err(),
        "there is no such button"
    );
    let q = r.cards()[0]["correlation_id"].as_str().unwrap().to_string();
    r.fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: ANA + 1,
            custom_id: &format!("kf:{q}"),
            fields: &[("code", &r.code_now()), ("value", VALUE)],
        })
        .unwrap();
    let refused = r.wait("the refusal", || {
        r.ledger("vault.keep")
            .into_iter()
            .find(|x| x["outcome"] == "stopped")
    });
    assert_eq!(refused["why"], "setting_off", "{refused}");
    assert!(common::vault::stored(r.vault.path()).is_none());
    assert_eq!(r.cards().len(), 1, "the card still waits");
    r.value_nowhere();
}

/// Someone who is not the owner cannot give a value: their submit does not reach the daemon's
/// keep at all, and nothing is written.
#[test]
fn a_stranger_cannot_give_a_value_in_the_owners_place() {
    let r = Rig::start(true);
    r.pair();
    let _card = r.card();
    let q = r.cards()[0]["correlation_id"].as_str().unwrap().to_string();
    let _ = r.fake.modal(&Modal {
        user: MALLORY,
        name: "mallory",
        channel: MALLORY + 1,
        custom_id: &format!("kf:{q}"),
        fields: &[("code", &r.code_now()), ("value", VALUE)],
    });
    // Give the binding time to have acted on it, if it was going to.
    std::thread::sleep(Duration::from_secs(2));
    assert!(common::vault::stored(r.vault.path()).is_none());
    assert_eq!(r.cards().len(), 1, "the card still waits");
    assert!(
        r.ledger("vault.keep")
            .iter()
            .all(|x| x["outcome"] != "kept"),
        "nothing was kept"
    );
    r.value_nowhere();
}

/// `/answer` in the owner's direct messages draws the form's button, as the card does, and its press opens
/// the form: the view is one more place the keep card is drawn.
#[test]
fn the_answer_view_in_the_dm_draws_the_forms_button_and_opens_it() {
    let r = Rig::start(true);
    r.pair();
    let _card = r.card();
    let iid = r
        .fake
        .slash(
            &theseus_sim::fake_discord::commands::Sender {
                user: ANA,
                name: "ana",
                channel: None,
            },
            "answer",
            &[],
        )
        .unwrap();
    let view = r.wait("the /answer view", || r.fake.reply_message(&iid));
    let labels: Vec<&str> = view.buttons.iter().map(|b| b.label.as_str()).collect();
    assert!(labels.contains(&"Type the value here"), "{labels:?}");
    assert!(labels.contains(&"Decline"), "{labels:?}");
    let seen = format!("{:?}", view);
    assert!(
        seen.contains("Discord's servers"),
        "the view says what the form costs: {seen}"
    );
    let id = view
        .buttons
        .iter()
        .find(|b| b.label == "Type the value here")
        .unwrap()
        .custom_id
        .clone();
    assert!(id.ends_with(":keep_form"), "{id}");
    r.value_nowhere();
}

/// A blank value and one over the limit (in bytes, though under the form's own count of characters), typed in
/// the form: the presser is told in the daemon's words, nothing is written, and each attempt leaves the keep
/// path's own `vault.keep` row by class, holding no value or length.
#[test]
fn a_value_the_form_refuses_leaves_the_keep_paths_own_row() {
    let r = Rig::start(true);
    r.pair();
    let card = r.card();
    r.fake
        .press(&Pressed {
            message: &card.id,
            button: "Type the value here",
            user: ANA,
            name: "ana",
        })
        .unwrap();
    let form = r.wait("the form", || {
        r.fake.replies().into_iter().find_map(|x| x.modal)
    });
    let id = form["custom_id"].as_str().unwrap().to_string();
    let long = format!("{VALUE}{}", "é".repeat(2100));
    for value in [long.as_str(), "   "] {
        r.fake
            .modal(&Modal {
                user: ANA,
                name: "ana",
                channel: ANA + 1,
                custom_id: &id,
                fields: &[("code", &r.code_now()), ("value", value)],
            })
            .unwrap();
    }
    let rows = r.wait("both attempts to leave a row", || {
        let rows: Vec<Value> = r
            .ledger("vault.keep")
            .into_iter()
            .filter(|x| x["outcome"] == "stopped")
            .collect();
        (rows.len() == 2).then_some(rows)
    });
    let mut why: Vec<&str> = rows.iter().map(|x| x["why"].as_str().unwrap()).collect();
    why.sort_unstable();
    assert_eq!(why, ["blank", "not_a_value"], "{rows:?}");
    for row in &rows {
        assert!(row["via"].as_str().unwrap().contains("discord:dm"), "{row}");
        assert!(
            row["question"].as_str().unwrap().starts_with("act_"),
            "{row}"
        );
    }
    let told: Vec<String> = r.wait("the replies", || {
        let t: Vec<String> = r
            .fake
            .replies()
            .into_iter()
            .filter_map(|x| x.content)
            .filter(|c| c.contains("Nothing was kept"))
            .collect();
        (t.len() == 2).then_some(t)
    });
    // The refusal's words complete the private deferral the submit was answered with: private still.
    let all = r.fake.replies();
    let mut refusals = all.iter().filter(|x| {
        x.content
            .as_deref()
            .is_some_and(|c| c.contains("Nothing was kept"))
    });
    assert!(
        refusals.all(|x| x.kind == "original" && x.ephemeral),
        "{all:?}"
    );
    // (The replies come back in either order.)
    assert!(
        told.iter()
            .any(|t| t.contains("a key is at most 4096 bytes")),
        "{told:?}"
    );
    assert!(
        told.iter().any(|t| t.contains("no value was given")),
        "{told:?}"
    );
    assert!(
        told.iter().all(|t| !t.contains('é') && !t.contains(VALUE)),
        "{told:?}"
    );
    assert!(common::vault::stored(r.vault.path()).is_none());
    assert_eq!(r.cards().len(), 1, "the card still waits");
    r.value_nowhere();
}
