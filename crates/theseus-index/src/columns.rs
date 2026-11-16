//! What a start reads of each segment (issue 21 of 1.0.7): fast-field columns, never the stored
//! documents. The vector side's rows need each chunk's text fingerprint and where its node
//! lives, and the stored documents hold the chunk's whole text beside them: reading one reads
//! the other, and the kernel keeps those pages mapped for hours. The columns are written beside
//! the text (`Writer::replace`) and read alone, so a start touches a few MiB of them.

use anyhow::Context as _;
use tantivy::columnar::{Column, StrColumn};
use tantivy::fastfield::FastFieldReaders;
use tantivy::{DocId, SegmentReader};

use crate::vectors::{ChunkKey, NodeChunks};

/// A chunk's text hash, as the two columns hold it.
pub(crate) fn halves(hash: u128) -> (u64, u64) {
    ((hash >> 64) as u64, hash as u64)
}

/// An alive document of a segment: its node (by that segment's term order, which is the node
/// ids' byte order), its chunk, and where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Doc {
    pub ord: u32,
    pub chunk: u32,
    pub doc: DocId,
}

/// One segment's columns.
pub(crate) struct Columns {
    node: StrColumn,
    session: StrColumn,
    kind: StrColumn,
    chunk: Column<u64>,
    position: Column<u64>,
    external: Column<bool>,
    hash_hi: Column<u64>,
    hash_lo: Column<u64>,
    tokens: Column<u64>,
    time: Column<u64>,
}

fn strings(fast: &FastFieldReaders, name: &str) -> anyhow::Result<StrColumn> {
    fast.str(name)?
        .with_context(|| format!("the index has no `{name}` column"))
}

impl Columns {
    /// The segment's columns, or `None` for a segment that holds no document.
    pub(crate) fn open(seg: &SegmentReader) -> anyhow::Result<Option<Self>> {
        let fast = seg.fast_fields();
        let Some(node) = fast.str("node_id")? else {
            anyhow::ensure!(
                seg.num_docs() == 0,
                "a segment with documents has no `node_id` column"
            );
            return Ok(None);
        };
        Ok(Some(Self {
            node,
            session: strings(fast, "session")?,
            kind: strings(fast, "kind")?,
            chunk: fast.u64("chunk")?,
            position: fast.u64("position")?,
            external: fast.bool("external")?,
            hash_hi: fast.u64("hash_hi")?,
            hash_lo: fast.u64("hash_lo")?,
            tokens: fast.u64("tokens")?,
            time: fast.u64("time")?,
        }))
    }

    /// The segment's alive documents, in the order of their node and then their chunk.
    pub(crate) fn docs(&self, seg: &SegmentReader) -> anyhow::Result<Vec<Doc>> {
        let mut out = Vec::with_capacity(seg.num_docs() as usize);
        for doc in seg.doc_ids_alive() {
            let ord = self
                .node
                .ords()
                .first(doc)
                .context("a document has no node id")?;
            out.push(Doc {
                ord: u32::try_from(ord).context("more node ids than a segment can hold")?,
                chunk: self.chunk.first(doc).unwrap_or(0) as u32,
                doc,
            });
        }
        out.sort_unstable();
        Ok(out)
    }

    /// Node `ord`'s id, in `into`.
    pub(crate) fn node_id(&self, ord: u32, into: &mut String) -> anyhow::Result<()> {
        word(&self.node, u64::from(ord), into)
    }

    /// The chunk of `doc`, as the vector side keys it.
    fn key(&self, doc: DocId) -> ChunkKey {
        let hi = self.hash_hi.first(doc).unwrap_or(0);
        let lo = self.hash_lo.first(doc).unwrap_or(0);
        ChunkKey {
            chunk: self.chunk.first(doc).unwrap_or(0) as u32,
            hash: u128::from(hi) << 64 | u128::from(lo),
            tokens: self.tokens.first(doc).unwrap_or(0) as u32,
        }
    }

    /// Node `id`, from its documents in this segment (`group`, one node's, by chunk): where it
    /// lives from the first, its chunks' keys from all.
    pub(crate) fn node_chunks(&self, id: &str, group: &[Doc]) -> anyhow::Result<NodeChunks> {
        let first = group[0].doc;
        let (mut session, mut kind) = (String::new(), String::new());
        for (col, into) in [(&self.session, &mut session), (&self.kind, &mut kind)] {
            let ord = col
                .ords()
                .first(first)
                .context("a document has no session or kind")?;
            word(col, ord, into)?;
        }
        Ok(NodeChunks {
            node_id: id.to_string(),
            position: self.position.first(first).unwrap_or(0),
            time_ms: self.time.first(first).unwrap_or(0),
            session,
            kind,
            external: self.external.first(first).unwrap_or(false),
            chunks: group.iter().map(|d| self.key(d.doc)).collect(),
        })
    }
}

/// A segment's documents, walked a node at a time.
struct Head {
    cols: Columns,
    docs: Vec<Doc>,
    at: usize,
    /// The id of the node `at` is in; empty once the segment is done.
    id: String,
}

impl Head {
    fn done(&self) -> bool {
        self.at >= self.docs.len()
    }

    /// The documents of the node `at` is in.
    fn group(&self) -> &[Doc] {
        let ord = self.docs[self.at].ord;
        let n = self.docs[self.at..].partition_point(|d| d.ord == ord);
        &self.docs[self.at..self.at + n]
    }

    /// Past this node, on to the next one's id.
    fn step(&mut self) -> anyhow::Result<()> {
        self.at += self.group().len();
        if let Some(d) = self.docs.get(self.at) {
            self.cols.node_id(d.ord, &mut self.id)?;
        }
        Ok(())
    }
}

/// Every node the segments hold, one at a time in node-id order: a merge of each segment's
/// documents, which are in that order already. Holds each segment's list of (node, chunk,
/// document), twelve bytes a chunk, and nothing else of the nodes.
pub(crate) struct Nodes {
    heads: Vec<Head>,
    least: String,
    nodes: usize,
    chunks: usize,
}

impl Nodes {
    pub(crate) fn open<'a>(
        segs: impl IntoIterator<Item = &'a SegmentReader>,
    ) -> anyhow::Result<Self> {
        let mut heads = Vec::new();
        let (mut nodes, mut chunks) = (0, 0);
        for seg in segs {
            let Some(cols) = Columns::open(seg)? else {
                continue;
            };
            let docs = cols.docs(seg)?;
            nodes += docs.chunk_by(|a, b| a.ord == b.ord).count();
            chunks += docs.len();
            let mut head = Head {
                cols,
                docs,
                at: 0,
                id: String::new(),
            };
            if let Some(d) = head.docs.first() {
                head.cols.node_id(d.ord, &mut head.id)?;
            }
            heads.push(head);
        }
        Ok(Self {
            heads,
            least: String::new(),
            nodes,
            chunks,
        })
    }

    /// (nodes, chunks) alive: a node in two segments counts twice.
    pub(crate) fn sizes(&self) -> (usize, usize) {
        (self.nodes, self.chunks)
    }

    /// The next node, with its chunks from every segment that holds some, by chunk.
    pub(crate) fn next(&mut self) -> anyhow::Result<Option<NodeChunks>> {
        let mut found = false;
        for h in self.heads.iter().filter(|h| !h.done()) {
            if !found || h.id < self.least {
                self.least.clone_from(&h.id);
                found = true;
            }
        }
        if !found {
            return Ok(None);
        }
        let mut node: Option<NodeChunks> = None;
        for h in &mut self.heads {
            if h.done() || h.id != self.least {
                continue;
            }
            let more = h.cols.node_chunks(&self.least, h.group())?;
            match &mut node {
                Some(have) => {
                    have.chunks.extend(more.chunks);
                    have.chunks.sort_by_key(|c| c.chunk);
                }
                None => node = Some(more),
            }
            h.step()?;
        }
        Ok(node)
    }
}

/// Term `ord` of `col`, in `into`.
fn word(col: &StrColumn, ord: u64, into: &mut String) -> anyhow::Result<()> {
    into.clear();
    anyhow::ensure!(col.ord_to_str(ord, into)?, "a column has no term {ord}");
    Ok(())
}
