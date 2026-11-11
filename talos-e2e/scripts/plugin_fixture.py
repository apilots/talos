"""Prepare the Plugin catalog files shared by the API and CLI scenarios."""
import json
import os
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]


def prepare_catalog(home):
    repo = home / 'plugin-catalog-fixture'
    skill = repo / 'skills/cli-skill'
    mcp = repo / 'mcp/cli-mcp'
    skill.mkdir(parents=True)
    mcp.mkdir(parents=True)
    (skill / 'SKILL.md').write_text('---\nname: cli-skill\ndescription: Plugin lifecycle fixture.\n---\nVERSION_ONE\n')
    shutil.copy2(ROOT / 'talos-rs/crates/action/mcp/tests/fixtures/server.py', mcp / 'server.py')
    (mcp / 'talos-mcp.json').write_text(json.dumps({
        'name': 'cli-mcp', 'transport': {'type': 'stdio', 'command': sys.executable,
        'args': ['server.py', str(home / 'plugin-peer'), 'normal'], 'env': {}, 'cwd': None}}))
    (mcp / 'README.md').write_text('VERSION_ONE\n')
    return repo


if __name__ == '__main__':
    print(prepare_catalog(Path(os.environ['TALOS_HOME'])))
