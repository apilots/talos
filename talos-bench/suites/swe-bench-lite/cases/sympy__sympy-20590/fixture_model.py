"""Controlled local model for the SymPy slots regression."""

import json
import shlex

from fixture import model_config as serve_fixture, tool_responses

MARKER = "Set empty slots on Printable."
EDIT = r"""
from pathlib import Path

source = Path('/app/sympy/core/_print_helpers.py')
text = source.read_text()
anchor = '    # Note, we always use the default ordering'
if text.count(anchor) != 1:
    raise RuntimeError('Expected one insertion point in the pinned SymPy source')
source.write_text(text.replace(anchor, '    __slots__ = ()\n\n' + anchor, 1))
print('Set empty slots on Printable.')
"""


def respond(request):
    results = tool_responses(request)
    if not results:
        return "Bash", {"command": f"python3 -c {shlex.quote(EDIT)}"}
    if MARKER not in json.dumps(results[-1]):
        raise AssertionError("Talos did not report the expected SymPy edit")
    return MARKER


def model_config():
    return serve_fixture("swe-bench-lite-fixture", respond)
