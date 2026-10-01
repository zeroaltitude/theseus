//! The search engine: tantivy, with M6 §2.2's fields, one document per
//! chunk. Ingest replaces a node whole (delete its id, then add its chunks),
//! so it is idempotent: a batch read twice leaves one copy.
//!
//! Three sources answer a query. Two rank by BM25 here: the text (the
//! default tokenizer: split on anything but letters and digits, lowercased,
//! so `127.0.0.1:7433` holds the token `7433`), and the entity field (exact
//! `type:value` terms, from the same rules the text went through). The third,
//! vectors (29c), ranks by cosine in `vectors.rs`, and its hits join here by
//! node and chunk. Their ranks are fused by reciprocal rank fusion, weighted
//! by source, `Σ w / (60 + rank)`, so no source's raw scores are weighed
//! against another's.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::Path;
use std::time::Instant;

use tantivy::collector::{Count, TopDocs};
use tantivy::columnar::Column;
use tantivy::query::{BooleanQuery, ConstScoreQuery, Occur, Query, RangeQuery, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, Value as _, FAST, INDEXED, STORED, STRING, TEXT,
};
use tantivy::{
    DocAddress, Index, IndexReader, IndexWriter, ReloadPolicy, Searcher, TantivyDocument, Term,
};

use crate::chunk;
use crate::entity::entities;
use crate::extract::Extracted;
use crate::fuse::{fuse, Ranked};
use crate::proto::{Hit, QueryParams, Timings, Weights};
use crate::vectors::{text_hash, ChunkKey, NodeChunks, Texts, VectorHit};

/// Bumped when the fields change: an index of another version is rebuilt.
pub const SCHEMA_VERSION: u32 = 1;

/// The writer's memory for buffered documents: tantivy's floor is 15 MB per
/// indexing thread, and the tender runs one.
const WRITER_MEMORY: usize = 32 << 20;

/// The sources a query may name.
pub const SOURCES: &[&str] = &["bm25", "entity", "vector"];

/// How many hits each source ranks for a query that wants `k`.
pub fn fetch_for(k: usize) -> usize {
    (k.clamp(1, 100) * 3).max(30)
}

#[derive(Debug, Clone, Copy)]
pub struct Fields {
    pub node_id: Field,
    pub chunk: Field,
    pub session: Field,
    pub position: Field,
    pub kind: Field,
    pub origin: Field,
    pub author: Field,
    pub place: Field,
    pub tool: Field,
    pub time: Field,
    pub external: Field,
    pub text: Field,
    pub entities: Field,
}

pub fn schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let f = Fields {
        node_id: b.add_text_field("node_id", STRING | STORED),
        chunk: b.add_u64_field("chunk", INDEXED | STORED),
        session: b.add_text_field("session", STRING | STORED),
        position: b.add_u64_field("position", INDEXED | STORED | FAST),
        kind: b.add_text_field("kind", STRING | STORED),
        origin: b.add_text_field("origin", STRING | STORED),
        author: b.add_text_field("author", STRING | STORED),
        place: b.add_text_field("place", STRING | STORED),
        tool: b.add_text_field("tool", STRING | STORED),
        time: b.add_u64_field("time", STORED | FAST),
        external: b.add_bool_field("external", INDEXED | STORED),
        text: b.add_text_field("text", TEXT | STORED),
        entities: b.add_text_field("entities", STRING | STORED),
    };
    (b.build(), f)
}

/// Open the index in `dir`, or create it there.
pub fn open_or_create(dir: &Path) -> tantivy::Result<(Index, Fields)> {
    let (schema, fields) = schema();
    std::fs::create_dir_all(dir)?;
    let index = if dir.join("meta.json").exists() {
        let index = Index::open_in_dir(dir)?;
        if index.schema() != schema {
            return Err(tantivy::TantivyError::SchemaError(
                "the index's fields are not this build's".into(),
            ));
        }
        index
    } else {
        Index::create_in_dir(dir, schema)?
    };
    Ok((index, fields))
}

/// The read side: shared with the socket's threads.
pub struct Engine {
    index: Index,
    reader: IndexReader,
    f: Fields,
}

/// The write side: the ingest thread's alone.
pub struct Writer {
    writer: IndexWriter,
    f: Fields,
}

impl Writer {
    pub fn new(index: &Index, f: Fields) -> tantivy::Result<Self> {
        Ok(Self {
            writer: index.writer_with_num_threads(1, WRITER_MEMORY)?,
            f,
        })
    }

    /// Replace node `e`, written at `position`, by its chunks; each chunk's
    /// key for the vector side.
    pub fn replace(
        &self,
        position: u64,
        place: Option<&str>,
        e: &Extracted,
    ) -> tantivy::Result<Vec<ChunkKey>> {
        let f = &self.f;
        self.writer
            .delete_term(Term::from_field_text(f.node_id, &e.node_id));
        let chunks = chunk::chunks(&e.text, chunk::MAX_TOKENS);
        let mut keys = Vec::with_capacity(chunks.len());
        for (i, text) in chunks.iter().enumerate() {
            keys.push(chunk_key(i as u64, text));
            let mut d = TantivyDocument::new();
            d.add_text(f.node_id, &e.node_id);
            d.add_u64(f.chunk, i as u64);
            d.add_text(f.session, &e.session_id);
            d.add_u64(f.position, position);
            d.add_text(f.kind, &e.kind);
            d.add_text(f.origin, &e.origin);
            if let Some(a) = &e.author {
                d.add_text(f.author, a);
            }
            if let Some(p) = place {
                d.add_text(f.place, p);
            }
            if let Some(t) = &e.tool {
                d.add_text(f.tool, t);
            }
            d.add_u64(f.time, e.created_at_ms);
            d.add_bool(f.external, e.external);
            d.add_text(f.text, text);
            for term in entities(text) {
                d.add_text(f.entities, term);
            }
            self.writer.add_document(d)?;
        }
        Ok(keys)
    }

    /// Node `id` leaves the index, all its chunks, at the next commit.
    pub fn delete_node(&self, id: &str) {
        self.writer
            .delete_term(Term::from_field_text(self.f.node_id, id));
    }

    /// One chunk of node `id` leaves the index, at the next commit.
    pub fn delete_chunk(&self, id: &str, chunk: u64) -> tantivy::Result<()> {
        let q: Vec<(Occur, Box<dyn Query>)> = vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.f.node_id, id),
                    IndexRecordOption::Basic,
                )),
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.f.chunk, chunk),
                    IndexRecordOption::Basic,
                )),
            ),
        ];
        self.writer.delete_query(Box::new(BooleanQuery::new(q)))?;
        Ok(())
    }

    /// Make everything added durable and visible to a reload.
    pub fn commit(&mut self) -> tantivy::Result<()> {
        self.writer.commit()?;
        Ok(())
    }

    /// Drop every document (a rebuild); durable once committed.
    pub fn clear(&mut self) -> tantivy::Result<()> {
        self.writer.delete_all_documents()?;
        self.writer.commit()?;
        Ok(())
    }

    /// Forget what was added since the last commit.
    pub fn rollback(&mut self) -> tantivy::Result<()> {
        self.writer.rollback()?;
        Ok(())
    }
}

/// One stored chunk, for status and tests.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StoredChunk {
    pub node_id: String,
    pub chunk: u64,
    pub session: String,
    pub position: u64,
    pub kind: String,
    pub origin: String,
    pub author: Option<String>,
    pub place: Option<String>,
    pub tool: Option<String>,
    pub time: u64,
    pub external: bool,
    pub text: String,
    pub entities: Vec<String>,
}

impl Engine {
    pub fn new(index: Index, f: Fields) -> tantivy::Result<Self> {
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        Ok(Self { index, reader, f })
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    pub fn fields(&self) -> Fields {
        self.f
    }

    /// See the last commit.
    pub fn reload(&self) -> tantivy::Result<()> {
        self.reader.reload()
    }

    /// (chunks, nodes) in the index.
    pub fn counts(&self) -> tantivy::Result<(u64, u64)> {
        let s = self.reader.searcher();
        let nodes = s.search(
            &TermQuery::new(
                Term::from_field_u64(self.f.chunk, 0),
                IndexRecordOption::Basic,
            ),
            &Count,
        )?;
        Ok((s.num_docs(), nodes as u64))
    }

    /// The last commit holds node `id`.
    pub fn holds(&self, id: &str) -> tantivy::Result<bool> {
        let q = TermQuery::new(
            Term::from_field_text(self.f.node_id, id),
            IndexRecordOption::Basic,
        );
        Ok(self.reader.searcher().search(&q, &Count)? > 0)
    }

    /// Node `id`'s chunks, as the last commit holds them: each one's index
    /// and text.
    pub fn node_chunks(&self, id: &str) -> tantivy::Result<Vec<(u64, String)>> {
        let s = self.reader.searcher();
        let q = TermQuery::new(
            Term::from_field_text(self.f.node_id, id),
            IndexRecordOption::Basic,
        );
        let n = s.search(&q, &Count)?;
        let mut out = Vec::with_capacity(n);
        for (_, a) in s.search(&q, &TopDocs::with_limit(n.max(1)).order_by_score())? {
            let c = self.stored(&s.doc(a)?);
            out.push((c.chunk, c.text));
        }
        out.sort();
        Ok(out)
    }

    /// Every chunk whose text is `text`, exactly: (node, chunk), sorted. The
    /// text's words find the candidates, and each candidate's stored text is
    /// compared whole; a text with no word is looked for in every chunk.
    pub fn chunks_holding(&self, text: &str) -> tantivy::Result<Vec<(String, u64)>> {
        let terms = self.text_terms(text)?;
        let mut out: Vec<(String, u64)> = if terms.is_empty() {
            self.dump()?
                .into_iter()
                .filter(|c| c.text == text)
                .map(|c| (c.node_id, c.chunk))
                .collect()
        } else {
            let s = self.reader.searcher();
            let all: Box<dyn Query> = Box::new(BooleanQuery::new(
                terms
                    .into_iter()
                    .map(|t| {
                        let q: Box<dyn Query> =
                            Box::new(TermQuery::new(t, IndexRecordOption::Basic));
                        (Occur::Must, q)
                    })
                    .collect(),
            ));
            let n = s.search(&all, &Count)?;
            let mut out = Vec::new();
            for (_, a) in s.search(&all, &TopDocs::with_limit(n.max(1)).order_by_score())? {
                let c = self.stored(&s.doc(a)?);
                if c.text == text {
                    out.push((c.node_id, c.chunk));
                }
            }
            out
        };
        out.sort();
        Ok(out)
    }

    /// Every chunk, sorted: what a rebuild must reproduce.
    pub fn dump(&self) -> tantivy::Result<Vec<StoredChunk>> {
        let s = self.reader.searcher();
        let mut out = Vec::new();
        for (ord, seg) in s.segment_readers().iter().enumerate() {
            for doc in seg.doc_ids_alive() {
                let d: TantivyDocument = s.doc(DocAddress::new(ord as u32, doc))?;
                out.push(self.stored(&d));
            }
        }
        out.sort();
        Ok(out)
    }

    fn stored(&self, d: &TantivyDocument) -> StoredChunk {
        let f = &self.f;
        let text = |field| {
            d.get_first(field)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        let num = |field| d.get_first(field).and_then(|v| v.as_u64()).unwrap_or(0);
        let mut ents: Vec<String> = d
            .get_all(f.entities)
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        ents.sort();
        StoredChunk {
            node_id: text(f.node_id).unwrap_or_default(),
            chunk: num(f.chunk),
            session: text(f.session).unwrap_or_default(),
            position: num(f.position),
            kind: text(f.kind).unwrap_or_default(),
            origin: text(f.origin).unwrap_or_default(),
            author: text(f.author),
            place: text(f.place),
            tool: text(f.tool),
            time: num(f.time),
            external: d
                .get_first(f.external)
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            text: text(f.text).unwrap_or_default(),
            entities: ents,
        }
    }

    /// The filters every source shares, as clauses that score nothing:
    /// `as_of` (only positions before it), the sessions to keep or leave
    /// out, the kinds, and the external flag.
    fn filters(&self, p: &QueryParams) -> Vec<(Occur, Box<dyn Query>)> {
        let f = &self.f;
        let quiet =
            |q: Box<dyn Query>| -> Box<dyn Query> { Box::new(ConstScoreQuery::new(q, 0.0)) };
        let any_of = |field: Field, values: &[String]| -> Box<dyn Query> {
            Box::new(BooleanQuery::new(
                values
                    .iter()
                    .map(|v| {
                        let q: Box<dyn Query> = Box::new(TermQuery::new(
                            Term::from_field_text(field, v),
                            IndexRecordOption::Basic,
                        ));
                        (Occur::Should, q)
                    })
                    .collect(),
            ))
        };
        let mut out: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        if let Some(as_of) = p.as_of {
            out.push((
                Occur::Must,
                quiet(Box::new(RangeQuery::new(
                    Bound::Unbounded,
                    Bound::Excluded(Term::from_field_u64(f.position, as_of)),
                ))),
            ));
        }
        for s in &p.exclude_sessions {
            out.push((
                Occur::MustNot,
                Box::new(TermQuery::new(
                    Term::from_field_text(f.session, s),
                    IndexRecordOption::Basic,
                )),
            ));
        }
        let filters = &p.filters;
        if !filters.sessions.is_empty() {
            out.push((Occur::Must, quiet(any_of(f.session, &filters.sessions))));
        }
        if !filters.kinds.is_empty() {
            out.push((Occur::Must, quiet(any_of(f.kind, &filters.kinds))));
        }
        if let Some(ext) = filters.external {
            out.push((
                Occur::Must,
                quiet(Box::new(TermQuery::new(
                    Term::from_field_bool(f.external, ext),
                    IndexRecordOption::Basic,
                ))),
            ));
        }
        out
    }

    /// The text's tokens, as the text field indexes them.
    fn text_terms(&self, text: &str) -> tantivy::Result<Vec<Term>> {
        let mut analyzer = self.index.tokenizer_for_field(self.f.text)?;
        let mut stream = analyzer.token_stream(text);
        let mut seen = BTreeSet::new();
        while stream.advance() {
            seen.insert(stream.token().text.clone());
        }
        Ok(seen
            .into_iter()
            .map(|t| Term::from_field_text(self.f.text, &t))
            .collect())
    }

    /// A chunk's document, if `searcher` holds it and it passes `extra`.
    fn address(
        &self,
        searcher: &Searcher,
        node_id: &str,
        chunk: u32,
        extra: Vec<(Occur, Box<dyn Query>)>,
    ) -> tantivy::Result<Option<DocAddress>> {
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(self.f.node_id, node_id),
                    IndexRecordOption::Basic,
                )),
            ),
            (
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_u64(self.f.chunk, u64::from(chunk)),
                    IndexRecordOption::Basic,
                )),
            ),
        ];
        clauses.extend(extra);
        let top = searcher.search(
            &BooleanQuery::new(clauses),
            &TopDocs::with_limit(1).order_by_score(),
        )?;
        Ok(top.first().map(|(_, a)| *a))
    }

    /// Hits for `p` from `sources` (validated by the caller): BM25's and the
    /// entities' top documents, and `vector`'s hits, joined by node and
    /// chunk and filtered as the others are; fused with `weights`, the best
    /// `k`.
    pub fn query(
        &self,
        p: &QueryParams,
        sources: &[&str],
        vector: Option<&[VectorHit]>,
        weights: &Weights,
    ) -> anyhow::Result<(Vec<Hit>, Timings)> {
        let t0 = Instant::now();
        let mut timings = Timings::default();
        let k = p.k.clamp(1, 100);
        let fetch = fetch_for(k);
        let searcher = self.reader.searcher();
        let query_entities: BTreeSet<String> = entities(&p.text);
        // source -> its weight, and its ranked (address, score)
        let mut ranked: Vec<Ranked<'_, DocAddress>> = Vec::new();
        for source in sources {
            let ts = Instant::now();
            let terms: Vec<Term> = match *source {
                "bm25" => self.text_terms(&p.text)?,
                "entity" => query_entities
                    .iter()
                    .map(|e| Term::from_field_text(self.f.entities, e))
                    .collect(),
                _ => {
                    if let Some(hits) = vector {
                        let mut list = Vec::with_capacity(hits.len());
                        for h in hits {
                            if let Some(a) =
                                self.address(&searcher, &h.node_id, h.chunk, self.filters(p))?
                            {
                                list.push((a, h.score));
                            }
                        }
                        timings.vector_ms = ts.elapsed().as_secs_f64() * 1000.0;
                        ranked.push(("vector", weights.vector, list));
                    }
                    continue;
                }
            };
            let hits = if terms.is_empty() {
                Vec::new()
            } else {
                let option = if *source == "bm25" {
                    IndexRecordOption::WithFreqs
                } else {
                    IndexRecordOption::Basic
                };
                let any: Box<dyn Query> = Box::new(BooleanQuery::new(
                    terms
                        .into_iter()
                        .map(|t| {
                            let q: Box<dyn Query> = Box::new(TermQuery::new(t, option));
                            (Occur::Should, q)
                        })
                        .collect(),
                ));
                let mut clauses = vec![(Occur::Must, any)];
                clauses.extend(self.filters(p));
                searcher.search(
                    &BooleanQuery::new(clauses),
                    &TopDocs::with_limit(fetch).order_by_score(),
                )?
            };
            let ms = ts.elapsed().as_secs_f64() * 1000.0;
            match *source {
                "bm25" => timings.bm25_ms = ms,
                _ => timings.entity_ms = ms,
            }
            ranked.push((
                source,
                weights.get(source),
                hits.into_iter().map(|(s, a)| (a, f64::from(s))).collect(),
            ));
        }

        // Fuse: Σ w / (60 + rank), rank from 1.
        let tf = Instant::now();
        // Ties go to the earlier position, so an index rebuilt with the same
        // nodes orders them the same (document addresses differ). Each
        // segment's position column is opened once, not once a key: opened
        // per key it was most of the stage (1.4 ms at k = 100).
        let mut columns: Vec<Option<Column<u64>>> = vec![None; searcher.segment_readers().len()];
        let mut order = Vec::new();
        for f in fuse(&ranked) {
            let col = match &mut columns[f.key.segment_ord as usize] {
                Some(c) => c,
                slot => slot.insert(
                    searcher
                        .segment_reader(f.key.segment_ord)
                        .fast_fields()
                        .u64("position")?,
                ),
            };
            let position = col.first(f.key.doc_id).unwrap_or(u64::MAX);
            order.push((f.key, f.score, f.sources, position));
        }
        order.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.3.cmp(&b.3)).then(a.0.cmp(&b.0)));
        order.truncate(k);
        timings.fuse_ms = tf.elapsed().as_secs_f64() * 1000.0;

        let tl = Instant::now();
        let mut hits = Vec::with_capacity(order.len());
        for (addr, score, sources, _) in order {
            let d: TantivyDocument = searcher.doc(addr)?;
            let c = self.stored(&d);
            let matched = c
                .entities
                .iter()
                .filter(|e| query_entities.contains(*e))
                .cloned()
                .collect();
            hits.push(Hit {
                node_id: c.node_id,
                chunk: c.chunk,
                session_id: c.session,
                position: c.position,
                kind: c.kind,
                origin: c.origin,
                author: c.author,
                place: c.place,
                tool: c.tool,
                time_ms: c.time,
                external: c.external,
                text: c.text,
                entities_matched: matched,
                sources,
                fused: score,
            });
        }
        timings.load_ms = tl.elapsed().as_secs_f64() * 1000.0;
        timings.total_ms = t0.elapsed().as_secs_f64() * 1000.0;
        Ok((hits, timings))
    }
}

/// A chunk's key for the vector side: its index, its text's hash, and the
/// chunker's token estimate.
pub fn chunk_key(chunk: u64, text: &str) -> ChunkKey {
    ChunkKey {
        chunk: chunk as u32,
        hash: text_hash(text),
        tokens: chunk::tokens(text).min(u32::MAX as usize) as u32,
    }
}

impl Texts for Engine {
    fn chunk_text(&self, node_id: &str, chunk: u32) -> Option<String> {
        let s = self.reader.searcher();
        let addr = self.address(&s, node_id, chunk, Vec::new()).ok()??;
        let d: TantivyDocument = s.doc(addr).ok()?;
        d.get_first(self.f.text)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    /// Every chunk, by node, read from the stored documents.
    fn all_nodes(&self) -> anyhow::Result<Vec<NodeChunks>> {
        let s = self.reader.searcher();
        let mut by_node: BTreeMap<String, NodeChunks> = BTreeMap::new();
        for (ord, seg) in s.segment_readers().iter().enumerate() {
            for doc in seg.doc_ids_alive() {
                let d: TantivyDocument = s.doc(DocAddress::new(ord as u32, doc))?;
                let c = self.stored(&d);
                let n = by_node
                    .entry(c.node_id.clone())
                    .or_insert_with(|| NodeChunks {
                        node_id: c.node_id.clone(),
                        position: c.position,
                        session: c.session.clone(),
                        kind: c.kind.clone(),
                        external: c.external,
                        chunks: Vec::new(),
                    });
                n.chunks.push(chunk_key(c.chunk, &c.text));
            }
        }
        let mut out: Vec<NodeChunks> = by_node.into_values().collect();
        for n in &mut out {
            n.chunks.sort_by_key(|c| c.chunk);
        }
        Ok(out)
    }
}
