"""Checksum and distribute only the current task's declared compiled products."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import tarfile

from ci_plan import BINARIES


def checksum(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def seal(path: Path) -> None:
    path.with_name(path.name + '.sha256').write_text(checksum(path))


def verified(path: Path) -> Path:
    if not path.is_file() or not path.with_name(path.name + '.sha256').is_file():
        raise ValueError(f'Missing compiled artifact: {path}')
    if checksum(path) != path.with_name(path.name + '.sha256').read_text().strip():
        raise ValueError(f'Compiled artifact checksum mismatch: {path}')
    return path


def pack_binary(directory: Path, binary: str, source: Path) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f'binary-{binary}.tar.gz'
    with tarfile.open(path, 'w:gz', compresslevel=3) as archive:
        archive.add(source, arcname=BINARIES[binary][1])
    seal(path)


def unpack(directory: Path, binaries: list[str]) -> None:
    for binary in binaries:
        name = BINARIES[binary][1]
        with tarfile.open(verified(directory / f'binary-{binary}.tar.gz')) as archive:
            members = archive.getmembers()
            if len(members) != 1 or members[0].name != name or not members[0].isfile():
                raise ValueError(f'Invalid binary archive: {binary}')
            with archive.extractfile(members[0]) as source, (directory / name).open('wb') as target:
                shutil.copyfileobj(source, target)
            (directory / name).chmod(0o755)


def inventory(directory: Path) -> dict[str, str]:
    return {p.name: checksum(p) for p in sorted(directory.iterdir())
            if p.name.endswith(('.tar.gz', '.tar.zst'))}


if __name__ == '__main__':
    unpack(Path('output/ci-binaries'), json.loads(os.environ['CI_BINARIES']))
