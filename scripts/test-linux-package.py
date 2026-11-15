#!/usr/bin/env python3
"""Test pinned tool downloads and the complete release asset publication gate."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    result = importlib.util.module_from_spec(spec); spec.loader.exec_module(result)
    return result


setup = module('setup_appimage', 'setup-appimage.py')
assets = module('release_assets', 'check-release-assets.py')


class LinuxRelease(unittest.TestCase):
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
            for name in names:
                (directory / name).write_bytes(name.encode())
                digest = hashlib.sha256(name.encode()).hexdigest()
                (directory / (name + '.sha256')).write_text(f'{digest}  {name}\n')
            self.assertEqual(assets.validate(directory, '1.2.3'), 10)
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
