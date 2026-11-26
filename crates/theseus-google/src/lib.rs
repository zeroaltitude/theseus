//! Theseus's Google Workspace client. See `AGENTS.md`.

pub mod classify;
pub mod client;
pub mod error;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod http1;
pub mod methods;
pub mod oauth;
pub mod retry;
pub mod scopes;
pub mod secret;
pub mod token;

pub use classify::{classify, Class, Classified};
pub use client::{Body, Client, Endpoints, Failure, Request, Response};
pub use error::{GoogleError, Kind};
pub use retry::{Policy, Repeat};
pub use scopes::Service;
pub use secret::ClientSecret;
pub use token::{AccessToken, Tokens};

#[cfg(test)]
mod tests;
