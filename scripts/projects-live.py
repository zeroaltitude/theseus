#!/usr/bin/env python3
"""The projects row's live check (theseus-n5ui2): a scratch daemon on the stand-in model, a project with three milestones a minute apart and a review every 2 minutes, a session attached and compacted, then the list, `theseus why` and the review's runs. Standard library only; run it from the repository root with debug binaries built. Works under /tmp/live/r."""
import json, os, subprocess, sys, time, socket, pathlib
B = pathlib.Path("/home/user/theseus/target/debug")
R = pathlib.Path("/tmp/live/r"); 
import shutil; shutil.rmtree(R, ignore_errors=True); R.mkdir()
def port():
    s=socket.socket(); s.bind(("127.0.0.1",0)); p=s.getsockname()[1]; s.close(); return p
mp = port()
(R/"rules.json").write_text(json.dumps([{"when":"","text":"Noted."}]))
(R/"work").mkdir()
(R/"t.toml").write_text(f'''[model]
live = "f"
api_base = "http://127.0.0.1:{mp}"
api_key_secret = "k"
[profiles.f]
provider = "anthropic"
model = "claude-sonnet-5-5"
[secrets]
k = "env:LIVE_KEY"
[server]
state_dir = "{R}/state"
socket = "{R}/s.sock"
disk_warn_mb = 0
disk_floor_mb = 0
[kernel]
min_repeat_minutes = 1
[tools]
projects_dir = "{R}/work"
roots = ["{R}/work"]
[catalog."claude-sonnet-5-5"]
context_window = 100000
max_output_tokens = 2000
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
[judge]
enabled = false
''')
env = dict(os.environ, LIVE_KEY="x", TZ="UTC", THESEUS_SOCKET=str(R/"s.sock"))
fm = subprocess.Popen([B/"theseus-sim","fake-model","--addr",f"127.0.0.1:{mp}","--rules",R/"rules.json","--dump",R/"dump"],stdout=open(R/"fm.log","w"),stderr=subprocess.STDOUT)
d = subprocess.Popen([B/"theseusd","--config",R/"t.toml","--socket",R/"s.sock","--state-dir",R/"state"],env=env,stdout=open(R/"d.log","w"),stderr=subprocess.STDOUT,start_new_session=True)
def cli(*a, t=60, inp=None):
    r = subprocess.run([B/"theseus","--socket",R/"s.sock",*a],capture_output=True,text=True,env=env,timeout=t,input=inp)
    return r.stdout+r.stderr
for _ in range(100):
    if "ok" in cli("health").lower() or "serving" in cli("health").lower(): break
    time.sleep(.3)
try:
    print(cli("project","new","Harbour survey","--objective","Chart every mooring","--accept","every mooring has a depth",
      "--milestone","Survey the moorings@1m","--milestone","Chart the channel@2m","--milestone","Publish the chart@3m","--review","every 2m"))
    print(cli("projects"))
    out = cli("--json","sessions","open") if False else cli("--json","ask","hello")
    sid = json.loads(out)["session_id"] if out.strip().startswith("{") else None
    print("session", sid)
    pid = json.loads(cli("--json","projects"))["projects"][0]["id"]
    print(cli("project","attach",sid,pid))
    filler = "the keeper logged the tide and the wind before the lamp was lit. "*1300
    for k in range(10):
        print(k, cli("ask","--session",sid,"-", t=120, inp=f"turn{k} "+filler)[-120:].replace(chr(10)," "))
    print(cli("why","--session",sid))
    time.sleep(120)
    print(cli("projects"))
    rid = json.loads(cli("--json","projects"))["projects"][0]["review"]
    print(cli("routine","runs",rid))
    print(cli("ledger","--help")[:0])
finally:
    cli("shutdown"); time.sleep(2); d.terminate(); fm.terminate()
