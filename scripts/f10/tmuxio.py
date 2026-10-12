"""The driver's terminal: a tmux server of its own (theseus-qy2a).

Every command goes to a private server, `tmux -L f10-<pid>`, started with no
config file (`-f /dev/null`), so the driver never reaches the default socket
and its sessions (a resurrect plugin may restore someone's there), and never
reads anyone's tmux.conf. `kill` ends the server and only it.
"""

import os
import shutil
import subprocess
import time


def socket_name(pid=None):
    """The private server's name: `f10-<pid>`, never tmux's `default`."""
    return f"f10-{os.getpid() if pid is None else pid}"


class Tmux:
    """One pane on a private server, at a fixed size."""

    def __init__(self, cols, rows, cwd, env=None, name=None, tmux="tmux"):
        self.name = name or socket_name()
        assert self.name != "default" and self.name.startswith("f10-"), self.name
        self.tmux = shutil.which(tmux) or tmux
        self.cols, self.rows = cols, rows
        self.calls = []  # every argv run, for the isolation test
        # A plain shell with a fixed prompt, so a screen's last line is the
        # same on every machine; the arm's environment on its command line
        # (a server already running would not pass `-e` to a login shell).
        env_args = [f"{k}={v}" for k, v in (env or {}).items()]
        self.run(
            "new-session", "-d", "-s", "f10", "-x", str(cols), "-y", str(rows), "-c", str(cwd),
            "env", *env_args, "PS1=$ ", "HISTFILE=/dev/null", "bash", "--norc", "--noprofile", "-i",
        )
        self.run("set-option", "-g", "status", "off")
        self.run("set-option", "-g", "history-limit", "5000")
        self.run("resize-window", "-t", "f10", "-x", str(cols), "-y", str(rows))

    def argv(self, *args):
        return [self.tmux, "-L", self.name, "-f", "/dev/null", *args]

    def run(self, *args, check=True):
        argv = self.argv(*args)
        self.calls.append(argv)
        r = subprocess.run(argv, capture_output=True, text=True)
        if check and r.returncode != 0:
            raise RuntimeError(f"tmux {' '.join(args)}: {r.stderr.strip()}")
        return r.stdout

    def keys(self, *keys):
        """tmux key names (`Enter`, `Escape`, `C-c`, `BTab`), each sent alone."""
        for k in keys:
            self.run("send-keys", "-t", "f10", k)

    def type(self, text):
        """Literal text, as typed."""
        self.run("send-keys", "-t", "f10", "-l", text)

    def paste(self, text):
        """`text` as one bracketed paste, as a terminal sends a paste."""
        self.run("set-buffer", "-b", "f10paste", "--", text)
        self.run("paste-buffer", "-p", "-d", "-b", "f10paste", "-t", "f10")

    def capture(self):
        """The visible screen, with its colours (`-e`), lines joined as drawn."""
        return self.run("capture-pane", "-p", "-e", "-t", "f10")

    def text(self):
        return self.run("capture-pane", "-p", "-t", "f10")

    def foreground(self):
        """The name of the program in front in the pane."""
        return self.run("display-message", "-p", "-t", "f10", "#{pane_current_command}").strip()

    def pane_pid(self):
        return int(self.run("display-message", "-p", "-t", "f10", "#{pane_pid}").strip())

    def wait(self, pred, timeout, every=0.05):
        """Poll the screen's text until `pred(text)` holds; the text, or None
        at the timeout."""
        end = time.monotonic() + timeout
        while True:
            t = self.text()
            if pred(t):
                return t
            if time.monotonic() > end:
                return None
            time.sleep(every)

    def kill(self):
        self.run("kill-server", check=False)
