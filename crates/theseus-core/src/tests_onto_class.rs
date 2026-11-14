//! A kind the sweep makes is named for its class (issue 9): three
//! conversations mention one
//! country, and the sweep makes the kind `country` with the country as its
//! entry, not a kind named after the country. When the class call fails, is
//! over the day's money, or gives the thing itself, no kind is made that
//! night.

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_protocol::LedgerKind;

use crate::judge::onto::class::parse;
use crate::judge::onto::judged::{Mark, MARK};
use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::tests_onto_judged::{classes, fresh, rig_classed, snapshot, Rig};

/// `other` under `names`, said by `session`.
fn said_other(r: &Rig, session: &str, names: &[&str]) {
    let data = json!({"judgment": format!("jdg_{session}"), "steps": [
        {"kind": "kind_of_thing", "outcome": "other", "names": names, "confidence": 0.9}]});
    let row = LedgerRow::new(LedgerKind::OntoFiled, Some(session), None, data);
    r.core.store.append_ledger(&row).unwrap();
}

fn france(r: &Rig) {
    for s in ["f1", "f2", "f3"] {
        said_other(r, s, &["France"]);
    }
}

fn answer(class: &str) -> Scripted {
    Scripted::tools("", &[("tu_1", "name_class", json!({ "class": class }))])
}

/// KEY: three conversations mention France. The kind is `country`, France is
/// an entry of it, each conversation is filed under that entry, and there is
/// no kind named `france`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_conversations_about_one_country_make_the_kind_country_with_france_in_it() {
    let r = rig_classed(FakeJev::start().unwrap(), classes("country", 4), |_| {});
    france(&r);
    let swept = r.core.onto_sweep_night(fresh()).await.unwrap();
    assert_eq!((swept.kinds, swept.failed), (1, 0), "{swept:?}");
    let o = snapshot(&r.core);
    let kind = o.kind("country").expect("a kind named for the class");
    assert_eq!(kind.examples, vec!["France".to_string()]);
    assert!(o.kind("france").is_none(), "no kind named for the thing");
    let entries: Vec<_> = o.categories().filter(|c| c.kind() == "country").collect();
    let [entry] = entries.as_slice() else {
        panic!("one entry: {entries:?}")
    };
    assert_eq!(entry.name, "France");
    for s in ["f1", "f2", "f3"] {
        let l = o.member_list(s, "country").expect("filed under the entry");
        assert_eq!(l.members.len(), 1);
        assert_eq!(l.members[0].category, entry.id);
    }
    // It is paid for, written as a row of its own, and asked once: the mark
    // holds the word, so the next night spends nothing on it.
    let mark: Mark = r.core.store.get_meta(MARK).unwrap().unwrap();
    assert_eq!(mark.classed.values().collect::<Vec<_>>(), ["country"]);
    let rows = crate::tests_judge::kinds(&r.core.store, "onto.named");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].data["kind"], "class");
    assert_eq!(rows[0].data["names"], json!(["country"]));
}

/// A class call that fails, or gives a word a kind cannot hold or the thing
/// itself, makes no kind that night, and the next night asks again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_class_call_that_gives_nothing_usable_makes_no_kind_and_is_asked_again() {
    let bad = vec![
        Scripted::text("I cannot say."),
        answer("France"),
        answer("two words"),
        answer("country!"),
        answer("country"),
    ];
    let r = rig_classed(FakeJev::start().unwrap(), bad, |_| {});
    france(&r);
    for night in 0..4 {
        let at = fresh() + night * 36 * 3_600_000;
        let swept = r.core.onto_sweep_night(at).await.unwrap();
        assert_eq!(swept.kinds, 0, "night {night}: {swept:?}");
        assert_eq!(swept.refused, 1, "night {night}: no class, no kind");
        assert!(
            snapshot(&r.core)
                .kinds()
                .iter()
                .all(|k| !k.added_by.starts_with("sweep:")),
            "night {night}: the sweep made no kind"
        );
    }
    let at = fresh() + 4 * 36 * 3_600_000;
    let swept = r.core.onto_sweep_night(at).await.unwrap();
    assert_eq!(swept.kinds, 1, "the fifth answer is a class: {swept:?}");
    assert!(snapshot(&r.core).kind("country").is_some());
}

/// The words a kind can hold are one lowercase word that is not a name of the
/// cluster itself.
#[test]
fn only_a_lowercase_word_that_is_not_the_thing_is_a_class() {
    let names = vec!["France".to_string(), "Ledger App".to_string()];
    let said = |class: &str| {
        let content = vec![
            json!({"type": "tool_use", "id": "tu", "name": "name_class", "input": {"class": class}}),
        ];
        parse(&content, &names)
    };
    assert_eq!(said(" Country "), Some("country".to_string()));
    assert_eq!(said("tool"), Some("tool".to_string()));
    assert_eq!(said("france"), None, "the thing itself");
    assert_eq!(said("ledger-app"), None, "a name of the cluster");
    assert_eq!(said("two words"), None);
    assert_eq!(said(""), None);
    assert_eq!(said("9lives"), None, "a kind starts with a letter");
    assert_eq!(said(&"x".repeat(33)), None);
    assert_eq!(
        parse(&[json!({"type": "text", "text": "country"})], &names),
        None
    );
}
