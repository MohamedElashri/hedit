#!/usr/bin/env python3
"""Merge both macOS executables into one signed app bundle and ZIP archive."""
import argparse
import hashlib
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib
import zlib
from package_resources import copy_documentation
from macos_plugin_helper import prepare as prepare_plugin_helper

ROOT = Path(__file__).resolve().parents[1]


def icon_png(path, size=1024):
    """Rasterize the existing geometric hedit mark without third-party tools."""
    rows = bytearray()
    for y in range(size):
        rows.append(0)
        py = (y + 0.5) * 64 / size
        for x in range(size):
            px = (x + 0.5) * 64 / size
            inside = (px - min(50, max(14, px))) ** 2 + (py - min(50, max(14, py))) ** 2 <= 196
            mark = (14 <= px < 22 and 14 <= py < 50 or
                    22 <= px < 35 and 27 <= py < 35 or
                    33 <= px < 41 and 33 <= py < 50 or
                    33 <= px <= 41 and 27 + max(px - 35, 0) <= py <= 35 - (px - 33) / 4)
            lines = 31 <= px < 50 and 14 <= py < 18 or 38 <= px < 50 and 21 <= py < 25
            color = ((100, 216, 203, 255) if mark else (154, 235, 223, 255) if lines
                     else (16, 22, 31, 255)) if inside else (0, 0, 0, 0)
            rows.extend(color)
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)) +
                     chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def package(arm64, intel, output, identity='-', helper_arm64=None, helper_intel=None):
    if sys.platform != 'darwin':
        raise ValueError('Universal macOS packages must be assembled on macOS with Xcode tools.')
    for binary, architecture in [(arm64, 'arm64'), (intel, 'x86_64')]:
        if not binary.is_file():
            raise ValueError(f'Build the {architecture} executable first: {binary}')
        subprocess.run(['lipo', str(binary), '-verify_arch', architecture], check=True)
    helper_arm64 = helper_arm64 or arm64.with_name('hedit-remote')
    helper_intel = helper_intel or intel.with_name('hedit-remote')
    for binary, architecture in [(helper_arm64, 'arm64'), (helper_intel, 'x86_64')]:
        if not binary.is_file():
            raise ValueError(f'Build the {architecture} helper with cargo build --release --locked --workspace: {binary}')
        subprocess.run(['lipo', str(binary), '-verify_arch', architecture], check=True)
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    output.mkdir(parents=True, exist_ok=True)
    name = f'hedit-{version}-macos-universal.zip'
    with tempfile.TemporaryDirectory(prefix='.hedit-macos-', dir=output) as temporary:
        stage = Path(temporary)
        app = stage / 'hedit.app'
        contents = app / 'Contents'
        resources = contents / 'Resources'
        executable = contents / 'MacOS/hedit'
        executable.parent.mkdir(parents=True)
        resources.mkdir()
        subprocess.run(['lipo', '-create', str(arm64), str(intel), '-output', str(executable)], check=True)
        executable.chmod(0o755)
        helper = contents / 'MacOS/hedit-remote'
        subprocess.run(['lipo', '-create', str(helper_arm64), str(helper_intel), '-output', str(helper)], check=True)
        helper.chmod(0o755)
        for architecture in ['arm64', 'x86_64']:
            subprocess.run(['lipo', str(helper), '-verify_arch', architecture], check=True)
        for architecture in ['arm64', 'x86_64']:
            subprocess.run(['lipo', str(executable), '-verify_arch', architecture], check=True)
        with (contents / 'Info.plist').open('wb') as stream:
            plistlib.dump({'CFBundleName': 'hedit', 'CFBundleDisplayName': 'hedit',
                           'CFBundleIdentifier': 'hedit.hedit', 'CFBundleExecutable': 'hedit',
                           'CFBundlePackageType': 'APPL', 'CFBundleVersion': version,
                           'CFBundleShortVersionString': version, 'CFBundleIconFile': 'hedit.icns',
                           'LSMinimumSystemVersion': '11.0', 'NSHighResolutionCapable': True,
                           'NSHumanReadableCopyright': 'MIT licensed'}, stream)
        icon = stage / 'hedit.png'
        icon_png(icon)
        iconset = stage / 'hedit.iconset'
        iconset.mkdir()
        for points in [16, 32, 128, 256, 512]:
            for scale in [1, 2]:
                pixels = str(points * scale)
                target = iconset / f'icon_{points}x{points}{"@2x" if scale == 2 else ""}.png'
                subprocess.run(['sips', '-z', pixels, pixels, str(icon), '--out', str(target)], check=True,
                               stdout=subprocess.DEVNULL)
        subprocess.run(['iconutil', '-c', 'icns', str(iconset), '-o', str(resources / 'hedit.icns')], check=True)
        shutil.copytree(ROOT / 'themes', resources / 'themes')
        shutil.copytree(ROOT / 'examples/cuda', resources / 'examples/cuda')
        docs = resources / 'Documentation'
        copy_documentation(ROOT, docs)
        signing = ['codesign', '--force', '--sign', identity]
        if identity != '-':
            signing += ['--options', 'runtime', '--timestamp']
        subprocess.run([*signing, str(helper)], check=True)
        prepare_plugin_helper(executable, contents / 'Helpers/hedit-plugin.app', identity)
        subprocess.run([*signing, str(app)], check=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
        reported = subprocess.run([str(executable), '--version'], check=True, capture_output=True, text=True)
        if reported.stdout.strip() != f'hedit {version}':
            raise ValueError('The bundled executable version does not match Cargo.toml.')
        reported_helper = subprocess.run([str(helper), '--version'], check=True, capture_output=True, text=True)
        if reported_helper.stdout.strip() != f'hedit-remote {version}':
            raise ValueError('The bundled helper version does not match Cargo.toml.')
        archive = stage / name
        subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(archive)], check=True)
        archive.replace(output / name)
        remote_name = f'hedit-remote-{version}-macos-universal'
        remote = stage / remote_name
        shutil.copy2(helper, remote)
        remote.replace(output / remote_name)
        (output / (remote_name + '.sha256')).write_text(f'{hashlib.sha256((output / remote_name).read_bytes()).hexdigest()}  {remote_name}\n')
    archive = output / name
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(name + '.sha256').write_text(f'{digest}  {name}\n')
    print(archive)
    print(archive.with_name(name + '.sha256'))
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arm64', type=Path, default=ROOT / 'target/aarch64-apple-darwin/release/hedit')
    parser.add_argument('--intel', type=Path, default=ROOT / 'target/x86_64-apple-darwin/release/hedit')
    parser.add_argument('--helper-arm64', type=Path)
    parser.add_argument('--helper-intel', type=Path)
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'dist')
    parser.add_argument('--identity', default='-', help='Developer ID identity, or - for an ad-hoc development signature')
    args = parser.parse_args()
    try:
        package(args.arm64.resolve(), args.intel.resolve(), args.out_dir.resolve(), args.identity, args.helper_arm64, args.helper_intel)
    except (ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
