#!/usr/bin/env python3
"""Open N watchers of one session on a scratch daemon, run one turn, and say
who heard its end (theseus-7vtp). Standard library only.

    scripts/conn-watchers.py --socket SOCK --conns 1100
    scripts/conn-watchers.py --socket SOCK --conns 3000

The daemon's turn is answered by a stand-in model (`theseus-sim fake-model`),
set in its config. Each connection watches the session; the one past the
ceiling is told why in one error frame (code -32007) and closed. Expect:
every watcher that was admitted hears `turn.ended`, every other connection got
the refusal frame, `health` answers after, and the daemon lives.
"""
import argparse, json, os, resource, selectors, socket, sys, time

LIMIT = -32007


def send(s, msg):
    s.sendall((json.dumps(msg) + "\n").encode())


def lines(s, buf):
    """Complete JSON lines in `s`'s buffer after a read; b'' at EOF."""
    try:
        data = s.recv(65536)
    except BlockingIOError:
        return [], False
    if not data:
        return [], True
    buf.extend(data)
    out = []
    while b"\n" in buf:
        i = buf.index(b"\n")
        out.append(json.loads(bytes(buf[:i])))
        del buf[: i + 1]
    return out, False


def call(s, buf, id_, method, params=None):
    send(s, {"jsonrpc": "2.0", "id": id_, "method": method, "params": params})
    s.settimeout(30)
    while True:
        got, eof = lines(s, buf)
        if eof:
            raise SystemExit(f"{method}: the connection closed")
        for v in got:
            if v.get("id") == id_:
                return v
        time.sleep(0.001)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--socket", required=True)
    ap.add_argument("--conns", type=int, required=True)
    ap.add_argument("--prompt", default="say hello")
    ap.add_argument("--wait", type=float, default=60)
    a = ap.parse_args()

    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    resource.setrlimit(resource.RLIMIT_NOFILE, (min(hard, a.conns + 200), hard))

    ctl = socket.socket(socket.AF_UNIX)
    ctl.connect(a.socket)
    cbuf = bytearray()
    opened = call(ctl, cbuf, 1, "session.open", {})
    sid = (opened.get("result") or {}).get("id") or (opened.get("result") or {}).get("session_id")
    if not sid:
        raise SystemExit(f"session.open: {opened}")
    health = call(ctl, cbuf, 2, "health")["result"]["push"]["connections"]
    print(f"health before: {health}")

    watching, refused, other = [], 0, 0
    t0 = time.time()
    for i in range(a.conns):
        s = socket.socket(socket.AF_UNIX)
        s.connect(a.socket)
        send(s, {"jsonrpc": "2.0", "id": 10, "method": "session.watch",
                 "params": {"session_id": sid}})
        s.settimeout(30)
        buf = bytearray()
        # The refusal frame, or session.watch's answer, whichever comes first.
        first = None
        while first is None:
            got, eof = lines(s, buf)
            if eof:
                break
            if got:
                first = got[0]
        if first is None or (first.get("error") or {}).get("code") == LIMIT:
            refused += 1
            s.close()
        elif first.get("id") == 10:
            s.setblocking(False)
            watching.append([s, buf, None])
        else:
            other += 1
    print(f"opened {a.conns} in {time.time()-t0:.1f}s: {len(watching)} watching, "
          f"{refused} refused with the frame, {other} other")

    sel = selectors.DefaultSelector()
    for w in watching:
        sel.register(w[0], selectors.EVENT_READ, w)
    send(ctl, {"jsonrpc": "2.0", "id": 3, "method": "turn.submit",
               "params": {"session_id": sid, "input": a.prompt}})
    t_submit = time.time()
    heard = 0
    deadline = t_submit + a.wait
    while heard < len(watching) and time.time() < deadline:
        for key, _ in sel.select(timeout=1):
            s, buf, at = key.data
            got, eof = lines(s, buf)
            for v in got:
                if v.get("method") == "turn.ended" and at is None:
                    key.data[2] = time.time() - t_submit
                    heard += 1
            if eof:
                sel.unregister(s)
    times = sorted(w[2] for w in watching if w[2] is not None)
    print(f"{heard} of {len(watching)} watchers heard turn.ended"
          + (f"; first {times[0]*1000:.0f} ms, last {times[-1]*1000:.0f} ms" if times else ""))
    after = call(ctl, cbuf, 4, "health")
    print(f"health after: {after['result']['push']['connections']}")
    ok = heard == len(watching) and refused == max(0, a.conns - len(watching)) and "result" in after
    print("OK" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
