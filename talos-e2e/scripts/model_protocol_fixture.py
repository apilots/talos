"""Secretless upstream fixture and assertions for the real Gateway/Agent protocol case.

The Agent owns the streaming tool loop. Non-streaming transport/tool coverage belongs
in ProtocolProvider's Rust tests; this fixture never invents a completion API.
"""
import copy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import sys
import threading
import time
import urllib.request

MODEL = "opaque-7f82"
KEY = "controlled-loopback-key"
REPLAYS = ("protocol_default", "omit", "reasoning", "reasoning_content")
PROVIDERS = {f"chat-{replay.replace('_', '-')}": ("chat", replay) for replay in REPLAYS}
PROVIDERS.update({"response-fixture": ("responses", "protocol_default"),
                  "message-fixture": ("messages", "protocol_default"),
                  "message-adaptive-fixture": ("messages", "protocol_default"),
                  "chat-progress-fixture": ("chat", "protocol_default"),
                  "chat-idle-fixture": ("chat", "protocol_default"),
                  "chat-total-fixture": ("chat", "protocol_default")})
MARKER = re.compile(r"PROTOCOL_CASE=([a-z_-]+):(on|off|medium|high)")


def modes(provider):
    if provider in ("chat-idle-fixture", "chat-total-fixture"):
        return ("on",)
    return ("medium", "high") if provider == "message-adaptive-fixture" else ("on", "off")


def http(base, path):
    request = urllib.request.Request(base + path, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)
    assert result["status"] == "success", result
    return result["data"]


def model(protocol, replay, *, adaptive=False):
    parameters = {"temperature": 0.25, "metadata": {"fixture": "root-level-kv"}}
    if protocol == "chat":
        if replay == "reasoning_content":
            parameters["enable_thinking"] = True
        else:
            parameters["chat_template_kwargs"] = {"enable_thinking": True, "fixture_flag": "preserved"}
    elif protocol == "messages":
        parameters["thinking"] = ({"type": "adaptive", "display": "summarized"} if adaptive
                                  else {"type": "enabled", "budget_tokens": 2048})
    return {"id": MODEL, "name": "Opaque fixture model", "context_window": 131072,
        "reasoning": True, "reasoning_effort": "medium", "reasoning_replay": replay,
        "speed": "standard", "input": ["text"],
        "cost": {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0},
        "request_parameters": parameters}


def prepare(template, destination, port, plan):
    import yaml
    config = yaml.safe_load(Path(template).read_text())
    config["agent"]["inner_model"] = {"provider": "chat-protocol-default", "id": MODEL}
    config["llm"]["provider"] = "chat-protocol-default"
    config["llm"]["model"] = MODEL
    config["llm"]["custom_providers"] = {
        provider: {"name": provider, "protocol": protocol, "api_key": KEY,
            "base_url": f"http://127.0.0.1:{port}/{provider}/v1",
            "http_timeouts": {"connect_secs": 1, "response_secs": 1, "stream_idle_secs": 1, "stream_total_secs": 3},
            "models": [model(protocol, replay, adaptive=provider == "message-adaptive-fixture")]}
        for provider, (protocol, replay) in PROVIDERS.items()}
    Path(destination).write_text(yaml.safe_dump(config, sort_keys=False))
    Path(plan).write_text("".join(f"{provider}\t{mode}\n" for provider in PROVIDERS for mode in modes(provider)))


def history(body, protocol):
    if protocol == "responses":
        return body["input"]
    return body["messages"]


def tool_result(items, protocol, call):
    if protocol == "chat":
        return next((item for item in items if item.get("role") == "tool" and item.get("tool_call_id") == call), None)
    if protocol == "responses":
        return next((item for item in items if item.get("type") == "function_call_output" and item.get("call_id") == call), None)
    return next((block for item in items for block in item.get("content", [])
        if isinstance(block, dict) and block.get("type") == "tool_result" and block.get("tool_use_id") == call), None)


def check_request(body, headers, provider, mode):
    protocol, replay = PROVIDERS[provider]
    assert body["model"] == MODEL, "qualified model identity changed"
    assert body["temperature"] == 0.25
    assert body["metadata"] == {"fixture": "root-level-kv"}, "KV not merged at root"
    assert "request_parameters" not in body and "extra_body" not in body
    if protocol == "messages":
        assert headers.get("x-api-key") == KEY and headers.get("anthropic-version") == "2023-06-01"
        assert "authorization" not in headers, "Messages must use native authentication"
        if provider == "message-adaptive-fixture":
            assert body["thinking"] == {"type": "adaptive", "display": "summarized"}, body
            assert body["output_config"] == {"effort": mode}, body
        else:
            assert body["thinking"]["type"] == ("enabled" if mode == "on" else "disabled"), body
            assert body["thinking"].get("budget_tokens") == (2048 if mode == "on" else None), body
    else:
        assert headers.get("authorization") == f"Bearer {KEY}"
        assert "x-api-key" not in headers and "anthropic-version" not in headers
        if protocol == "chat":
            if replay == "reasoning_content":
                assert body["enable_thinking"] is (mode == "on")
                assert "chat_template_kwargs" not in body, "root control was moved into extra kwargs"
            else:
                assert body["chat_template_kwargs"] == {"enable_thinking": mode == "on", "fixture_flag": "preserved"}
                assert "enable_thinking" not in body, "nested control duplicated at root"
            assert body.get("reasoning_effort") == ("medium" if mode == "on" else None), "thinking switch suppressed effort or Off sent an effort"
        else:
            assert body.get("reasoning", {}).get("effort") == ("medium" if mode == "on" else "none"), body
    items = history(body, protocol)
    call = f"call_{provider}_{mode}"
    result = tool_result(items, protocol, call)
    if result is not None:
        assert f"TOOL_{provider}_{mode}" in json.dumps(result), "tool stdout not replayed"
        if protocol == "chat":
            assistant = next(item for item in items if any(c.get("id") == call for c in item.get("tool_calls", [])))
            tool = next(tool for tool in assistant["tool_calls"] if tool["id"] == call)
            assert tool["function"]["name"] == "Bash"
            assert json.loads(tool["function"]["arguments"]) == {"command": f"printf 'TOOL_{provider}_{mode}'"}
            assert items.index(assistant) < items.index(result), "tool result precedes its call"
            expected = "reasoning" if replay == "protocol_default" else replay
            for field in ("reasoning", "reasoning_content"):
                assert assistant.get(field) == (f"private:{call}" if mode == "on" and field == expected else None), (replay, assistant)
        elif protocol == "responses":
            tool = next(item for item in items if item.get("type") == "function_call" and item.get("call_id") == call)
            assert tool["name"] == "Bash" and json.loads(tool["arguments"]) == {"command": f"printf 'TOOL_{provider}_{mode}'"}
            assert items.index(tool) < items.index(result), "function output precedes its call"
            if mode == "on":
                assert any(item.get("type") == "reasoning" and item.get("encrypted_content") == f"encrypted:{call}" for item in items), items
        else:
            blocks = [block for item in items for block in item.get("content", []) if isinstance(block, dict)]
            tool = next(block for block in blocks if block.get("type") == "tool_use" and block.get("id") == call)
            assert tool["name"] == "Bash" and tool["input"] == {"command": f"printf 'TOOL_{provider}_{mode}'"}
            assert blocks.index(tool) < blocks.index(result), "tool result precedes tool_use"
            if mode != "off":
                assert any(block.get("type") == "thinking" and block.get("signature") == f"signature:{call}" for block in blocks)
    return call, result is not None


def chat_chunks(call, mode, done):
    delta = {"role": "assistant"}
    if done:
        delta["content"] = f"PROTOCOL_OK:{call}"
    else:
        if mode == "on":
            delta["reasoning_content"] = f"private:{call}"
        delta["tool_calls"] = [{"index": 0, "id": call, "type": "function", "function": {
            "name": "Bash", "arguments": json.dumps({"command": f"printf 'TOOL_{call[5:]}'"})}}]
    return [{"id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": MODEL,
        "choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
        {"id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": MODEL,
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop" if done else "tool_calls"}],
        "usage": {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110}}]


def response_events(call, mode, done):
    events = []
    index = 0
    if not done and mode == "on":
        item = {"type": "reasoning", "id": f"rs_{call}", "summary": [{"type": "summary_text", "text": f"private:{call}"}], "encrypted_content": f"encrypted:{call}"}
        events.extend([{"type": "response.output_item.added", "output_index": index, "item": item},
            {"type": "response.reasoning_summary_text.delta", "output_index": index, "delta": f"private:{call}"},
            {"type": "response.output_item.done", "output_index": index, "item": item}])
        index += 1
    if done:
        item = {"type": "message", "id": "msg_fixture", "role": "assistant", "content": [{"type": "output_text", "text": f"PROTOCOL_OK:{call}"}]}
        delta = {"type": "response.output_text.delta", "output_index": index, "delta": f"PROTOCOL_OK:{call}"}
    else:
        item = {"type": "function_call", "id": f"fc_{call}", "call_id": call, "name": "Bash", "arguments": json.dumps({"command": f"printf 'TOOL_{call[5:]}'"})}
        delta = {"type": "response.function_call_arguments.delta", "output_index": index, "delta": item["arguments"]}
    start = copy.deepcopy(item)
    if not done:
        start["arguments"] = ""
    events.extend([{"type": "response.output_item.added", "output_index": index, "item": start}, delta,
        {"type": "response.output_item.done", "output_index": index, "item": item},
        {"type": "response.completed", "response": {"id": "resp_fixture", "status": "completed", "output": [item],
            "usage": {"input_tokens": 100, "output_tokens": 10, "total_tokens": 110}}}])
    return events


def message_events(call, mode, done):
    events = [{"type": "message_start", "message": {"id": "msg_fixture", "type": "message", "role": "assistant", "model": MODEL, "content": [], "usage": {"input_tokens": 100, "output_tokens": 0}}}]
    index = 0
    if not done and mode != "off":
        events.extend([{"type": "content_block_start", "index": index, "content_block": {"type": "thinking", "thinking": ""}},
            {"type": "content_block_delta", "index": index, "delta": {"type": "thinking_delta", "thinking": f"private:{call}"}},
            {"type": "content_block_delta", "index": index, "delta": {"type": "signature_delta", "signature": f"signature:{call}"}},
            {"type": "content_block_stop", "index": index}])
        index += 1
    block = {"type": "text", "text": ""} if done else {"type": "tool_use", "id": call, "name": "Bash", "input": {}}
    delta = {"type": "text_delta", "text": f"PROTOCOL_OK:{call}"} if done else {"type": "input_json_delta", "partial_json": json.dumps({"command": f"printf 'TOOL_{call[5:]}'"})}
    events.extend([{"type": "content_block_start", "index": index, "content_block": block},
        {"type": "content_block_delta", "index": index, "delta": delta},
        {"type": "content_block_stop", "index": index},
        {"type": "message_delta", "delta": {"stop_reason": "end_turn" if done else "tool_use"}, "usage": {"output_tokens": 10}},
        {"type": "message_stop"}])
    return events


def serve(directory):
    directory = Path(directory)
    lock = threading.Lock()
    rounds = {}
    class Fixture(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                provider = self.path.split("/")[1]
                protocol, _ = PROVIDERS[provider]
                endpoint = {"chat": "chat/completions", "responses": "responses", "messages": "messages"}[protocol]
                assert self.path == f"/{provider}/v1/{endpoint}", self.path
                headers = {key.lower(): value for key, value in self.headers.items()}
                if not body.get("stream", False):
                    # Existing InnerModel title calls are auxiliary, not Agent tool coverage.
                    assert protocol == "chat" and headers.get("authorization") == f"Bearer {KEY}"
                    self.reply({"id": "title_fixture", "model": MODEL, "choices": [{"index": 0, "message": {"role": "assistant", "content": "Controlled protocol session"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13}})
                    return
                matches = MARKER.findall(json.dumps(history(body, protocol)))
                assert matches, "Agent case marker missing"
                selected_provider, mode = matches[-1]
                assert provider == selected_provider, "same-ID provider routing lost"
                with lock:
                    key = (provider, mode)
                    rounds[key] = rounds.get(key, 0) + 1
                    assert rounds[key] <= 2, "Agent exceeded the expected two-round tool loop"
                call, done = check_request(body, headers, provider, mode)
                with lock:
                    with (directory / "requests.jsonl").open("a") as output:
                        output.write(json.dumps({"provider": provider, "mode": mode, "followup": done, "body": body}) + "\n")
                events = {"chat": chat_chunks, "responses": response_events, "messages": message_events}[protocol](call, mode, done)
                data = "".join((f"event: {event['type']}\n" if protocol != "chat" else "") + f"data: {json.dumps(event)}\n\n" for event in events)
                if protocol == "chat":
                    data += "data: [DONE]\n\n"
                encoded = data.encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                heartbeat_count = 3 if provider == "chat-progress-fixture" else 15 if provider == "chat-total-fixture" else 0
                heartbeat = b": heartbeat\n\n"
                self.send_header("Content-Length", str(len(encoded) + heartbeat_count * len(heartbeat)))
                self.end_headers()
                self.wfile.flush()
                if provider == "chat-idle-fixture":
                    time.sleep(2)
                for _ in range(heartbeat_count):
                    time.sleep(0.4)
                    self.wfile.write(heartbeat)
                    self.wfile.flush()
                self.wfile.write(encoded)
            except (BrokenPipeError, ConnectionResetError):
                if provider not in ("chat-idle-fixture", "chat-total-fixture"):
                    raise
            except Exception as error:
                with lock:
                    with (directory / "errors.jsonl").open("a") as output:
                        output.write(json.dumps({"path": self.path, "error": repr(error)}) + "\n")
                self.reply({"error": {"type": "fixture_assertion", "message": "controlled upstream assertion failed"}}, 400)

        def reply(self, value, status=200):
            encoded = json.dumps(value).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    (directory / "port").write_text(str(server.server_port))
    server.serve_forever()


def verify(base, directory):
    directory = Path(directory)
    errors = directory / "errors.jsonl"
    assert not errors.exists(), errors.read_text() if errors.exists() else ""
    requests = [json.loads(line) for line in (directory / "requests.jsonl").read_text().splitlines()]
    for provider, (protocol, replay) in PROVIDERS.items():
        for mode in modes(provider):
            selected = [request for request in requests if request["provider"] == provider and request["mode"] == mode]
            result = json.loads((directory / f"{provider}-{mode}.json").read_text())
            if provider in ("chat-idle-fixture", "chat-total-fixture"):
                assert len(selected) == 1, (provider, selected)
                assert result["stop_reason"] == "error", result
                stored = http(base, f"/v1/sessions/{result['session_id']}/history?limit=100")
                (directory / f"{provider}-history.json").write_text(json.dumps(stored, indent=2))
                assert any(turn["result"]["status"] == "failed" and turn["result"]["result"]["failure"]["error"]["code"] == "timeout" for turn in stored["data"]), stored
                print(f"CASE stream_timeout_replay PASS provider={provider} code=timeout upstream_requests=1")
                continue
            assert len(selected) == 2 and [request["followup"] for request in selected] == [False, True], (provider, mode, selected)
            assert result["stop_reason"] != "error", result
            assert any(item.get("type") == "output_text" and f"PROTOCOL_OK:call_{provider}_{mode}" in item.get("text", "") for item in result["output"]), result
            assert sum(item.get("type") == "tool_call" and item.get("name") == "Bash" for item in result["output"]) == 1, result
            assert sum(item.get("type") == "tool_result" and not item.get("is_error") for item in result["output"]) == 1, result
            if mode != "off":
                assert result["reasoning"], "omit policy must preserve canonical reasoning display/history"
            print(f"CASE protocol_agent_tool_loop PASS protocol={protocol} replay={replay} thinking={mode} upstream_requests=2")
        if provider in ("chat-idle-fixture", "chat-total-fixture"):
            continue
        stored = http(base, f"/v1/sessions/{result['session_id']}/history?limit=100")
        (directory / f"{provider}-history.json").write_text(json.dumps(stored, indent=2))
        assert f"private:call_{provider}_{modes(provider)[0]}" in json.dumps(stored), "reasoning replay policy changed canonical stored history"
    catalog = http(base, "/v1/models")["models"]
    adaptive_model = next(item for item in catalog if item["provider"] == "message-adaptive-fixture")
    assert adaptive_model["selection_capabilities"]["reasoning"] == "effort", adaptive_model
    print("CASE messages_adaptive_effort PASS session=medium-to-high thinking=preserved tool_history=preserved")



if __name__ == "__main__":
    action, *arguments = sys.argv[1:]
    if action == "serve":
        serve(*arguments)
    elif action == "prepare":
        prepare(*arguments)
    elif action == "verify":
        verify(*arguments)
    elif action == "payload":
        provider, mode, workspace = arguments
        print(json.dumps({"input": {"content": [{"type": "text", "text": f"PROTOCOL_CASE={provider}:{mode}. Run the controlled Bash tool and return its result."}]},
            "modelSelection": {"modelRef": {"provider": provider, "id": MODEL}, "reasoningEffort": None if mode == "off" else "high" if mode == "high" else "medium"},
            "userPermissionMode": "fullAccess", "currentDirectory": workspace}))
    else:
        raise ValueError(f"Unknown action: {action}")
