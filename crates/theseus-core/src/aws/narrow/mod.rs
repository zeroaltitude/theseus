//! Self-narrow (theseus-pw1q.15): Theseus proposes a narrower policy for its
//! own hands from what CloudTrail shows they called, checks it, and sends it
//! to the owner's card. An IAM change is the hard keel: nothing here applies
//! a policy but the owner's yes.
//!
//! - [`usage`]: the distinct `service:Action` pairs and resource ARNs a
//!   hand's role called over a window (default 14 days), from CloudTrail's
//!   event history, paged at its rate and up to a page cap. Data events the
//!   history does not record are named as not visible, never assumed unused.
//! - [`build`]: the candidate, from the observed set and the policy it
//!   replaces: observed actions, narrowed resources where they sit inside what
//!   is allowed today, the keep list taken from the code, the data actions
//!   kept as they are, the guards copied. [`policy::wider`] proves it is never
//!   wider than today on any statement.
//! - [`verify`]: IAM Access Analyzer's `ValidatePolicy` (errors and security
//!   warnings block; suggestions are listed) and `SimulateCustomPolicy` of
//!   every observed call against the candidate (a denial means no proposal).
//! - [`card`]: the diff in words and as JSON, and the owner's card.
//! - [`apply`], [`watch`]: the apply through a change set as an owner act, and
//!   the week's watch for `AccessDenied` after it.
//! - [`pass`]: the weekly pass and the owner's verbs, under `rsi::gate`.

pub mod apply;
pub mod build;
pub mod glob;
pub mod live;
pub mod policy;
pub mod proposal;
pub mod run;
pub mod usage;
pub mod verify;
