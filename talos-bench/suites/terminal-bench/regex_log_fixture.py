"""Deterministic controlled solution for the regex-log smoke trial."""

import shlex

from fixture import model_config as serve_fixture, tool_responses

TOOL_MARKER = "regex-log artifact written"


def artifact_command():
    octet = r"(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9][0-9]?|0)"
    ipv4 = rf"{octet}(?:\.{octet}){{3}}"
    day31 = r"(?:0[1-9]|[12][0-9]|3[01])"
    month_day = rf"(?:(?:0[13578]|1[02])-{day31}|(?:0[469]|11)-(?:0[1-9]|[12][0-9]|30)|02-(?:0[1-9]|[12][0-9]))"
    boundary = r"[A-Za-z0-9]"
    pattern = rf"^(?=.*(?<!{boundary}){ipv4}(?!{boundary})).*(?<!{boundary})([0-9]{{4}}-{month_day})(?!{boundary})"
    return f"printf '%s\\n' {shlex.quote(pattern)} > /app/regex.txt && printf '%s\\n' {shlex.quote(TOOL_MARKER)}"



def respond(request):
    if results := tool_responses(request):
        assert TOOL_MARKER in results[-1]["content"], "Talos did not return the successful tool output"
        return "Wrote the regex to /app/regex.txt."
    return "Bash", {"command": artifact_command()}


def model_config():
    return serve_fixture("regex-log-fixture", respond)
