"""Secretless upstream for real Automation Tool/Service/Worker acceptance.

Only the model protocol is controlled. All business mutations and observations
use the real SDK; files coordinate model streaming with the test driver.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import json
import re
import sys
import threading
import time

MODEL = "automation-controlled"
PROVIDER = "automation-fixture"
KEY = "loopback-fixture-key"
MARKER = re.compile(r"AUTO_CASE=([a-z]+_[a-f0-9]+)")


def prepare(template, destination, port):
    import yaml
    config = yaml.safe_load(Path(template).read_text())
    config["llm"]["provider"] = PROVIDER
    config["llm"]["model"] = MODEL
    config["llm"]["custom_providers"] = {PROVIDER: {
        "name": "Controlled automation", "protocol": "chat", "api_key": KEY,
        "base_url": f"http://127.0.0.1:{port}/v1",
        "http_timeouts": {"connect_secs": 3, "response_secs": 30,
                          "stream_idle_secs": 30, "stream_total_secs": 180},
        "models": [{"id": MODEL, "name": MODEL, "context_window": 131072,
                    "reasoning": False, "input": ["text"], "cost": {}}],
    }}
    config["agent"]["inner_model"] = {"provider": PROVIDER, "id": MODEL}
    config["agent"]["kernel"]["tools"]["enable_mcp_tools"] = False
    config["gateway"]["auth"] = False
    config["sync"] = {**config.get("sync", {}), "enabled": False}
    config.setdefault("cloud", {})["api_base_url"] = f"http://127.0.0.1:{port}"
    config["cloud"]["agent_base_url"] = "https://127.0.0.1:9"
    config["automation"]["dream_memory_maintenance"]["cron"] = "0 0 0 1 1 *"
    Path(destination).write_text(yaml.safe_dump(config, sort_keys=False))
    Path(destination).with_suffix(".json").write_text(json.dumps(config))


def nested(value):
    yield value
    if isinstance(value, dict):
        for item in value.values():
            yield from nested(item)
    elif isinstance(value, list):
        for item in value:
            yield from nested(item)
    elif isinstance(value, str) and value.lstrip().startswith(("{", "[")):
        try:
            yield from nested(json.loads(value))
        except json.JSONDecodeError:
            pass


def snapshot(messages):
    for message in reversed(messages):
        if message.get("role") == "tool":
            for value in nested(message.get("content")):
                if isinstance(value, dict) and "sessionId" in value and "loops" in value and "goal" in value:
                    return value
    raise AssertionError("real Tool result did not contain its Session snapshot")


def serve(directory):
    directory.mkdir(parents=True, exist_ok=True)
    lock = threading.Lock()

    def record(name, value):
        with lock:
            with (directory / name).open("a") as output:
                output.write(json.dumps(value) + "\n")

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def chunk(self, delta, finish=None):
            value = {"id": "automation-fixture", "object": "chat.completion.chunk",
                     "created": 1, "model": MODEL,
                     "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(f"data: {json.dumps(value)}\n\n".encode())
            self.wfile.flush()

        def gate(self, marker, name):
            (directory / f"{marker}.{name}").touch()
            until = time.monotonic() + 150
            while not (directory / f"{marker}.release-{name}").exists():
                assert time.monotonic() < until, f"driver did not release {marker}.{name}"
                self.wfile.write(b": waiting for test driver\n\n")
                self.wfile.flush()
                time.sleep(0.1)

        def action(self, call, action):
            self.chunk({"role": "assistant", "tool_calls": [{"index": 0, "id": call,
                "type": "function", "function": {"name": "automation", "arguments": json.dumps(action)}}]})
            self.chunk({}, "tool_calls")

        def done(self, text):
            self.chunk({"role": "assistant", "content": text})
            self.chunk({}, "stop")

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            try:
                if self.path == "/v1/auth/introspect":
                    raw = json.dumps({"active": body.get("token") == "user-1",
                        "user": {"user_id": "user-1", "email": "fixture@example.invalid", "username": "fixture",
                                 "role": "user", "type": "free", "status": "active"},
                        "capabilities": {"is_admin": False, "role": "user", "scopes": ["sessions:read", "sessions:write"]},
                        "entitlement": {"can_use": True, "plan": "free", "reason": None}, "expires_at_ms": None}).encode()
                    self.send_response(200); self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(raw))); self.end_headers(); self.wfile.write(raw)
                    return
                assert self.path == "/v1/chat/completions"
                assert self.headers.get("Authorization") == f"Bearer {KEY}"
                assert body["model"] == MODEL
                if not body.get("stream"):
                    raw = json.dumps({"id": "aux", "model": MODEL, "choices": [{"index": 0,
                        "message": {"role": "assistant", "content": '{"operation":"noop"}'},
                        "finish_reason": "stop"}], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}).encode()
                    self.send_response(200); self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(raw))); self.end_headers(); self.wfile.write(raw)
                    return
                messages = body["messages"]
                matches = MARKER.findall(json.dumps(messages))
                assert matches, "Agent automation marker missing"
                marker = matches[-1]
                kind = marker.split("_", 1)[0]
                triggers = [index for index, message in enumerate(messages)
                            if message.get("role") == "user" and marker in json.dumps(message.get("content"))]
                registration = "controlled automation work" in json.dumps(messages[triggers[-1]].get("content"))
                iteration_path = directory / f"{marker}.iteration"
                iteration = 0 if registration else int(iteration_path.read_text()) if iteration_path.exists() else 1
                current = messages[triggers[-1]:]
                calls = {message.get("tool_call_id"): message for message in current if message.get("role") == "tool"}
                assert any(tool.get("function", {}).get("name") == "automation" for tool in body.get("tools", []))
                record("requests.jsonl", {"marker": marker, "iteration": iteration, "body": body})
                self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.end_headers()
                if kind == "pending":
                    self.gate(marker, "busy"); self.done("Ordinary User Turn finished")
                elif iteration == 0:
                    instruction = "check progress every 10081 minutes" if kind == "fixedlong" else (
                        "check progress; model decides the next check time" if kind in {"dynamic", "dynamicstop"} else "check progress every 1 minute")
                    if kind != "goal" and kind != "goalcarry":
                        assert instruction in json.dumps(messages[triggers[-1]]), "controlled input must state the requested cadence"
                    call = f"create-{marker}"
                    if call in calls:
                        snapshot(messages)
                        if kind == "stop":
                            self.gate(marker, "registered")
                        self.done("Registration acknowledged; automatic work uses the Session queue")
                    else:
                        action = {"type": "create", "objective": f"AUTO_CASE={marker} controlled Goal work", "tokenBudget": None} if kind in {"goal", "goalcarry"} else {
                            "type": "create", "prompt": f"AUTO_CASE={marker} controlled Loop work; {instruction}",
                            "schedule": {"type": "modelDecided"} if kind in {"dynamic", "dynamicstop"} else {"type": "fixedInterval", "minutes": 10081 if kind == "fixedlong" else 1}}
                        self.action(call, {"target": "goal" if kind in {"goal", "goalcarry"} else "loop", "action": action})
                elif kind == "goalcarry" and iteration == 1:
                    call = f"progress-{marker}"
                    if call in calls:
                        (directory / f"{marker}.iteration").write_text("2")
                        self.done("Inspected the controlled work artifact; continue the same Goal next Turn")
                    else:
                        self.gate(marker, "work-1")
                        self.chunk({"role": "assistant", "tool_calls": [{"index": 0, "id": call,
                            "type": "function", "function": {"name": "Read", "arguments": json.dumps({"path": str(directory / "port")})}}]})
                        self.chunk({}, "tool_calls")
                elif kind in {"goal", "goalcarry"}:
                    read, call = f"read-{marker}-{iteration}", f"complete-{marker}"
                    if call in calls:
                        self.done("Goal completion declared; settle this Turn normally")
                    elif read not in calls:
                        self.action(read, {"target": "goal", "action": {"type": "read"}})
                    else:
                        self.gate(marker, f"work-{iteration}")
                        goal = snapshot(messages)["goal"]
                        self.action(call, {"target": "goal", "action": {"type": "complete", "goalId": goal["id"],
                            "objectiveRevision": goal["objectiveRevision"], "evidence": "Controlled Goal work finished"}})
                elif kind in {"fixed", "fixedlong"}:
                    self.gate(marker, f"work-{iteration}")
                    self.done(f"Fixed Loop work {iteration} finished")
                elif kind == "dynamicstop":
                    read, stop = f"read-{marker}-{iteration}", f"stop-{marker}"
                    if read not in calls:
                        self.action(read, {"target": "loop", "action": {"type": "list"}})
                    elif stop in calls:
                        self.gate(marker, "declared")
                        self.done("Model Stop declaration settles with the current Loop Turn")
                    else:
                        self.gate(marker, f"work-{iteration}")
                        task = snapshot(messages)["loops"][0]
                        self.action(stop, {"target": "loop", "action": {"type": "decide", "loopId": task["id"],
                            "configRevision": task["configRevision"], "decision": {"type": "stop", "reason": "Controlled task is complete; stop checking"}}})
                elif kind == "dynamic":
                    read, valid = f"read-{marker}-{iteration}", f"continue-{marker}"
                    if read not in calls:
                        self.action(read, {"target": "loop", "action": {"type": "list"}})
                    elif valid in calls:
                        self.gate(marker, "declared")
                        self.done("Old revision decision awaits normal settlement")
                    else:
                        task = snapshot(messages)["loops"][0]
                        self.gate(marker, f"work-{iteration}")
                        self.action(valid, {"target": "loop", "action": {
                            "type": "decide", "loopId": task["id"], "configRevision": task["configRevision"],
                            "decision": {"type": "continue", "delayMinutes": 1,
                                         "reason": "Controlled model next check"}}})
                else:
                    raise AssertionError(f"stopped Loop unexpectedly executed: {marker}")
                self.wfile.write(b"data: [DONE]\n\n"); self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                # Original Steer/cancellation may retire the prior model stream.
                record("retired-streams.jsonl", {"at": time.time()})
            except Exception as error:
                record("errors.jsonl", {"error": repr(error), "body": body})
                raise

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    (directory / "port").write_text(str(server.server_port))
    server.serve_forever()


if __name__ == "__main__":
    if sys.argv[1] == "serve":
        serve(Path(sys.argv[2]))
    elif sys.argv[1] == "prepare":
        prepare(sys.argv[2], sys.argv[3], int(sys.argv[4]))
