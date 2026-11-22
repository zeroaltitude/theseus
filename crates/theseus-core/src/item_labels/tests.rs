use super::*;

fn label(flag: Flag, node: &str, at_ms: u64) -> Label {
    of_kind(flag, node, at_ms, ItemKind::ToolResult)
}

/// A label raised by an item of `kind`: the kind the session decision reads.
fn of_kind(flag: Flag, node: &str, at_ms: u64, kind: ItemKind) -> Label {
    Label {
        flag,
        node: Some(node.into()),
        kind,
        source: "fs.read".into(),
        confidence: 0.9,
        band: Band::Act,
        at_ms,
        judgment: "jdg_1".into(),
    }
}

/// The one session these pure tests read for.
const SID: &str = "ses_1";

fn read(flags: &ContentFlags) -> Reading<'_> {
    Reading {
        session: SID,
        flags,
    }
}

fn asked(outcome: &str, answers: Vec<Answer>) -> Asked {
    Asked {
        pack: "content.v4".into(),
        at_ms: 5,
        outcome: outcome.into(),
        answers,
        pieces: 1,
        truncated: false,
    }
}

fn no(q: Flag) -> Answer {
    Answer {
        question: q,
        choice: "no".into(),
        confidence: 0.95,
        band: Band::Act,
    }
}

/// A tiny generator, so the agreement holds over many runs with no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// **(d) The item records and the session's flags always agree**, over generated runs of the three things
/// that write them: an item's flag landing (late or on time), and the owner removing a flag. Both are written
/// by the same functions the store's writers call (`land`, `ContentFlags::remove` with `removed`), and after
/// every step: the session holds a flag exactly when some INPUT's record says it stands, and every label the
/// session holds names an item whose record says so. A reply's flags stand on the reply alone.
#[test]
fn the_items_and_the_session_always_agree() {
    for seed in 0..400u64 {
        let mut g = Lcg(seed);
        let mut session = ContentFlags::default();
        let mut items: Vec<ItemLabels> = Vec::new();
        let mut clock = 10u64;
        for _ in 0..40 {
            clock += 1 + g.next(3);
            let flag = Flag::ALL[g.next(4) as usize];
            match g.next(5) {
                0 => {
                    if let Some(r) = session.remove(flag, "the owner", "cli", clock) {
                        let gone = session.removed.last().cloned().expect("kept");
                        let node = r.node.expect("named");
                        let item = items.iter_mut().find(|i| i.node == node).expect("its item");
                        removed(item, SID, &gone);
                    }
                }
                _ => {
                    // A new item, or one already seen landing again; it entered at most now.
                    let at = g.next(items.len() as u64 + 1) as usize;
                    if at == items.len() {
                        let entered = clock - g.next(clock.min(8));
                        let node = format!("n{at}");
                        // One item in four is a reply, whose labels stay on it.
                        let kind = match g.next(4) {
                            0 => ItemKind::Reply,
                            _ => ItemKind::ToolResult,
                        };
                        items.push(ItemLabels::new(&node, kind, "fs.read", entered));
                    }
                    let item = &mut items[at];
                    let entered = item.entered_ms;
                    // The label carries its item's true kind: the session decision reads it, never the record.
                    let l = of_kind(flag, &item.node.clone(), clock, item.kind);
                    land(&mut session, Some(item), l, entered);
                }
            }
            for f in Flag::ALL {
                let stands = (items.iter())
                    .filter(|i| i.kind.on_session())
                    .any(|i| i.stands(f, read(&session)));
                assert_eq!(session.has(f), stands, "seed {seed}, {f:?}");
            }
            for l in &session.labels {
                let node = l.node.as_deref().expect("named");
                let item = items.iter().find(|i| i.node == node).expect("its item");
                assert!(
                    item.stands(l.flag, read(&session)),
                    "seed {seed}: {node} {:?}",
                    l.flag
                );
            }
        }
    }
}

/// **(a) A failure is never clean**, and never unmakes an answer: a failed ask reads `failed`, a later answer
/// replaces it, and a later failure leaves the answer kept.
#[test]
fn a_failed_ask_is_failed_and_never_clean() {
    let s = ContentFlags::default();
    let mut i = ItemLabels::new("n1", ItemKind::ToolResult, "fs.read", 1);
    i.asked(asked("failed", vec![]));
    assert_eq!(state(Some(&i), read(&s)), "failed");
    assert_eq!(
        view("n1", Some(&i), read(&s), vec![]).line(),
        "failed (failed)"
    );
    i.asked(asked("answered", vec![no(Flag::Secret)]));
    assert_eq!(state(Some(&i), read(&s)), "clean");
    i.asked(asked("refused", vec![]));
    assert_eq!(
        state(Some(&i), read(&s)),
        "clean",
        "a later failure keeps the answer"
    );
    assert_eq!(
        i.asked.as_ref().map(|a| a.outcome.as_str()),
        Some("answered")
    );
}

/// An item asked and answered `no` throughout reads "asked, clean", each question it was asked with its band,
/// and a question the pack did not ask (`injection`, of the owner's words) reads `not_asked`. No record at all
/// is "not judged then".
#[test]
fn clean_not_asked_and_not_judged_are_told_apart() {
    let s = ContentFlags::default();
    let mut i = ItemLabels::new("n1", ItemKind::OperatorMessage, "operator message", 1);
    let answers = [Flag::Secret, Flag::Confidential, Flag::Personal];
    i.asked(asked("answered", answers.into_iter().map(no).collect()));
    let v = view("n1", Some(&i), read(&s), vec!["topic:rust".into()]);
    assert_eq!(v.state, "clean");
    assert_eq!(v.line(), "asked, clean; topics topic:rust");
    let q = |w: &str| {
        v.questions
            .iter()
            .find(|q| q.question == w)
            .cloned()
            .unwrap()
    };
    assert_eq!(q("injection").said, "not_asked");
    assert_eq!(q("secret").said, "no");
    assert_eq!(q("secret").band.as_deref(), Some("act"));
    let none = view("n2", None, read(&s), vec![]);
    assert_eq!(
        (none.state.as_str(), none.line()),
        ("not_judged", "not judged then".into())
    );
}

/// A flag on the item reads with its band, confidence and pack; a label no model raised reads
/// `deterministic`; the owner's removal reads `removed`, on the item and the session alike (c).
#[test]
fn a_flag_reads_its_band_and_pack_and_a_removal_is_on_both() {
    let mut s = ContentFlags::default();
    let mut i = ItemLabels::new("n1", ItemKind::PersonMessage, "discord:dm", 1);
    i.asked(asked("answered", vec![no(Flag::Secret)]));
    let mut l = label(Flag::Personal, "n1", 2);
    l.band = Band::Confirm;
    l.confidence = 0.91;
    assert!(land(&mut s, Some(&mut i), l, 1));
    let mut d = label(Flag::Confidential, "n1", 3);
    d.judgment = String::new();
    assert!(land(&mut s, Some(&mut i), d, 1));
    let v = view("n1", Some(&i), read(&s), vec![]);
    assert_eq!(v.state, "labelled");
    assert_eq!(
        v.line(),
        "confidential (act 0.90, deterministic), personal (confirm 0.91, content.v4)"
    );
    s.remove(Flag::Personal, "the owner", "cli", 9)
        .expect("held");
    removed(&mut i, SID, s.removed.last().unwrap());
    let v = view("n1", Some(&i), read(&s), vec![]);
    assert!(!s.has(Flag::Personal));
    assert_eq!(
        v.line(),
        "confidential (act 0.90, deterministic), personal removed"
    );
    let p = v.flags.iter().find(|f| f.flag == "personal").unwrap();
    assert_eq!(p.removed.as_deref(), Some("the owner cli"));
}

/// The record is a META family of its own, keyed by the node, and an older reader's shape (no ask, no
/// removals) decodes.
#[test]
fn the_record_is_keyed_by_its_node_and_decodes_sparse() {
    let i = ItemLabels::new("nod_1", ItemKind::Reply, "reply", 4);
    let r = i.record().unwrap();
    assert_eq!(r.kind, kinds::META);
    assert_eq!(r.key.as_deref(), Some("labels.nod_1"));
    let back: ItemLabels = serde_json::from_slice(&r.payload).unwrap();
    assert_eq!(back, i);
    let sparse: ItemLabels =
        serde_json::from_str(r#"{"node":"n","kind":"tool_result","source":"s","entered_ms":1}"#)
            .unwrap();
    assert!(sparse.asked.is_none() && sparse.raised.is_empty());
}

/// A reply's label is kept on the reply and never goes on its session: nothing that acts on a session's
/// labels reads it.
#[test]
fn a_replys_label_stays_on_the_reply() {
    let mut s = ContentFlags::default();
    let mut i = ItemLabels::new("r1", ItemKind::Reply, "reply", 1);
    let l = of_kind(Flag::Injection, "r1", 2, ItemKind::Reply);
    assert!(!land(&mut s, Some(&mut i), l, 1));
    assert!(s.labels.is_empty(), "the session carries nothing");
    assert!(i.stands(Flag::Injection, read(&s)));
    assert_eq!(state(Some(&i), read(&s)), "labelled");
}

/// **5. The session decision reads the incoming label, not the stored record.** A stored record that says
/// `reply` does not keep an input's label off the session, and one that says `tool_result` does not put a
/// reply's on it.
#[test]
fn the_session_decision_reads_the_incoming_label_alone() {
    let mut s = ContentFlags::default();
    let mut stale = ItemLabels::new("n1", ItemKind::Reply, "reply", 1);
    assert!(land(
        &mut s,
        Some(&mut stale),
        label(Flag::Secret, "n1", 2),
        1
    ));
    assert!(s.has(Flag::Secret), "an input's label goes on the session");
    let mut other = ItemLabels::new("n2", ItemKind::ToolResult, "fs.read", 1);
    let reply = of_kind(Flag::Personal, "n2", 3, ItemKind::Reply);
    assert!(!land(&mut s, Some(&mut other), reply, 1));
    assert!(
        !s.has(Flag::Personal),
        "a reply's label stays off the session"
    );
    assert!(!land(
        &mut s,
        None,
        of_kind(Flag::Injection, "n3", 4, ItemKind::Reply),
        1
    ));
    assert!(
        land(&mut s, None, label(Flag::Injection, "n3", 4), 1),
        "no record: the session still decides"
    );
}

/// **3. A removal is its session's.** Kept on the item for the session that removed it, it covers the item
/// read for that session and no other.
#[test]
fn a_removal_kept_on_the_item_covers_its_own_session_alone() {
    let mut fork = ContentFlags::default();
    let mut i = ItemLabels::new("n1", ItemKind::PersonMessage, "discord:dm", 1);
    let source = ContentFlags::default();
    assert!(land(
        &mut fork,
        Some(&mut i),
        label(Flag::Personal, "n1", 2),
        1
    ));
    fork.remove(Flag::Personal, "the owner", "cli", 9)
        .expect("held");
    removed(&mut i, "ses_fork", fork.removed.last().unwrap());
    let in_fork = Reading {
        session: "ses_fork",
        flags: &fork,
    };
    let in_source = Reading {
        session: SID,
        flags: &source,
    };
    assert!(!i.stands(Flag::Personal, in_fork));
    assert!(
        i.stands(Flag::Personal, in_source),
        "the source's item keeps it"
    );
}
