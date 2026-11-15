#!/usr/bin/env python3
"""Check Linux package payloads and optionally test distro install/upgrade/removal."""
import argparse
import hashlib
import importlib.util
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
ARCH = platform.machine()


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def payload(root):
    prefix = root / 'usr'
    assert os.access(prefix / 'bin/hedit', os.X_OK)
    assert (prefix / 'bin/hedit').read_bytes() == (ROOT / 'target/release/hedit').read_bytes()
    assert (prefix / 'share/hedit/package-managed').is_file()
    for relative in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md', 'LICENSE'] + [p.relative_to(ROOT).as_posix() for p in (ROOT / 'docs/images').iterdir() if p.is_file()]:
        assert (prefix / 'share/doc/hedit' / relative).read_bytes() == (ROOT / relative).read_bytes(), relative
    assert (prefix / 'share/icons/hicolor/scalable/apps/hedit.svg').is_file()
    run('desktop-file-validate', str(prefix / 'share/applications/hedit.desktop'))
    result = subprocess.check_output([str(prefix / 'bin/hedit'), '--version'], text=True)
    assert result.strip() == f'hedit {VERSION}', result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dist-dir', type=Path, default=ROOT / 'dist')
    parser.add_argument('--containers', action='store_true', help='Also test Ubuntu and Fedora using Docker')
    args = parser.parse_args()
    packages = {kind: args.dist_dir.resolve() / f'hedit-{VERSION}-linux-{ARCH}.{kind}' for kind in ['deb', 'rpm', 'AppImage']}
    for path in packages.values():
        assert hashlib.sha256(path.read_bytes()).hexdigest() == path.with_name(path.name + '.sha256').read_text().split()[0], path
    with tempfile.TemporaryDirectory(prefix='hedit-linux-formats-') as directory:
        work = Path(directory)
        deb = work / 'deb'; deb.mkdir()
        run('dpkg-deb', '-x', str(packages['deb']), str(deb))
        run('dpkg-deb', '-e', str(packages['deb']), str(work / 'control'))
        info = subprocess.check_output(['dpkg-deb', '-f', str(packages['deb'])], text=True)
        assert f'Version: {VERSION.replace("-", "~", 1)}' in info
        assert f'Architecture: {dict(x86_64="amd64", aarch64="arm64")[ARCH]}' in info
        assert 'libc6 (' in info and 'libxkbcommon0' in info and 'openssh-client' in info
        payload(deb)
        rpm = work / 'rpm'; rpm.mkdir()
        with tempfile.TemporaryFile() as archive:
            run('rpm2cpio', str(packages['rpm']), stdout=archive)
            archive.seek(0)
            run('cpio', '-idm', '--quiet', '--no-absolute-filenames', cwd=rpm, stdin=archive)
        info = subprocess.check_output(['rpm', '-qp', '--queryformat', '%{NAME} %{VERSION} %{ARCH}', str(packages['rpm'])], text=True)
        assert info == f'hedit {VERSION.split("-")[0]} {ARCH}', info
        requires = subprocess.check_output(['rpm', '-qp', '--requires', str(packages['rpm'])], text=True)
        assert 'libc.so.6(GLIBC_' in requires and 'libxkbcommon.so.0()(64bit)' in requires
        payload(rpm)
        image = work / 'image with spaces'; image.mkdir()
        run(str(packages['AppImage']), '--appimage-extract', cwd=image, stdout=subprocess.DEVNULL)
        appdir = image / 'squashfs-root'
        payload(appdir)
        assert (appdir / 'usr/share/doc/hedit/bundled-libraries').is_dir()
        assert list((appdir / 'usr/lib').glob('libxkbcommon*'))
        env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN='1')
        assert subprocess.check_output([str(packages['AppImage']), '--version'], env=env, text=True).strip() == f'hedit {VERSION}'
        if args.containers:
            container_tests(work, packages, deb, rpm)
    print('DEB, RPM, and AppImage checksums, metadata, resources, and executable checks passed.')


def container_tests(work, packages, deb, rpm):
    # Install a lower package version first to exercise actual package upgrades.
    mounted = work / 'packages'; mounted.mkdir()
    for p in packages.values():
        shutil.copy2(p, mounted / p.name)
    old_deb = work / 'old-deb'
    shutil.copytree(deb, old_deb)
    shutil.copytree(work / 'control', old_deb / 'DEBIAN')
    control = old_deb / 'DEBIAN/control'
    control.write_text('\n'.join('Version: 0.0.0' if line.startswith('Version:') else line for line in control.read_text().splitlines()) + '\n')
    run('dpkg-deb', '--root-owner-group', '-b', str(old_deb), str(mounted / 'old.deb'))
    spec = importlib.util.spec_from_file_location('linux_package', ROOT / 'scripts/package-linux.py')
    package = importlib.util.module_from_spec(spec); spec.loader.exec_module(package)
    stage = work / 'old-rpm'; stage.mkdir()
    package.rpm(stage, rpm, '0.0.0', ARCH, mounted)
    (mounted / f'hedit-0.0.0-linux-{ARCH}.rpm').rename(mounted / 'old.rpm')
    cases = [
        ('ubuntu:24.04', f'''set -eu
apt-get update -qq
apt-get install -y --no-install-recommends /packages/old.deb
apt-get install -y --no-install-recommends /packages/{packages['deb'].name}
test "$(dpkg-query -W -f='${{Version}}' hedit)" = '{VERSION.replace('-', '~', 1)}'
hedit --version
mkdir -p /root/.local/share/hedit; echo keep > /root/.local/share/hedit/test-note
dpkg --remove hedit
test ! -e /usr/bin/hedit
test "$(cat /root/.local/share/hedit/test-note)" = keep
'''),
        ('fedora:43', f'''set -eu
dnf install -y --setopt=install_weak_deps=False /packages/old.rpm
dnf upgrade -y --setopt=install_weak_deps=False /packages/{packages['rpm'].name}
test "$(rpm -q --qf '%{{VERSION}}' hedit)" = '{VERSION.split('-')[0]}'
hedit --version
mkdir -p /root/.local/share/hedit; echo keep > /root/.local/share/hedit/test-note
rpm -e hedit
test ! -e /usr/bin/hedit
test "$(cat /root/.local/share/hedit/test-note)" = keep
''')]
    for image, command in cases:
        run('docker', 'run', '--rm', '--mount', f'type=bind,src={mounted},dst=/packages,readonly', image, 'sh', '-c', command)
    print('Ubuntu DEB and Fedora RPM installation, upgrade, removal, and user-data preservation passed.')


if __name__ == '__main__':
    main()
