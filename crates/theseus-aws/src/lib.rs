//! Theseus's AWS client (AWS design §3.1, "Reach: one generic caller"): one
//! caller for every operation of every service, driven by the models in
//! [`theseus_aws_catalog`], with no per-service SDK crates.
//!
//! A call is a service, an operation, and its input as JSON keyed by member
//! names. The client:
//! 1. finds the operation in the catalog and checks the input against its
//!    shape ([`value`]'s conventions say how timestamps and blobs are
//!    written);
//! 2. fills an idempotency token from the call's correlation id;
//! 3. serializes the request in the service's protocol (query, EC2, JSON 1.0
//!    and 1.1, REST-JSON, REST-XML) at the operation's endpoint;
//! 4. signs it with AWS's `aws-sigv4`, with the attribution user agent
//!    (`theseus/<version> exec/<execution> call/<correlation id>`, §3.5);
//! 5. sends it on the workspace's reqwest, retrying by the operation's retry
//!    class, never more;
//! 6. reads the answer through the output shape into JSON, or AWS's error into
//!    its code, its message, its request id, and the enforcer that refused;
//! 7. follows the paginator for the pages the call asks for.
//!
//! Nothing here runs at start, and nothing touches the network before a
//! call. Event streams, SigV2, bearer tokens, endpoint discovery, and S3's
//! directory buckets stay with the CLI, and say so.

mod build;
mod client;
mod creds;
mod error;
mod json;
mod paginate;
mod query;
mod request;
mod response;
mod rest;
mod retry;
mod scalar;
pub mod sign;
pub mod value;
mod xml;

pub use client::{Attribution, Call, Client, ClientConfig, Output, Prepared};
pub use creds::Credentials;
pub use error::{parse_denial, AwsError, CallError, Denial, Enforcer, ErrorRetry};
pub use request::HttpRequest;
pub use retry::RetryPolicy;
pub use theseus_aws_catalog as catalog;
