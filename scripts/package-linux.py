#!/usr/bin/env python3
"""Build Linux tarball, DEB, RPM, and AppImage release packages."""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from package_resources import copy_documentation

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/hedit')
    parser.add_argument('--helper', type=Path, default=ROOT / 'target/release/hedit-remote')
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'dist')
    parser.add_argument('--formats', nargs='+', choices=['tar.gz', 'deb', 'rpm', 'appimage'], default=['tar.gz'])
    parser.add_argument('--appimage-tools', type=Path, default=ROOT / 'target/appimage-tools')
    args = parser.parse_args()
    args.binary = args.binary.resolve()
    args.helper = args.helper.resolve()
    args.out_dir = args.out_dir.resolve()
    if platform.system() != 'Linux':
        parser.error('Linux packages must be built on Linux.')
    if not args.binary.is_file() or not os.access(args.binary, os.X_OK):
        parser.error('Build the executable first with cargo build --release --locked.')
    if not args.helper.is_file() or not os.access(args.helper, os.X_OK):
        parser.error('Build the matching helper with cargo build --release --locked --workspace.')
    machine = platform.machine()
    arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(machine)
    if not arch:
        parser.error(f'Unsupported package architecture: {machine}')
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    reported = subprocess.check_output([str(args.helper), '--version'], text=True).strip()
    if reported != f'hedit-remote {version}':
        parser.error('The helper version must match the editor version.')
    name = f'hedit-{version}-linux-{arch}'
    args.out_dir.mkdir(parents=True, exist_ok=True)
    archive = args.out_dir / f'{name}.tar.gz'
    with tempfile.TemporaryDirectory(prefix='.hedit-package-', dir=args.out_dir) as work:
        stage = Path(work) / name
        stage.mkdir()
        shutil.copy2(args.binary, stage / 'hedit')
        shutil.copy2(args.helper, stage / 'hedit-remote')
        standalone = stage / f'hedit-remote-{version}-linux-{arch}'
        shutil.copy2(args.helper, standalone)
        publish(standalone, args.out_dir / standalone.name)
        copy_documentation(ROOT, stage)
        shutil.copytree(ROOT / 'themes', stage / 'themes')
        shutil.copytree(ROOT / 'examples/cuda', stage / 'examples/cuda')
        shutil.copy2(ROOT / 'packaging/linux/hedit.desktop', stage / 'hedit.desktop')
        shutil.copy2(ROOT / 'packaging/linux/install.sh', stage / 'install.sh')
        (stage / 'install.sh').chmod(0o755)
        if 'tar.gz' in args.formats:
            temporary = Path(work) / archive.name
            with tarfile.open(temporary, 'w:gz') as output:
                output.add(stage, arcname=name)
            publish(temporary, archive)
        if set(args.formats) - {'tar.gz'}:
            prefix = Path(work) / 'root/usr'
            subprocess.run(['sh', str(stage / 'install.sh'), str(prefix)], check=True)
            (prefix / 'share/hedit/package-managed').write_text('Use the package manager or replace the AppImage.\n')
            shutil.copy2(stage / 'LICENSE', prefix / 'share/doc/hedit/copyright')
            if 'deb' in args.formats:
                deb(stage, prefix.parent, version, arch, args.out_dir)
            if 'rpm' in args.formats:
                rpm(stage, prefix.parent, version, arch, args.out_dir)
            if 'appimage' in args.formats:
                appimage(prefix.parent, version, arch, args.out_dir, args.appimage_tools)



def publish(source, destination):
    source.replace(destination)
    checksum = destination.with_name(destination.name + '.sha256')
    checksum.write_text(f'{hashlib.sha256(destination.read_bytes()).hexdigest()}  {destination.name}\n')
    print(destination)
    print(checksum)


def deb(stage, root, version, arch, out):
    architecture = {'x86_64': 'amd64', 'aarch64': 'arm64'}[arch]
    control = root / 'DEBIAN'
    control.mkdir()
    # ELF dependencies alone miss the display libraries loaded at runtime.
    (stage / 'debian').mkdir()
    (stage / 'debian/control').write_text('Source: hedit\n\nPackage: hedit\nArchitecture: any\nDescription: Native text editor\n')
    dependencies = subprocess.check_output(
        ['dpkg-shlibdeps', '--ignore-missing-info', '-O', '-e' + str(root / 'usr/bin/hedit'), '-e' + str(root / 'usr/bin/hedit-remote')],
        cwd=stage, text=True).strip().removeprefix('shlibs:Depends=')
    display = 'libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libegl1, libgl1'
    (control / 'control').write_text(f"""Package: hedit
Version: {version.replace('-', '~', 1)}
Architecture: {architecture}
Maintainer: Mohamed Elashri <git@elashri.com>
Section: editors
Priority: optional
Homepage: https://github.com/MohamedElashri/hedit
License: MIT
Installed-Size: {sum(p.stat().st_size for p in (root / 'usr').rglob('*') if p.is_file()) // 1024}
Depends: {dependencies}, {display}
Recommends: git (>= 2.30), openssh-client
Description: Native text editor with durable recovery and SSH workspaces
 Edit code and technical notes with Git, Markdown preview, and host-only SSH recovery.
""")
    destination = out / f'hedit-{version}-linux-{arch}.deb'
    temporary = stage / destination.name
    subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(root), str(temporary)], check=True)
    publish(temporary, destination)
    shutil.rmtree(control)


def rpm(stage, root, version, arch, out):
    top = stage / 'rpmbuild'
    top.mkdir()
    release = '1'
    rpm_version = version
    if '-' in version:
        rpm_version, prerelease = version.split('-', 1)
        release = '0.1.' + prerelease.replace('-', '.')
    spec = top / 'hedit.spec'
    spec.write_text(f"""Name: hedit
Version: {rpm_version}
Release: {release}
Summary: Native text editor with durable recovery and SSH workspaces
License: MIT
URL: https://github.com/MohamedElashri/hedit
Requires: libX11.so.6()(64bit), libXcursor.so.1()(64bit), libXi.so.6()(64bit), libXrandr.so.2()(64bit)
Requires: libxkbcommon.so.0()(64bit), libxkbcommon-x11.so.0()(64bit), libwayland-client.so.0()(64bit)
Requires: libEGL.so.1()(64bit), libGL.so.1()(64bit)
Recommends: git >= 2.30, openssh-clients
%description
Edit code and technical notes with Git, Markdown preview, and host-only SSH recovery.
%install
mkdir -p %{{buildroot}}
cp -a "{root}/usr" %{{buildroot}}/
%files
/usr/bin/hedit
/usr/bin/hedit-remote
/usr/share/applications/hedit.desktop
/usr/share/icons/hicolor/scalable/apps/hedit.svg
/usr/share/hedit
%doc /usr/share/doc/hedit
""")
    subprocess.run(['rpmbuild', '-bb', '--target', arch, '--define', f'_topdir {top}',
                    '--define', '_build_id_links none', '--define', '__os_install_post %{nil}',
                    str(spec)], check=True)
    package, = top.glob('RPMS/*/*.rpm')
    publish(package, out / f'hedit-{version}-linux-{arch}.rpm')


def appimage(root, version, arch, out, tools):
    # A detached window must outlive the calling runtime, including extraction
    # fallback (whose default cache directory is shared by identical images).
    launcher = root / 'usr/bin/hedit-appimage-launch'
    launcher.write_text('''#!/bin/sh
set -eu
image=${APPIMAGE:?Missing AppImage path}
runtime_dir=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/hedit-appimage.XXXXXXXX")
trap 'rm -rf -- "$runtime_dir"' 0
TMPDIR="$runtime_dir" APPIMAGE_EXTRACT_AND_RUN=1 "$image" "$@"
''')
    launcher.chmod(0o755)
    # AppRun keeps the caller's working directory for relative project/file paths.
    (root / 'AppRun').write_text("""#!/bin/sh
set -eu
APPDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export APPDIR
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$APPDIR/usr/bin/hedit" "$@"
""")
    (root / 'AppRun').chmod(0o755)
    shutil.copy2(root / 'usr/share/applications/hedit.desktop', root / 'hedit.desktop')
    shutil.copy2(root / 'usr/share/icons/hicolor/scalable/apps/hedit.svg', root / 'hedit.svg')
    (root / '.DirIcon').symlink_to('hedit.svg')
    bundle_libraries(root)
    temporary = root.parent / 'hedit.AppImage'
    env = dict(os.environ, ARCH=arch, VERSION=version, APPIMAGE_EXTRACT_AND_RUN='1')
    subprocess.run([str(tools.resolve() / 'appimagetool'), '--runtime-file',
                    str(tools.resolve() / 'runtime'), '--no-appstream',
                    '--mksquashfs-opt', '-processors', '--mksquashfs-opt', '2',
                    str(root), str(temporary)],
                   env=env, check=True)
    temporary.chmod(0o755)
    publish(temporary, out / f'hedit-{version}-linux-{arch}.AppImage')


def bundle_libraries(root):
    # Leave glibc and the GPU stack to the host; bundle display clients and their
    # dependencies. dlopen'd libraries are not reported by ldd on the Rust binary.
    import re
    excluded = re.compile(r'^(ld-linux|lib(c|m|dl|pthread|rt|resolv)\.so)')
    cache = subprocess.check_output(['ldconfig', '-p'], text=True)
    names = ['libgcc_s.so.1', 'libX11.so.6', 'libXcursor.so.1', 'libXi.so.6',
             'libXrandr.so.2', 'libxkbcommon.so.0', 'libxkbcommon-x11.so.0',
             'libwayland-client.so.0', 'libwayland-cursor.so.0', 'libwayland-egl.so.1']
    pending = []
    for name in names:
        match = re.search(r'^\s*' + re.escape(name) + r'\s+.* => (.+)$', cache, re.MULTILINE)
        if match is None:
            raise RuntimeError(f'Install the build host display library {name}')
        pending.append(Path(match.group(1)))
    dest = root / 'usr/lib'
    dest.mkdir()
    notices = root / 'usr/share/doc/hedit/bundled-libraries'
    notices.mkdir()
    while pending:
        library = pending.pop()
        if excluded.match(library.name) or (dest / library.name).exists():
            continue
        shutil.copy2(library, dest / library.name, follow_symlinks=True)
        owner = None
        for candidate in [library, library.resolve(), Path('/usr') / library.relative_to('/')]:
            result = subprocess.run(['dpkg-query', '-S', str(candidate)], capture_output=True, text=True)
            if result.returncode == 0:
                owner = result.stdout.split(': ')[0].split(':')[0]
                break
        if not owner:
            raise RuntimeError(f'Cannot locate copyright for {library}')
        shutil.copy2(Path('/usr/share/doc') / owner / 'copyright', notices / (owner + '.copyright'))
        linked = subprocess.check_output(['ldd', str(library)], text=True)
        pending.extend(Path(p) for p in re.findall(r'=> (/\S+)', linked))


if __name__ == '__main__':
    main()
