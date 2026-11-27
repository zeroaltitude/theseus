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
card says so: that text could have steered the request, so read it with care. A card never shows cut text as if it were
whole. A setting whose value is long (more than 400 characters, up to 4,096 bytes) is shown on the same kind of card as a
prompt file (see below); a value larger than that is refused, and you edit the file.

Press **Approve** (or answer on the card from your terminal), or **Decline**. A card that is not answered in time writes
nothing. In Discord a card that is longer than one message arrives as several, the buttons on the last, and none of it is
cut; when you have answered, the line that replaces it says in words what you approved or declined.

A plain "set X to Y" is **one card and nothing else to answer**. Theseus does not open the settings file: it reads a
setting by name (`config.show`: the value now, whether the file sets it or it is the default, whether a change to it
needs the code, whether it restarts) or, for a plain change, just proposes it, and the proposal's result already says
each value before. Reading is free in a turn that has read only your own words; in a turn that has read text from outside
since your last message, it asks you first. It never gives a secret's value, the names of vault items, or where a secret
is read from. (Reading is free at the default `notify` posture; at `[policy] enforcement = "approve"` it is asked like every
other call.)

To look at Theseus itself (its models, whether each provider's key resolved, its health and build), at the names in
its vault, or at its work (every conversation's tasks, what waits on you, projects, routines and what is coming), a turn
has `look.theseus`, `look.vault` and `look.work`, on the same terms: free in a clean turn of yours, asked first after
outside text, never a value or a note's text (for one item, the names and kinds of its fields;
`docs/design/look-tools.md`). A shell command that names Theseus's own program or the vault's is still held for you, and
its card now says which program and where the look is.

To use a secret that is in the vault, tell Theseus where it is in words ("the key is in the vault, in the item Lantern key,
field credential"); `look.vault` can list an item's field names (never a value). Theseus's own code finds the item and the
field, checks its account can read them, builds the reference itself, and puts **one card** that names them, says it read
them, and says what is given the secret (a provider's key, a program's variable, a server's key); the value never goes to
the model, a card or a record. Where its account cannot see the vault, it says so and says what you can do: grant the
account that vault, or move the item to one it sees. A vault reference is never written as text by a turn
(`docs/design/vault-use.md`). For ONE run of a program or a script ("run ./call.sh with the key in Lantern key"), there is
no card and nothing is written to the settings: Theseus reads the item, gives it to that one run in one variable, and tells
you in a note. Nothing is written to the settings; Theseus remembers which items it used this way, by name only, and reads
them again at each start so that it keeps masking their values. A key Theseus itself runs on still goes by the card.

To keep a NEW secret in the vault, ask Theseus to keep it: it puts one card that says which item and field, and whether it makes an
item, adds a field or replaces a value. Then give the value yourself, where only you see it: `theseus vault keep` in your own terminal
(nothing is shown as you type) or the card's field in the cockpit, with a code from your authenticator app (a fresh one for a replace).
The value never goes to the model, a card or a record; Theseus writes it with its own account, reads it back, and tells you where it is
by name. Do not paste a secret into chat: it is then with the model's provider and in Theseus's record. An item Theseus's own settings
read is replaced only from your terminal: `theseus vault keep --vault V --item T --field F`.

**In your Discord direct messages only**, the keep card also has a button, "Type the value here", that opens a form with one field. The
card says so before you press: a value typed in that form goes through Discord's servers on its way to Theseus, the form does not hide
what you type (Discord's text field has no hidden style), and the terminal does neither. In any other place the card has a Decline and
says where to type. `[vault] keep_by_discord` (true by default, your yes of 2026-11-19) turns the form off: the card is then the
terminal's and the cockpit's alone, and a value sent from Discord is refused. Changing it asks for the code, as the other settings
that decide where a secret goes (`docs/design/vault-keep.md`).

A fact its maker publishes (a model's exact name, a provider's address and protocol, a server's command) Theseus may look up
on the web instead of asking you. It puts it on the card with where it says it found it, under that setting, in the words
"Theseus's source for this, in its own words (nothing here checked it)": that is Theseus's word, not a check, so read the
address itself. Where a secret is sent is always yours to confirm, on the card, with the code (`docs/design/public-facts.md`).

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
- the proof itself, and attaching or removing an MCP server;
- where a secret's value may be typed (`[vault] keep_by_discord`, above).

Some of those ask for a **fresh code every time**, whatever else is open: a change to who the owner is (`[places]
owner`, and `[discord] bindings_file`, the file that says who the owner is on Discord), a change to where Theseus keeps
its state (`[server] state_dir`, which holds that file and the proof's own), a change to the proof's own settings
(`[owner_proof]`), and turning `[self] install` to `auto` (below).

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

## Changing a prompt file, or a long setting

The files the settings name as Theseus's own prompt text (the context files every conversation gets, and each persona's
file) go into every later prompt, so they are changed the same way as a setting, and no more loosely. Ask in a private
place: "make the persona warmer", "add this rule to your context file". Theseus reads the file, proposes the change as
exact replacements (or the whole new text), and the **daemon** builds the card from the file itself, never from what
Theseus says about it:

- the file's path, its size and a **fingerprint** (the first 16 hexadecimal digits of its SHA-256) before and after, and
  the number of lines added and removed;
- **the changed lines themselves**, whole, with two lines of context: a three-line change to a long persona shows those
  three lines, not "+3 −1". When the changed lines do not fit the card, the card says plainly that it is a summary and
  not the text, and says where the whole is;
- the **whole text, before and after**, is always there to read before you answer: `theseus config show` in your
  terminal, "Read the whole text" on the card in the cockpit. (Discord cannot attach a file to a card, so there the
  card is the changed lines, or the summary and the command.)
- a character you cannot see (a control character, a direction mark, a zero-width or hidden one) is written `\u{..}` and
  counted, on the card and in the whole text; a backslash is shown doubled so a written `\u{..}` is never mistaken for one.

Your yes needs **the second proof**, the same as for any setting that matters, with the same window: this is the
highest-leverage text in the system. Theseus's own program writes the file (a temporary file beside it, then a rename;
the same permissions), after checking that it is still byte for byte the file the card was built from: a file you or
anything else edited meanwhile is not written, and you ask again. The text before is kept (the last ten), and
`theseus config history` lists the change; "undo that" puts the text before back, exactly, by the same card and the
same proof, and says so if the file changed again since.

A prompt file is read again at every request, so **a change takes effect with Theseus's next message and nothing
restarts**. Only a file the settings already name can be changed: this never makes a file or names a new path, never
touches the settings file, the bindings, a secret or the proof's file, and a file larger than 256 KiB or a link is
refused. A persona's file may not grow past the persona cap. Only the first 64 KiB of a context file reach a prompt, and
the card says so when a new text is longer.

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
meantime. You can also ask for a plain restart in your own words; it has its own card. That card says how many turns are running and
how many executions are **waiting on a question to you**. Count the second number before you say yes: a build of the
self-improvement loop that is parked on a question is not running, but `theseus self pipeline` shows it as building,
waiting on you, with the command to answer it; a restart is not safe for it. A restart does not answer or drop those
questions; they stay waiting, but anything of theirs that was running is cut.

## Installing what its own loop joined

If Theseus's programs are in a directory only the administrator can write, its self-improvement loop can join a change to
`main` but cannot make it the build that runs. `[self] install` says what it may do about that:

- `off`: nothing; the loop's own message tells you the command to run by hand.
- `ask` (the default): Theseus builds the joined change and puts **one card** to you, which needs the code; your yes
  installs it and restarts onto it, and goes back to the build before if the new one is not healthy.
  An older settings file that says `install = true` is read as its own value: it installs where the loop can write the
  program directory, and on a root-owned one it **asks**, like `ask`. Only the word `auto` installs into a root-owned
  directory with no card.
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
- The prompt files are on the same floor as the settings file: a file tool, a shell command naming one and a job
  stop and wait for you at every setting, whatever the tool's posture (only a call that names the file is held: a glob,
  or a whole directory moved or copied, names none and is not stopped, as for the rest of the floor). That is the files
  the settings name as context files, both places a persona's file is kept by default (`personas/` and the older `persona/` in the state directory,
  whole, with the copies kept beside a file), every file a persona's own record names as its file wherever it is, and a
  copy kept beside any of them. Only Theseus's own program writes them, after your yes, and only your own turn reads
  them, with `config.read_file`; a tool that reads one waits for you. A search over a directory that holds one is not
  stopped, as it is not for the rest of the floor.
- The window covers a place, not a person: while one is open, whoever is at that terminal or that cockpit can approve.
  `theseus owner proof lock` is the way to close it when you step away.

## See also

- [design/config-change.md](design/config-change.md): how the change, the write and the restart are built.
- [design/owner-proof.md](design/owner-proof.md): how the code, the file and the windows are built.
- [design/prompt-file-change.md](design/prompt-file-change.md): how a prompt file's change, its card and its undo are built.
- [design/config-undo.md](design/config-undo.md), [design/mcp-attach.md](design/mcp-attach.md),
  [design/self-install.md](design/self-install.md).
- [cli.md](cli.md) for every command named here.
