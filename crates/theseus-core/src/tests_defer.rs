//! Deferred tools through the whole core (theseus-7m7w.2): a hidden family's
//! tools go out with `defer_loading` and the search tool beside them; without
//! the mechanism the model gets `tools_find`; the loaded set holds still
//! across a conversation's turns and changes where it recompiles; a call to a
//! tool that was never found is answered with the search's words and counted;
//! a search that found a tool lets the call through.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::TurnSubmitResult;

use crate::bus::EventSink;
use crate::catalog::CatalogRow;
use crate::compiler::Recompile;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_places::{
    answers_a_call, last_user, offered, result_of, rig_deferring, rows, session, Rig,
};
use crate::tools_defer::{SEARCH_TOOL, SEARCH_TYPE};
use crate::turn::TurnRequest;
use crate::Core;

/// A stand-in model: `HIDDEN` calls a tool the request defers, with no
/// search; `SEARCH` searches for the web family and calls its fetch;
/// `FIND` asks `tools_find` for a terminal; `FINDCALL` asks, and calls the
/// terminal tool once a request offers it; anything else answers.
fn script(req: &ProviderRequest) -> Scripted {
    if serde_json::to_string(&req.messages).is_ok_and(|m| m.contains("FINDCALL")) {
        let last = serde_json::to_string(&req.messages.last()).unwrap_or_default();
        return match (answers_a_call(req), last.contains("\"f1\"")) {
            (false, _) => Scripted::tools(
                "",
                &[("f1", "tools_find", json!({"query": "open a terminal"}))],
            ),
            (true, true) if offered(req).contains(&"term_open".to_string()) => {
                Scripted::tools("", &[("c1", "term_open", json!({"argv": ["true"]}))])
            }
            (true, true) => Scripted::text("Not callable."),
            (true, false) => Scripted::text("Done."),
        };
    }
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let last = last_user(req);
    if last.contains("HIDDEN") {
        return Scripted::tools("", &[("h1", "term_open", json!({"argv": ["true"]}))]);
    }
    if last.contains("SEARCH") {
        return Scripted::Blocks {
            blocks: vec![
                json!({"type": "server_tool_use", "id": "srvtoolu_1", "name": SEARCH_TOOL,
                       "input": {"query": "fetch a web page"}}),
                json!({"type": "tool_search_tool_result", "tool_use_id": "srvtoolu_1",
                       "content": {"type": "tool_search_tool_search_result",
                                   "tool_references": [{"type": "tool_reference", "tool_name": "http_fetch"}]}}),
                json!({"type": "tool_use", "id": "s1", "name": "http_fetch", "input": {"url": "not a url"}}),
            ],
            stop_reason: "tool_use".into(),
        };
    }
    if last.contains("FIND") {
        return Scripted::tools(
            "",
            &[("f1", "tools_find", json!({"query": "open a terminal"}))],
        );
    }
    Scripted::text("Hello.")
}

fn rig() -> Rig {
    rig_deferring(|_| {}, script)
}

/// A model without the provider's tool search: `tools_find` is its way.
fn rig_without_search() -> Rig {
    rig_deferring(
        |c| {
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                CatalogRow {
                    tool_search: Some(false),
                    ..Default::default()
                },
            );
        },
        script,
    )
}

async fn turn_as(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    recompile: Option<Recompile>,
) -> TurnSubmitResult {
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
            answer_at: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
        })
        .await
        .unwrap()
}

async fn say(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    turn_as(core, sid, input, None).await
}

fn tool<'a>(req: &'a ProviderRequest, name: &str) -> Option<&'a Value> {
    req.tools.iter().find(|t| t["name"] == name)
}

fn deferred(req: &ProviderRequest, name: &str) -> bool {
    tool(req, name).is_some_and(|t| t["defer_loading"] == true)
}

/// The tools of a request, as the provider reads their bytes.
fn tool_bytes(req: &ProviderRequest) -> String {
    serde_json::to_string(&req.tools).unwrap()
}

#[tokio::test]
async fn a_hidden_familys_tools_carry_defer_loading_and_the_search_tool_is_there() {
    let r = rig();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let req = &r.requests()[0];
    let names = offered(req);
    assert_eq!(names[0], SEARCH_TOOL, "{names:?}");
    assert_eq!(req.tools[0]["type"], SEARCH_TYPE);
    assert!(req.tools[0].get("defer_loading").is_none());
    // A CLI session loads files, commands, code and memory.
    for loaded in ["fs_read", "proc_run", "memory_lookup"] {
        assert!(
            tool(req, loaded).is_some() && !deferred(req, loaded),
            "{loaded}"
        );
    }
    // The tasks, the terminals and the web are declared, deferred.
    for hidden in ["task_create", "term_open", "http_fetch"] {
        assert!(deferred(req, hidden), "{hidden}: {names:?}");
    }
    assert!(
        tool(req, "tools_find").is_none(),
        "the provider's search replaces tools_find"
    );
    // The manifest records what loaded, for the next append.
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap();
    let c = r
        .core
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let loaded = c
        .manifest
        .loaded
        .expect("the manifest records the loaded families");
    assert!(loaded.contains(&"files".to_string()) && !loaded.contains(&"terminals".to_string()));
}

#[tokio::test]
async fn without_the_mechanism_the_hidden_tools_are_left_out_and_tools_find_is_offered() {
    let r = rig_without_search();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let req = &r.requests()[0];
    let names = offered(req);
    assert!(names.contains(&"tools_find".to_string()), "{names:?}");
    assert!(!names.contains(&SEARCH_TOOL.to_string()));
    assert!(
        !names.contains(&"term_open".to_string()),
        "left out: {names:?}"
    );
    assert!(req.tools.iter().all(|t| t.get("defer_loading").is_none()));
    assert!(names.contains(&"fs_read".to_string()));
}

#[tokio::test]
async fn tools_find_names_the_family_and_the_conversation_keeps_it() {
    let r = rig_without_search();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let before = tool_bytes(&r.requests()[0]);
    say(&r.core, &sid, "FIND").await;
    let answer = result_of(&r.core, &sid, "f1");
    assert!(
        answer.contains("term_open") && answer.contains("loaded now"),
        "{answer}"
    );
    // The request that opened the turn held the tools still; the one after the find carries the family.
    let reqs = r.requests();
    assert_eq!(tool_bytes(&reqs[1]), before);
    assert!(!offered(&reqs[1]).contains(&"term_open".to_string()));
    let after = reqs.last().unwrap();
    assert!(
        offered(after).contains(&"term_open".to_string()) && !deferred(after, "term_open"),
        "{:?}",
        offered(after)
    );
    // The next turn appends with the family still loaded: no boundary was needed.
    say(&r.core, &sid, "hello again").await;
    let last = r.requests().pop().unwrap();
    assert_eq!(tool_bytes(&last), tool_bytes(after));
    // Another conversation did not ask, and still lacks it.
    let other = session(&r.core, None);
    say(&r.core, &other, "hello").await;
    assert!(!offered(&r.requests().pop().unwrap()).contains(&"term_open".to_string()));
}

#[tokio::test]
async fn a_tool_found_by_tools_find_is_callable_in_the_same_turn() {
    let r = rig_without_search();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "FINDCALL").await;
    let reqs = r.requests();
    assert!(!offered(&reqs[0]).contains(&"term_open".to_string()));
    // The loop after the find offers the tool, and the model's call to it is
    // run, not answered with a miss.
    assert!(offered(&reqs[1]).contains(&"term_open".to_string()));
    assert!(!deferred(&reqs[1], "term_open"));
    let result = result_of(&r.core, &sid, "c1");
    assert!(!result.contains("is not loaded"), "{result}");
    assert!(rows(&r.core, "tools.missed").is_empty());
    // The turn ended on the answer to the call, not on a refusal to find.
    assert!(reqs.len() >= 3, "{}", reqs.len());
}

async fn say_boundary(core: &Arc<Core>, sid: &str) {
    turn_as(core, sid, "go on", Some(Recompile::Transcript)).await;
}

/// Rows of the ledger that make a place kind use a family in most turns.
fn seed_use(core: &Core, kind: &str, family: &str, turns: u32) {
    let at = theseus_protocol::now_unix_ms() - 3 * 86_400_000;
    for i in 0..turns {
        core.runner.tools.defer.apply_used(
            at + u64::from(i),
            &json!({"place": kind, "loaded": ["files", "commands"], "used": [family], "searched": [], "unused": []}),
        );
    }
}

#[tokio::test]
async fn the_loaded_set_holds_across_a_conversation_and_changes_where_it_recompiles() {
    let r = rig();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let first = tool_bytes(&r.requests()[0]);
    assert!(deferred(&r.requests()[0], "project_attach"));
    // The ledger now shows projects used in most turns of the CLI.
    seed_use(&r.core, "cli", "projects", 12);
    say(&r.core, &sid, "hello again").await;
    say(&r.core, &sid, "and again").await;
    for q in r.requests() {
        assert_eq!(
            tool_bytes(&q),
            first,
            "mid-conversation the request's tools never change"
        );
    }
    // At a recompile the fresh choice applies: the learned family loads.
    say_boundary(&r.core, &sid).await;
    let after = r.requests().pop().unwrap();
    assert!(
        tool(&after, "project_attach").is_some() && !deferred(&after, "project_attach"),
        "{:?}",
        offered(&after)
    );
    // A new conversation starts from the learned set.
    let fresh = session(&r.core, None);
    say(&r.core, &fresh, "hello").await;
    assert!(!deferred(&r.requests().pop().unwrap(), "project_attach"));
}

#[tokio::test]
async fn a_family_unused_for_the_hide_days_hides() {
    let r = rig();
    // The CLI has been watched for 30 days, 40 turns a day, using only files.
    let day = 86_400_000u64;
    let now = theseus_protocol::now_unix_ms();
    for d in 1..=30u64 {
        for i in 0..2 {
            r.core.runner.tools.defer.apply_used(
                now - d * day + i,
                &json!({"place": "cli", "loaded": ["files", "commands", "memory", "tasks", "code"],
                        "used": ["files"], "searched": [], "unused": ["memory", "tasks", "code"]}),
            );
        }
    }
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let req = &r.requests()[0];
    for hidden in ["memory_lookup", "task_create"] {
        assert!(
            deferred(req, hidden),
            "{hidden} is unused for 30 days: {:?}",
            offered(req)
        );
    }
    assert!(
        !deferred(req, "fs_read"),
        "the always-loaded families never hide"
    );
}

#[tokio::test]
async fn a_headless_daemon_uses_its_place_kinds_defaults() {
    let r = rig();
    r.core.runner.failed_turns.leave_to_the_client();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let req = &r.requests()[0];
    assert!(!deferred(req, "term_open"), "headless loads terminals");
    assert!(
        deferred(req, "memory_lookup"),
        "and leaves memory to the model's search"
    );
    let used = rows(&r.core, "tools.used");
    assert_eq!(used[0]["place"], "headless");
}

#[tokio::test]
async fn a_direct_message_loads_memory_and_the_web_and_defers_the_terminals() {
    let r = rig();
    let sid = session(&r.core, Some("dm:271828182845904523"));
    say(&r.core, &sid, "hello").await;
    let req = &r.requests()[0];
    assert!(!deferred(req, "memory_lookup") && !deferred(req, "http_fetch"));
    assert!(deferred(req, "term_open"));
    assert_eq!(rows(&r.core, "tools.used")[0]["place"], "dm");
}

#[tokio::test]
async fn a_call_to_a_tool_nothing_found_is_answered_with_the_search_and_counted() {
    let r = rig();
    let sid = session(&r.core, None);
    let res = say(&r.core, &sid, "HIDDEN").await;
    assert_eq!(res.tool_calls, 0, "nothing ran");
    let answer = result_of(&r.core, &sid, "h1");
    assert!(
        answer.starts_with("Not run:") && answer.contains(SEARCH_TOOL),
        "{answer}"
    );
    assert!(
        rows(&r.core, "term.opened").is_empty(),
        "the terminal was not opened"
    );
    let missed = rows(&r.core, "tools.missed");
    assert_eq!(missed.len(), 1, "{missed:?}");
    assert_eq!(
        (missed[0]["tool"].as_str(), missed[0]["family"].as_str()),
        (Some("term_open"), Some("terminals"))
    );
    assert_eq!(missed[0]["place"], "cli");
    let h = r.core.defer_health();
    assert_eq!((h.missed, h.top_miss.as_deref()), (1, Some("terminals")));
    let listed = r.core.tool_defer();
    let cli = listed.places.iter().find(|p| p.place == "cli").unwrap();
    let term = cli
        .families
        .iter()
        .find(|f| f.family == "terminals")
        .unwrap();
    assert_eq!((term.loaded, term.missed), (false, 1));
}

#[tokio::test]
async fn a_search_that_found_the_tool_lets_the_call_through_and_is_counted() {
    let r = rig();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "SEARCH").await;
    let answer = result_of(&r.core, &sid, "s1");
    assert!(
        !answer.contains("is not loaded"),
        "the call reached the gate: {answer}"
    );
    assert!(rows(&r.core, "tools.missed").is_empty());
    let searched = rows(&r.core, "tools.searched");
    assert_eq!(searched.len(), 1);
    assert_eq!(searched[0]["surfaced"], json!(["http_fetch"]));
    assert_eq!(searched[0]["families"], json!(["web"]));
    assert_eq!(searched[0]["called"], true);
    // The search's blocks go back to the provider with the conversation.
    let again = r
        .requests()
        .into_iter()
        .find(answers_a_call)
        .expect("the call's answer went back");
    let kinds: Vec<String> = again
        .messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|b| b["type"].as_str().map(str::to_string))
        .collect();
    for want in ["server_tool_use", "tool_search_tool_result"] {
        assert!(
            kinds.iter().any(|k| k == want),
            "{want} went back: {kinds:?}"
        );
    }
    let used = rows(&r.core, "tools.used");
    assert_eq!(used[0]["searched"], json!(["web"]));
    assert_eq!(used[0]["used"], json!(["web"]));
}

#[tokio::test]
async fn the_ledgers_calls_promote_a_family_the_next_conversation_loads() {
    // The ledger shows threads called in most CLI turns: the selector
    // loads it, without a hand-tuned list. (A search alone would not: theseus-buz3h.)
    let r = rig();
    for i in 0..12u64 {
        r.core.runner.tools.defer.apply_used(
            theseus_protocol::now_unix_ms() - 3 * 86_400_000 + i,
            &json!({"place": "cli", "loaded": ["files"], "used": ["threads"], "searched": [], "unused": []}),
        );
    }
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    assert!(!deferred(&r.requests()[0], "thread_read"));
}

#[tokio::test]
async fn off_means_every_tool_loads() {
    let r = rig_deferring(|c| c.tools.defer.enabled = false, script);
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let req = &r.requests()[0];
    let names = offered(req);
    assert!(
        !names.contains(&SEARCH_TOOL.to_string()) && !names.contains(&"tools_find".to_string())
    );
    assert!(req.tools.iter().all(|t| t.get("defer_loading").is_none()));
    assert!(names.contains(&"term_open".to_string()));
    assert!(
        rows(&r.core, "tools.used").is_empty()
            || rows(&r.core, "tools.used")[0]["deferred"] == json!([])
    );
}

/// The first request of each kind of place, whole and deferred: the tools'
/// estimated tokens (a deferred tool counts a constant, `ProviderRequest::census`),
/// and the tool list's estimate from `tool.defer`. The figures print with
/// `--no-capture`; the assertion is that every kind of place sends less.
#[tokio::test]
async fn each_place_kinds_first_request_sends_less_than_the_whole_catalog() {
    let whole = rig_deferring(|c| c.tools.defer.enabled = false, script);
    let on = rig();
    let places: [(&str, Option<&str>); 3] = [
        ("cli", None),
        ("dm", Some("dm:271828182845904523")),
        ("channel", Some("channel:314159265358979323")),
    ];
    let listed = on.core.tool_defer();
    let tokens = |q: &ProviderRequest| q.census().tokens(crate::catalog::TokenRates::CLAUDE);
    for (name, place) in places {
        let (a, b) = (session(&whole.core, place), session(&on.core, place));
        say(&whole.core, &a, "hello").await;
        say(&on.core, &b, "hello").await;
        let (w, d) = (
            whole.requests().pop().unwrap(),
            on.requests().pop().unwrap(),
        );
        let sent = d
            .tools
            .iter()
            .filter(|t| t.get("defer_loading").is_none())
            .count();
        eprintln!(
            "first request, {name}: ≈{} tokens whole, ≈{} deferred ({} of {} tools loaded, {} declared)",
            tokens(&w),
            tokens(&d),
            sent,
            w.tools.len(),
            d.tools.len()
        );
        // A shared channel is offered the public tools alone, and loads them all.
        assert!(tokens(&d) < tokens(&w) || name == "channel", "{name}");
    }
    for f in &listed.places[0].families {
        eprintln!(
            "  family {:<10} ≈{:>5} tokens, {} tools",
            f.family,
            f.tokens,
            f.tools.len()
        );
    }
    for p in &listed.places {
        eprintln!(
            "{}: tool list ≈{} tokens whole, ≈{} sent",
            p.place, p.tokens_all, p.tokens_sent
        );
        assert!(
            p.tokens_sent < p.tokens_all || p.place == "channel",
            "{}",
            p.place
        );
    }
}

#[test]
fn a_requests_estimate_counts_deferred_tools_as_a_constant() {
    let tool = |n: &str| json!({"name": n, "description": "x".repeat(2000), "input_schema": {"type": "object"}});
    let mut req = ProviderRequest::simple("claude-sonnet-5-5", 100, "hi");
    req.tools = vec![tool("fs_read")];
    let one = req.census().json;
    let mut many = req;
    many.tools = vec![tool("fs_read")]
        .into_iter()
        .chain((0..20).map(|i| {
            let mut t = tool(&format!("aws_{i}"));
            t["defer_loading"] = json!(true);
            t
        }))
        .collect();
    assert_eq!(many.census().json, one + crate::tools_defer::DEFERRED_BYTES);
    many.tools.insert(
        0,
        json!({"type": crate::tools_defer::SEARCH_TYPE, "name": SEARCH_TOOL}),
    );
    assert_eq!(
        many.census().json,
        one + crate::tools_defer::DEFERRED_BYTES + crate::tools_defer::SEARCH_BYTES
    );
}

/// The counts are read back from the ledger's rows: a board built from them
/// (as a restart builds it) counts what the running one did.
#[tokio::test]
async fn the_ledgers_rows_rebuild_the_counts_after_a_restart() {
    let r = rig();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "HIDDEN").await;
    say(&r.core, &sid, "SEARCH").await;
    say(&r.core, &sid, "hello").await;
    let live = r
        .core
        .runner
        .tools
        .defer
        .stats(crate::tools_defer::PlaceKind::Cli);
    assert_eq!(live.turns_since(0), 3);
    let fresh = crate::tools_defer::Board::default();
    let ready = crate::tools_defer::warm::read(
        &r.core.store,
        &fresh,
        theseus_protocol::now_unix_ms() + 1000,
        14,
    )
    .unwrap();
    assert!(ready, "the index answers");
    let back = fresh.stats(crate::tools_defer::PlaceKind::Cli);
    assert_eq!(back.turns_since(0), 3);
    for pick in [
        (|d: &crate::tools_defer::select::Day| d.used.clone()) as fn(&_) -> _,
        |d| d.searched.clone(),
        |d| d.missed.clone(),
        |d| d.loaded.clone(),
    ] {
        let sum = |s: &crate::tools_defer::select::Stats| {
            let mut all = std::collections::BTreeMap::new();
            for d in s.days.values() {
                for (f, n) in pick(d) {
                    *all.entry(f).or_insert(0u32) += n;
                }
            }
            all
        };
        assert_eq!(sum(&live), sum(&back));
    }
    assert!(fresh.warmed());
    assert_eq!(back.sum_since(0, |d| &d.missed)["terminals"], 1);
    assert_eq!(back.sum_since(0, |d| &d.searched)["web"], 1);
}

/// `context.explain` says the tools part as the request sent it: its digest is
/// the turn's compilation's, so the part reads `same`, and its tokens count the
/// deferred tools as the constant they cost.
#[tokio::test]
async fn context_explain_reads_the_tools_a_request_sent_not_the_whole_catalog() {
    let r = rig();
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    let x = r
        .core
        .context_explain(
            &theseus_protocol::context::ContextExplainParams {
                session_id: sid.clone(),
                turn_id: None,
            },
            true,
        )
        .unwrap();
    let tools = x
        .parts
        .iter()
        .find(|p| p.block == "tools")
        .expect("a tools part");
    assert_eq!(tools.then.as_deref(), Some("same"), "{tools:?}");
    assert!(tools.name.contains("deferred"), "{}", tools.name);
    // The tokens are the loaded tools and the constants, far under the whole catalog's.
    let whole: u64 = r.requests()[0]
        .tools
        .iter()
        .map(|t| serde_json::to_string(t).unwrap().len() as u64)
        .sum();
    assert!(
        tools.tokens * 3 < whole,
        "{} tokens for {whole} bytes",
        tools.tokens
    );
}

/// A stand-in for the Haiku 5.5 install #29 saw (theseus-jpjcd): it calls a
/// tool the request carries when the person asked for it, never calls
/// `tools_find`, and otherwise says the tool is not loaded.
fn haiku(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    if last_user(req).contains("terminal") && offered(req).contains(&"term_open".to_string()) {
        return Scripted::tools("", &[("h1", "term_open", json!({"argv": ["true"]}))]);
    }
    if last_user(req).contains("project") && offered(req).contains(&"project_attach".to_string()) {
        return Scripted::tools("", &[("h1", "project_attach", json!({"project": "none"}))]);
    }
    Scripted::text("That tool is not loaded here.")
}

fn rig_haiku() -> Rig {
    rig_deferring(
        |c| {
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                CatalogRow {
                    tool_search: Some(false),
                    ..Default::default()
                },
            );
        },
        haiku,
    )
}

#[tokio::test]
async fn a_model_that_never_calls_tools_find_still_reaches_the_family_the_message_names() {
    let r = rig_haiku();
    let sid = session(&r.core, None);
    // theseus-3bs4e: a family the message names is counted on the whole request, and at the cli the
    // terminals' (1,264 tokens) no longer fit beside the default families, so the test names the
    // projects' (153), which do; a model without tool search still reaches it with no `tools_find`.
    let res = say(&r.core, &sid, "detach the project from this conversation").await;
    let req = &r.requests()[0];
    assert!(
        offered(req).contains(&"project_attach".to_string()),
        "{:?}",
        offered(req)
    );
    assert_eq!(res.tool_calls, 1, "the call reached the tool");
    assert!(rows(&r.core, "tools.missed").is_empty());
    // The conversation's next turn holds the set still: no cache rewrite.
    let first = tool_bytes(req);
    say(&r.core, &sid, "and detach the project again").await;
    assert!(r.requests().iter().all(|q| tool_bytes(q) == first));
}

/// install #30's live check (theseus-ezad2): asked "Call the thread_read tool once", Haiku on the owner's cli
/// found nothing loaded, because the place's tools were already over its budget and no family could be added.
/// With the language servers on (the owner's config) and deferred, the threads family fits and is offered.
#[tokio::test]
async fn on_the_owners_cli_a_model_without_tool_search_reaches_a_tool_the_message_names() {
    let r = rig_deferring(
        |c| {
            c.lsp.enabled = true;
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                CatalogRow {
                    tool_search: Some(false),
                    ..Default::default()
                },
            );
        },
        |req: &ProviderRequest| {
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            if last_user(req).contains("thread_read")
                && offered(req).contains(&"thread_read".to_string())
            {
                return Scripted::tools("", &[("t1", "thread_read", json!({}))]);
            }
            Scripted::text("No such tool is loaded here.")
        },
    );
    let sid = session(&r.core, None);
    let res = say(&r.core, &sid, "Call the thread_read tool once").await;
    let names = offered(&r.requests()[0]);
    assert!(names.contains(&"thread_read".to_string()), "{names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("lsp_")),
        "the language servers are deferred: {names:?}"
    );
    assert_eq!(res.tool_calls, 1, "the call reached the tool");
    assert!(rows(&r.core, "tools.missed").is_empty());
}

#[tokio::test]
async fn a_message_that_names_nothing_leaves_the_fallback_request_as_it_was() {
    let r = rig_haiku();
    let sid = session(&r.core, None);
    let res = say(&r.core, &sid, "hello there").await;
    assert_eq!(res.tool_calls, 0);
    let names = offered(&r.requests()[0]);
    assert!(names.contains(&"tools_find".to_string()), "{names:?}");
    assert!(!names.contains(&"term_open".to_string()), "{names:?}");
}

#[tokio::test]
async fn a_model_with_tool_search_sends_the_same_first_request_whatever_the_message_names() {
    let r = rig();
    let (a, b) = (session(&r.core, None), session(&r.core, None));
    say(&r.core, &a, "hello there").await;
    say(&r.core, &b, "open a terminal and run true").await;
    let reqs = r.requests();
    assert_eq!(tool_bytes(&reqs[0]), tool_bytes(&reqs[1]));
    assert!(deferred(&reqs[1], "term_open"));
}

/// The plan's two tools are in the deferred `plan` family (theseus-lyxy): no
/// place's first request loads them, a conversation in plan mode does, and one
/// that moves to plan mode in the middle takes them at its next turn (the move
/// asks for a recompile, since an append keeps the set its compilation
/// recorded).
#[tokio::test]
async fn the_plan_tools_are_deferred_in_every_place_and_load_in_plan_mode() {
    use crate::approval::Surface::Cli;
    use crate::tests_m3::{rpc_as, surface};
    use theseus_protocol::method;
    let r = rig();
    for place in [None, Some("dm:271828182845904523")] {
        let sid = session(&r.core, place);
        say(&r.core, &sid, "hello").await;
        let req = r.requests().pop().unwrap();
        for name in ["plan_present", "plan_step"] {
            assert!(
                deferred(&req, name),
                "{name} in {place:?}: {:?}",
                offered(&req)
            );
        }
    }
    let mode = |sid: &str| {
        rpc_as(
            &r.core,
            surface("sock#1", Cli),
            method::SESSION_MODE,
            json!({"session_id": sid, "mode": "plan"}),
        )
    };
    // In plan mode from its first message: loaded.
    let sid = session(&r.core, None);
    mode(&sid).await.unwrap();
    say(&r.core, &sid, "hello").await;
    let req = r.requests().pop().unwrap();
    for name in ["plan_present", "plan_step"] {
        assert!(
            tool(&req, name).is_some() && !deferred(&req, name),
            "{name}: {:?}",
            offered(&req)
        );
    }
    // Moved to plan mode after a turn that hid them: the next turn loads them.
    let sid = session(&r.core, None);
    say(&r.core, &sid, "hello").await;
    assert!(deferred(&r.requests().pop().unwrap(), "plan_present"));
    mode(&sid).await.unwrap();
    say(&r.core, &sid, "now plan it").await;
    let req = r.requests().pop().unwrap();
    assert!(
        tool(&req, "plan_present").is_some() && !deferred(&req, "plan_present"),
        "{:?}",
        offered(&req)
    );
}

/// A listing that says the ledger is read has the counts the read applied
/// (theseus-nyczg): `warmed` was read after the places' counts, so a read that
/// ended between the two gave a listing that said `warmed` with none of its
/// counts. The read ends (its rows applied, then `warmed` set, as `warm::read`
/// does) at the one moment the listing is open to it, right after the listing
/// took its counts: it must then not say `warmed` with the counts it took, and
/// the next one holds every row. Both of the ledger's readers, `tool.defer`
/// and health's line.
#[tokio::test]
async fn a_listing_that_says_warmed_has_the_counts_the_read_applied() {
    use crate::tools_defer::PlaceKind;
    const ROWS: u32 = 20;
    let r = rig();
    let board = &r.core.runner.tools.defer;
    let end_read = || {
        let core = r.core.clone();
        move || {
            let board = &core.runner.tools.defer;
            let row = json!({"place": "cli", "loaded": ["files"], "used": [], "searched": [], "unused": []});
            for _ in 0..ROWS {
                board.apply_used(theseus_protocol::now_unix_ms(), &row);
            }
            board.set_warmed();
        }
    };
    let cli_turns = |l: &theseus_protocol::defer::DeferListResult| {
        let cli = l.places.iter().find(|p| p.place == PlaceKind::Cli.as_str());
        cli.unwrap().turns
    };
    board.reset();
    board.after_counts(end_read());
    let during = r.core.tool_defer();
    assert!(
        !during.warmed || cli_turns(&during) == ROWS,
        "tool.defer said warmed with {} of the read's {ROWS} rows",
        cli_turns(&during)
    );
    let after = r.core.tool_defer();
    assert!(after.warmed);
    assert_eq!(cli_turns(&after), ROWS);
    board.reset();
    board.after_counts(end_read());
    let during = r.core.defer_health();
    assert!(
        !during.warmed || during.turns == ROWS,
        "health said warmed with {} of the read's {ROWS} rows",
        during.turns
    );
    let after = r.core.defer_health();
    assert!(after.warmed);
    assert_eq!(after.turns, ROWS);
}
