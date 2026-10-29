#!/usr/bin/env python3
"""Dev-Hilfe: steuert Firefox per Marionette (venv mit marionette_driver) und macht Screenshots.
Vorher: support/shot.sh dashboard  (startet den Testserver auf :8099)."""
import base64, subprocess, sys, time, os
from marionette_driver.marionette import Marionette

PROF = "/tmp/nt/prof2"
os.makedirs(PROF, exist_ok=True)
ff = subprocess.Popen(["firefox", "--no-remote", "--profile", PROF, "--headless", "--marionette", "--window-size", "1440,900", "about:blank"],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(5)
m = Marionette(host="127.0.0.1", port=2828)
m.start_session()

def shot(name):
    data = m.screenshot(format="binary")
    open(f"/tmp/nt/m-{name}.png", "wb").write(data)

def js(code, *args):
    return m.execute_script(code, script_args=list(args))

def size(w, h):
    m.set_window_rect(width=w, height=h)

try:
    exec(open(sys.argv[1]).read())
finally:
    try: m.delete_session()
    except Exception: pass
    ff.terminate()
