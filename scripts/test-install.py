#!/usr/bin/env python3
"""Exercise release installation offline, including failed and private downloads."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TAG = 'v0.1.2'
MOCK = '''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import hashlib
import shutil
import sys
import zipfile

args = sys.argv[1:]
tool = Path(sys.argv[0]).name
with open(os.environ['MOCK_LOG'], 'a') as log:
    log.write(json.dumps([tool, *args]) + '\\n')
if tool == 'uname':
    print(os.environ.get('MOCK_OS', 'Linux') if args == ['-s'] else os.environ.get('MOCK_ARCH', 'x86_64'))
    sys.exit(0)
if os.environ.get('MOCK_FAIL') == 'download':
    sys.exit(22)
if tool == 'shasum':
    print(f"{hashlib.sha256(Path(args[-1]).read_bytes()).hexdigest()}  {args[-1]}")
    sys.exit(0)
if tool == 'codesign':
    sys.exit(1 if os.environ.get('MOCK_FAIL') == 'codesign' else 0)
if tool == 'ditto':
    if args[:2] == ['-x', '-k']:
        with zipfile.ZipFile(args[2]) as archive:
            for member in archive.infolist():
                path = Path(archive.extract(member, args[3]))
                path.chmod(member.external_attr >> 16 or 0o644)
    else:
        shutil.copytree(args[0], args[1], symlinks=True)
    sys.exit(0)
fixtures = Path(os.environ['MOCK_FIXTURES'])
if tool == 'curl':
    url = args[-1]
    if url.endswith('/releases/latest'):
        print('https://github.com/MohamedElashri/hedit/releases/tag/v0.1.2', end='')
    else:
        if '/releases/download/v0.1.2/' not in url:
            sys.exit(22)
        shutil.copyfile(fixtures / url.rsplit('/', 1)[1], args[args.index('--output') + 1])
elif tool == 'gh':
    if args[:2] == ['auth', 'status']:
        sys.exit(1 if os.environ.get('MOCK_FAIL') == 'auth' else 0)
    elif args[:2] == ['release', 'view']:
        print('v0.1.2')
    elif args[:2] == ['release', 'download']:
        if args[2] != 'v0.1.2':
            sys.exit(1)
        destination = Path(args[args.index('--dir') + 1])
        for i, value in enumerate(args):
            if value == '--pattern':
                shutil.copyfile(fixtures / args[i + 1], destination / args[i + 1])
    else:
        sys.exit('Unexpected gh invocation')
'''


class ReleaseInstaller(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='hedit-installer-test-')
        self.addCleanup(self.temporary.cleanup)
        self.work = Path(self.temporary.name)
        self.fixtures = self.work / 'fixtures'
        self.fixtures.mkdir()
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        for command in ['curl', 'gh', 'uname', 'ditto', 'shasum', 'codesign']:
            path = self.bin / command
            path.write_text(MOCK)
            path.chmod(0o755)
        self.prefix = self.work / 'installation with spaces'
        self.app_dir = self.work / 'Applications dir'
        self.log = self.work / 'requests.jsonl'
        self.environment = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                                MOCK_FIXTURES=str(self.fixtures), MOCK_LOG=str(self.log),
                                TMPDIR=str(self.work))
        for key in ['MOCK_OS', 'MOCK_ARCH', 'MOCK_FAIL']:
            self.environment.pop(key, None)
        for arch in ['x86_64', 'aarch64']:
            name = f'hedit-0.1.2-linux-{arch}'
            stage = self.work / name
            stage.mkdir()
            binary = stage / 'hedit'
            binary.write_text('#!/bin/sh\nprintf "hedit 0.1.2\\n"\n')
            binary.chmod(0o755)
            shutil.copy2(ROOT / 'packaging/linux/install.sh', stage / 'install.sh')
            shutil.copy2(ROOT / 'packaging/linux/hedit.desktop', stage / 'hedit.desktop')
            for filename in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'LICENSE']:
                shutil.copy2(ROOT / filename, stage / filename)
            for directory in ['assets', 'themes']:
                shutil.copytree(ROOT / directory, stage / directory)
            archive = self.fixtures / f'{name}.tar.gz'
            with tarfile.open(archive, 'w:gz') as output:
                output.add(stage, arcname=name)
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
        self.write_macos_fixture('#!/bin/sh\nprintf "hedit 0.1.2\\n"\n')

    def write_macos_fixture(self, executable):
        name = 'hedit-0.1.2-macos-universal.zip'
        archive = self.fixtures / name
        with zipfile.ZipFile(archive, 'w') as output:
            for path, text, mode in [('hedit.app/Contents/MacOS/hedit', executable, 0o755),
                                     ('hedit.app/Contents/Info.plist', '<plist/>', 0o644)]:
                info = zipfile.ZipInfo(path)
                info.external_attr = mode << 16
                output.writestr(info, text)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(name + '.sha256').write_text(f'{digest}  {name}\n')

    def run_macos_installer(self, *args, succeeds=True):
        self.environment['MOCK_OS'] = 'Darwin'
        self.environment['MOCK_ARCH'] = 'arm64'
        result = subprocess.run(['sh', str(ROOT / 'install.sh'), '--prefix', str(self.prefix),
                                 '--app-dir', str(self.app_dir), *args],
                                env=self.environment, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode == 0, succeeds, result.stdout + result.stderr)
        self.assertFalse(list(self.work.glob('hedit-install.*')), 'Temporary download leaked')
        return result

    def run_installer(self, *args, succeeds=True, piped=False):
        invocation = ['sh', '-s', '--'] if piped else ['sh', str(ROOT / 'install.sh')]
        result = subprocess.run([*invocation, '--prefix', str(self.prefix), *args],
                                input=(ROOT / 'install.sh').read_text() if piped else None,
                                env=self.environment, capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode == 0, succeeds, result.stdout + result.stderr)
        self.assertFalse(list(self.work.glob('hedit-install.*')), 'Temporary download leaked')
        if succeeds:
            self.assertTrue((self.prefix / 'bin/hedit').is_file())
            self.assertTrue((self.prefix / 'share/doc/hedit/USER_GUIDE.md').is_file())
        return result

    def requests(self):
        if not self.log.exists():
            return []
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_latest_public_piped_install(self):
        self.run_installer(piped=True)
        self.assertTrue(any(request[-1].endswith('/releases/latest') for request in self.requests()))

    def test_pinned_arm64_install(self):
        self.environment['MOCK_ARCH'] = 'aarch64'
        self.run_installer('--version', TAG)
        requests = self.requests()
        self.assertFalse(any(request[-1].endswith('/releases/latest') for request in requests))
        self.assertTrue(any('aarch64.tar.gz' in request[-1] for request in requests))

    def test_private_latest_and_pinned(self):
        for args in [(), ('--version', TAG)]:
            with self.subTest(args=args):
                self.run_installer('--github-cli', *args)
        self.assertFalse(any(request[0] == 'curl' for request in self.requests()))

    def test_corrupt_archive_preserves_existing_installation(self):
        archive = self.fixtures / 'hedit-0.1.2-linux-x86_64.tar.gz'
        archive.write_bytes(b'corrupt download')
        self.prefix.mkdir()
        sentinel = self.prefix / 'existing-work'
        sentinel.write_text('keep')
        result = self.run_installer(succeeds=False)
        self.assertIn('Checksum mismatch', result.stderr)
        self.assertEqual(sentinel.read_text(), 'keep')
        self.assertFalse((self.prefix / 'bin').exists())

    def test_checksum_with_unexpected_path_is_rejected(self):
        checksum = self.fixtures / 'hedit-0.1.2-linux-x86_64.tar.gz.sha256'
        checksum.write_text('0' * 64 + '  ../../other-file\n')
        self.run_installer(succeeds=False)
        self.assertFalse(self.prefix.exists())

    def test_failed_download_is_not_installed(self):
        self.environment['MOCK_FAIL'] = 'download'
        self.run_installer('--version', TAG, succeeds=False)
        self.assertFalse(self.prefix.exists())

    def test_missing_private_access_is_not_installed(self):
        self.environment['MOCK_FAIL'] = 'auth'
        result = self.run_installer('--github-cli', succeeds=False)
        self.assertIn('gh auth login', result.stderr)
        self.assertFalse(self.prefix.exists())

    def test_macos_installs_app_and_launcher(self):
        self.run_macos_installer()
        launcher = self.prefix / 'bin/hedit'
        result = subprocess.run([str(launcher), '--version'], capture_output=True, text=True, check=True)
        self.assertEqual(result.stdout, 'hedit 0.1.2\n')
        self.assertTrue((self.app_dir / 'hedit.app/Contents/Info.plist').is_file())
        self.assertTrue(any('macos-universal.zip' in request[-1] for request in self.requests()))

    def test_macos_upgrade_replaces_existing_app(self):
        old = self.app_dir / 'hedit.app/Contents/stale-file'
        old.parent.mkdir(parents=True)
        old.write_text('old')
        self.run_macos_installer('--version', TAG)
        self.assertFalse(old.exists())
        self.assertEqual([path.name for path in self.app_dir.iterdir()], ['hedit.app'])

    def test_macos_failures_preserve_existing_app(self):
        for failure in ['checksum', 'codesign', 'binary']:
            with self.subTest(failure=failure):
                self.setUp()
                sentinel = self.app_dir / 'hedit.app/Contents/existing'
                sentinel.parent.mkdir(parents=True)
                sentinel.write_text('keep')
                if failure == 'checksum':
                    (self.fixtures / 'hedit-0.1.2-macos-universal.zip').write_bytes(b'corrupt')
                elif failure == 'codesign':
                    self.environment['MOCK_FAIL'] = 'codesign'
                else:
                    self.write_macos_fixture('#!/bin/sh\nexit 1\n')
                self.run_macos_installer(succeeds=False)
                self.assertEqual(sentinel.read_text(), 'keep')
                self.assertFalse((self.prefix / 'bin/hedit').exists())

    def test_macos_rejects_unsafe_app_directory(self):
        self.app_dir = self.work / 'bad"dir'
        self.run_macos_installer(succeeds=False)
        self.assertFalse(any(request[0] in {'gh', 'curl'} for request in self.requests()))

    def test_unsupported_system_does_not_download(self):
        for setting, value in [('MOCK_OS', 'FreeBSD'), ('MOCK_ARCH', 'riscv64')]:
            with self.subTest(setting=setting):
                self.environment[setting] = value
                self.run_installer(succeeds=False)
                self.environment.pop(setting)
        self.assertFalse(any(request[0] in {'gh', 'curl'} for request in self.requests()))

    def test_invalid_arguments_do_not_download(self):
        for args in [('--version', '../escape'), ('--prefix', 'relative'), ('--unknown',)]:
            with self.subTest(args=args):
                self.run_installer(*args, succeeds=False)
        self.assertFalse(any(request[0] in {'gh', 'curl'} for request in self.requests()))

    def test_incompatible_binary_preserves_existing_binary(self):
        name = 'hedit-0.1.2-linux-x86_64'
        stage = self.work / name
        (stage / 'hedit').write_text('#!/bin/sh\nexit 1\n')
        archive = self.fixtures / f'{name}.tar.gz'
        with tarfile.open(archive, 'w:gz') as output:
            output.add(stage, arcname=name)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
        old_binary = self.prefix / 'bin/hedit'
        old_binary.parent.mkdir(parents=True)
        old_binary.write_text('existing binary')
        result = self.run_installer(succeeds=False)
        self.assertIn('cannot run on your system', result.stderr)
        self.assertEqual(old_binary.read_text(), 'existing binary')


if __name__ == '__main__':
    unittest.main()
