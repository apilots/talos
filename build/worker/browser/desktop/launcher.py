"""Closed local desktop launch actions, inherited from the supervised WM/dock."""
import json
import os
from pathlib import Path
import socket
import sys

profile = Path(os.environ["TALOS_BROWSER_PROFILE"])
workspace = profile.parent / "workspace"
workspace.mkdir(mode=0o700, exist_ok=True)
os.chdir(workspace)
action = sys.argv[1] if len(sys.argv) == 2 else "browser" if len(sys.argv) == 1 else None
if action == "terminal":
    os.execv("/usr/bin/xterm", ["xterm", "-title", "Terminal", "-fa", "Monospace", "-fs", "11",
        "-geometry", "96x28+220+130", "-e", "/bin/bash", "--noprofile", "--norc"])
if action == "files":
    os.execv("/usr/bin/pcmanfm", ["pcmanfm", "--no-desktop", "--new-win", str(workspace)])
if action != "browser":
    raise SystemExit("Unsupported desktop action")
port = int((profile / "DevToolsActivePort").read_text().splitlines()[0])
with socket.create_connection(("127.0.0.1", port), timeout=2):
    pass
package = json.loads(Path("/opt/talos/browser/runtime.json").read_text())
os.execv(package["chromium"], [package["chromium"], "--user-data-dir=" + str(profile),
    "--no-first-run", "--no-default-browser-check", "--new-window",
    "file:///opt/talos/browser/desktop/home.html"])
