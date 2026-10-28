# Theseus in herdr: the operator's guide

[herdr](https://herdr.dev) is a terminal workspace manager. Theseus reports each task's state to its pane
(`theseus watch`, `theseus herdr sync`; see `docs/technical-overview.md`). This guide is the rest: a plugin with
keys that answer a question without hunting for its pane, a link under each question, and the config that
makes herdr show it all. Nothing here runs by itself: a key starts a popup, and the popup ends when it is done.

## Set it up (once, as yourself)

```sh
theseus herdr plugin                  # prints the plugin's directory and the one command that links it
herdr plugin link <that directory>    # plugins are global to your user, so you link it; Theseus never does
theseus herdr setup > /tmp/herdr.snippet     # the config to paste into herdr's config.toml
theseus herdr setup --config ~/.config/herdr/config.toml    # what your config lacks, read only
```

`setup` never writes a config. The snippet holds toasts through herdr (`ui.toast.delivery = "herdr"`), the
agent list sorted by what needs you (`agent_panel_sort = "priority"`), symbols for states, a 36-column sidebar,
rows that show a task's name, what it does now, its cost, model and parent (the `$tokens` the reporter sends:
`$doing`, `$cost`, `$model`, `$parent`), and the keys below. herdr's `herdr config check` validates the result.
The plugin needs herdr 0.9.0 or later and `theseus` on the PATH of the herdr server.

## The keys

| key | does |
|---|---|
| `prefix+a` | the answer card for the focused task's question, else the one that has waited longest |
| `prefix+shift+a` | jump to the pane of the question that has waited longest (`theseus herdr next`); on a running install that is often a self-improvement question (a self `fs.edit`), not one of your own tasks |
| `prefix+t` | the terminal UI, as a 92% by 90% popup |
| `prefix+shift+s` | stop the focused task or session: asks y/N, then cancels a task or halts a conversation |
| `prefix+n` | start a task from one line of words (`theseus task start`), under Home (your DM): the key passes no `--in`, so the task is not under the conversation you are looking at |
| `prefix+m` | back to the main conversation (`theseus herdr main`) |
| `prefix+i` | read the focused task's transcript and live stream, without joining it |
| `prefix+p` | pause the focused task at its next step, or resume it when it is held (`task.pause` / `task.resume`) |

Each key is a plugin action (`theseus.answer`, `theseus.next`, `theseus.tui`, `theseus.stop`, `theseus.new`,
`theseus.main`, `theseus.inspect`, `theseus.pause`), so the command palette lists them too.

A task's pane answers every question it shows, without the popup: `y/N/t` for an approval, and for a question
with options (`owner_ask`) its options numbered `1`..`6`, or words of your own, sent as `work.answer`. Once the
task is over, a line typed in its pane goes to the conversation that started it, with the redirect line every
surface says.

## The answer card

`theseus herdr answer [--pane P | --oldest | CID]` shows one waiting question whole: the task's title and short
id, where it came from, the model, the spend against its limit, when it was asked and when it expires, then the
call, why it asks, and its input. Its keys come from the question's own answers:

- `y` approves (or resets a spend limit, accepts a change, acknowledges an extension);
- `n` declines, and any other words decline with the words as the note (the model reads why);
- `1`..`6` pick an ask question's choices;
- `tab` shows the next question that waits ("2 more wait: … [tab] next"), `q` closes.

It is line mode: type the key and Enter. An empty line answers nothing. The answer is one `work.answer` with
author `herdr`. Which question: the one named, else the one the focused pane's `$confirm` token names, else the
one that has waited longest; with none it says "nothing waits for you". A question answered somewhere else while
the card is open is said ("… was answered elsewhere by discord"), and the card moves on or closes.

## Links under a question

In a herdr pane, `theseus watch --interactive` prints `[answer] [history] [open]` under a question, as OSC 8
hyperlinks to `theseus://answer/<question>`, `theseus://history/<session>` and `theseus://open/<session>`.
herdr captures the mouse, so Ctrl-click a link (herdr hands it to the plugin's link handler, whose pattern allows
exactly those three paths and an id). `answer` opens the card; `history` pages the session's history; `open`
focuses the session's pane. **No link approves anything**: the card still waits for your key.
Outside herdr nothing is printed.

## After herdr restarts

The plugin's startup hook runs `theseus herdr up --boot 2>/dev/null || theseus herdr sync`. It closes the panes herdr
restored as bare shells (the focused one too: the hook runs in no pane, so its `HERDR_PANE_ID` is not a pane of ours), then makes the `theseus` workspace with a `main` tab (`theseus chat`, the home conversation)
and a `board` tab (the follower, `theseus herdr up --follow --here`), even when no task is running, so a task that
starts later gets its tab by itself. A tab that is already there is left alone: running it again makes nothing new.
`theseus herdr up --follow` does the same from a shell, then switches herdr to the `theseus` workspace (`--no-focus` leaves the front alone; with no herdr client attached it says to run `herdr`), and returns at once because the `board` tab is the follower. `theseus herdr sync` is safe to run at any time.

## Troubleshooting

- A key does nothing: `herdr plugin list` shows whether the plugin is linked and enabled; `herdr plugin log` (no id: herdr 0.9.1
  answers `herdr plugin log theseus` with `unknown option: theseus`) shows each action's command, exit code, stdout and stderr.
- "is the plugin linked?": `theseus herdr popup` opens the plugin's own panes, so it needs the link above.
- `theseus: ... cannot connect`: the card and `next` need the daemon (`theseus health`).
