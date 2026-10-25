#!/usr/bin/env python3
"""Check the Windows archive contract without requiring a Windows host."""
import hashlib
import importlib.util
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('windows_package', Path(__file__).with_name('package-windows.py'))
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


class WindowsPackage(unittest.TestCase):
    def test_invalid_inputs_fail_before_creating_output(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            binary, out = root / 'hedit.exe', root / 'out'
            with self.assertRaises(FileNotFoundError):
                packager.package(binary, out)
            for contents in [b'\x7fELF', b'MZ', self.pe(0xaa64)]:
                binary.write_bytes(contents)
                with self.assertRaisesRegex(ValueError, 'Windows PE'):
                    packager.package(binary, out)
                self.assertFalse(out.exists())

    @staticmethod
    def pe(machine=0x8664):
        header = bytearray(64)
        header[:2] = b'MZ'
        struct.pack_into('<I', header, 60, 64)
        return bytes(header) + b'PE\0\0' + struct.pack('<H', machine)

    def test_portable_layout_contents_and_checksum(self):
        with tempfile.TemporaryDirectory(prefix='windows package ') as work:
            root = Path(work)
            binary = root / 'hedit.exe'
            binary.write_bytes(self.pe())
            archive = packager.package(binary, root / 'out')
            self.assertEqual(archive.name, f'hedit-{packager.version()}-windows-x86_64.zip')
            prefix = archive.stem + '/'
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(bundle.read(prefix + 'hedit.exe'), binary.read_bytes())
                for relative in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'LICENSE',
                                 'themes/hedit-ink.json', 'assets/hedit.svg',
                                 'assets/dictionaries/en_US-LICENSE.txt', 'examples/cuda/vector_add.cu']:
                    self.assertEqual(bundle.read(prefix + relative), (packager.ROOT / relative).read_bytes())
                self.assertTrue(all(name.startswith(prefix) and '\\' not in name for name in bundle.namelist()))
                self.assertNotIn(prefix + 'install.sh', bundle.namelist())
            checksum = archive.with_name(archive.name + '.sha256').read_bytes()
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(checksum, f'{digest}  {archive.name}\n'.encode())

    def test_smoke_runs_extracted_binary_and_rejects_wrong_version(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            binary = root / 'hedit.exe'
            binary.write_bytes(self.pe())
            archive = packager.package(binary, root / 'out')
            calls = []

            def run(arguments, **kwargs):
                executable = Path(arguments[0])
                self.assertTrue(executable.is_file())
                self.assertEqual(executable.name, 'hedit.exe')
                self.assertIn('hedit portable test ', str(executable))
                self.assertNotEqual(executable.parent, binary.parent)
                self.assertTrue(kwargs['check'])
                self.assertEqual(kwargs['timeout'], 30)
                calls.append(arguments[1])
                output = f'hedit {packager.version()}\r\n'.encode() if arguments[1] == '--version' else b'Usage: hedit\r\n'
                return subprocess.CompletedProcess(arguments, 0, stdout=output)

            with patch.object(packager.subprocess, 'run', side_effect=run):
                packager.smoke(archive)
            self.assertEqual(calls, ['--version', '--help'])
            with patch.object(packager.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, stdout=b'hedit 0.0.0\n')):
                with self.assertRaisesRegex(ValueError, 'version does not match'):
                    packager.smoke(archive)

    def test_smoke_refuses_a_corrupted_archive_before_execution(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            binary = root / 'hedit.exe'
            binary.write_bytes(self.pe())
            archive = packager.package(binary, root / 'out')
            archive.write_bytes(archive.read_bytes() + b'corruption')
            with patch.object(packager.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'checksum does not match'):
                    packager.smoke(archive)
                run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
