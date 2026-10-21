# Privacy

Updated: 2026-10-19

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
the configuration says; and a model Theseus cannot place counts as of unknown origin. The owner can list names
(organisations, clients, projects, people) under `[privacy] sensitive_names`. Those names go only to models of US
origin: the rule is an allowlist, so a model of any other origin (EU, China, or one Theseus cannot place) is refused,
and so is a model the built-in catalog does not itself list as of US origin, whatever a proxy's configuration says. A
turn whose request contains one of those names runs on a model of US origin, or ends before any model is called; and
any request to a model of another origin that contains one of those names is refused before it is sent.

Google content is held back by more than names. Every result Google gives Theseus marks its conversation private. From
then on that conversation's requests, including the summary that shortens it, go only to models of US origin, by the
same allowlist: the route skips every other model, and a request that would reach one is refused before it is sent. A
conversation that has not read anything from Google is not affected. Theseus's other background steps do not yet
carry the mark, so an owner who wants none of it to reach a model of another origin configures no provider of any other
origin.

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
