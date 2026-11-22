#!/usr/bin/env python3
"""Test release resources, pinned downloads, and the complete publication gate."""
from contextlib import redirect_stderr, redirect_stdout
import hashlib
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest
from unittest.mock import patch


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    result = importlib.util.module_from_spec(spec); spec.loader.exec_module(result)
    return result


setup = module('setup_appimage', 'setup-appimage.py')
assets = module('release_assets', 'check-release-assets.py')
packager = module('linux_package', 'package-linux.py')
smoke = module('package_smoke', 'smoke-package.py')


class LinuxRelease(unittest.TestCase):
    def test_smoke_tree_runs_its_copied_packager_without_the_checkout(self):
        with tempfile.TemporaryDirectory() as work:
            source = Path(work) / 'source with spaces'
            smoke.prepare_source(source)
            result = subprocess.run(
                [sys.executable, '-E', str(source / 'scripts/package-linux.py'), '--help'],
                cwd=source, check=True, capture_output=True, text=True)
            self.assertIn('--binary', result.stdout)

    def test_linux_archive_checks_both_versions_before_replacing_existing_outputs(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            source = root / 'source'
            smoke.prepare_source(source)
            version = tomllib.loads((source / 'Cargo.toml').read_text())['package']['version']
            binaries = [root / 'hedit', root / 'hedit-remote']
            for binary in binaries:
                binary.write_bytes(b'executable fixture')
                binary.chmod(0o755)
            output = root / 'output'
            output.mkdir()
            name = f'hedit-{version}-linux-x86_64'
            archive = output / (name + '.tar.gz')
            archive.write_bytes(b'previous archive')
            arguments = ['package-linux.py', '--binary', str(binaries[0]),
                         '--helper', str(binaries[1]), '--out-dir', str(output)]
            stale = None

            def reported_version(arguments, **kwargs):
                binary = Path(arguments[0]).name
                return f'{binary} {"0.0.0" if binary == stale else version}\n'

            with patch.object(packager, 'ROOT', source), \
                    patch.object(sys, 'argv', arguments), \
                    patch.object(packager.platform, 'system', return_value='Linux'), \
                    patch.object(packager.platform, 'machine', return_value='x86_64'), \
                    patch.object(packager.subprocess, 'check_output', side_effect=reported_version), \
                    redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                for stale in ['hedit', 'hedit-remote']:
                    with self.subTest(stale=stale), self.assertRaises(SystemExit):
                        packager.main()
                    self.assertEqual(archive.read_bytes(), b'previous archive')
                    self.assertEqual(list(output.iterdir()), [archive])
                stale = None
                packager.main()
            with tarfile.open(archive) as package:
                self.assertEqual(package.extractfile(name + '/hedit').read(), binaries[0].read_bytes())
                self.assertEqual(package.extractfile(name + '/docs/extensions.md').read(),
                                 (source / 'docs/extensions.md').read_bytes())
                self.assertEqual(package.extractfile(name + '/docs/extension-security.md').read(),
                                 (source / 'docs/extension-security.md').read_bytes())
                self.assertNotIn(name + '/examples/development_probe.rs', package.getnames())
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(archive.with_name(archive.name + '.sha256').read_text(),
                             f'{digest}  {archive.name}\n')

    def test_source_documentation_install_updates_an_existing_directory_with_spaces(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as work:
            destination = Path(work) / 'share docs/hedit'
            command = [sys.executable, str(root / 'scripts/package_resources.py'),
                       '--out-dir', str(destination)]
            subprocess.run(command, check=True)
            (destination / 'docs/extensions.md').write_text('previous guide')
            subprocess.run(command, check=True)
            for relative in ['USER_GUIDE.md', 'docs/extensions.md', 'docs/extension-security.md',
                             'assets/licenses/lua-MIT.txt',
                             'vendor/libghostty-vt-sys/PATCH.md',
                             'examples/extensions/markdown-quote.json']:
                self.assertEqual((destination / relative).read_bytes(), (root / relative).read_bytes())

    def test_download_verifies_bytes_before_replacing_cache_and_skips_valid_cache(self):
        with tempfile.TemporaryDirectory() as work:
            path = Path(work) / 'tool'
            path.write_bytes(b'old cache')
            digest = hashlib.sha256(b'verified tool').hexdigest()
            with patch.object(setup.urllib.request, 'urlopen', return_value=io.BytesIO(b'bad tool')):
                with self.assertRaisesRegex(RuntimeError, 'Checksum mismatch'):
                    setup.download('https://example.invalid/tool', path, digest)
            self.assertEqual(path.read_bytes(), b'old cache')
            self.assertEqual(list(Path(work).iterdir()), [path])
            with patch.object(setup.urllib.request, 'urlopen', return_value=io.BytesIO(b'verified tool')) as request:
                setup.download('https://example.invalid/tool', path, digest)
                setup.download('https://example.invalid/tool', path, digest)
                self.assertEqual(request.call_count, 1)
            self.assertEqual(path.read_bytes(), b'verified tool')

    def test_publication_requires_both_linux_architectures_and_every_format(self):
        with tempfile.TemporaryDirectory() as work:
            directory = Path(work)
            names = [f'hedit-1.2.3-linux-{arch}.{kind}' for arch in ['x86_64', 'aarch64']
                     for kind in ['tar.gz', 'deb', 'rpm', 'AppImage']]
            names += ['hedit-1.2.3-macos-universal.zip', 'hedit-1.2.3-windows-x86_64.zip']
            names += [f'hedit-remote-1.2.3-{platform}' for platform in ['linux-x86_64', 'linux-aarch64', 'macos-universal']]
            for name in names:
                (directory / name).write_bytes(name.encode())
                digest = hashlib.sha256(name.encode()).hexdigest()
                (directory / (name + '.sha256')).write_text(f'{digest}  {name}\n')
            self.assertEqual(assets.validate(directory, '1.2.3'), 13)
            missing = directory / 'hedit-1.2.3-linux-aarch64.AppImage'
            data = missing.read_bytes(); missing.unlink()
            with self.assertRaisesRegex(ValueError, 'missing='):
                assets.validate(directory, '1.2.3')
            missing.write_bytes(data + b'corrupted')
            with self.assertRaisesRegex(ValueError, 'Invalid checksum'):
                assets.validate(directory, '1.2.3')
            missing.write_bytes(data)
            (directory / 'stale-package.deb').touch()
            with self.assertRaisesRegex(ValueError, 'unexpected='):
                assets.validate(directory, '1.2.3')


if __name__ == '__main__':
    unittest.main()
