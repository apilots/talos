"""Run the CLI's unattended lifecycle against a controlled loopback provider."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import threading
from uuid import uuid4

import yaml

from model_protocol_fixture import MODEL, chat_chunks, prepare

METRICS_PREFIX = "TALOS_EXEC_METRICS "
PROVIDER = "chat-protocol-default"


class Fixture(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if not body.get("stream"):
            self.reply({"choices": [{"message": {"role": "assistant", "content": "CLI fixture"},
                                     "finish_reason": "stop"}]})
            return
        history = body["messages"]
        user_index = max(i for i, message in enumerate(history) if message["role"] == "user")
        case = history[user_index]["content"]
        call = "call_" + case
        if case == "fail":
            self.reply({"error": {"message": "Controlled provider failure", "type": "invalid_request_error"}}, 400)
            return
        done = case == "seed" or any(message.get("tool_call_id") == call for message in history[user_index:])
        marker = shlex.quote(str(self.server.workspace / case))
        command = f"test ! -s {marker} && printf tool-ran > {marker}"
        chunks = chat_chunks(call, "off", done, command=command)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(("".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks) + "data: [DONE]\n\n").encode())

    def reply(self, payload, status=200):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(payload).encode())


def exercise(root, work, report):
    workspace = work / "workspace"
    workspace.mkdir()
    home = work / "home"
    config = home / "conf/talos.yaml"
    config.parent.mkdir(parents=True)
    with ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server:
        server.workspace = workspace
        prepare(root / "conf/talos-dev.yaml", config, server.server_port, work / "plan.tsv")
        settings = yaml.safe_load(config.read_text())
        settings["gateway"]["auth"] = False
        settings["agent"]["kernel"]["tools"]["sandbox_policy"]["user_permission_mode"] = "ask_for_approval"
        settings["agent"]["kernel"]["plugins"]["sources"] = []
        config.write_text(yaml.safe_dump(settings))
        original = config.read_bytes()
        environment = dict(os.environ, TALOS_HOME=str(home), TALOS_CONFIG=str(config),
                           TALOS_RESOURCE_DIR=str(root), TALOS_TUI_NO_BROWSER="1")
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        try:
            def run(case, expected, session, *flags):
                trace_file = report / f"{case}.trajectory.json"
                transcript_file = report / f"{case}.transcript.json"
                if case == "new":
                    flags = (*flags, "--transcript", str(transcript_file))
                result = subprocess.run(
                    [environment["TALOS_E2E_CLI_BIN"], "exec", "--session", session,
                     "--model", f"{PROVIDER}:{MODEL}", "--trajectory", str(trace_file), *flags, case],
                    env=environment, cwd=workspace, capture_output=True, text=True, timeout=25)
                (report / f"{case}.stdout.txt").write_text(result.stdout)
                (report / f"{case}.stderr.txt").write_text(result.stderr)
                assert result.returncode == expected, (case, result.returncode, result.stderr)
                lines = result.stderr.splitlines()
                records = [line[len(METRICS_PREFIX):] for line in lines
                           if line.startswith(METRICS_PREFIX)]
                assert len(records) == 1 and METRICS_PREFIX not in result.stdout, result
                if case != "fail":
                    rounds = 2 if case in {"new", "enable", "saved"} else 1
                    usage = json.loads(records[0])["usage"]
                    assert usage["input_tokens"] == 100 * rounds, usage
                    assert usage["output_tokens"] == 10 * rounds, usage
                    assert usage["total_tokens"] == 110 * rounds, usage
                trace = json.loads(trace_file.read_text())
                if expected == 3:
                    assert trace is None, trace
                else:
                    assert set(trace) == {"identity", "clockSegments", "data", "nextCursor"}, trace
                    assert trace["identity"]["sessionId"] == session and trace["nextCursor"] is None, trace
                if expected == 0:
                    stdout_lines = result.stdout.splitlines()
                    assert stdout_lines[0] == f"PROTOCOL_OK:call_{case}", result.stdout
                    status = json.loads(stdout_lines[-1])
                    assert status["sessionId"] == session and status["status"] == "completed", status
                    requests = [entry["request"] for entry in trace["data"]
                                if entry["type"] == "modelRequest"]
                    assert len(requests) == rounds, trace
                    assert all(req["callRecords"][0]["metrics"]["ttftMs"] is not None for req in requests), trace
                    if rounds == 2:
                        tool, = [line for line in lines if line.startswith("TOOL_CALL_COMPLETED ")]
                        assert "internal=Bash is_error=false" in tool
                        assert json.loads(tool.rsplit(" metrics=", 1)[1])["executionDurationMs"] is not None
                if case == "new":
                    transcript = transcript_file.read_text()
                    contents = [block["content"] for block in json.loads(transcript)]
                    assert contents[0]["type"] == "userContent", contents
                    assert {"assistantText", "toolCall", "toolResult"} <= {item["type"] for item in contents}
                    assert "controlled-loopback-key" not in transcript
                assert "controlled-loopback-key" not in trace_file.read_text()
                return result

            blocked = run("blocked", 3, str(uuid4()))
            assert "WAITING_FOR_INPUT " in blocked.stderr and "kind=approval" in blocked.stderr and "call_id=call_blocked" in blocked.stderr
            waiting = json.loads(blocked.stdout.splitlines()[-1])
            assert waiting["status"] == "waiting" and waiting["sessionId"], waiting
            assert not (workspace / "blocked").exists()
            run("fail", 1, str(uuid4()))
            new = run("new", 0, str(uuid4()), "--dangerously-skip-permissions")
            assert "Saving Full Access" in new.stderr
            assert (workspace / "new").read_text() == "tool-ran"
            existing = str(uuid4())
            run("seed", 0, existing)
            run("enable", 0, existing, "--dangerously-skip-permissions")
            run("saved", 0, existing)
            assert (workspace / "enable").read_text() == (workspace / "saved").read_text() == "tool-ran"
            assert config.read_bytes() == original
            print("CASE exec_unattended PASS waiting=3 failure=1 completion=0 usage=stderr permission=session")
        finally:
            server.shutdown()
            server_thread.join()


if __name__ == "__main__":
    exercise(*map(Path, sys.argv[1:]))
