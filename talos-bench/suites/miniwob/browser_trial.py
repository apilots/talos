"""Host and observe the upstream episode while Talos runs its normal CLI."""

from contextlib import contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
from urllib.request import urlopen

from websockets.exceptions import ConnectionClosed
from websockets.sync.client import connect

from run_talos import SUBMISSIONS, main


MINIWOB = Path("/opt/miniwob")
RESOURCES = Path("/opt/talos")
READY = "typeof core !== 'undefined' && !!core.cover_div && document.readyState === 'complete'"
METADATA = """({
    done: WOB_DONE_GLOBAL, env_reward: WOB_REWARD_GLOBAL,
    raw_reward: WOB_RAW_REWARD_GLOBAL, reason: WOB_REWARD_REASON
})"""


def evaluate(connection, expression):
    connection.send(json.dumps({
        "id": 1, "method": "Runtime.evaluate", "params": {"expression": expression, "returnByValue": True},
    }))
    while (message := json.loads(connection.recv(timeout=2))).get("id") != 1:
        pass
    if "error" in message or "exceptionDetails" in message.get("result", {}):
        raise RuntimeError(f"MiniWoB++ browser evaluation failed: {message}")
    return message["result"]["result"].get("value")


def observe_episode(home, episode, stop, receipt):
    """Use the managed browser's private DevTools socket for setup and readout."""
    endpoint = home / "browser/profile/DevToolsActivePort"
    target_url = episode["url"]
    while not stop.is_set():
        try:
            port = int(endpoint.read_text().splitlines()[0])
            with urlopen(f"http://127.0.0.1:{port}/json/list", timeout=1) as response:
                pages = json.load(response)
        except (FileNotFoundError, IndexError, OSError, ValueError):
            stop.wait(0.05)
            continue
        page = next((page for page in pages if page.get("url") == target_url), None)
        if page:
            break
        stop.wait(0.05)
    else:
        return

    with connect(page["webSocketDebuggerUrl"], open_timeout=2) as connection:
        while not evaluate(connection, READY):
            if stop.wait(0.05):
                return
        evaluate(connection, f"Math.seedrandom({episode['seed']}); core.startEpisodeReal();")
        if not evaluate(connection, "WOB_TASK_READY"):
            raise RuntimeError("The upstream MiniWoB++ page did not initialize")
        with urlopen(f"http://127.0.0.1:{port}/json/version", timeout=1) as response:
            receipt["browserVersion"] = json.load(response)["Browser"]
        receipt["utterance"] = evaluate(connection, "core.getUtterance()")
        while True:
            try:
                receipt.update(evaluate(connection, METADATA))
            except ConnectionClosed:
                if "done" not in receipt:
                    raise
                receipt["browserDisconnected"] = True
                break
            if receipt["done"] or stop.wait(0.05):
                break


@contextmanager
def trial_observer(home):
    episode = json.loads((MINIWOB / "episode-config.json").read_text())
    receipt = {}
    errors = []
    stop = threading.Event()

    def observe():
        try:
            observe_episode(home, episode, stop, receipt)
        except Exception as error:
            errors.append(error)

    handler = partial(SimpleHTTPRequestHandler, directory=MINIWOB / "html")
    with ThreadingHTTPServer(("127.0.0.1", 8765), handler) as server:
        server_thread = threading.Thread(target=server.serve_forever)
        observer = threading.Thread(target=observe)
        server_thread.start()
        observer.start()
        try:
            yield
        finally:
            stop.set()
            observer.join()
            server.shutdown()
            server_thread.join()
    if errors:
        raise RuntimeError("The MiniWoB++ episode observer failed") from errors[0]
    if "done" not in receipt:
        raise RuntimeError("Talos did not reach an initialized MiniWoB++ browser episode")
    SUBMISSIONS.mkdir(parents=True, exist_ok=True)
    (SUBMISSIONS / "episode.json").write_text(json.dumps({**episode, **receipt}, indent=2) + "\n")


if __name__ == "__main__":
    raise SystemExit(main(resources=RESOURCES, observe=trial_observer))
