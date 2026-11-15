#!/usr/bin/env python3
"""Check disposable SSH fixture placement without starting sshd."""
import importlib.util
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('remote_live', Path(__file__).with_name('test-remote-live.py'))
live = importlib.util.module_from_spec(spec)
spec.loader.exec_module(live)


class FixtureLocationTests(unittest.TestCase):
    def setUp(self):
        ports = patch.object(live, 'available_port', return_value=12345)
        ports.start()
        self.addCleanup(ports.stop)

    def test_default_uses_checkout_and_cleanup_preserves_parent(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'checkout with spaces'
            with patch.object(live, 'ROOT', root), patch.object(live, 'ARGS', None):
                lab = live.Lab()
                try:
                    self.assertEqual(lab.base.parent, (root / 'target/rl').resolve())
                    if os.name != 'nt':
                        self.assertEqual(lab.base.stat().st_mode & 0o777, 0o700)
                    marker = lab.base.parent / 'keep'
                    marker.write_text('keep')
                    base = lab.base
                finally:
                    lab.close()
                self.assertFalse(base.exists())
                self.assertEqual(marker.read_text(), 'keep')

    def test_explicit_parent_and_concurrent_labs_are_isolated(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / 'disk with spaces'
            with patch.object(live, 'ARGS', SimpleNamespace(work_dir=parent)):
                first, second = live.Lab(), live.Lab()
                try:
                    self.assertEqual(first.base.parent, parent.resolve())
                    self.assertNotEqual(first.base, second.base)
                    first.close()
                    self.assertTrue(second.base.is_dir())
                finally:
                    second.close()

    def test_cli_overrides_environment_and_empty_environment_uses_default(self):
        with patch.dict(os.environ, {'HEDIT_REMOTE_LIVE_DIR': '/disk/environment'}):
            args = live.parse_args(['--fault-binary', 'fault'])
            self.assertEqual(args.work_dir, Path('/disk/environment'))
            args = live.parse_args(['--fault-binary', 'fault', '--work-dir', '/disk/explicit'])
            self.assertEqual(args.work_dir, Path('/disk/explicit'))
        with patch.dict(os.environ, {'HEDIT_REMOTE_LIVE_DIR': ''}):
            self.assertIsNone(live.parse_args(['--fault-binary', 'fault']).work_dir)

    def test_existing_file_is_rejected_without_altering_it(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / 'keep'
            parent.write_text('keep')
            with patch.object(live, 'ARGS', SimpleNamespace(work_dir=parent)):
                with self.assertRaises(FileExistsError):
                    live.Lab()
            self.assertEqual(parent.read_text(), 'keep')

    def test_config_values_quote_spaces_and_reject_injection_or_expansion(self):
        self.assertEqual(live.config_value('/disk/with spaces'), '"/disk/with spaces"')
        self.assertEqual(live.config_value('/disk/with"quote'), '"/disk/with\\"quote"')
        for value in ['/disk/\nListenAddress 0.0.0.0', '/disk/\rpath', '/disk/\0path',
                      '/disk/%h', '/disk/${HOME}']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                live.config_value(value)

    @unittest.skipUnless(os.name == 'posix', 'Unix recovery permission policy')
    def test_group_writable_ancestor_is_explained_before_server_start(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / 'shared'
            parent.mkdir(mode=0o700)
            child = parent / 'private'
            child.mkdir(mode=0o700)
            parent.chmod(0o770)
            with self.assertRaisesRegex(ValueError, r'group/other-writable: .*shared.*--work-dir'):
                live.validate_fixture_ancestors(child)

    @unittest.skipUnless(os.name == 'posix', 'Unix recovery permission policy')
    def test_private_and_sticky_ancestors_are_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / 'sticky'
            parent.mkdir(mode=0o700)
            child = parent / 'private'
            child.mkdir(mode=0o700)
            parent.chmod(0o1777)
            live.validate_fixture_ancestors(child)


if __name__ == '__main__':
    unittest.main()
