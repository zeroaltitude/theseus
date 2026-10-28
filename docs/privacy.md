# Privacy

Updated: 2026-10-28

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
calendar can be marked `sensitive` too. Its events are never
offered to a shared place such as a Discord channel others can read, and are written to no log or ledger row: the
record of a read holds the calendar's label and a status, never an event's title, place, guests or call link.
Reading one raises a `personal` label on the conversation (next section), which keeps it off models of China or
unknown origin until the owner removes the label.

## Who else sees it

Theseus sends data only to services the owner has configured, under those services' own terms:

- the model providers the owner configures: the parts that a turn, or one of Theseus's own background steps (such as
  shortening a long conversation), needs, in that request;
- the routing service the owner may turn on (Jev): the text it is asked to judge, such as which model a message
  needs or how risky a call is;
- the chat surfaces the owner connects (for example a Discord server), which show the replies posted there;
- any other service the owner turns on, such as web search or speech.

**Models of China origin.** Theseus knows the origin of each model it can call. Its built-in catalog marks the models
of China origin; a model served from a China-origin host, or of a China-origin family, counts as China origin whatever
the configuration says; and a model Theseus cannot place counts as of unknown origin. What such a model may be sent is
decided by labels on the conversation, which the owner can see and remove.

Every item that enters a conversation (what the owner writes, a tool's result, a page, a file) is classified once by
Jev, the routing service, and a conversation carries a label when something in it showed the value of a credential
(`secret`), an organisation's non-public matter (`confidential`) or a person's private information (`personal`).
Two rules raise a label without a model: an item that holds a word the owner listed under `[privacy] sensitive_names`
(organisations, clients, projects, people) raises `confidential`, and everything Google returns raises `confidential`
(a sensitive calendar's, `personal`). While a conversation carries one of those three, a request for it to a model of
China or unknown origin is refused before it is sent, and routing skips those models. The owner removes a label with
one tap in Discord, the cockpit, or `theseus content remove`, and those models may then be used for that
conversation: the removal is the owner's call, and nothing overrides it. The setting `[content_based_security]
cn_models` is the one switch: `follow_flags` (the default) reads the labels, `never` uses such models for nothing and
`always` ignores the labels. The check is one read of the conversation's record; Theseus does not search a request for
a name. A turn bound for such a model waits a few seconds (`wait_ms`) for the checks still out for its conversation,
then goes on the labels it has; an item whose check failed twice reaches such a model with no label, and the owner is
told once. A task, a retry, a routine's run and a draft start with their parent's labels. A fourth label, `injection`,
is not about origin: it says something tried to steer the assistant, and until the owner removes it every call except
a local read of files and code is refused.

What the labels do not do: they are not a promise about every other origin. A model of EU origin is refused nothing
today, because no label keeps it off; the built-in catalog lists no model of EU origin, so the owner would have to add one. The labels also do not rewrite anything:
Theseus stores what happened as it happened (see "Secrets" below).

Such a model only drafts code: in its own copy of an open repository, with no programs, and a Claude model reviews and
fixes the draft before anything is applied.

Google content is labelled as it arrives (above): a conversation that reads anything from Google carries
`confidential`, and a sensitive calendar's events `personal`, and the labels keep that conversation, and a task, retry
or routine it starts, off models of China or unknown origin until the owner removes them. A conversation that has not
read anything from Google is not affected. A request to a model of China or unknown origin carries no recalled
notes, and leaves out what other conversations wrote for others to see (a standing order, a milestone reminder).
A sensitive calendar's events, and anything marked `personal`, are never published to a
shared place.

**Secrets.** Theseus's store holds what happened as it happened: a tool's output, a file it read and the owner's own
messages are not rewritten, and a key that appears in them is in the store, on the owner's machine. What leaves the
machine is different: the value of every secret Theseus itself read from the owner's secret store (and its base64,
percent-encoded and JSON-escaped forms) is replaced by `[redacted:<name>]` in each request to a model provider, each
Discord message, the text sent to Jev and each push line. Anything
that only looks like a key, and a key Theseus never read from the store, is not recognised and goes as written;
a value of a secret the daemon does not hold at that moment (failed at its start, rotated, removed), or one cut in
two by a size limit, is not recognised either; for a
model of China or unknown origin the `secret` label is the guard. `theseus inspect` says which secret, by name, went
in which request to which model. `[privacy] redact_outbound = false` turns the replacement off.

**Personal details.** The owner's personal details may live in Theseus's context; that is what makes it a personal
assistant. When the routing service is on, it is also asked whether each message tries to draw out the owner's
personal details, or to open a conversation with other people in which they could come out. A confident yes sends the
owner a warning in their own direct messages and is recorded. Where only the owner writes, it holds nothing back. In a
shared place the reply is held: no model is called, the person who asked is told "I don't share that here.", and the
owner's direct message says who asked, where, what they wrote and why it tripped. In a shared place, Theseus does not
recall the owner's private conversations in the first place.

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
