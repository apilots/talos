"""Build metadata from the installed upstream package and the official source lock."""
import hashlib
import importlib.metadata
import json
from pathlib import Path
import platform
import sys

root = Path('/opt/talos/browser')
lock = Path('/opt/talos/plugins/apilots-mcps/mcp/browser/requirements.lock')
version = importlib.metadata.version('browser-use')
runtime = json.loads((root / 'runtime.json').read_text())
runtime['browserUseVersion'] = version
(root / 'runtime.json').write_text(json.dumps(runtime))
(root / 'environment.json').write_text(json.dumps({
    'lockSha256': hashlib.sha256(lock.read_bytes()).hexdigest(),
    'platform': sys.platform + '-' + platform.machine(),
    'pythonAbi': sys.implementation.cache_tag,
    'hostApi': 1,
    'python': str(root / 'venv/bin/python'),
    'browserUseVersion': version,
}))
