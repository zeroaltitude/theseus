# Privacy

Updated: 2026-11-23

## What Theseus is

Theseus is personal, self-hosted software. One owner runs it on their own machine. There is no Theseus service, server
or company that receives anyone's data: the source is this repository, and nothing in it reports to its authors.

## What Google data it reads, and why

Theseus can sign in to the owner's own Google account, once, when the owner runs `theseus google auth` from their own
shell and approves it on Google's consent screen. It then reads and changes only the services the owner turned on in
its `[google] services` setting, and only with the permissions the owner granted there: mail (Gmail), files (Drive,
Docs, Sheets, Slides) and calendar events. It uses that data only for the work the owner has asked of it: answering in
a conversation, running a task, or running a routine the owner set up. Anything that sends mail, deletes, shares or
invites other people waits for the owner's approval each time.

## Where it stays

What Google returns becomes part of the conversation, task or routine that asked for it, and is kept on the
owner's machine, in Theseus's own store. Theseus does not sell it, does not use it for advertising, and trains no model
on it. It sends it nowhere except as the next section says.

Theseus reads every calendar the account can see (a calendar shared into the account appears with no config edit), or
only those `[google] calendars` lists. A calendar of a workplace's domain (a Google Workspace domain, by the calendar's
id or its owner) is `sensitive` with no config; `[google] sensitive_domains` names those domains instead, and a listed
calendar can be marked `sensitive` too. A read raises a `personal` label; its tool ledger row records the
calendar's label and status rather than event details. That is not a memory boundary: the raw result stays in the
store and may enter context in another trusted place. Explicit publication of a node from a session carrying a
`personal` label to a shared place is refused; this does not withhold that session from recall.

## Who else sees it

Theseus sends data only to services the owner has configured, under those services' own terms:

- the model providers the owner configures: the parts that a turn, or one of Theseus's own background steps (such as
  shortening a long conversation), needs, in that request;
- the routing service the owner may turn on (Jev): the text it is asked to judge, such as which model a message
  needs or how risky a call is;
- the chat surfaces the owner connects (for example a Discord server), which show the replies posted there;
- any other service the owner turns on, such as web search or speech.

**Models and labels.** Configured models receive context through the same view, regardless of origin.
There is no origin firewall, origin-specific recall filter or mandatory draft-and-review path. The former
`[content_based_security] cn_models` and `wait_ms` keys still load with a startup warning, but are ignored;
`cn_models = "never"` does not exclude any model. Configure only providers you trust with this memory.

Jev labels content as `secret`, `confidential`, `personal` or `injection`. A word listed under `[privacy]
sensitive_names` raises `confidential` without a model call, as does a Google result; a sensitive calendar raises
`personal`. These labels describe content, not which provider may see it. The owner can inspect them with
`theseus content SESSION` and remove them with `theseus content remove`.

The `injection` label still affects actions: at the shipped default (`[content_based_security] injection = "notify"`)
the owner is told and the label refuses nothing. With `injection = "refuse"`, every call except a local read of files
and code is refused until the owner removes it. Approvals, place tool ceilings and publish approval still apply;
none of these is a promise to keep private conversation out of shared-place context.

**Secrets.** Theseus's store holds what happened as it happened: a tool's output, a file it read and the owner's own
messages are not rewritten, and a key that appears in them is in the store, on the owner's machine. What leaves the
machine is different: the value of every secret Theseus itself read from the owner's secret store (and its base64,
percent-encoded and JSON-escaped forms) is replaced by `[redacted:<name>]` in each request to a model provider, each
Discord message, the text sent to Jev and each push line. Anything
that only looks like a key, and a key Theseus never read from the store, is not recognised and goes as written;
a value of a secret the daemon does not hold at that moment (failed at its start, rotated, removed), or one cut in
two by a size limit, is not recognised either. A `secret` label does not stop a provider request. `theseus inspect` says which secret, by name, went
in which request to which model. `[privacy] redact_outbound = false` turns the replacement off.

**Personal details.** The owner's personal details may live in Theseus's context; that is what makes it a personal
assistant. Since 2026-11-22, a request for those details in a shared place runs like any other turn: there is no
personal-details reply hold or automatic warning to the owner's DM. Recall and context come from the same memory
in every place. The owner must put Theseus only in places whose participants they trust with that memory.

## Google API Services User Data Policy

Theseus's use of data received from Google APIs adheres to the
[Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including
the Limited Use requirements.

## How to revoke access

- At Google: open <https://myaccount.google.com/permissions> and remove the app the owner authorized. Google then stops
  accepting its tokens.
- With Theseus: run `theseus google revoke`. It revokes the grant at Google, clears the stored refresh token, and
  turns the Google tools off until the owner signs in again.
- On the machine: the refresh token lives where the owner's `[secrets]` entry `google_refresh_token` points: an item
  in the owner's secret store, or a file only the owner can read ([setup.md](setup.md) says how that is set up).
  Removing it there ends access from this machine. The short-lived access token is kept in memory only and never
  written anywhere.
- What Theseus has already read stays in its store on the owner's machine until the owner deletes it; deleting the
  daemon's state directory removes all of it.

## Contact

Open an issue at <https://github.com/zeroaltitude/theseus/issues>.
