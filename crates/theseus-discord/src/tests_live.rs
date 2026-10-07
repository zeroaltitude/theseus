//! The bindings file read while the binding runs (theseus-ocwt), through the
//! stand-in's REST and gateway (`tests_gateway`'s rig): a place removed from
//! the file stops, one added starts, one changed takes its new settings, and
//! a file that does not load changes nothing, each with no restart.

use std::sync::Arc;
use std::time::Duration;

use theseus_core::approval::{Client, Surface};
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_kernel::ActionState;
use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};
use theseus_sim::fake_discord::{Guild, DEFAULT_GUILD};

use crate::bindings::Bindings;
use crate::rpc_client::RpcClient;
use crate::tests_gateway::{Rig, ANA, BEN, LAB};

/// Invented channels beside `#lab`.
const DOCK: u64 = 900_000_000_000_000_020;
const PIER: u64 = 900_000_000_000_000_030;
const REEF: u64 = 900_000_000_000_000_040;

const BOUND: &str = "🔗 Theseus is bound here";

/// A guild channel bound shared, where ana may drive Theseus.
fn channel(id: u64, name: &str, users: &[u64]) -> String {
    let users: Vec<String> = users.iter().map(|u| format!("\"{u}\"")).collect();
    format!(
        "[[channel]]\nid = \"{id}\"\nname = \"{name}\"\nusers = [{}]\nmention_only = false\n",
        users.join(", ")
    )
}

/// A bindings file of `#lab`, ana's DM, and `more`.
fn file(more: &[String]) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n{}[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n{}",
        channel(LAB, "lab", &[ANA]),
        more.concat()
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .channel(LAB, "lab")
        .channel(DOCK, "dock")
        .channel(PIER, "pier")
        .channel(REEF, "reef")
}

async fn rig(script: Vec<Scripted>, bindings: &str, more: &[u64]) -> Rig {
    let more: Vec<String> = more.iter().map(|c| format!("channel:{c}")).collect();
    Rig::start_on(
        |_, _| Arc::new(FakeProvider::scripted(script)),
        guild(),
        bindings,
        &more,
    )
    .await
}

impl Rig {
    /// Rewrite the bindings file.
    fn rewrite(&self, text: &str) {
        std::fs::write(self.dir.path().join("bindings.toml"), text).unwrap();
    }

    /// The places health lists, by label.
    fn labels(&self) -> Vec<String> {
        let b = self.core.bindings.all();
        b.iter()
            .flat_map(|b| b.places.iter().map(|p| p.label.clone()))
            .collect()
    }

    fn detail(&self) -> Option<String> {
        self.core.bindings.all().first()?.detail.clone()
    }

    fn session(&self, channel: u64) -> String {
        let key = format!("channel:{channel}");
        self.core.outbox.place_session(&key).unwrap().unwrap()
    }

    /// The posts refused, by target.
    fn refused(&self) -> Vec<String> {
        let rows = self.ledger("action.failed");
        rows.iter()
            .filter_map(|r| r["outbox"].as_str().map(str::to_string))
            .collect()
    }

    async fn ask(&self, sid: &str, input: &str) -> TurnSubmitResult {
        let (rpc, _) = RpcClient::connect(self.core.clone(), Client::new("test", Surface::Cli));
        let p = TurnSubmitParams {
            carried: false,
            prompt: None,
            session_id: Some(sid.into()),
            input: input.into(),
            profile: None,
            provider: None,
            model: None,
            author: Some("test".into()),
            attachments: vec![],
            reply_to: None,
            opened_from: None,
        };
        rpc.call(theseus_protocol::method::TURN_SUBMIT, p)
            .await
            .unwrap()
    }

    fn answered(&self, channel: u64, text: &str) -> bool {
        self.posted(channel)
            .iter()
            .any(|m| m.content.contains(text))
    }
}

/// theseus-ocwt: a DM and two channels bound; the file rewritten without
/// `#dock`, with no restart: health no longer lists it, its session's next
/// post is refused (an `action.failed` row naming its outbox), a message
/// typed there starts no turn, and `#lab`'s reply still goes. Rewritten with
/// `#pier`: its bind notice posts and a message typed there is answered. Put
/// back, `#dock` answers again, in the session it had: as at a restart, a
/// place whose session goes on says no second bind notice.
#[tokio::test]
async fn a_place_removed_or_added_live_is_unbound_or_bound_with_no_restart() {
    let script = vec![
        Scripted::text("Refused at the dock."),
        Scripted::text("Lab answers."),
        Scripted::text("Pier answers."),
        Scripted::text("Dock answers again."),
    ];
    let dock = channel(DOCK, "dock", &[ANA]);
    let r = rig(script, &file(std::slice::from_ref(&dock)), &[DOCK]).await;
    r.until("both bind notices", || {
        r.answered(LAB, BOUND) && r.answered(DOCK, BOUND)
    })
    .await;
    let dock_sid = r.session(DOCK);
    // The file without #dock.
    r.rewrite(&file(&[]));
    r.until("#dock leaves health", || {
        !r.labels().contains(&"#dock".to_string())
    })
    .await;
    assert!(r.labels().contains(&"#lab".to_string()), "{:?}", r.labels());
    // Its session's reply is written, and refused.
    let out = r.ask(&dock_sid, "hello").await;
    assert_eq!(out.output, "Refused at the dock.");
    let target = format!("discord:channel:{DOCK}");
    r.until("the dock's reply is refused", || {
        r.refused().contains(&target)
    })
    .await;
    // A message typed in #dock starts nothing; one in #lab is answered.
    let turns = r.ledger("discord.message.in").len();
    r.say((ANA, "ana"), Some(DOCK), "anyone at the dock?");
    r.say((ANA, "ana"), Some(LAB), "anyone in the lab?");
    r.until("#lab's reply", || r.answered(LAB, "Lab answers."))
        .await;
    assert_eq!(
        r.ledger("discord.message.in").len(),
        turns + 1,
        "only #lab's message is a turn"
    );
    let dock_posts: Vec<String> = r.posted(DOCK).iter().map(|m| m.content.clone()).collect();
    assert_eq!(dock_posts.len(), 1, "only the bind notice: {dock_posts:?}");
    // #lab's reply shows as it streams; its post settles after.
    r.until("every post settled", || {
        r.core.outbox.status("discord").pending == 0
    })
    .await;
    // The file with #pier: bound, its bind notice posts, and it answers.
    r.rewrite(&file(&[channel(PIER, "pier", &[ANA])]));
    r.until("#pier's bind notice", || r.answered(PIER, BOUND))
        .await;
    assert!(
        r.labels().contains(&"#pier".to_string()),
        "{:?}",
        r.labels()
    );
    r.say((ANA, "ana"), Some(PIER), "anyone on the pier?");
    r.until("#pier's reply", || r.answered(PIER, "Pier answers."))
        .await;
    // #dock back: it answers in its own session, with no new bind notice.
    r.rewrite(&file(&[channel(PIER, "pier", &[ANA]), dock]));
    r.until("#dock is back in health", || {
        r.labels().contains(&"#dock".to_string())
    })
    .await;
    assert_eq!(r.session(DOCK), dock_sid);
    r.say((ANA, "ana"), Some(DOCK), "back at the dock?");
    r.until("#dock's reply", || r.answered(DOCK, "Dock answers again."))
        .await;
    let notices = r
        .posted(DOCK)
        .iter()
        .filter(|m| m.content.starts_with(BOUND))
        .count();
    assert_eq!(notices, 1);
}

/// theseus-ocwt: a file that does not parse changes nothing (the places stay
/// bound, a message is answered) and the board says why; mended, the board
/// clears. A place whose settings changed is updated in place: renamed in
/// health, and driven by the users the file now lists.
#[tokio::test]
async fn a_file_that_does_not_load_changes_nothing_and_a_changed_place_updates_in_place() {
    let script = vec![
        Scripted::text("Still bound."),
        Scripted::text("Ben drives now."),
    ];
    let r = rig(script, &file(&[]), &[]).await;
    let sid = r.session(LAB);
    r.rewrite("guild_id = \"not closed\n[[channel]\n");
    r.until("the board says why", || {
        r.detail().is_some_and(|d| d.contains("does not load"))
    })
    .await;
    let d = r.detail().unwrap();
    assert!(d.contains("stays bound until it does"), "{d}");
    assert!(!d.contains('\n'), "health's one line: {d}");
    // Saved again with the same fault: read again, and said once.
    r.rewrite("guild_id = \"not closed\n[[channel]\n\n");
    tokio::time::sleep(crate::runtime::LIVE_PERIOD * 2).await;
    let errors = r.ledger("discord.error");
    let said = errors.iter().filter(|e| e["op"] == "bindings file").count();
    assert_eq!(said, 1, "{errors:?}");
    assert!(r.labels().contains(&"#lab".to_string()));
    r.say((ANA, "ana"), Some(LAB), "still there?");
    r.until("#lab's reply", || r.answered(LAB, "Still bound."))
        .await;
    // Mended, with #lab renamed and driven by ben alone.
    let renamed = format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n{}[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n",
        channel(LAB, "lab2", &[BEN])
    );
    r.rewrite(&renamed);
    r.until("the board clears and #lab is renamed", || {
        r.detail().is_none() && r.labels().contains(&"#lab2".to_string())
    })
    .await;
    assert!(
        !r.labels().contains(&"#lab".to_string()),
        "{:?}",
        r.labels()
    );
    assert_eq!(r.session(LAB), sid, "the same session goes on");
    let ignored = r.ledger("discord.ignored").len();
    r.say((ANA, "ana"), Some(LAB), "can I still drive?");
    r.until("ana's message ignored", || {
        r.ledger("discord.ignored").len() > ignored
    })
    .await;
    r.say((BEN, "ben"), Some(LAB), "my turn?");
    r.until("ben's reply", || r.answered(LAB, "Ben drives now."))
        .await;
}

/// theseus-ocwt: a post the lane is sending when its place leaves the file
/// settles as sent, and the post behind it is refused; nothing after it
/// reaches the place.
#[tokio::test]
async fn a_post_in_flight_when_its_place_leaves_settles_as_sent_and_the_rest_are_refused() {
    let r = rig(vec![], &file(&[channel(DOCK, "dock", &[ANA])]), &[DOCK]).await;
    r.until("#dock's bind notice", || r.answered(DOCK, BOUND))
        .await;
    let sid = r.session(DOCK);
    let target = format!("discord:channel:{DOCK}");
    r.fake.hold_writes_containing(Some("held at the dock"));
    let notice = |text: &str| {
        let body = serde_json::json!({"kind": "notice", "text": text});
        r.core.outbox.post(&sid, "", &target, body).unwrap()
    };
    let first = notice("held at the dock");
    let second = notice("never sent");
    r.until("the first post's write is held", || {
        r.fake.seen().iter().any(|s| s.outcome == "held")
    })
    .await;
    r.rewrite(&file(&[]));
    r.until("#dock leaves health", || {
        !r.labels().contains(&"#dock".to_string())
    })
    .await;
    // The lane still sends the first: nothing is refused meanwhile.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(r.refused().is_empty(), "{:?}", r.refused());
    r.fake.hold_writes_containing(None);
    let state = |id: &str| r.core.kernel.outbox_action(id).unwrap().map(|a| a.state);
    r.until("both settle", || {
        state(&first.correlation_id) == Some(ActionState::Succeeded)
            && state(&second.correlation_id) == Some(ActionState::Failed)
    })
    .await;
    assert_eq!(r.refused(), [target]);
    let dock: Vec<String> = r.posted(DOCK).iter().map(|m| m.content.clone()).collect();
    assert_eq!(dock.len(), 2, "the bind notice and the first: {dock:?}");
    assert!(!dock.iter().any(|m| m.contains("never sent")));
}

/// theseus-u6v6: a place added live whose bind fails (its session cannot be
/// opened, once) is said on the board, and bound by the watch's next ticks
/// with no further save: one bind notice, one error said, and the board's
/// note gone once it binds.
#[tokio::test]
async fn a_place_whose_bind_failed_is_tried_again_until_it_binds() {
    let r = rig(vec![], &file(&[]), &[]).await;
    crate::rpc_client::refuse_once(theseus_protocol::method::SESSION_OPEN, "discord #reef");
    r.rewrite(&file(&[channel(REEF, "reef", &[ANA])]));
    r.until("the board says why #reef did not bind", || {
        r.detail()
            .is_some_and(|d| d.contains(&format!("channel:{REEF}")) && d.contains("refused"))
    })
    .await;
    let d = r.detail().unwrap();
    assert!(d.contains("tried again"), "{d}");
    assert!(!d.contains('\n'), "health's one line: {d}");
    // No second save: the watch tries the place again by itself.
    r.until("#reef binds", || r.answered(REEF, BOUND)).await;
    r.until("the note clears", || r.detail().is_none()).await;
    assert!(
        r.labels().contains(&"#reef".to_string()),
        "{:?}",
        r.labels()
    );
    let notices = r
        .posted(REEF)
        .iter()
        .filter(|m| m.content.starts_with(BOUND))
        .count();
    assert_eq!(notices, 1, "one bind notice");
    let errors = r.ledger("discord.error");
    let said = errors.iter().filter(|e| e["op"] == "bind place").count();
    assert_eq!(said, 1, "the failure is said once: {errors:?}");
}

/// A voice channel in the file, driven by `users`.
fn voice(id: u64, name: &str, users: &[u64]) -> String {
    format!("{}voice = true\n", channel(id, name, users))
}

/// theseus-btt4: what waits for the next start is measured from what the
/// start bound: a voice channel added is named on the board, #lab's users
/// changed after it still leaves the note, and the voice channel removed
/// again clears it, since nothing waits any more.
#[tokio::test]
async fn the_note_of_what_waits_holds_until_a_start_makes_it_true() {
    let r = rig(vec![], &file(&[]), &[]).await;
    let names_voice = || r.detail().is_some_and(|d| d.contains("the voice channels"));
    r.rewrite(&file(&[voice(DOCK, "dock", &[ANA])]));
    r.until("the board names the voice channels", names_voice)
        .await;
    // An unrelated change: #lab's users.
    let lab = channel(LAB, "lab", &[ANA, BEN]);
    let lab_changed =
        file(&[voice(DOCK, "dock", &[ANA])]).replace(&channel(LAB, "lab", &[ANA]), &lab);
    r.rewrite(&lab_changed);
    r.until("#lab's change is bound", || {
        r.core.bindings.all().first().is_some_and(|b| {
            b.revision.is_some()
                && b.revision
                    != Some(
                        Bindings::parse(&file(&[voice(DOCK, "dock", &[ANA])]))
                            .unwrap()
                            .revision,
                    )
        })
    })
    .await;
    // Two periods on: the note is still there.
    tokio::time::sleep(crate::runtime::LIVE_PERIOD * 2).await;
    assert!(names_voice(), "{:?}", r.detail());
    // The voice channel removed: nothing waits.
    r.rewrite(&file(&[]).replace(&channel(LAB, "lab", &[ANA]), &lab));
    r.until("the note clears", || r.detail().is_none()).await;
}

/// theseus-sn2z: a save seen half written (a valid prefix, cut after a table)
/// is not acted on: with the rest written within the period, no place leaves
/// health, no post is refused, and the full file is what stays bound.
#[tokio::test]
async fn a_save_seen_half_written_is_not_acted_on() {
    let full = file(&[channel(DOCK, "dock", &[ANA]), channel(PIER, "pier", &[ANA])]);
    let torn = file(&[channel(DOCK, "dock", &[ANA])]);
    assert!(full.starts_with(&torn), "the torn file is a prefix");
    let r = rig(vec![], &full, &[DOCK, PIER]).await;
    let path = r.dir.path().join("bindings.toml");
    // The sights: ticks that saw a stamp the tick before had not.
    let sights = || crate::runtime::live_ticks(&path).1;
    let target = format!("discord:channel:{PIER}");
    // The prefix is saved, and the watch sees it (a sight counts once its
    // tick has ended, its action, if any, included).
    let s0 = sights();
    r.rewrite(&torn);
    r.until("the watch has stat'ed the prefix", || sights() > s0)
        .await;
    assert!(
        r.labels().contains(&"#pier".to_string()),
        "{:?}",
        r.labels()
    );
    let sid = r.session(PIER);
    let body = serde_json::json!({"kind": "notice", "text": "after the tear"});
    r.core.outbox.post(&sid, "", &target, body).unwrap();
    r.until("#pier's post goes out", || {
        r.answered(PIER, "after the tear")
    })
    .await;
    assert!(r.refused().is_empty(), "{:?}", r.refused());
    // The rest is written inside the period: the full file is bound.
    r.rewrite(&full);
    let s1 = sights();
    r.until("the watch has stat'ed the full file", || sights() > s1)
        .await;
    let ticked = crate::runtime::live_ticks(&path).0;
    r.until("and acted on it", || {
        crate::runtime::live_ticks(&path).0 >= ticked + 2
    })
    .await;
    let labels = r.labels();
    for l in ["#lab", "#dock", "#pier"] {
        assert!(labels.contains(&l.to_string()), "{labels:?}");
    }
    assert!(r.refused().is_empty(), "{:?}", r.refused());
}

/// A bindings file of `#lab` and the DMs of `users`, in order.
fn with_dms(users: &[(u64, &str)]) -> String {
    let dms: String = users
        .iter()
        .map(|(u, n)| format!("[[dm]]\nuser = \"{u}\"\nname = \"{n}\"\n"))
        .collect();
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n{}{dms}",
        channel(LAB, "lab", &[ANA])
    )
}

/// theseus-nz3q: the DMs stay in the file's order across live changes, so an
/// operator's notice, which goes to the first owner's DM, goes to ana's after
/// her DM was removed and put back, as at a restart, not to ben's.
#[tokio::test]
async fn a_dm_put_back_live_keeps_its_place_in_the_files_order() {
    let both = with_dms(&[(ANA, "ana"), (BEN, "ben")]);
    let r = Rig::start_on(
        |_, _| Arc::new(FakeProvider::scripted(vec![])),
        guild(),
        &both,
        &[format!("dm:{BEN}")],
    )
    .await;
    let dm = |who: u64| who + 1;
    let posts = |who: u64| r.posted(dm(who)).len();
    r.rewrite(&with_dms(&[(BEN, "ben")]));
    r.until("ana's DM leaves health", || {
        !r.labels().contains(&"DM @ana".to_string())
    })
    .await;
    r.rewrite(&both);
    r.until("ana's DM is back", || {
        r.labels().contains(&"DM @ana".to_string())
    })
    .await;
    let (ana, ben) = (posts(ANA), posts(BEN));
    let body = serde_json::json!({"kind": "restarted", "at_unix_ms": 1, "tables": ["model"]});
    r.core.outbox.to_operator(None, body).unwrap();
    r.until("the notice is posted", || {
        posts(ANA) > ana || posts(BEN) > ben
    })
    .await;
    assert_eq!(posts(ANA), ana + 1, "ana's DM is first in the file");
    assert_eq!(posts(BEN), ben, "ben's DM takes none");
}
