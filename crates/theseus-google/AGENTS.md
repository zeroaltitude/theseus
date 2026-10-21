# theseus-google

Google Workspace as the harness's own capability, on the terms of AWS (the Google design, `CLOUD_TASK.md`'s §1 to §4).
No Google SDK: plain JSON REST on the workspace `reqwest`.

Key modules: `token.rs`, `client.rs`, `oauth.rs`, `classify.rs`, `fake.rs`. Read by: core (`google/`).

- `scopes.rs`: the service list, its scopes, and each tool's accepted scopes. The request is exactly the declared set.
- `secret.rs`: the client's id and secret from one secret's text (two labelled lines, or the console's JSON).
- `token.rs`: the access token, in memory only (`Zeroizing`, a masked `Debug`), refreshed single-flight about
  five minutes before it expires.
- `oauth.rs`: PKCE S256, the consent URL, the one-shot loopback listener, the paste parser, the code's redeem.
- `client.rs`, `retry.rs`, `error.rs`: one caller for every method; 429 and 5xx back off with full jitter; a call
  that may not repeat (`Repeat::NonRepeatable`) that may have gone out is `Unknown`, never sent again.
- `methods.rs`, `classify.rs`: the method table and the class of a call: a read, a write, or authority (a send,
  a delete, a permission or ACL change, an invitation): authority is the one thing no config loosens.
- `fake.rs` (feature `fake`, and this crate's tests): a loopback Google: the token endpoint, a route table, a
  request log, and switches for 429, 5xx, `invalid_grant` and `SERVICE_DISABLED`.

- `fake/gmail.rs`: the fake's mailbox (`Fake::mail()`): threads, drafts, a sent log and labels, answering the Gmail REST calls
  the mail tools make (the fake reads a draft's MIME, so threading headers and encoded words come back as a client sees
  them). `Mailbox::after_send` makes a send record and then fail on the way back, for `outcome_unknown`.

A token, a client secret or a code is never on a command line, in a log, or in an error. A test plants one and greps.
