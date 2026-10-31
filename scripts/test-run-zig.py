#!/usr/bin/env python3
"""Offline tests for the subprocess-local macOS SDK compatibility layer."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('run_zig', Path(__file__).with_name('run-zig.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


@unittest.skipUnless(os.name == 'posix', 'macOS SDK overlays require POSIX symlinks')
class RunZigTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory()
        self.addCleanup(self.work.cleanup)
        self.root = Path(self.work.name).resolve()
        self.sdk = self.root / 'Apple SDK'
        self.system = self.sdk / 'usr/lib/libSystem.tbd'
        self.system.parent.mkdir(parents=True)
        self.system.write_text('targets: [ x86_64-macos, arm64e-macos ]\ninstall-name: libSystem\n')
        (self.sdk / 'usr/include').mkdir()
        (self.sdk / 'System/Library/Frameworks').mkdir(parents=True)
        (self.sdk / 'usr/lib/other.tbd').write_text('SDK library')
        self.lib = self.root / 'Zig compiler/lib'
        self.bundled = self.lib / 'libc/darwin/libSystem.tbd'
        self.bundled.parent.mkdir(parents=True)
        self.bundled.write_text('bundled ARM64 definitions')
        self.cache = self.root / 'cache with spaces'
        self.env = patch.dict(os.environ, {'PATH': '/usr/bin:/bin', 'SDKROOT': str(self.sdk)}, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)
        self.platform = patch.object(runner.sys, 'platform', 'darwin')
        self.platform.start()
        self.addCleanup(self.platform.stop)

    def zig_env(self, arguments, **kwargs):
        self.assertEqual(arguments, ['zig', 'env'])
        return subprocess.CompletedProcess(arguments, 0, f'.{{ .lib_dir = {json.dumps(str(self.lib))}, }}')

    def prepare(self):
        with patch.object(runner.subprocess, 'run', side_effect=self.zig_env):
            return runner.macos_environment(Path('zig'), self.cache)

    def test_other_platforms_and_compatible_sdks_are_untouched(self):
        with patch.object(runner.sys, 'platform', 'linux'), patch.object(runner.subprocess, 'run') as run:
            self.assertEqual(runner.macos_environment(Path('zig'), self.cache), dict(os.environ))
            run.assert_not_called()
        self.system.write_text('targets: [ arm64-macos, arm64e-macos ]\ninstall-name: libSystem\n')
        with patch.object(runner.subprocess, 'run') as run:
            self.assertEqual(runner.macos_environment(Path('zig'), self.cache), dict(os.environ))
            run.assert_not_called()
        self.assertFalse(self.cache.exists())

    def test_only_libsystem_is_overridden_and_original_sdk_is_preserved(self):
        env = self.prepare()
        sdk = Path(env['SDKROOT'])
        self.assertEqual((sdk / 'usr/lib/libSystem.tbd').resolve(), self.bundled)
        self.assertEqual((sdk / 'usr/lib/other.tbd').resolve(), self.sdk / 'usr/lib/other.tbd')
        self.assertEqual((sdk / 'usr/include').resolve(), self.sdk / 'usr/include')
        self.assertEqual((sdk / 'System').resolve(), self.sdk / 'System')
        self.assertIn('arm64e-macos', self.system.read_text())
        self.assertEqual(os.environ['SDKROOT'], str(self.sdk))
        self.assertEqual(os.environ['PATH'], '/usr/bin:/bin')
        self.assertTrue(env['PATH'].endswith(':/usr/bin:/bin'))

    def test_cache_is_reused_and_sdk_or_compiler_changes_invalidate_it(self):
        first = self.prepare()
        self.assertEqual(self.prepare(), first)
        self.system.write_text(self.system.read_text() + '# new SDK\n')
        second = self.prepare()
        self.assertNotEqual(first['SDKROOT'], second['SDKROOT'])
        self.bundled.write_text('updated compiler definitions')
        self.assertNotEqual(second['SDKROOT'], self.prepare()['SDKROOT'])
        self.assertFalse(any(path.name.startswith('.sdk-') for path in self.cache.iterdir()))

    def test_new_math_module_protocol_gets_fallback_macros_without_changing_sdk(self):
        self.system.write_text('targets: [ arm64-macos ]\ninstall-name: libSystem\n')
        math = self.sdk / 'usr/include/math.h'
        source = '#define __need_infinity_nan\n#include <float.h>\n'
        math.write_text(source)
        env = self.prepare()
        sdk = Path(env['SDKROOT'])
        self.assertEqual((sdk / 'usr/lib/libSystem.tbd').resolve(), self.system)
        adapted = (sdk / 'usr/include/math.h').read_text()
        self.assertTrue(adapted.startswith(source))
        self.assertIn('#ifndef INFINITY\n#define INFINITY (__builtin_inff())', adapted)
        self.assertIn('#ifndef NAN\n#define NAN (__builtin_nanf(""))', adapted)
        self.assertEqual(math.read_text(), source)
        math.write_text(source + '// SDK update\n')
        self.assertNotEqual(self.prepare()['SDKROOT'], env['SDKROOT'])

    def test_xcrun_shim_handles_spaces_and_passes_other_queries_through(self):
        env = self.prepare()
        shim = Path(env['PATH'].split(os.pathsep)[0]) / 'xcrun'
        result = subprocess.run([str(shim), '--sdk', 'macosx', '--show-sdk-path'],
                                check=True, capture_output=True, text=True)
        self.assertEqual(result.stdout.strip(), env['SDKROOT'])
        self.assertIn('exec /usr/bin/xcrun "$@"', shim.read_text())

    def test_default_sdk_comes_from_the_selected_developer_tools(self):
        del os.environ['SDKROOT']
        def run(arguments, **kwargs):
            if arguments[0] == '/usr/bin/xcrun':
                self.assertEqual(arguments[1:], ['--sdk', 'macosx', '--show-sdk-path'])
                return subprocess.CompletedProcess(arguments, 0, str(self.sdk) + '\n')
            return self.zig_env(arguments, **kwargs)
        with patch.object(runner.subprocess, 'run', side_effect=run):
            self.assertNotEqual(runner.macos_environment(Path('zig'), self.cache)['SDKROOT'], str(self.sdk))

    def test_compiler_probe_failure_leaves_no_partial_sdk(self):
        with patch.object(runner.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'bad metadata')):
            with self.assertRaisesRegex(RuntimeError, 'compiler library directory'):
                runner.macos_environment(Path('zig'), self.cache)
        self.assertFalse(self.cache.exists())

    def test_command_arguments_environment_and_failure_status_are_preserved(self):
        argv = ['run-zig.py', '--zig', '/compiler/zig', '--sdk-cache', str(self.cache), '--',
                'build', '-Demit-lib-vt=true', '--prefix', '/output with spaces']
        env = {'SDKROOT': 'compatible-sdk'}
        with patch.object(sys, 'argv', argv), patch.object(runner, 'macos_environment', return_value=env), \
                patch.object(runner.subprocess, 'run', return_value=subprocess.CompletedProcess([], 7)) as run:
            with self.assertRaises(SystemExit) as result:
                runner.main()
        self.assertEqual(result.exception.code, 7)
        run.assert_called_once_with(['/compiler/zig', 'build', '-Demit-lib-vt=true',
                                     '--prefix', '/output with spaces'], env=env)


if __name__ == '__main__':
    unittest.main()
