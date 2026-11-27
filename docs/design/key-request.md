# A request that carries a key from the vault (`http.request`, issue 65)

"Using a secret should not be an ask" (the owner, 2026-11-27). `vault.use`'s run form gives a key to one run of a
program (`vault-use.md`). `http.request` is the other half: the daemon itself sends **one HTTPS request to one public
address** with a key from the vault, and no program runs. No card, no settings, nothing stays; the owner is told.

## What the tool is

A harness tool beside `http.fetch`, which is unchanged. A fetch carries nothing of the caller's (no header, no body), and
that is what lets a fetch of a LAN address run and be told (issue 57); a request with a key and a body is a different
thing, so it is a different tool, with its own `[policy.tools]` line. It loads with the vault's family, on demand.

`{method, url, key: {vault, item, field, section?, as}, headers?, json? | text? | form? | file?, content_type?, reason}`

- **`method`**: GET, POST, PUT, PATCH or DELETE.
- **`url`**: `https://`, at a public address, with no user or password and no `#` part, at most 2,048 bytes. Its query
  is the model's own; the key never goes there.
- **`key.as`**, one of a closed list: `{"bearer": {}}` (`Authorization: Bearer <key>`), `{"header": {"name", "prefix"?}}`
  (one named header, an optional prefix of at most 32 characters before the key), or `{"basic": {"username"}}` (basic
  auth, the key its password).
- **`headers`**: at most 16 ordinary headers, one line each. Not `Authorization`, `Proxy-Authorization`, `Cookie`,
  `Host`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Content-Type` (a body's type is `content_type`), nor the
  key's own header.
- **A body**, at most one, and none with GET: `json` (sent as `application/json`), `text` (with `content_type`), a
  multipart `form`, or one `file` sent whole (`application/octet-stream` unless `content_type` says otherwise). A JSON or
  text body, and a form's JSON or text part, is at most 1 MiB.
- **A form** is at most 8 parts, each `{name, json | text | file, content_type?, filename?}`, sent as
  `multipart/form-data` with a boundary the daemon makes. The body is written by hand (no new dependency).
- **A file** is resolved as `fs.read` resolves one and is a Read resource in the plan, so the roots and the floor judge
  it as any read (a file in Theseus's own state asks at the floor). The files together are at most 32 MiB.
- **Placeholders**: `{{file.bytes}}`, `{{file.sha256}}` (hex) and `{{file.sha256_base64}}` are the file's byte count and
  SHA-256, filled in a `json` or `text` value, a form's other parts and the header values. With more than one file the
  placeholder names its part: `{{file:<part>.bytes}}`. A JSON string that is only `{{file.bytes}}` becomes a number. The
  result always says each file's bytes and SHA-256. There is no MD5.

## Where a key may go

- **https, at one public host.** The client follows no redirect and uses no proxy, and every name is resolved by the
  checked resolver, which refuses a name with any non-public answer on the very answer it connects with. One request a
  call.
- **Never to a private, loopback or LAN address**, nor `localhost`: that is an input error in fixed words, before
  anything is read. It is the one "not sent" that is not an ask, because there is no safe way to do it: such an address
  almost never has a certificate that ties the key to the service, and the key would cross the LAN in clear or to
  whatever answers there.
- **No redirect is followed.** A 3xx is returned with its status, its `Location` and the words "Theseus follows no
  redirect with a key". A named header is not stripped on a hop the way `Authorization` is, and a 307 after a POST would
  send the body to a host the owner never named.
- **Not in the query.** The URL is written into the plan, the gate's record, the call's rows, the node's marker and
  error text; all of that is the stored record. A service that takes its key only in the query is not served.

## What asks, and what is told

- **GET reads.** POST, PUT and PATCH run at the tool's posture and are told; the reversibility layer stays on their path as
  for every write. **DELETE asks every time**, one card with Approve and Decline: Theseus cannot undo a deletion at
  another service.
- **It asks first**, before the vault is read, at both `[content_based_security] outside` settings, as the run form does:
  after outside text since the owner's last message, in a flagged session, and in a turn the owner's own message did not
  begin (a wake, a task's report). One difference: **the answer of an earlier `http.request` to the same host with the
  same vault item is not outside text for this tool** (the owner's ruling, K3). That service already holds the key, so its
  answer can only steer calls back to itself. A page from elsewhere, a search, another host or another item still asks.
- **The wall is `vault.use`'s**: the owner's own turn in a private place; never a task, a job, the self loop, an MCP
  session, a shared place, or a routine (`routine::grant::NEVER`).
- **A key Theseus itself runs on is not sent** this way (the run form's `runs_on`, by reference and by value), in words
  that point to the card form. Nor a value shorter than 8 characters, which the scrubber cannot mask.
- **The owner is told** once per item and host in a span, in an FYI note: the item, the field and the vault by the
  vault's names, the key's place, the method, the host and the path. None of the model's words; no Undo, since nothing
  stands.

## The answer

- The status line, a closed list of headers (`Content-Type`, `Content-Length`, `Location`, `ETag`, `Retry-After`,
  `Content-Range`, `Range`), what files were sent, and the body: text, JSON and XML as they are, HTML as text, anything
  else named with its size. Up to `[tools.web] max_bytes`, fitted to `[tools] result_max_chars`.
- **It is outside text**: the node carries the marker, as a fetched page's does.
- **A 4xx or 5xx is an answer**, shown under the same mask and marker, with the node's status an error: a service's
  "field X is missing" is what the model needs to fix its call.
- **Theseus's own errors** ("Not sent: …") carry nothing of the server's and are not marked.
- **Masked before it is stored.** The value is held on the board under `vault-use:<call>` before anything is sent, so
  every exit masks it while the daemon runs, and the answer's text and every string of its meta are scrubbed of it before
  the node is written, so it holds none after a restart.
- **No answer within `[tools.web] timeout_secs`** (four times that when files are sent) is "whether it acted on it is not
  known", status unknown. **A restart while a request is out** answers the same and never sends it again.

## What is kept

The node's meta, names and facts only: `vault {vault, item, field}`, `as`, `host`, `method`, `status`, `content_type`,
`bytes`, `sent` (each file's part, bytes and sha256), `external`. One `secret.granted` row a use, as for every use of a
vault value, with `once: true`, the vault's names and `request {host, method}`.

## What it does not do

No key in a query parameter. No private, loopback or LAN address. No redirect. No MD5. No proxy. No second request in a
call, no retry. No new setting.
