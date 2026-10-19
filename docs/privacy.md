# Privacy

Updated: 2026-10-18

## What Theseus is

Theseus is personal, self-hosted software. One owner runs it on their own machine. There is no Theseus service, server
or company that receives anyone's data: the source is this repository, and nothing in it reports to its authors.

## What Google data it reads, and why

Theseus has no Google sign-in of its own. It reads Google data (for example mail, calendar events or files) only
through a connection the owner sets up and authorizes on Google's own consent screen, such as a tool the owner adds,
and only what that authorization allows. It uses that data only for the work the owner has asked of it: answering in a
conversation, running a task, or running a routine the owner set up.

## Where it stays

What such a tool returns becomes part of the conversation, task or routine that asked for it, and is kept on the
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
origin: the rule is an allowlist, so a model of any other origin (EU, China, or one Theseus cannot place) is refused. A
turn whose request contains one of those names runs on a model of US origin, or ends before any model is called; and
any request to a model of another origin that contains one of those names is refused before it is sent. Theseus
does not yet tell data apart by where it came from: Google content that contains none of the listed names is not held
back by this rule. An owner who wants none of it to reach such a model configures no provider of China or unknown
origin.

**Personal details.** The owner's personal details may live in Theseus's context; that is what makes it a personal
assistant. When the routing service is on, it is also asked whether each message tries to draw out the owner's
personal details, or to open a conversation with other people in which they could come out. A confident yes sends the
owner a warning in their own direct messages and is recorded; it holds nothing back. In a shared place, Theseus does
not recall the owner's private conversations in the first place.

## Google API Services User Data Policy

Theseus's use of data received from Google APIs adheres to the
[Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including
the Limited Use requirements.

## How to revoke access

- At Google: open <https://myaccount.google.com/permissions> and remove the app the owner authorized. Google then stops
  accepting its tokens.
- On the machine: Theseus keeps no Google token of its own. The tool the owner added keeps its sign-in token where that
  tool stores it; deleting it there, or removing the tool, ends access from this machine. A credential Theseus itself
  hands to a tool is named by reference in its config's `[secrets]` section and held in the owner's secret store
  ([setup.md](setup.md) says how that is set up); removing the entry there removes it.
- What Theseus has already read stays in its store on the owner's machine until the owner deletes it; deleting the
  daemon's state directory removes all of it.

## Contact

Open an issue at <https://github.com/zeroaltitude/theseus/issues>.
