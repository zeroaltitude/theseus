//! A task's piece made from a look back kept by reference (issue 52): the piece keeps the look back's own
//! quotes, so the child reads it as the window reads a look back. A message it showed that is erased after the
//! task started blanks only that message's words in the child's next request; one marked wrong shows the line
//! saying what was said since. Real turns: the conversation reads, starts the task, and the task runs.

use std::sync::OnceLock;

use crate::provider::ProviderRequest;
use crate::quotes::ERASED_QUOTE;

use super::*;

/// The brief of the task each test starts.
const BRIEF: &str = "PIECE-LOOK: say what the look back holds";

/// What every task's brief opens with (`task::brief_text_in`).
const TASK_HEAD: &str = ": a background task";

/// Run once, at the task's first request.
type Hook = Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>;

/// The text blocks of `req`'s user messages (a tool's result is not a text block).
fn user_texts(req: &ProviderRequest) -> Vec<String> {
    (req.messages.iter())
        .filter(|m| m["role"] == "user")
        .flat_map(|m| match &m["content"] {
            Value::String(s) => vec![s.clone()],
            Value::Array(b) => (b.iter())
                .filter_map(|b| b["text"].as_str().map(str::to_string))
                .collect(),
            _ => vec![],
        })
        .collect()
}

/// Whether `req` is the task's: its opening's brief is in a text block.
fn of_task(req: &ProviderRequest) -> bool {
    user_texts(req)
        .iter()
        .any(|t| t.contains("[Task ") && t.contains(TASK_HEAD) && t.contains(BRIEF))
}

/// A rig whose conversation reads by `CALL {…}` ([`script`]) and starts a task by `CREATE {…}`; the task's first
/// turn runs `first` and sets a wake, and the wake's turn answers.
fn rig_with_task(first: Hook) -> Rig {
    rig_scripted(
        |_| {},
        move |req| {
            if of_task(req) {
                if req.tools.is_empty() {
                    return Scripted::text("Noted.");
                }
                if last_user(req).contains("⏰ wake") {
                    return Scripted::text("WOKE: read it again.");
                }
                if answers_a_call(req) {
                    return Scripted::text("Waiting.");
                }
                if let Some(f) = first.get() {
                    f();
                }
                let wake = json!({"after": "1s", "note": "read the look back again"});
                return Scripted::tools("Waiting.", &[("t_wake", "wake_at", wake)]);
            }
            match last_user(req).split_once("CREATE ") {
                Some((_, input)) if !answers_a_call(req) => {
                    // The first JSON value: what follows the message in its user turn is not the input.
                    let input: Value = (serde_json::Deserializer::from_str(input).into_iter())
                        .next()
                        .expect("a task.create input")
                        .expect("valid JSON");
                    Scripted::tools("Starting it.", &[("t_task", "task_create", input)])
                }
                _ => script(req),
            }
        },
    )
}

/// `parent` starts the task with one piece, the look back `copy`.
async fn start(r: &Rig, parent: &str, copy: &str) {
    let input = json!({"brief": BRIEF, "arrangement":
        {"pieces": [{"node": copy, "role": "design"}]}});
    turn(&r.core, parent, &format!("CREATE {input}")).await;
}

/// The task's requests (a turn's: they offer tools), in the order they were sent.
fn task_requests(r: &Rig) -> Vec<ProviderRequest> {
    (r.requests().into_iter())
        .filter(|q| of_task(q) && !q.tools.is_empty())
        .collect()
}

/// The piece as `req` lays it: the text block that holds it, from its heading on.
fn piece_in(req: &ProviderRequest) -> String {
    let texts = user_texts(req);
    let block = (texts.iter())
        .find(|t| t.contains("--- Piece 1 of 1"))
        .unwrap_or_else(|| panic!("no piece: {texts:#?}"));
    block[block.find("--- Piece 1 of 1").unwrap()..].to_string()
}

/// The stored look back `copy` as the renderer reads it now (an erased quote's fixed line in its place).
fn rendered_now(r: &Rig, copy: &str) -> String {
    let (_, n) = r.core.store.get_node(copy).unwrap().expect("the look back");
    let Body::ToolResult { content, parts, .. } = n.body else {
        panic!("not a result: {n:?}")
    };
    assert!(!parts.is_empty(), "kept by reference");
    crate::tests_places::kept_text(&r.core, content, &parts)
}

/// An imported conversation, opened by a look back, then quoted by a task's piece; the import is erased after the
/// task's first request. The piece's stored record holds none of the import's words; the first request lays the
/// look back's rendering as it was, and the wake's request lays it as the renderer reads it now: the erased
/// message's words blanked, every other byte as it was.
#[tokio::test]
async fn an_erased_import_blanks_only_its_words_in_a_piece_of_a_look_back() {
    use crate::tests_imports::{episode, import};
    let first: Hook = Arc::default();
    let r = rig_with_task(first.clone());
    let (store, once) = (r.core.store.clone(), std::sync::Once::new());
    let _ = first.set(Box::new(move || {
        once.call_once(|| {
            crate::import::write::erase(&store, "kelp-2026", Some("a test"), "cli").unwrap();
        })
    }));
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", SAID)],
    );
    let parent = session(&r.core, None);
    let (copy, then) = read_in(&r.core, &parent, json!({ "open": ids[0] })).await;
    assert!(then.contains(WORD), "{then}");
    start(&r, &parent, &copy).await;
    r.until("the task's wake", 30, |r| {
        task_requests(r)
            .iter()
            .any(|q| last_user(q).contains("⏰ wake"))
    })
    .await;
    let reqs = task_requests(&r);
    let laid_first = piece_in(&reqs[0]);
    assert!(laid_first.contains(&then), "{laid_first}");
    let wake = (reqs.iter())
        .find(|q| last_user(q).contains("⏰ wake"))
        .unwrap();
    let laid_wake = piece_in(wake);
    let now = rendered_now(&r, &copy);
    assert!(now.contains(ERASED_QUOTE) && !now.contains(WORD), "{now}");
    assert!(laid_wake.contains(&now), "{laid_wake}\n---\n{now}");
    assert!(!laid_wake.contains(WORD), "{laid_wake}");
    let sent = serde_json::to_string(&wake.messages).unwrap();
    assert!(!sent.contains(WORD), "the wake's request: {sent}");
    // The task's stored arrangement holds the look back's quotes, none of the import's words.
    let stored = (r.core.kernel.tasks(None).unwrap().iter())
        .flat_map(|e| r.core.store.session_nodes(&e.session_id).unwrap())
        .find(|(_, n)| matches!(n.body, Body::Arrangement { .. }))
        .map(|(_, n)| serde_json::to_string(&n).unwrap())
        .expect("the task's arrangement");
    assert!(!stored.contains(WORD), "{stored}");
}

/// A conversation's message, opened by a look back, is marked wrong; a task then quotes the look back. The child
/// reads the piece as the window reads a look back: the message's words, and after them the line saying what was
/// said since.
#[tokio::test]
async fn a_piece_shows_the_since_line_of_a_quoted_message_marked_wrong() {
    let r = rig_with_task(Arc::default());
    let said = say(&r.core).await;
    let parent = session(&r.core, None);
    let (copy, then) = read_in(&r.core, &parent, json!({ "open": said.turn })).await;
    assert!(then.contains(WORD), "{then}");
    retract(&r.core, &said.node);
    start(&r, &parent, &copy).await;
    r.until("the task's first request", 30, |r| {
        !task_requests(r).is_empty()
    })
    .await;
    let laid = piece_in(&task_requests(&r)[0]);
    assert!(laid.contains(&then), "{laid}");
    assert!(laid.contains("[Since this was read: "), "{laid}");
    assert!(laid.contains(WHY), "{laid}");
}
