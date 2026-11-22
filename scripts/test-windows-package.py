#!/usr/bin/env python3
"""Check the Windows archive contract without requiring a Windows host."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
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
            binary.with_name('hedit-plugin-worker.exe').write_bytes(self.pe())
            project = root / 'source'
            project.mkdir()
            for filename in ['Cargo.toml', 'README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md', 'LICENSE']:
                shutil.copy2(packager.ROOT / filename, project / filename)
            for directory in ['assets', 'themes', 'docs/images', 'examples/cuda']:
                shutil.copytree(packager.ROOT / directory, project / directory)
            from package_resources import DOCUMENTATION_FILES
            documents = DOCUMENTATION_FILES
            for relative in documents:
                (project / relative).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(packager.ROOT / relative, project / relative)
            (project / 'examples/development_probe.rs').write_text('not a release resource')
            with patch.object(packager, 'ROOT', project):
                archive = packager.package(binary, root / 'out')
            self.assertEqual(archive.name, f'hedit-{packager.version()}-windows-x86_64.zip')
            prefix = archive.stem + '/'
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(bundle.read(prefix + 'hedit.exe'), binary.read_bytes())
                self.assertEqual(bundle.read(prefix + 'hedit-plugin-worker.exe'), binary.with_name('hedit-plugin-worker.exe').read_bytes())
                for relative in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md', 'LICENSE',
                                 'themes/hedit-ink.json', 'assets/hedit.svg',
                                 'assets/dictionaries/en_US-LICENSE.txt', 'assets/licenses/egui_commonmark-MIT.txt',
                                 'examples/cuda/vector_add.cu']:
                    self.assertEqual(bundle.read(prefix + relative), (packager.ROOT / relative).read_bytes())
                images = [path.relative_to(packager.ROOT).as_posix()
                          for path in (packager.ROOT / 'docs/images').glob('*') if path.is_file()]
                for relative in documents + images:
                    self.assertEqual(bundle.read(prefix + relative), (packager.ROOT / relative).read_bytes())
                self.assertTrue(all(name.startswith(prefix) and '\\' not in name for name in bundle.namelist()))
                self.assertNotIn(prefix + 'install.sh', bundle.namelist())
                examples = {name[len(prefix):] for name in bundle.namelist()
                            if name.startswith(prefix + 'examples/')}
                self.assertEqual(examples, {'examples/cuda/vector_add.cu',
                                            'examples/extensions/markdown-task.json',
                                            'examples/extensions/markdown-quote.json',
                                            'examples/extensions/document-assist.json',
                                            'examples/extensions/document-assist.fixtures.json',
                                            'examples/extensions/demo-visuals.json',
                                            *{f'examples/extensions/sdk-{name}{suffix}.json'
                                              for name in ['markdown', 'table', 'trim']
                                              for suffix in ['', '.fixtures']}})
            checksum = archive.with_name(archive.name + '.sha256').read_bytes()
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(checksum, f'{digest}  {archive.name}\n'.encode())

    def test_smoke_runs_extracted_binary_and_rejects_wrong_version(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            binary = root / 'hedit.exe'
            binary.write_bytes(self.pe())
            binary.with_name('hedit-plugin-worker.exe').write_bytes(self.pe())
            archive = packager.package(binary, root / 'out')
            calls = []

            def run(arguments, **kwargs):
                executable = Path(arguments[0])
                self.assertTrue(executable.is_file())
                self.assertIn(executable.name, {'hedit.exe', 'hedit-plugin-worker.exe'})
                self.assertIn('hedit portable test ', str(executable))
                self.assertNotEqual(executable.parent, binary.parent)
                self.assertTrue(kwargs['check'])
                self.assertEqual(kwargs['timeout'], 30)
                calls.append((executable.name, arguments[1]))
                if executable.name == 'hedit-plugin-worker.exe':
                    self.assertEqual(arguments[1:], ['--version'])
                    output = f'hedit-plugin-worker {packager.version()}\r\n'.encode()
                elif arguments[1] == 'plugin':
                    self.assertEqual(arguments[2], 'test')
                    manifest = Path(arguments[3])
                    if manifest.name == 'markdown-quote.json':
                        self.assertEqual(manifest, executable.parent / 'examples/extensions/markdown-quote.json')
                        self.assertEqual(json.loads(Path(arguments[4]).read_text(encoding='utf-8')), [{
                            'command': 'markdown-quote.quote',
                            'input': 'café / 東京', 'expected': '> café / 東京',
                        }])
                    else:
                        self.assertIn(manifest.stem, {'sdk-markdown', 'sdk-table', 'sdk-trim'})
                        self.assertEqual(manifest.parent, executable.parent / 'examples/extensions')
                        self.assertEqual(Path(arguments[4]), manifest.with_suffix('.fixtures.json'))
                        self.assertEqual(json.loads(manifest.read_text())['api_version'], 3)
                        self.assertTrue(json.loads(Path(arguments[4]).read_text(encoding="utf-8")))
                    output = b''
                else:
                    output = f'hedit {packager.version()}\r\n'.encode() if arguments[1] == '--version' else b'Usage: hedit\r\n'
                return subprocess.CompletedProcess(arguments, 0, stdout=output)

            with patch.object(packager.subprocess, 'run', side_effect=run):
                packager.smoke(archive)
            self.assertEqual(calls, [
                ('hedit.exe', '--version'), ('hedit-plugin-worker.exe', '--version'),
                ('hedit.exe', '--help'), *[('hedit.exe', 'plugin')] * 4,
            ])
            with patch.object(packager.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, stdout=b'hedit 0.0.0\n')):
                with self.assertRaisesRegex(ValueError, 'version does not match'):
                    packager.smoke(archive)

            def stale_worker(arguments, **kwargs):
                if Path(arguments[0]).name == 'hedit-plugin-worker.exe':
                    return subprocess.CompletedProcess(arguments, 0, stdout=b'hedit-plugin-worker 0.0.0\n')
                return run(arguments, **kwargs)

            with patch.object(packager.subprocess, 'run', side_effect=stale_worker) as probe:
                with self.assertRaisesRegex(ValueError, 'Lua worker version does not match'):
                    packager.smoke(archive)
                self.assertEqual(probe.call_count, 2)

    def test_smoke_refuses_a_corrupted_archive_before_execution(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            binary = root / 'hedit.exe'
            binary.write_bytes(self.pe())
            binary.with_name('hedit-plugin-worker.exe').write_bytes(self.pe())
            archive = packager.package(binary, root / 'out')
            archive.write_bytes(archive.read_bytes() + b'corruption')
            with patch.object(packager.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'checksum does not match'):
                    packager.smoke(archive)
                run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
