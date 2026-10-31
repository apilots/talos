"""Assertions for real Worker capture, HTTP reads and canonical Session compatibility.

Uses the secretless protocol fixture's completed Turn. No alternate capture path.
"""
import json
import urllib.error
import urllib.request


def query(base, resource, body):
    request = urllib.request.Request(base + "/v1/trajectory/" + resource,
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=10) as response:
        result = json.load(response)
    assert result["status"] == "success", result
    return result["data"]


def payload(base, identity, reference):
    assert reference and reference["complete"] is True and reference["payloadId"], reference
    cursor, chunks, seen = None, [], set()
    while True:
        page = query(base, "payload/read", {"identity": identity, "payloadId": reference["payloadId"], "cursor": cursor, "limitBytes": 127})
        assert len(page["data"].encode()) <= 127
        chunks.append(page["data"])
        cursor = page["nextCursor"]
        if cursor is None:
            return json.loads("".join(chunks))
        assert cursor not in seen
        seen.add(cursor)


def verify(base, directory, history):
    from pathlib import Path
    identity = history["data"][0]["identity"]
    # Terminal history is published after optional history supplementation settles.
    page = query(base, "turn/read", {"identity": identity, "cursor": None, "limit": 32})
    entries = page["data"] or []
    requests = [entry["request"] for entry in entries if entry["type"] == "modelRequest"]
    assert len(requests) == 2, page
    assert page["clockSegments"], page
    details = [query(base, "request/read", {"identity": identity, "modelRequestId": r["modelRequestId"]}) for r in requests]
    inputs = [payload(base, identity, d["prompt"]) for d in details]
    origins = [m["inputOrigin"] for m in inputs[0]["messages"] if m["inputOrigin"]]
    assert any(o["kind"] == "user" and o["source"] == "user" for o in origins), origins
    assert any(o["kind"] == "context" and o["source"] == "runtime" for o in origins), origins
    outputs = [payload(base, identity, d["outputs"][0]["response"]) for d in details]
    options = payload(base, identity, details[0]["parameters"])["options"]
    assert options["requestParameters"]["temperature"] == 0.25 and options["requestParameters"]["metadata"] == {"fixture": "root-level-kv"}, options
    tools = payload(base, identity, details[0]["tools"])
    assert any(d["name"] == "Bash" and d["parameters"] for d in tools["definitions"])
    call = next(part["call"] for part in outputs[0]["parts"] if part["type"] == "toolCall")
    assert call["argumentsText"] == json.dumps(call["arguments"]), call
    tool = query(base, "tool/read", {"identity": identity, "callId": call["callId"]})
    assert tool["tool"]["sourceModelRequestId"] == requests[0]["modelRequestId"], tool
    consumed = next(u for u in tool["usages"] if u["modelRequestId"] == requests[1]["modelRequestId"])
    message = inputs[1]["messages"][consumed["messageIndex"]]
    assert message["role"] == "tool" and message["toolCallId"] == call["callId"] and "TOOL_" in message["content"]
    assert message["name"] == call["name"] and message["inputOrigin"]["source"] == "runtime", message
    result = next(block["content"]["data"]
        for message in history["data"][0]["result"]["result"]["messages"] for block in message["blocks"]
        if block["content"]["type"] == "toolResult" and block["content"]["data"]["callId"] == call["callId"])
    assert all(result["metrics"][field] is not None for field in
        ("queuedAtMs", "startedAtMs", "finishedAtMs", "queueDurationMs", "executionDurationMs")), result
    for request in requests:
        attempt = request["callRecords"][0]
        assert attempt["observedState"] == "completed" and attempt["metrics"]["usage"]["total_tokens"] == 110, request
        assert attempt["metrics"]["executionDurationMs"] is not None and attempt["metrics"]["ttftMs"] is not None
    evidence = {"turn": page, "requests": details, "inputs": inputs, "outputs": outputs, "tool": tool, "parameters": options}
    serialized = json.dumps(evidence)
    assert "controlled-loopback-key" not in serialized and "encrypted:" not in serialized and "signature:" not in serialized
    Path(directory, "trajectory.json").write_text(json.dumps(evidence, indent=2))
    print("CASE trajectory_capture_http PASS requests=2 tool_link=exact timing=provider token=observed payload_pages=utf8 canonical=preserved")
