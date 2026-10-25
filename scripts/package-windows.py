#!/usr/bin/env python3
"""Package an x86-64 Windows executable as a portable ZIP with a checksum."""
import argparse
import hashlib
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def version():
    return tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']


def check_binary(binary):
    """Reject a missing executable, a Unix binary, or the wrong PE architecture."""
    with binary.open('rb') as source:
        header = source.read(64)
        if len(header) != 64 or header[:2] != b'MZ':
            raise ValueError('Expected a Windows PE executable; build with the MSVC toolchain.')
        source.seek(struct.unpack_from('<I', header, 60)[0])
        pe = source.read(6)
        if pe != b'PE\0\0\x64\x86':
            raise ValueError('Expected an x86-64 Windows PE executable.')


def package(binary, out_dir):
    check_binary(binary)
    name = f'hedit-{version()}-windows-x86_64'
    out_dir.mkdir(parents=True, exist_ok=True)
    archive = out_dir / f'{name}.zip'
    with tempfile.TemporaryDirectory(prefix='.hedit-windows-', dir=out_dir) as work:
        stage = Path(work) / name
        stage.mkdir()
        shutil.copy2(binary, stage / 'hedit.exe')
        for filename in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'LICENSE']:
            shutil.copy2(ROOT / filename, stage / filename)
        for directory in ['assets', 'themes', 'examples']:
            shutil.copytree(ROOT / directory, stage / directory)
        temporary = Path(work) / archive.name
        with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED) as output:
            for path in sorted(stage.rglob('*')):
                if path.is_file():
                    output.write(path, path.relative_to(stage.parent).as_posix())
        temporary.replace(archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + '.sha256').write_text(
        f'{digest}  {archive.name}\n', encoding='utf-8', newline='\n')
    return archive


def smoke(archive):
    digest, filename = archive.with_name(archive.name + '.sha256').read_text(encoding='utf-8').strip().split('  ')
    if filename != archive.name or digest != hashlib.sha256(archive.read_bytes()).hexdigest():
        raise ValueError('Archive checksum does not match.')
    # Spaces in the installation path catch common Windows launch mistakes.
    with tempfile.TemporaryDirectory(prefix='hedit portable test ') as work:
        with zipfile.ZipFile(archive) as bundle:
            bundle.extractall(work)
        root = Path(work) / archive.stem
        for relative in ['hedit.exe', 'LICENSE', 'USER_GUIDE.md',
                         'themes/hedit-ink.json', 'assets/dictionaries/en_US-LICENSE.txt',
                         'examples/cuda/vector_add.cu']:
            if not (root / relative).is_file():
                raise ValueError(f'Missing package resource: {relative}')
        binary = root / 'hedit.exe'
        output = subprocess.run([str(binary), '--version'], check=True, capture_output=True, timeout=30)
        if output.stdout.decode('utf-8').strip() != f'hedit {version()}':
            raise ValueError('Packaged executable version does not match Cargo.toml.')
        output = subprocess.run([str(binary), '--help'], check=True, capture_output=True, timeout=30)
        if b'Usage: hedit' not in output.stdout:
            raise ValueError('Packaged executable did not print its help.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/hedit.exe')
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'dist')
    parser.add_argument('--smoke', action='store_true', help='Extract and run the package (requires Windows).')
    args = parser.parse_args()
    try:
        archive = package(args.binary, args.out_dir)
        if args.smoke:
            smoke(archive)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f'{error}\n')
    print(archive)
    print(archive.with_name(archive.name + '.sha256'))


if __name__ == '__main__':
    main()
