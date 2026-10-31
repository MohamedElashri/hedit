#!/usr/bin/env python3
"""Offline tests for compiler selection and isolated installation."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('zig_setup', Path(__file__).with_name('setup-zig.py'))
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)


class ZigSetupTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.cache = Path(self.directory.name) / 'toolchain'
        self.env = patch.dict(os.environ, {}, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_version_must_match_exactly(self):
        for code, version, expected in [(0, setup.ZIG_VERSION + '\n', True),
                                        (0, '0.17.0\n', False), (1, setup.ZIG_VERSION, False)]:
            with self.subTest(version=version, code=code), patch.object(setup.subprocess, 'run', return_value=subprocess.CompletedProcess([], code, version)):
                self.assertEqual(setup.matches_version('zig'), expected)

    def test_explicit_override_is_authoritative(self):
        executable = Path(self.directory.name) / 'custom-zig'
        with patch.dict(os.environ, {'ZIG': str(executable)}), patch.object(setup, 'matches_version', return_value=True), patch.object(setup.shutil, 'which', return_value=None):
            self.assertEqual(setup.ensure_zig(self.cache), executable)
        with patch.dict(os.environ, {'ZIG': str(executable)}), patch.object(setup, 'matches_version', return_value=False), patch.object(setup.shutil, 'which', return_value=None):
            with self.assertRaisesRegex(RuntimeError, 'ZIG must name'):
                setup.ensure_zig(self.cache)
        self.assertFalse(self.cache.exists())

    def test_matching_system_compiler_needs_no_install(self):
        executable = str(Path(self.directory.name) / 'zig')
        with patch.object(setup.shutil, 'which', return_value=executable), patch.object(setup, 'matches_version', return_value=True):
            self.assertEqual(setup.ensure_zig(self.cache), Path(executable))
        self.assertFalse(self.cache.exists())

    def test_newer_system_compiler_uses_cached_compiler(self):
        cached = setup.compiler_in(self.cache)
        with patch.object(setup.shutil, 'which', return_value='wrong-zig'), patch.object(setup, 'matches_version', side_effect=lambda p: p == cached), patch.object(setup.subprocess, 'run') as install:
            self.assertEqual(setup.ensure_zig(self.cache), cached)
        install.assert_not_called()

    def test_install_is_isolated_and_reused(self):
        def install(arguments, **kwargs):
            staged = Path(arguments[arguments.index('--target') + 1])
            compiler = setup.compiler_in(staged)
            compiler.parent.mkdir(parents=True)
            compiler.write_text('test compiler')
            self.assertIn('--no-cache-dir', arguments)
            self.assertIn('--no-deps', arguments)
            self.assertIn(f'ziglang=={setup.ZIG_VERSION}', arguments)
            self.assertIs(kwargs['stdout'], sys.stderr)
        with patch.object(setup.shutil, 'which', return_value='wrong-zig'), patch.object(setup, 'matches_version', side_effect=lambda p: Path(p).is_file()), patch.object(setup.subprocess, 'run', side_effect=install) as installer:
            self.assertEqual(setup.ensure_zig(self.cache), setup.compiler_in(self.cache))
            self.assertEqual(setup.ensure_zig(self.cache), setup.compiler_in(self.cache))
            self.assertEqual(installer.call_count, 1)
        self.assertEqual([p.name for p in self.cache.parent.iterdir()], ['toolchain'])

    def test_failed_download_leaves_no_partial_cache(self):
        with patch.object(setup.shutil, 'which', return_value=None), patch.object(setup, 'matches_version', return_value=False), patch.object(setup.subprocess, 'run', side_effect=subprocess.CalledProcessError(1, ['pip'])):
            with self.assertRaises(subprocess.CalledProcessError):
                setup.ensure_zig(self.cache)
        self.assertFalse(self.cache.exists())
        self.assertEqual(list(self.cache.parent.iterdir()), [])

    def test_incomplete_cache_has_actionable_error(self):
        self.cache.mkdir()
        with patch.object(setup.shutil, 'which', return_value=None), patch.object(setup, 'matches_version', return_value=False):
            with self.assertRaisesRegex(RuntimeError, 'Incomplete Zig toolchain'):
                setup.ensure_zig(self.cache)

    def test_cargo_output_is_only_the_compiler_path(self):
        executable = Path(self.directory.name) / 'zig'
        output = io.StringIO()
        with patch.object(sys, 'argv', ['setup-zig.py', '--print-path']), patch.object(setup, 'ensure_zig', return_value=executable), contextlib.redirect_stdout(output):
            setup.main()
        self.assertEqual(output.getvalue(), str(executable) + '\n')


if __name__ == '__main__':
    unittest.main()
