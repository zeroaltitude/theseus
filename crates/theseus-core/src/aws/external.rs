//! Data-plane reads are outside text (AWS design §3.9, T1; C3 = 14c): an
//! object's body read as text, log lines, and CloudTrail's events (whose
//! user agents and parameters a caller sets) bring words into the context
//! that Theseus did not write. Their results carry the marker a fetched
//! page's does (`theseus_tools::External`), so the runtime writes the
//! session's hold in the frame that writes the result (`crate::external`),
//! and a later call that acts waits for the operator. Control-plane reads
//! (Describe, List) are not marked (§6, question 4), nor is an object
//! written to a file, whose bytes never reach the context.

use theseus_tools::External;

/// The marker for a data-plane read of `url`: `s3://bucket/key`,
/// `logs:<region>:<group>`, or `cloudtrail:<region>:LookupEvents`.
pub fn marker(url: &str) -> External {
    External { url: url.into() }
}
