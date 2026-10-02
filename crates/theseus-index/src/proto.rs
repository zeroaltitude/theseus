//! The tender's protocol (M6 §2.2): JSON-RPC 2.0 over NDJSON on
//! `<index>/sock`, mode 0600, with the core its only client. Since the
//! wire-in (roadmap row 51) its shapes are `theseus-protocol`'s `index`
//! module, one definition for the tender, the core, and the CLI. Here they
//! keep the names the tender's code has always used.

pub use theseus_protocol::index::{
    method, IndexBackfill as Backfill, IndexChunkRef as ChunkRef, IndexCompactions as Compactions,
    IndexEmbedParams as EmbedParams, IndexEmbedResult as EmbedResult,
    IndexEmbedStats as EmbedStats, IndexEmbedTask as Task, IndexFilters as Filters,
    IndexForgetParams as ForgetParams, IndexForgetResult as ForgetResult, IndexHit as Hit,
    IndexLag as Lag, IndexNeighbour as Neighbour, IndexNeighboursParams as NeighboursParams,
    IndexNeighboursResult as NeighboursResult, IndexQueryParams as QueryParams,
    IndexQueryResult as QueryResult, IndexRebuildResult as RebuildResult, IndexReembed as Reembed,
    IndexSourceRank as SourceRank, IndexStamp as Stamp, IndexStatus, IndexTimings as Timings,
    IndexVectorStatus as VectorStatus, IndexWarmResult as WarmResult, IndexWeights as Weights,
};
