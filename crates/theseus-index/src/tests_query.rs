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
