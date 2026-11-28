"""Run the CLI's unattended lifecycle against a controlled loopback provider."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
import os
from pathlib import Path
import shlex
import sqlite3
import subprocess
import sys
import threading
import time
from uuid import uuid4

import yaml

from model_protocol_fixture import MODEL, chat_chunks, prepare

METRICS_PREFIX = "TALOS_EXEC_METRICS "
PROVIDER = "chat-protocol-default"


DEPTH_CASES = ("admission-root", "admission-child", "admission-grandchild", "admission-great-grandchild")
CHECKPOINT_CASES = ("checkpoint-effects", "checkpoint-unreadable")


def admission_chunks(case, history):
    level = DEPTH_CASES.index(case)
    at_limit = level == len(DEPTH_CASES) - 1
    target = ("child", "grandchild", "great_grandchild", "too_deep")[level]
    next_prompt = (*DEPTH_CASES[1:], "unexpected-beyond-admission")[level]
    results = [message for message in history if message["role"] == "tool"]
    if at_limit and results:
        response = json.loads(results[-1]["content"])
        assert response["ok"] is False and "3 child levels" in response["error"]["message"], response
    if not at_limit and len(results) == 2:
        outcome = json.loads(results[-1]["content"])["result"]["result"]["outcome"]
        assert outcome["status"] == "completed" and outcome["output"] == f"PROTOCOL_OK:call_{next_prompt}", outcome
    done = len(results) == (1 if at_limit else 2)
    chunks = chat_chunks("call_" + case, "off", done)
    if not done:
        name, args = ("spawn_agent", {"taskName": target, "prompt": next_prompt}) if not results else (
            "wait_agent", {"target": {"type": "directChild", "name": target}, "timeoutMs": 10000},
        )
        tool = chunks[0]["choices"][0]["delta"]["tool_calls"][0]
        tool.update(id=f"call_{case}_{len(results)}", function={"name": name, "arguments": json.dumps(args)})
    return chunks


class Fixture(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if not body.get("stream"):
            content = "CLI fixture"
            user_input = body["messages"][-1].get("content")
            if isinstance(user_input, str) and user_input.startswith("{"):
                prompt = json.loads(user_input)
                if "userMessage" in prompt:
                    case, answer, effect = prompt["userMessage"], prompt["assistantResponse"], "memory"
                else:
                    case, answer, effect = prompt.get("user_message"), prompt.get("assistant_response"), "title"
                if case in CHECKPOINT_CASES:
                    if effect == "title":
                        # Terminal delivery must not let CLI shutdown cancel product effects.
                        time.sleep(0.2)
                    self.server.checkpoint_effects.append((case, effect, answer))
                    content = '{"operation":"noop"}' if effect == "memory" else "Checkpoint effects verified"
            self.reply({"model": MODEL,
                        "choices": [{"message": {"role": "assistant", "content": content},
                                     "finish_reason": "stop"}]})
            return
        history = body["messages"]
        user_index = max(i for i, message in enumerate(history) if message["role"] == "user")
        case = history[user_index]["content"]
        if case == "hold-for-owner-crash":
            self.server.holder_started.set()
            self.server.release_holder.wait(timeout=45)
            return
        call = "call_" + case
        if case == "fail":
            self.reply({"error": {"message": "Controlled provider failure", "type": "invalid_request_error"}}, 400)
            return
        if case in DEPTH_CASES:
            chunks = admission_chunks(case, history[user_index:])
        elif case == "delegate":
            chunks = self.delegate_chunks(call, history[user_index:])
        elif case in CHECKPOINT_CASES:
            if case == "checkpoint-unreadable":
                # Corrupt only the product view, leaving canonical Worker history intact.
                with sqlite3.connect(self.server.database) as db:
                    changed = db.execute(
                        """UPDATE agent_events SET payload_json=json_set(payload_json, '$.data.status', 'inProgress')
                           WHERE session_id=? AND json_extract(payload_json, '$.type')='messageCompleted'
                             AND json_extract(payload_json, '$.data.role')='user'""",
                        (self.server.unreadable_session,))
                    assert changed.rowcount == 1
            chunks = chat_chunks(call, "off", True)
        else:
            done = case in {"seed", "recovered", "recovered-again"} or any(
                message.get("tool_call_id") == call for message in history[user_index:]
            )
            marker = shlex.quote(str(self.server.workspace / case))
            command = f"test ! -s {marker} && printf tool-ran > {marker}"
            chunks = chat_chunks(call, "off", done, command=command)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(("".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks) + "data: [DONE]\n\n").encode())

    def delegate_chunks(self, call, messages):
        results = {message.get("tool_call_id"): message for message in messages
                   if message["role"] == "tool"}
        if f"{call}_wait" in results:
            assert (self.server.workspace / "child").read_text() == "tool-ran", results
            return chat_chunks(call, "off", True)
        if f"{call}_spawn" in results:
            name = "wait_agent"
            arguments = {"target": {"type": "directChild", "name": "inheritance"}, "timeoutMs": 10000}
            call_id = f"{call}_wait"
        else:
            name = "spawn_agent"
            arguments = {"taskName": "inheritance", "prompt": "child"}
            call_id = f"{call}_spawn"
        chunks = chat_chunks(call_id, "off", False)
        chunks[0]["choices"][0]["delta"]["tool_calls"][0]["function"] = {
            "name": name, "arguments": json.dumps(arguments),
        }
        return chunks

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
        server.database = home / "data/talos.db"
        server.checkpoint_effects = []
        server.checkpoint_details = {}
        server.holder_started = threading.Event()
        server.release_holder = threading.Event()
        prepare(root / "conf/talos-dev.yaml", config, server.server_port, work / "plan.tsv")
        settings = yaml.safe_load(config.read_text())
        settings["gateway"]["auth"] = False
        settings["llm"]["custom_providers"][PROVIDER]["http_timeouts"].update(
            response_secs=40, stream_idle_secs=40, stream_total_secs=45)
        settings["agent"]["kernel"]["tools"]["sandbox_policy"]["user_permission_mode"] = "ask_for_approval"
        settings["agent"]["kernel"]["plugins"]["sources"] = []
        config.write_text(yaml.safe_dump(settings))
        original = config.read_bytes()
        environment = dict(os.environ, TALOS_HOME=str(home), TALOS_CONFIG=str(config),
                           TALOS_RESOURCE_DIR=str(root), TALOS_TUI_NO_BROWSER="1")
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        try:
            def run(case, expected, session, *flags, evidence_name=None):
                evidence_name = evidence_name or case
                trace_file = report / f"{evidence_name}.trajectory.json"
                transcript_file = report / f"{evidence_name}.transcript.json"
                if case == "new":
                    flags = (*flags, "--transcript", str(transcript_file))
                result = subprocess.run(
                    [environment["TALOS_E2E_CLI_BIN"], "exec", "--session", session,
                     "--model", f"{PROVIDER}:{MODEL}", "--trajectory", str(trace_file), *flags, case],
                    env=environment, cwd=workspace, capture_output=True, text=True, timeout=25)
                (report / f"{evidence_name}.stdout.txt").write_text(result.stdout)
                (report / f"{evidence_name}.stderr.txt").write_text(result.stderr)
                assert result.returncode == expected, (case, result.returncode, result.stderr)
                if case in CHECKPOINT_CASES:
                    with sqlite3.connect(server.database) as db:
                        title = db.execute("SELECT title FROM sessions WHERE session_key=?", (session,)).fetchone()
                        notices = [json.loads(row[0]) for row in db.execute(
                            "SELECT payload_json FROM agent_events WHERE session_id=? AND sequence IS NULL", (session,))]
                        user_status = db.execute(
                            """SELECT json_extract(payload_json,'$.data.status') FROM agent_events
                               WHERE session_id=? AND json_extract(payload_json,'$.type')='messageCompleted'
                                 AND json_extract(payload_json,'$.data.role')='user'""", (session,)).fetchone()
                    server.checkpoint_details[case] = {"title": title, "notices": notices}
                    if case == "checkpoint-unreadable":
                        server.checkpoint_details[case]["userStatus"] = user_status[0]
                    (report / f"{case}.product.json").write_text(json.dumps(server.checkpoint_details[case]))

                lines = result.stderr.splitlines()
                records = [line[len(METRICS_PREFIX):] for line in lines
                           if line.startswith(METRICS_PREFIX)]
                assert len(records) == 1 and METRICS_PREFIX not in result.stdout, result
                if case != "fail":
                    rounds = {"new": 2, "enable": 2, "saved": 2, "delegate": 3, DEPTH_CASES[0]: 3}.get(case, 1)
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
            first = run("seed", 0, existing, "--request-id", "seed-request")
            replay = run("seed", 0, existing, "--request-id", "seed-request",
                         evidence_name="seed-replay")
            assert json.loads(replay.stdout.splitlines()[-1]) == json.loads(
                first.stdout.splitlines()[-1])
            run("enable", 0, existing, "--dangerously-skip-permissions")
            run("saved", 0, existing)
            assert (workspace / "enable").read_text() == (workspace / "saved").read_text() == "tool-ran"
            run("delegate", 0, str(uuid4()), "--dangerously-skip-permissions")
            assert (workspace / "child").read_text() == "tool-ran"
            run(DEPTH_CASES[0], 0, str(uuid4()))
            print("CASE subagent_admission PASS default depth=3 nested result delivered")
            readable_session = str(uuid4())
            server.unreadable_session = str(uuid4())
            run("checkpoint-effects", 0, readable_session)
            run("checkpoint-unreadable", 0, server.unreadable_session)
            assert sorted(server.checkpoint_effects) == [
                ("checkpoint-effects", "memory", "PROTOCOL_OK:call_checkpoint-effects"),
                ("checkpoint-effects", "title", "PROTOCOL_OK:call_checkpoint-effects"),
                ("checkpoint-unreadable", "memory", "PROTOCOL_OK:call_checkpoint-unreadable"),
                ("checkpoint-unreadable", "title", "PROTOCOL_OK:call_checkpoint-unreadable"),
            ], (server.checkpoint_effects, server.checkpoint_details)
            with sqlite3.connect(server.database) as db:
                titles = dict(db.execute("SELECT session_key,title FROM sessions WHERE session_key IN (?,?)",
                                         (readable_session, server.unreadable_session)))
                events = db.execute(
                    """SELECT json_extract(payload_json,'$.content.data.code'),
                              json_extract(payload_json,'$.type') FROM agent_events
                       WHERE session_id=? AND (
                           sequence IS NULL
                           OR json_extract(payload_json,'$.type')='turnCompleted')
                       ORDER BY event_seq""", (server.unreadable_session,)).fetchall()
            assert titles == {session: "Checkpoint effects verified"
                              for session in (readable_session, server.unreadable_session)}, titles
            assert server.checkpoint_details["checkpoint-unreadable"]["userStatus"] == "inProgress", server.checkpoint_details
            # Product-view corruption cannot suppress Worker-owned title/Memory
            # or rewrite the committed outcome; neither workload needs a product replay.
            assert events == [(None, "turnCompleted")], events
            assert all(not details["notices"] for details in server.checkpoint_details.values()), server.checkpoint_details
            print("CASE checkpoint_completion_effects PASS Worker title and memory ignore corrupted product history")
            assert config.read_bytes() == original
            # Seed an unowned due Loop in the isolated database. The unrelated
            # exec stays alive across scan ticks while the fixture holds its model.
            loop_id = "unowned-due-loop"
            due = int(time.time()) - 1
            database = server.database
            with sqlite3.connect(database) as db:
                db.execute(
                    """INSERT INTO session_loops
                       (id,session_id,prompt,schedule_type,minutes,config_revision,status,next_due_at,created_at,updated_at)
                       VALUES (?,?,'Unrelated Loop work','fixedInterval',2,1,'active',?,?,?)""",
                    (loop_id, existing, due, due - 60, due - 60))
            crash_session = str(uuid4())
            command = [environment["TALOS_E2E_CLI_BIN"], "exec", "--session", crash_session,
                       "--model", f"{PROVIDER}:{MODEL}"]
            holder = subprocess.Popen(
                [*command, "hold-for-owner-crash"], env=environment, cwd=workspace,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            marker_path = (home / "data/worker/execution-admission"
                           / f"{hashlib.sha256(crash_session.encode()).hexdigest()}.json")
            try:
                assert server.holder_started.wait(timeout=20), "owner never reached the fixture model"
                marker = json.loads(marker_path.read_text())
                assert marker["active"] and marker["cleanup_confirmed"] is False, marker
                competing = subprocess.run(
                    [*command, "competing"], env=environment, cwd=workspace,
                    capture_output=True, text=True, timeout=15)
                (report / "competing.stderr.txt").write_text(competing.stderr)
                assert competing.returncode == 4, competing.stderr
                plugin = work / "running-turn-mcp.json"
                plugin.write_text(json.dumps({
                    "name": "while-running", "transport": {
                        "type": "streamable_http", "url": "http://127.0.0.1:1",
                    },
                }))
                install = subprocess.run(
                    [environment["TALOS_E2E_CLI_BIN"], "plugin", "mcp", "add", "--file", str(plugin)],
                    env=environment, cwd=workspace, capture_output=True, text=True, timeout=15)
                (report / "running-turn-plugin.stdout.txt").write_text(install.stdout)
                (report / "running-turn-plugin.stderr.txt").write_text(install.stderr)
                # A separate local entry cannot create another Plugin runtime
                # while the foreground Worker owns this namespace.
                assert install.returncode == 4, install.stderr
                time.sleep(2)
                assert holder.poll() is None, "owner exited before two scan ticks"
                with sqlite3.connect(database) as db:
                    unchanged = db.execute(
                        "SELECT next_due_at FROM session_loops WHERE id=?", (loop_id,)).fetchone()
                assert unchanged == (due,), unchanged
            finally:
                if holder.poll() is None:
                    holder.kill()
                holder_stdout, holder_stderr = holder.communicate(timeout=10)
                server.release_holder.set()
                (report / "killed-owner.stdout.txt").write_text(holder_stdout)
                (report / "killed-owner.stderr.txt").write_text(holder_stderr)
            assert holder.returncode == -9, holder_stderr
            install = subprocess.run(
                [environment["TALOS_E2E_CLI_BIN"], "plugin", "mcp", "add", "--file", str(plugin)],
                env=environment, cwd=workspace, capture_output=True, text=True, timeout=15)
            (report / "released-owner-plugin.stdout.txt").write_text(install.stdout)
            (report / "released-owner-plugin.stderr.txt").write_text(install.stderr)
            assert install.returncode == 0, install.stderr
            assert json.loads(install.stdout)["install"]["enabled"]
            run("recovered", 0, crash_session)
            run("recovered-again", 0, crash_session)
            recovered_marker = json.loads(marker_path.read_text())
            assert recovered_marker["active"] != marker["active"], recovered_marker
            with sqlite3.connect(database) as db:
                failures = db.execute(
                    """SELECT payload_json FROM agent_events
                       WHERE session_id=? AND turn_id=?
                         AND json_extract(payload_json,'$.type')='turnFailed'""",
                    (crash_session, marker["active"]["turnId"])).fetchall()
            assert len(failures) == 1
            recovered_failure = json.loads(failures[0][0])["data"]
            assert recovered_failure["error"]["code"] == "cancelled", recovered_failure
            assert recovered_failure["retryable"] is True, recovered_failure
            print("CASE exec_unattended PASS waiting=3 failure=1 completion=0 usage=stderr permission=session-and-child held=4 request_id=replayed sigkill=recovered unrelated_loop=untouched plugin=owner_guarded_then_installed")
        finally:
            server.shutdown()
            server_thread.join()


if __name__ == "__main__":
    exercise(*map(Path, sys.argv[1:]))
