//! The provider chokepoint on content flags (theseus-9ymz0, design §4.2a and
//! §11's tests 10 to 14): a request to a model of restricted origin goes out
//! unless the session it is made for carries a label that disqualifies such a
//! model, or has an item that was not judged; a request with no session is
//! refused. The rig is `tests_route`'s: `anthropic` (Claude) and `zai` (GLM, a
//! China-origin endpoint), each a fake that records its requests. The record's
//! `data` is written as JSON so the same test reads against the layout it
//! replaces.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::band::Band;
use theseus_protocol::SessionKind;
use theseus_store::NewRecord;

use crate::content::{ContentFlags, Flag, ItemKind, Label};
use crate::node::Node;
use crate::provider::{Provider, ProviderRequest};
use crate::session::SessionRecord;
use crate::tests_judge::kinds;
use crate::tests_route::{rig, Rig};

const PLAIN: &str = "Rename the call sites in the billing module.";

fn request(model: &str, text: &str) -> ProviderRequest {
    ProviderRequest {
        model: model.into(),
        max_tokens: 16,
        system: vec![],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": text}]})],
        tools: vec![],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: vec![],
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// Send `req` through `p` as a call made for `session` (or for none): its
/// error's class and words, if refused. The chokepoint reads who the call is
/// for when it is made, so the scope wraps the call, not its polling.
async fn send_for(
    p: &Arc<dyn Provider>,
    session: Option<&str>,
    req: &ProviderRequest,
) -> Result<(), (String, String)> {
    let mut sink = |_: crate::provider::Delta<'_>| {};
    let made = match session {
        Some(s) => crate::data_gate::for_session(s, || p.stream_message(req, &mut sink)),
        None => p.stream_message(req, &mut sink),
    };
    match made.await {
        Ok(_) => Ok(()),
        Err(e) => Err((
            e.downcast_ref::<crate::provider::ProviderError>()
                .map_or("", |p| p.class())
                .to_string(),
            format!("{e:#}"),
        )),
    }
}

fn label(flag: Flag) -> Label {
    Label {
        flag,
        node: Some("nod_raised".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    }
}

/// A session record holding `flags` (as JSON, whatever the record's `data` is
/// typed as) and a user node for each of `said`, written to the store. With
/// `judged`, the mark stands at the last node: every item was judged.
fn session_with(r: &Rig, flags: Option<ContentFlags>, said: &[&str], judged: bool) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    let nodes: Vec<Node> = said
        .iter()
        .map(|t| Node::user(&rec.session_id, Some("trn_1"), "cli", t))
        .collect();
    let mut flags = flags;
    if judged {
        let mut f = flags.take().unwrap_or_default();
        f.judged_through = nodes.last().map(|n| n.id.clone());
        flags = Some(f);
    }
    let mut v = serde_json::to_value(&rec).unwrap();
    if let Some(f) = &flags {
        v["data"] = serde_json::to_value(f).unwrap();
    }
    r.core.store.put_session(&rec.session_id, &v).unwrap();
    let records: Vec<NewRecord> = nodes.iter().map(|n| n.record().unwrap()).collect();
    if !records.is_empty() {
        r.core.store.append(&records).unwrap();
    }
    rec.session_id
}

fn blocked(r: &Rig) -> Vec<Value> {
    kinds(&r.core.store, "data_gate.blocked")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// Test 10, the one the old system could not pass and the one that proves the
/// redesign: a request to a model of restricted origin, for a session with no
/// label, is SENT. Nothing proved the request open, because nothing is
/// proved: nothing found in the session says otherwise.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_model_is_sent_a_session_with_no_label() {
    let r = rig(None, 1, |_| {});
    let sid = session_with(&r, None, &[], false);
    let zai = &r.core.runner.providers["zai"];
    send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .expect("a session with no label may use a model of restricted origin");
    assert_eq!(r.zai.requests().len(), 1, "it was sent");
    assert!(blocked(&r).is_empty(), "and nothing was refused");
}

/// Test 10, with items: every node the session carries has been judged, so
/// the request goes; one the judge has not reached holds it back, and the row
/// says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_model_is_sent_only_what_was_judged() {
    let r = rig(None, 1, |_| {});
    let zai = &r.core.runner.providers["zai"];
    let sid = session_with(&r, None, &["Rename the call sites."], true);
    send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .expect("every item was judged");
    assert_eq!(r.zai.requests().len(), 1);
    // The same session, its item not yet judged.
    let sid = session_with(&r, None, &["And the tests."], false);
    let (class, said) = send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate", "{said}");
    assert!(said.contains("judged"), "{said}");
    assert_eq!(r.zai.requests().len(), 1, "the second was never sent");
    assert_eq!(blocked(&r).len(), 1);
}

/// Test 11: a session with `confidential` on it is refused a restricted
/// model, with nothing sent, a `data_gate.blocked` row, and the flag named so
/// the operator can find it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_model_is_refused_a_session_with_a_cn_label() {
    let r = rig(None, 1, |_| {});
    let zai = &r.core.runner.providers["zai"];
    for flag in [Flag::Secret, Flag::Confidential, Flag::Personal] {
        let mut f = ContentFlags::default();
        f.raise(label(flag));
        let sid = session_with(&r, Some(f), &[], false);
        let (class, said) = send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
            .await
            .unwrap_err();
        assert_eq!(class, "data_gate", "{flag:?}: {said}");
        assert!(said.contains(flag.as_str()), "{said}");
    }
    assert!(r.zai.requests().is_empty(), "nothing was sent");
    let rows = blocked(&r);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(rows[0]["provider"], "zai");
    assert_eq!(rows[0]["model"], "glm-5.3");
    assert_eq!(rows[0]["origin"], "cn");
}

/// An `injection` label freezes the session's calls that act and disqualifies
/// no model: it is not a cn word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_injection_label_does_not_keep_a_restricted_model_off() {
    let r = rig(None, 1, |_| {});
    let mut f = ContentFlags::default();
    f.raise(label(Flag::Injection));
    let sid = session_with(&r, Some(f), &[], false);
    let zai = &r.core.runner.providers["zai"];
    send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .unwrap();
    assert_eq!(r.zai.requests().len(), 1);
}

/// Test 12: a request with no session in `CALLER` to a restricted model is
/// refused: fail closed. Nothing says what the call is for, so nothing can say
/// it is clean.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_model_is_refused_a_call_with_no_session() {
    let r = rig(None, 1, |_| {});
    let zai = &r.core.runner.providers["zai"];
    let (class, said) = send_for(zai, None, &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate", "{said}");
    assert!(said.contains("restricted origin"), "{said}");
    assert!(r.zai.requests().is_empty(), "nothing was sent");
    assert_eq!(blocked(&r).len(), 1);
    // A session that has no record is no better.
    let (class, _) = send_for(zai, Some("ses_nowhere"), &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate");
    assert!(r.zai.requests().is_empty());
}

/// Test 13: a names hit is a label, not a refusal (theseus-gyjyo.4). An item
/// that holds a sensitive name raises `confidential` on its session at its
/// entry, with no judge behind it; the label is what refuses a restricted
/// model the session's next request, the chokepoint walks no request for the
/// name, and after the operator removes the label the same request is sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_names_hit_is_a_label() {
    let named = "Rename the call sites in Examplecorp's billing module.";
    let r = rig(None, 1, |c| {
        c.privacy.sensitive_names = vec!["Examplecorp".into()];
    });
    let sid = session_with(&r, None, &[], false);
    let node = Node::user(&sid, Some("trn_1"), "cli", named);
    r.core.store.append(&[node.record().unwrap()]).unwrap();
    let item = crate::judge::content::ContentItem::of_message(
        &node,
        ItemKind::OperatorMessage,
        "operator message",
    )
    .unwrap();
    r.core.runner.judge.ask_content(&sid, vec![item]);
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let flags = rec.data.as_deref().cloned().unwrap_or_default();
    assert_eq!(
        flags.label(Flag::Confidential).map(|l| l.source.as_str()),
        Some("name: Examplecorp")
    );
    // The label refuses the restricted model, and says so by the label.
    let zai = &r.core.runner.providers["zai"];
    let (class, said) = send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate", "{said}");
    assert!(said.contains("`confidential` label"), "{said}");
    assert!(!said.contains("sensitive_names"), "{said}");
    assert!(r.zai.requests().is_empty());
    // The operator removes it (the mark stands: the item was judged).
    r.core
        .store
        .update_session(&sid, |rec| {
            let mut f = rec.data.as_deref().cloned().unwrap_or_default();
            f.remove(Flag::Confidential, "owner", "test", 2);
            f.judged_through = Some(node.id.clone());
            rec.data = Some(Box::new(f));
            Ok(vec![])
        })
        .unwrap();
    send_for(zai, Some(&sid), &request("glm-5.3", named))
        .await
        .expect("nothing overrides the operator: the named request is sent");
    assert_eq!(r.zai.requests().len(), 1);
    // A request that names a word, in a session no item of which did, is not
    // stopped at the chokepoint: the accepted residual (the review's P1 trade).
    let other = session_with(&r, None, &[], false);
    send_for(zai, Some(&other), &request("glm-5.3", named))
        .await
        .expect("the request walk is gone");
    assert_eq!(r.zai.requests().len(), 2);
}

/// A model of EU origin is no longer refused for a name: the US allowlist went
/// with the request walk (theseus-gyjyo.4). Only a label keeps a session off a
/// model, and only a model of restricted origin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_does_not_refuse_a_model_of_eu_origin() {
    let named = "Rename the call sites in Examplecorp's billing module.";
    let r = rig(None, 1, |c| {
        c.privacy.sensitive_names = vec!["Examplecorp".into()];
        c.serve_default_from_eu();
    });
    let sid = session_with(&r, None, &[], false);
    let claude = &r.core.runner.providers["anthropic"];
    send_for(claude, Some(&sid), &request("claude-sonnet-5-5", named))
        .await
        .expect("a model of EU origin is sent it");
    assert_eq!(r.claude.requests().len(), 1);
}

/// Test 14: a label does not affect a request to a model of US origin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_does_not_touch_a_us_model() {
    let r = rig(None, 1, |_| {});
    let mut f = ContentFlags::default();
    f.raise(label(Flag::Confidential));
    f.raise(label(Flag::Secret));
    let sid = session_with(&r, Some(f), &[], false);
    let claude = &r.core.runner.providers["anthropic"];
    send_for(claude, Some(&sid), &request("claude-sonnet-5-5", PLAIN))
        .await
        .unwrap();
    assert_eq!(r.claude.requests().len(), 1);
    assert!(
        blocked(&r).is_empty(),
        "an unrestricted model writes no row"
    );
}

/// The chokepoint reads labels and nothing else (theseus-gyjyo.6): a request
/// to a model of restricted origin costs one read of the session's record and
/// no read of its transcript, whether it is sent or refused. A session with an
/// item nobody has judged is sent (the item's label, if it earns one, is what
/// would refuse the next request), and the store's own counter of full
/// transcript reads does not move.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_chokepoint_reads_no_transcript() {
    let r = rig(None, 1, |_| {});
    let zai = &r.core.runner.providers["zai"];
    let unjudged = session_with(&r, None, &["One.", "Two.", "Three."], false);
    let mut f = ContentFlags::default();
    f.raise(label(Flag::Secret));
    let labelled = session_with(&r, Some(f), &["One.", "Two."], false);
    let before = r.core.store.transcript_reads();
    send_for(zai, Some(&unjudged), &request("glm-5.3", PLAIN))
        .await
        .expect("an item nobody judged is no refusal: only a label is");
    let (class, said) = send_for(zai, Some(&labelled), &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate", "{said}");
    assert!(said.contains("`secret` label"), "{said}");
    assert_eq!(
        r.core.store.transcript_reads(),
        before,
        "the chokepoint read a transcript"
    );
    assert_eq!(r.zai.requests().len(), 1);
    let rows = blocked(&r);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["why"], "secret");
}

/// `cn_models = "always"` sends a labelled session's request to a model of
/// restricted origin: the operator said so, and the label is still on the
/// session for the next time he changes his mind. Synthetic content only.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cn_models_always_sends_a_labelled_session() {
    use crate::config::content_based_security::CnModels;
    let r = rig(None, 1, |c| {
        c.content_based_security.cn_models = CnModels::Always;
    });
    let mut f = ContentFlags::default();
    f.raise(label(Flag::Confidential));
    let sid = session_with(&r, Some(f), &["Synthetic words."], false);
    let zai = &r.core.runner.providers["zai"];
    send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .expect("always sends, whatever the labels say");
    assert_eq!(r.zai.requests().len(), 1);
    assert!(blocked(&r).is_empty(), "{:?}", blocked(&r));
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    assert!(rec
        .data
        .as_deref()
        .is_some_and(|f| f.has(Flag::Confidential)));
}

/// `cn_models = "never"` refuses a model of restricted origin at the
/// chokepoint as well as in the route, for every caller: a pin or a
/// background call that gets past routing is stopped here, session clean or
/// not.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cn_models_never_refuses_a_clean_session() {
    use crate::config::content_based_security::CnModels;
    let r = rig(None, 1, |c| {
        c.content_based_security.cn_models = CnModels::Never;
    });
    let sid = session_with(&r, None, &[], false);
    let zai = &r.core.runner.providers["zai"];
    let (class, said) = send_for(zai, Some(&sid), &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate", "{said}");
    assert!(said.contains("cn_models"), "{said}");
    assert!(r.zai.requests().is_empty());
    assert_eq!(blocked(&r)[0]["why"], "never");
    // A model that is not restricted is sent it all the same.
    let claude = &r.core.runner.providers["anthropic"];
    send_for(claude, Some(&sid), &request("claude-sonnet-5-5", PLAIN))
        .await
        .unwrap();
}
