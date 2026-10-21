"""Real Git/context regression for both product Dockerfiles in renamed worktrees."""
from pathlib import Path
import subprocess
import tempfile
import unittest

from image_context import prepare_context

ROOT = Path(__file__).resolve().parents[3]


class ImageContextTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / 'renamed worktree'
        self.repo.mkdir()
        self.destination = self.root / 'context/talos'
        self.git('init', '--quiet')
        self.write('.gitignore', '**/.env\n**/target/\n**/node_modules/\n**/dist/\n')

    def git(self, *args):
        subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True)

    def write(self, path, value):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(value)
        return target

    def test_current_source_inputs_satisfy_both_dockerfiles_without_private_files(self):
        required = set()
        for product in ('worker', 'server'):
            for line in (ROOT / f'build/{product}/Dockerfile').read_text().splitlines():
                if not line.startswith('COPY ') or '--from=' in line:
                    continue
                for source in line.split()[1:-1]:
                    if not source.startswith('talos/') or source.startswith('talos/.release-product/'):
                        continue  # Default source build; packaged release inputs are separate.
                    source = Path(source.removeprefix('talos/'))
                    leaf = source / 'probe.txt' if (ROOT / source).is_dir() else source
                    required.add(leaf)
                    self.write(leaf, str(leaf))
        self.git('add', '.')
        uncommitted = 'talos-app/shared/browser-home/current-candidate.html'
        self.write(uncommitted, 'current uncommitted source')
        for private in ('.env', 'talos-rs/.env', 'talos-app/.env',
                        'talos-rs/target/private', 'talos-app/node_modules/private',
                        'talos-app/web/dist/private', 'unrelated-file'):
            self.write(private, 'must not enter image context')
        prepare_context(self.repo, self.destination)
        actual = {path.relative_to(self.destination) for path in self.destination.rglob('*') if path.is_file()}
        self.assertEqual(actual, required | {Path(uncommitted)})
        for path in required:
            self.assertEqual((self.destination / path).read_text(), str(path))
        self.assertFalse((self.destination / '.git').exists())

    def test_symlink_cannot_import_ignored_or_other_checkout_credentials(self):
        ignored = self.write('talos-app/.env', 'private')
        outside = self.root / 'other-checkout-secret'
        outside.write_text('private')
        link = self.repo / 'talos-app/linked-input'
        for target in (ignored, outside):
            with self.subTest(target=target.name):
                link.symlink_to(target)
                self.git('add', 'talos-app/linked-input')
                with self.assertRaisesRegex(ValueError, 'Git-listed checkout files'):
                    prepare_context(self.repo, self.destination)
                link.unlink()


if __name__ == '__main__':
    unittest.main()
