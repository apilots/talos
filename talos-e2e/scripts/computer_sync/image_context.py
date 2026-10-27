"""Stage current-checkout inputs shared by the product Server and Worker images."""
from pathlib import Path
import shutil
import subprocess
import sys


def prepare_context(repo, destination):
    repo, destination = Path(repo).resolve(), Path(destination)
    listed = subprocess.check_output(
        ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=repo)
    paths = {Path(raw.decode()) for raw in listed.split(b'\0') if raw}
    for path in sorted(paths):
        if (path.parts[0] not in ('talos-rs', 'talos-app', 'conf')
                and not path.is_relative_to('build/worker/browser')
                and not path.is_relative_to('build/container')
                and not path.is_relative_to('build/plugins')
                and str(path) not in ('LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES')):
            continue
        source, target = repo / path, destination / path
        if source.is_file():
            # A tracked symlink must not smuggle another checkout or ignored
            # credential into an otherwise isolated Docker context.
            if source.is_symlink():
                resolved = source.resolve(strict=True)
                if not resolved.is_relative_to(repo) or resolved.relative_to(repo) not in paths:
                    raise ValueError(f'Image input resolves outside Git-listed checkout files: {path}')
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)


if __name__ == '__main__':
    prepare_context(*sys.argv[1:])
