#!/usr/bin/env python3
"""Test bundle/archive contracts with mocked Apple tools; native CI verifies Mach-O."""
import hashlib
import importlib.util
from pathlib import Path
import plistlib
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch
import zipfile
import zlib

spec = importlib.util.spec_from_file_location('macos_package', Path(__file__).with_name('package-macos.py'))
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


class MacPackage(unittest.TestCase):
    def test_non_macos_and_missing_inputs_fail_before_writing_output(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            with patch.object(packager.sys, 'platform', 'linux'):
                with self.assertRaisesRegex(ValueError, 'macOS'):
                    packager.package(root / 'arm', root / 'intel', root / 'out')
            with patch.object(packager.sys, 'platform', 'darwin'):
                with self.assertRaisesRegex(ValueError, 'arm64'):
                    packager.package(root / 'arm', root / 'intel', root / 'out')
            self.assertFalse((root / 'out').exists())

    def test_universal_bundle_layout_version_signature_and_checksum_contract(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            arm = root / 'arm'
            intel = root / 'intel'
            arm.write_bytes(b'arm fixture')
            intel.write_bytes(b'intel fixture')
            calls = []
            def run(arguments, **kwargs):
                calls.append(arguments)
                if arguments[:2] == ['lipo', '-create']:
                    Path(arguments[-1]).write_bytes(b'universal fixture')
                elif arguments[0] == 'sips':
                    Path(arguments[-1]).write_bytes(b'icon fixture')
                elif arguments[0] == 'iconutil':
                    Path(arguments[-1]).write_bytes(b'icns fixture')
                elif arguments[0] == 'ditto':
                    app, archive = map(Path, arguments[-2:])
                    with zipfile.ZipFile(archive, 'w') as output:
                        for path in app.rglob('*'):
                            if path.is_file():
                                output.write(path, path.relative_to(app.parent))
                version = tomllib.loads((packager.ROOT / 'Cargo.toml').read_text())['package']['version']
                return subprocess.CompletedProcess(arguments, 0, stdout=f'hedit {version}\n')
            with patch.object(packager.sys, 'platform', 'darwin'), patch.object(packager.subprocess, 'run', side_effect=run):
                archive = packager.package(arm, intel, root / 'out')
            with zipfile.ZipFile(archive) as bundle:
                info = plistlib.loads(bundle.read('hedit.app/Contents/Info.plist'))
                self.assertEqual(info['CFBundleIdentifier'], 'hedit.hedit')
                self.assertIn(info['CFBundleShortVersionString'], archive.name)
                self.assertEqual(bundle.read('hedit.app/Contents/MacOS/hedit'), b'universal fixture')
                self.assertIn('hedit.app/Contents/Resources/hedit.icns', bundle.namelist())
                self.assertIn('hedit.app/Contents/Resources/Documentation/LICENSE', bundle.namelist())
                self.assertEqual(bundle.read('hedit.app/Contents/Resources/Documentation/assets/licenses/egui_commonmark-MIT.txt'),
                                 (packager.ROOT / 'assets/licenses/egui_commonmark-MIT.txt').read_bytes())
                self.assertIn('hedit.app/Contents/Resources/themes/hedit-ink.json', bundle.namelist())
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(archive.with_name(archive.name + '.sha256').read_text(), f'{digest}  {archive.name}\n')
            self.assertIn(['lipo', str(arm), '-verify_arch', 'arm64'], calls)
            self.assertIn(['lipo', str(intel), '-verify_arch', 'x86_64'], calls)
            for architecture in ['arm64', 'x86_64']:
                self.assertTrue(any(call[:1] == ['lipo'] and '/hedit.app/' in call[1] and
                                    call[2:] == ['-verify_arch', architecture] for call in calls))
            self.assertTrue(any(call[:4] == ['codesign', '--force', '--sign', '-'] for call in calls))
            self.assertTrue(any(call[:4] == ['codesign', '--verify', '--deep', '--strict'] for call in calls))

    def test_tool_failure_keeps_previous_archive(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            arm, intel = root / 'arm', root / 'intel'
            arm.touch()
            intel.touch()
            output = root / 'out'
            output.mkdir()
            version = tomllib.loads((packager.ROOT / 'Cargo.toml').read_text())['package']['version']
            existing = output / f'hedit-{version}-macos-universal.zip'
            existing.write_bytes(b'keep')
            def fail_on_merge(arguments, **kwargs):
                if arguments[:2] == ['lipo', '-create']:
                    raise subprocess.CalledProcessError(1, 'lipo')
                return subprocess.CompletedProcess(arguments, 0)
            with patch.object(packager.sys, 'platform', 'darwin'), patch.object(packager.subprocess, 'run', side_effect=fail_on_merge):
                with self.assertRaises(subprocess.CalledProcessError):
                    packager.package(arm, intel, output)
            self.assertEqual(existing.read_bytes(), b'keep')
            self.assertEqual(list(output.iterdir()), [existing])

    def test_icon_has_rgba_png_header_and_transparent_corners(self):
        with tempfile.TemporaryDirectory() as work:
            icon = Path(work) / 'icon.png'
            packager.icon_png(icon, 64)
            data = icon.read_bytes()
            self.assertEqual(data[:8], b'\x89PNG\r\n\x1a\n')
            self.assertEqual(data[24:26], b'\x08\x06')
            # Decode the generated, unfiltered RGBA scanlines.
            size = int.from_bytes(data[33:37], 'big')
            self.assertEqual(data[37:41], b'IDAT')
            rows = zlib.decompress(data[41:41 + size])
            self.assertEqual(rows[1:5], bytes(4))
            center = 32 * (1 + 64 * 4) + 1 + 32 * 4
            self.assertEqual(rows[center + 3], 255)


if __name__ == '__main__':
    unittest.main()
