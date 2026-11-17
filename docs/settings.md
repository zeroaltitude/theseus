# Changing Theseus's settings by asking

You can ask Theseus to change its own settings, in your own words, and it will. It changes nothing until you have seen
exactly what changes and said yes, and for the settings that matter most it also asks for a code from an authenticator
app on your phone. This page is the whole of it, for the person who owns the machine.

## The idea in four lines

- You ask in a private place: your own terminal, the cockpit, or a direct message to Theseus. "Raise the day's ceiling
  to 300." "Turn the notes import off." "Attach the word-count server."
- Theseus does not write the settings file. It works out the change, checks that Theseus would still start with it, and
  shows you **one card**.
- Only your yes on that card changes the file, and Theseus's own program makes the change, in one step, after keeping a
  copy of the file as it was.
- If Theseus does not start well on the new settings, it goes back to the copy, by itself, and tells you.

Nothing here is offered in a shared channel, to anyone but you, or to a program that is only a tool of a conversation.

## Pair the authenticator app (once)

In your **own terminal**, not through a pipe and not inside a Theseus job:

```
theseus owner proof enroll
```

It prints an address and a secret, once, on that terminal. Scan the address with your authenticator app, or type the
secret in by hand (time-based, six digits). It then asks for the code the app shows and keeps nothing until you type a
good one, so a mistyped pairing cannot lock you out. Pairing does not trust anything by itself.

The secret lives in one file that only Theseus's own program reads, and it is never shown again. A code is never typed as
a chat message: it goes into the card's own form, or on the command line, so it never passes through a conversation.
Without a paired app, a change that needs the code cannot be approved, and its card says so and how to pair one.

`theseus owner proof status` says whether an app is paired and which places a good code still covers.
`theseus owner proof reset` takes a code and removes the pairing. If you lose the phone, stop Theseus, delete
`owner-proof.json` from its state directory, start it again and pair a new app.

## Asking for a change, and the card

Say what you want. Theseus proposes it as a change to named settings, with their new values, and the card shows, for each
setting, what it is now and what it would be, in plain words. It also says whether Theseus has to restart for the change
to take effect (today every change restarts it), what a restart cuts, and Theseus's own reason for the change in its own
words. If, since your last message, the conversation read text from outside (a web page, a search, a tool's reply), the
card says so: that text could have steered the request, so read it with care. A change too big to show whole is refused
rather than shown cut.

Press **Approve** (or answer on the card from your terminal), or **Decline**. A card that is not answered in time writes
nothing.

## Which settings need the code

An **ordinary** change needs the card's yes and nothing else: the notes import, a theme, a model's name, a count.

A change to any of these also asks for the code (the list lives in one place in the code, `config/sensitivity.rs`, and
a setting nobody has classified counts as sensitive):

- what needs approval, and who may approve it;
- where secrets are read from;
- the sandbox, the hosts it may reach, and the folders Theseus may touch;
- who you are, and which places are private;
- a spending ceiling or budget;
- Theseus's own prompt text, the models and providers it uses, how outside text is handled, and what may see private
  text;
- which AWS accounts, Google access and secrets broker it uses;
- where Theseus listens and keeps its state;
- self-improvement: its mode, its budget, and what it may change;
- the proof itself, and attaching or removing an MCP server.

Three of those ask for a **fresh code every time**, whatever else is open: a change to who the owner is (`[places]
owner`, and `[discord] bindings_file`, the file that says who the owner is on Discord), a change to the proof's own settings (`[owner_proof]`), and turning
`[self] install` to `auto` (below).

## The 24-hour window, and locking it

A good code opens a **window of 24 hours** for the place it was typed in (your terminal, the cockpit, or your direct
messages). For that long, a change that needs the code, asked and answered from that same place, goes through on the
card's yes alone, and the card says so: "second proof: given here 3 hours ago, good for 21 hours more". Another place
asks for its own code. A use does not extend the window.

`theseus owner proof lock` ends every window at once. A restart that goes back to the old settings ends them too. The
length is `[owner_proof] window_hours` (24 at most; 0 asks for a code every time).

## History and undo

`theseus config history` lists the last ten changes, newest first: when, each setting before and after, where it was asked,
who approved it, and whether it was rolled back or undone since. Ask Theseus "what settings have you changed?" and it
says the same.

Say **"undo that change"** (or "undo change 3") and Theseus shows a card that puts back exactly the settings that change
touched, leaving later changes to other settings alone. It is a change like any other: the same card, the same code if
the settings need it, the same restart and way back. To redo, undo the undo. Only the last ten changes are kept.

## Attaching and removing an MCP server

Tell Theseus the command to run, or the address of the server. It first tries the server once, in the sandbox, with no
secret and no network beyond what you named, lists its tools, and puts them on one card with what the server asks for.
Your yes (with the code) adds it, and its tools are offered from the next turn with no restart. A secret the server needs
is named on the card and comes from your vault; its value never passes through the conversation. "Remove the word-count
server" is the same road backwards. `theseus mcp` shows which servers were attached this way, when, and by whom.

## The restart, and when the new settings fail

After your yes, Theseus waits up to two minutes for a turn that is running to end (the card says so), then restarts in
place and tells you, in the place you asked, that it is back and on which settings. If the new settings do not load, or
Theseus is not healthy within two minutes, it puts the copy of the old settings back, restarts once more onto them, and
tells you. It never restarts twice for one change, and it never overwrites a change you made to the file by hand in the
meantime. You can also ask for a plain restart in your own words; it has its own card.

## Installing what its own loop joined

If Theseus's programs are in a directory only the administrator can write, its self-improvement loop can join a change to
`main` but cannot make it the build that runs. `[self] install` says what it may do about that:

- `off`: nothing; the loop's own message tells you the command to run by hand.
- `ask` (the default): Theseus builds the joined change and puts **one card** to you, which needs the code; your yes
  installs it and restarts onto it, and goes back to the build before if the new one is not healthy.
- `auto`: it installs each joined change with no card. Turning it on asks for a fresh code.

For `ask` or `auto` to work the administrator runs one command, once: `scripts/root-install.sh install --user NAME
--helper FILE` (see [root-install.md](root-install.md)). That installs a small helper that can do exactly one thing:
put a build of a commit that `main` holds into that directory. What this gives up is written down in
[design/self-install.md](design/self-install.md): a program running as you can then ask the helper too, so the directory
is protected by what the helper checks, and no longer by the administrator's password.

## What this does not protect against

Say it plainly: the code protects you against Theseus being talked into changing its own settings, and against another
person or program answering a card. It does not protect against a program that runs on your machine as you.

- A program that runs **unsandboxed as you** can do what you can do, including reading and writing Theseus's files and
  talking to its sockets. By default Theseus's jobs run in the lightest sandbox level, which holds against a turn only as
  far as its simple guards do. For the code and the settings file to be a boundary against a turn, set `[sandbox] default
  = "l1"`.
- A program that can read your authenticator app, your phone, or what you type has the code.
- Anyone with your account's shell can remove the proof's file and pair their own app.
- The window covers a place, not a person: while one is open, whoever is at that terminal or that cockpit can approve.
  `theseus owner proof lock` is the way to close it when you step away.

## See also

- [design/config-change.md](design/config-change.md): how the change, the write and the restart are built.
- [design/owner-proof.md](design/owner-proof.md): how the code, the file and the windows are built.
- [design/config-undo.md](design/config-undo.md), [design/mcp-attach.md](design/mcp-attach.md),
  [design/self-install.md](design/self-install.md).
- [cli.md](cli.md) for every command named here.
