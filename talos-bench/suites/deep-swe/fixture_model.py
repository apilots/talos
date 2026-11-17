"""Deterministic local model for the Pier adapter smoke test."""

import json

from fixture import model_config as serve_fixture, tool_responses

MARKER = "Talos Pier smoke fixture"


def respond(request):
    results = tool_responses(request)
    if not results:
        return "Bash", {
            "command": f"printf '%s\\n' '{MARKER}' > talos-pier-smoke.txt"
            " && git add -- talos-pier-smoke.txt"
            " && git commit -m 'Add the Talos Pier smoke marker'"
            " && cat talos-pier-smoke.txt"
        }
    if MARKER not in json.dumps(results[-1]):
        raise AssertionError("Talos did not report the expected committed fixture marker")
    return MARKER


def model_config():
    return serve_fixture("deep-swe-pier-fixture", respond)
