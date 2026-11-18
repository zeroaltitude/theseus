# The owner's second proof (issue 22)

A code from the owner's own authenticator app, asked for by the changes that matter most. The owner, 2026-11-16:
"authenticator app, let's do the work right now!" The first thought was a code sent to the owner's mail and typed back
in chat; Theseus can read that mailbox, so a turn talked into proposing a change could have read the code too. **The
point of the proof is that neither the secret nor a code ever passes through a turn.** The model never sees the
secret, never sees a code, and cannot supply one.

Code: `crates/theseus-core/src/owner_proof/` (the code, the file, the keeper, the windows, the marks),
`crates/theseus-core/src/rpc/owner_proof.rs` (the five methods), `rpc/proof_gate.rs` (the answer path),
`crates/theseus-protocol/src/owner_proof.rs` (the wire), `crates/theseus/src/owner_proof_cmd.rs` (the CLI),
`crates/theseus-discord/src/runtime/proof_form.rs` (the card's form).

## The owner's how-to

**Pair the app, once.** In your own terminal (not through a pipe, not in a Theseus job):

```
theseus owner proof enroll
```

It prints an `otpauth://` address and the secret in base32, once, to that terminal. Scan the address or paste it
into your authenticator app, or add the secret by hand (time-based, six digits). It then asks "Code from your
authenticator app:" and keeps nothing until you type a good one, so a mistyped pairing cannot lock you out. Pairing
trusts nothing by itself: it opens no window.

**See where it stands.** `theseus owner proof status` says whether an app is paired and since when, whether codes are
locked, and each open window (the place, since when, for how much longer). Never the secret.

**Reset.** `theseus owner proof reset` takes a code from the app and removes the pairing; `enroll` pairs a new one.
Lost your phone: stop the daemon and remove `owner-proof.json` from its state directory by hand, then start it and
enroll again. (There is no restart flag: your own account owns that file, so the shell that could pass a flag could
remove the file too, and the daemon never takes the file's absence for anything but "not paired".)

**Answer a change that needs it.** On a card, Approve opens a form with one field for the code, unless a window of
this place already covers the question (below): then Approve goes through at once, with no form. At the CLI,
`theseus answer <id> approve --code 123456` (`--code -` reads the digits from standard input; inside a window it needs
no code and says on what footing it went through). In the cockpit, the card has a field only when a code is needed. A
code typed as an ordinary chat message counts for nothing.

**For a while, once is enough.** A good code opens a *window* over the place it was typed in (the terminal, the
cockpit, a Discord DM): for `[owner_proof] window_hours` (24 by default; 0 asks every time; at most 24), a question
that needs the proof, answered from that same place, goes through on the card's yes alone. Another place asks for its
own code. A use does not extend the window: it ends 24 hours from the code. The card says, before the press, which it
is: "Second proof: none given here earlier covers this: Approve opens a form for the code from your authenticator
app", or "second proof: given here 3 hours ago, good for 21 hours more; Approve goes through". The words are fixed when
the card is drawn; the press is decided by the daemon when it comes (a window that ended in between asks for the code
then: the form opens, or, when it ended between the press and the answer, the answer is refused with the daemon's words
and the next press opens the form).
A question that needs a fresh code every time is never covered. The approved card's settled line says the footing: "on the
second proof given here 3 minutes earlier", or "with a fresh code".
`theseus owner proof lock` ends every window at once. Lowering `window_hours` shortens a window already open (its end
is capped by the setting in force when it is read), and at 0 ends it.

## What it is

- **The code**: TOTP as an authenticator app does it (RFC 6238 over RFC 4226: HMAC-SHA1, six digits, thirty-second
  steps), forty lines over `hmac` and `sha1`, tested against the RFCs' own vectors. A code is accepted for the present
  step and one either side. **A code is used once**: the last accepted step is stored, and a code for that step or an
  earlier one is refused.
- **The secret**: 20 random bytes from the OS, in one file of its own in the state dir, `owner-proof.json`, mode 0600,
  written by the daemon alone (a sibling file, synced, then renamed over it). The file is on the floor
  (`toolrun/guarded.rs`, with its write-beside name): no tool reads or writes it and no L1 view shows it. It is never
  in the config, the ledger, a log, a node, a card, health, or any answer after the enrolment's own. The file also holds
  the last step used, the recent wrong tries, the end of a lock and the open windows, so a restart clears no count and
  revives no used code. A file that does not parse, or that other users can read, is not trusted.
- **The rate limit**: five wrong codes in ten minutes lock checking for ten minutes, one count for every surface and
  across restarts. A good code clears the count. A used code counts as a wrong one. A check that cannot save its
  counters, or cannot write its ledger row, is refused.
- **The ledger**: every check, good or bad, is `owner.proof.checked` (the outcome: `ok`, `wrong`, `used`, `locked`,
  `missing`, `not_enrolled`, `unreadable`, `unsaved`, `agent`; and the place), never the code. Also `owner.proof.changed`
  (`enrolled`, `reset`, `refused`), `owner.proof.window_opened`, `owner.proof.window_used` (which question, from where)
  and `owner.proof.locked` (who, from where, how many windows).

## A question that needs it

The asker marks the question in the daemon's own record (`owner_proof::mark(correlation_id, fresh)`, a META record
staged in the frame that asks it), never by anything an answer says. `ConfirmRequest` and `Question` carry
`needs_proof` and `needs_fresh_proof`. The gate is in `Core::confirm_action_with`, **the one place an answer becomes a
decision**, so every surface meets it; a decline is never gated.

- Approved on a good code (`Given`), or on the open window of the answer's own place (`GivenEarlier`); otherwise
  `Missing` or `Refused`, and nothing changes. A question with no app paired cannot be approved at all.
- **`needs_fresh_proof`** asks for a code every time, window or not, and a good code given for it opens no window.
- **A code is never a message.** The chat's answer tool refuses a marked question outright (a model's tool never
  approves it in the owner's name, whatever window is open), and an answer typed or spoken as words (a Discord reply,
  a voice command) carries `typed`, which the daemon never takes a proof from and no window covers.
- **An agent's process never answers it** (a process under an agent, or on the CLI one with no terminal: the answer's
  `Answerer::agent`, `peer_proc`). The CLI's socket and the web port are open to every process of the owner's user, so
  such an answer carries no code, rides no window and spends none of the tries: it is refused and recorded (an
  `owner.proof.checked` row, outcome `agent`, and no count).
- A reset or a re-enrolment always takes a fresh code.
- **The seam** the change branch calls: `Proofs::required_and_given(&question, &answer) -> Proof`, where `Proof` is
  `NotRequired | Given | GivenEarlier { at_ms, until_ms } | Missing | Refused(why)` and `Answer` is the code and the
  place. It is a method because the check needs the keeper's file and clock; the wiring calls it as
  `core.proofs.required_and_given(..)`. `Proofs::judge` is the same judgment over a question's id and marks, which the
  answer path calls. `Proofs::standing(place, fresh)` is the card's own words for a place.

## Surfaces

| Surface | How the code is carried |
|---|---|
| Discord | Approve on a card for such a question opens a form (a modal) with one required field, unless a window of that place (the DM, or a private channel) covers the answer: then the press answers and no form opens (`runtime/proof_form.rs`, `covered`). With nothing enrolled, the press says so and opens no form. The submit is `work.answer` as the presser; the code goes from the form to the daemon and nowhere else (not a message, not a content item, not a transcript line, not a binding ledger row). |
| CLI | `theseus answer <id> approve --code 123456`. |
| Cockpit | A field on the card, shown only when a code is needed; inside a window the card says Approve goes through. |

## Who may do what

`owner.proof.enroll`, `confirm`, `status` and `reset` are the owner's own terminal's alone: the CLI on the daemon's own
socket (mode 0600), refused in a job's shell (`THESEUS_SESSION`, the CLI's own refusal and the daemon's), from a process
under an agent, over the web UI, Discord and MCP. A refusal is an `owner.proof.changed` row and says where to run it.
`owner.proof.lock` also takes a private place (it only ends windows), never a job, an agent's process or MCP.

## What it does not do (read before relying on it)

- The floor holds the file at every posture: a call naming it waits for the owner's approval, and an L1 job's view does
  not show it. As with the config file, the owner approving such a call is the owner's own act.
- The CLI's `--code` is on a command line: another process of the same user can read it for the seconds the command
  runs (`--code -` avoids it). A code is single-use and thirty seconds long.
- A config rollback's restart should end every window (`Proofs::end_windows`), since the settings a window was given
  under are gone. The daemon's rollback is the change branch's; its wiring calls this.
- The windows never survive `reset` (the file is removed) and never cover `needs_fresh_proof`.
