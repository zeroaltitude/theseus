//! A recall's vector query inside its deadline (theseus-zo1y): the vector
//! source embeds `vector_text` (the turn's new text), cut at `vector_tokens`
//! word pieces by the model's tokenizer, while BM25 reads `text` whole; and a
//! query whose caller has gone stops at the embedder's next layer, so its
//! connection's slot is free within one layer and a burst of abandoned
//! recalls never fills the socket's [`server::MAX_CONNECTIONS`].

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_protocol::{Id, Request};

use crate::client::Client;
use crate::embedder::MAX_TOKENS;
use crate::proto::{method, IndexStatus, QueryParams, QueryResult, Task};
use crate::server;
use crate::tender::Tender;
use crate::tests::user;
use crate::vtests::{settle_all, tiny_embedder, VRig, TEXTS};

/// A query cut at `max` word pieces is one window of the prefix and the
/// text's first `max` pieces, counted by the tokenizer, not by characters;
/// a text within the cut reads exactly as uncut; 0 is uncut.
#[test]
fn a_query_is_cut_at_its_word_pieces() {
    let e = tiny_embedder();
    // The tiny vocabulary covers a word it does not hold letter by letter:
    // "kumquats" is 8 pieces, so 40 words are far past 32.
    let long = ["kumquats are small citrus fruits"; 8].join(" ");
    let whole = e.tokenize(Task::SearchQuery, &long);
    let cut = e.tokenize_query(&long, 32);
    let prefix = e.tokenize(Task::SearchQuery, "").ids[0].len();
    assert!(whole.tokens() > prefix + 32 * 3, "{}", whole.tokens());
    assert_eq!(cut.ids.len(), 1, "one window");
    assert_eq!(
        cut.tokens(),
        prefix + 32,
        "the prefix, [CLS], [SEP] and 32 pieces"
    );
    assert!(cut.truncated);
    assert_eq!(
        cut.ids[0][..prefix - 1],
        whole.ids[0][..prefix - 1],
        "the same start"
    );
    let short = e.tokenize_query("yes", 32);
    assert_eq!(
        short,
        e.tokenize(Task::SearchQuery, "yes"),
        "within the cut, as uncut"
    );
    assert!(!short.truncated);
    assert_eq!(e.tokenize_query(&long, 0), whole, "0: uncut");
    // A cut past one window's room is held to it.
    assert_eq!(e.tokenize_query(&long, 10_000).ids.len(), 1);
    assert!(e.tokenize_query(&long, 10_000).tokens() <= MAX_TOKENS);
    // The vector is the cut text's: embedding the cut equals embedding the
    // pieces it kept.
    let v = e.embed_query(&long, 32, &|| false).unwrap();
    assert_eq!(v.tokens, prefix + 32);
    assert!(v.truncated);
}

/// The vector source ranks by `vector_text` when a query carries one, and
/// BM25 by `text`: the same query asks two things of the two sources.
#[test]
fn the_vector_source_embeds_the_vector_text_and_bm25_reads_the_text() {
    let v = VRig::new();
    let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    let ask = |sources: &[&str], vector_text: Option<&str>| -> QueryResult {
        let mut p = QueryParams::new("kumquats are small citrus fruits");
        p.sources = sources.iter().map(|s| s.to_string()).collect();
        p.vector_text = vector_text.map(str::to_string);
        p.vector_tokens = 32;
        p.k = 1;
        p.wait_ms = 10_000;
        shared.query(&p).unwrap()
    };
    let top = |r: &QueryResult| r.hits[0].text.clone();
    let tides = Some("tides follow the moon");
    assert_eq!(top(&ask(&["vector"], tides)), "tides follow the moon");
    assert_eq!(
        top(&ask(&["vector"], None)),
        "kumquats are small citrus fruits"
    );
    assert_eq!(
        top(&ask(&["bm25"], tides)),
        "kumquats are small citrus fruits"
    );
}

/// A burst of recalls whose callers go mid-embedding (each a vector query on
/// a connection closed while it embeds, behind a slow stand-in model): every
/// query stops at its next layer, every slot is free within about one layer,
/// and the next connection is served. Without the stop, each held its slot
/// for its whole embedding, and the 17th was closed at once.
#[test]
fn an_abandoned_query_stops_and_frees_its_slot_within_a_layer() {
    const STEP: Duration = Duration::from_millis(400);
    let v = VRig::new();
    let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut cfg = v.cfg("test-engine-1");
    cfg.vectors.query_step = STEP;
    let mut t = Tender::open(cfg).unwrap();
    settle_all(&mut t);
    let shared = t.shared();
    let sock = v.rig._tmp.path().join("q.sock");
    let (_served, active) = server::spawn_counted(&sock, shared).unwrap();

    let mut p = QueryParams::new("tides follow the moon");
    p.sources = vec!["vector".into()];
    p.wait_ms = 10_000;
    let mut line = serde_json::to_vec(&Request::new(Id::Num(1), method::QUERY, &p)).unwrap();
    line.push(b'\n');
    let callers: Vec<UnixStream> = (0..server::MAX_CONNECTIONS)
        .map(|_| {
            let mut s = UnixStream::connect(&sock).unwrap();
            s.write_all(&line).unwrap();
            s
        })
        .collect();
    let t0 = Instant::now();
    while active.load(Ordering::SeqCst) < server::MAX_CONNECTIONS {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the burst was not accepted"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    // Mid-embedding: the queries are in their first layer's pause.
    std::thread::sleep(STEP / 4);
    drop(callers);
    let gone = Instant::now();
    while active.load(Ordering::SeqCst) > 0 && gone.elapsed() < STEP + STEP / 2 {
        std::thread::sleep(Duration::from_millis(2));
    }
    let (held, freed) = (active.load(Ordering::SeqCst), gone.elapsed());
    // The 17th connection: served once the slots are free, closed at once
    // while 16 are held.
    let next = Client::connect(&sock, Duration::from_secs(5))
        .and_then(|mut c| c.call::<IndexStatus>(method::STATUS, Value::Null));
    assert!(
        next.is_ok(),
        "the 17th connection was refused ({:?}): {held} slots still held {freed:?} after their \
         callers went (a layer is {STEP:?})",
        next.err()
    );
    assert_eq!(held, 0, "{held} slots still held after {freed:?}");
    eprintln!("16 abandoned queries freed their slots in {freed:?} (a layer: {STEP:?})");
}

/// Within the cut, a query's vector is the one the uncut path gives, to the
/// bit (review of theseus-zo1y): the cut changes only what is past it.
#[test]
fn a_query_within_the_cut_embeds_as_before() {
    let e = tiny_embedder();
    for text in [
        "yes",
        "tides follow the moon",
        "kumquats are small citrus fruits",
    ] {
        let before = e.embed(Task::SearchQuery, &[text]).unwrap().remove(0);
        for max in [0, 32] {
            let now = e.embed_query(text, max, &|| false).unwrap();
            assert_eq!(now.full, before.full, "{text:?} at {max}");
            assert_eq!(now.cut, before.cut, "{text:?} at {max}");
            assert_eq!(now.tokens, before.tokens, "{text:?} at {max}");
        }
    }
}

/// `server::closed` reads a connection's end, not its unread requests: a
/// recall's second query, sent and not yet read, leaves it open; the peer's
/// half-close or close trips it (review of theseus-zo1y).
#[test]
fn a_second_request_sent_and_unread_leaves_the_connection_open() {
    use std::os::fd::AsRawFd as _;
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    assert!(!server::closed(ours.as_raw_fd()), "a quiet connection");
    theirs.write_all(b"{\"one\":1}\n{\"two\":2}\n").unwrap();
    assert!(!server::closed(ours.as_raw_fd()), "two requests unread");
    theirs.shutdown(std::net::Shutdown::Write).unwrap();
    assert!(server::closed(ours.as_raw_fd()), "the peer's half-close");
    drop(theirs);
    assert!(server::closed(ours.as_raw_fd()), "the peer's close");
}

/// A recall's two queries written at once on one connection, as the core's
/// `call_two` writes them, behind a slow model: the words' answer, then the
/// whole one with its vector source, neither stopped as abandoned.
#[test]
fn a_recall_s_two_queries_on_one_connection_both_answer() {
    let v = VRig::new();
    let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut cfg = v.cfg("test-engine-1");
    cfg.vectors.query_step = Duration::from_millis(20);
    let mut t = Tender::open(cfg).unwrap();
    settle_all(&mut t);
    let sock = v.rig._tmp.path().join("two.sock");
    let (_served, _active) = server::spawn_counted(&sock, t.shared()).unwrap();
    let mut words = QueryParams::new("kumquats are small citrus fruits");
    words.sources = vec!["bm25".into()];
    let mut whole = words.clone();
    whole.sources = vec!["bm25".into(), "vector".into()];
    whole.vector_text = Some("tides follow the moon".into());
    whole.vector_tokens = 32;
    whole.wait_ms = 10_000;
    let mut lines = Vec::new();
    for (id, p) in [(1, &words), (2, &whole)] {
        lines.extend(serde_json::to_vec(&Request::new(Id::Num(id), method::QUERY, p)).unwrap());
        lines.push(b'\n');
    }
    let mut s = UnixStream::connect(&sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    s.write_all(&lines).unwrap();
    let mut r = std::io::BufReader::new(s.try_clone().unwrap());
    for id in [1, 2] {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut r, &mut line).unwrap();
        let resp: theseus_protocol::Response = serde_json::from_str(&line).unwrap();
        assert_eq!(resp.id, Id::Num(id));
        assert!(resp.error.is_none(), "{id}: {:?}", resp.error);
        let q: QueryResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(!q.skipped.contains_key("vector"), "{id}: {:?}", q.skipped);
        if id == 2 {
            assert!(q.hits.iter().any(|h| h.sources.contains_key("vector")));
        }
    }
}

/// The real model (review of theseus-zo1y): a query within the cut embeds,
/// to the bit, as the uncut path did; a long one is one window of 32 pieces.
/// The live check runs it.
#[test]
#[ignore = "loads the real weights from ~/.cache/theseus/models (the live check runs it)"]
fn the_real_model_embeds_a_query_within_the_cut_as_before() {
    let home = std::env::var("HOME").unwrap();
    let weights = std::path::Path::new(&home).join(".cache/theseus/models");
    let spec = crate::embedder::ModelSpec::nomic_v1_5();
    let e = crate::embedder::Embedder::load(&weights, &spec).unwrap();
    for text in [
        "yes",
        "Which port does the web interface listen on?",
        "Where do the otters den under the alder roots at the weir?",
    ] {
        let before = e.embed(Task::SearchQuery, &[text]).unwrap().remove(0);
        let now = e.embed_query(text, 32, &|| false).unwrap();
        assert_eq!(now.full, before.full, "{text:?}");
        assert_eq!(now.tokens, before.tokens, "{text:?}");
        assert!(!now.truncated, "{text:?}");
    }
    let long = ["the ferry crosses the harbour at dawn with its lantern lit"; 6].join(" ");
    let cut = e.embed_query(&long, 32, &|| false).unwrap();
    let prefix = e.tokenize(Task::SearchQuery, "").ids[0].len();
    assert!(cut.truncated);
    assert_eq!(cut.tokens, prefix + 32);
}
