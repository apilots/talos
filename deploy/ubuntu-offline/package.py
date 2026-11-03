#!/usr/bin/env python3
"""Package exported Docker images without depending on the current Git checkout."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import tarfile


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def checksums(output, name, files):
    (output / name).write_text(''.join(
        f'{digest(output / filename)}  {filename}\n' for filename in files
    ))


def archive(output, name, files):
    # Python tarfile emits portable entries without macOS AppleDouble sidecars.
    with tarfile.open(output / name, 'w', format=tarfile.USTAR_FORMAT) as target:
        for filename in files:
            target.add(output / filename, arcname=filename, recursive=False)


def package(output, revision, architectures):
    if not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise ValueError('Source revision must be a full Git commit hash')
    if not architectures or len(set(architectures)) != len(architectures):
        raise ValueError('Provide each target architecture exactly once')
    if any(arch not in ('arm64', 'amd64') for arch in architectures):
        raise ValueError('Supported architectures: arm64, amd64')
    prefix = (output / 'image-prefix.txt').read_text().strip()
    if not prefix.endswith(':' + revision[:8]):
        raise ValueError('Image prefix does not match the supplied source revision')
    images = [f'talos-ubuntu-pre-{arch}.tar.gz' for arch in architectures]
    shared = ['chromium-seccomp.json', 'run.sh', 'image-prefix.txt']
    for filename in images + shared:
        path = output / filename
        if path.is_symlink() or not path.is_file():
            raise ValueError(f'Expected a regular exported file: {filename}')

    (output / 'source-revision.txt').write_text(revision + '\n')
    (output / 'README.txt').write_text('''Talos Ubuntu offline image

Extract the delivery archive into its own directory.
Verify SHA256SUMS-images for the dual-architecture bundle, or SHA256SUMS-<arch>
for a single-architecture bundle: sha256sum -c <checksum-file>
Run: sudo bash ./run.sh
The launcher selects the host architecture and loads its image when needed.
An explicit arm64 or amd64 argument is also supported.
WebUI: http://<server-ip>:19091
TUI: sudo docker exec -it talos-pre talos

Defaults: CONTAINER_NAME=talos-pre, DATA_VOLUME=talos-pre-data, HTTP_PORT=19091,
CONTAINER_PORT=8080 (the production template's internal listener).
When upgrading, remove only the old container and reuse its existing data volume.
Set CONTAINER_PORT to the gateway.port saved in that volume; older offline
installations normally use: sudo env CONTAINER_PORT=19091 bash ./run.sh
Docker and Chromium user namespace support must be available on the host.
The image contains only the production upgrade template, never a private profile.
Talos upgrades the saved configuration in the data volume at normal startup.
New volumes use production defaults (gateway authentication enabled).
Keep model credentials and endpoints in the deployment volume or its .env.
No startup package downloads are needed.
source-revision.txt records the full immutable product source revision.
''')
    shared += ['source-revision.txt', 'README.txt']
    sums = []
    deliveries = []
    for arch, image in zip(architectures, images):
        checksum = f'SHA256SUMS-{arch}'
        checksums(output, checksum, [image] + shared)
        sums.append(checksum)
        name = f'talos-ubuntu-pre-{arch}-offline.tar'
        archive(output, name, [image] + shared + [checksum])
        deliveries.append(name)
    if len(architectures) == 2:
        checksums(output, 'SHA256SUMS-images', images + shared + sums)
        sums.append('SHA256SUMS-images')
        name = f'talos-ubuntu-pre-{revision[:8]}-dual-arch-offline.tar'
        archive(output, name, images + shared + sums)
        deliveries.append(name)
    files = images + shared + sums + deliveries
    checksums(output, 'SHA256SUMS', files)
    manifest = {
        'revision': revision,
        'imagePrefix': prefix,
        'architectures': architectures,
        'files': [{'file': name, 'size': (output / name).stat().st_size,
                   'sha256': digest(output / name)} for name in files],
    }
    (output / 'bundle-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return deliveries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='Directory containing exported images and launcher assets')
    parser.add_argument('revision', help='Full source revision used to build the exported images')
    parser.add_argument('architectures', nargs='+', choices=('arm64', 'amd64'))
    args = parser.parse_args()
    for name in package(args.output, args.revision, args.architectures):
        print(args.output / name)


if __name__ == '__main__':
    main()
