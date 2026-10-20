#!/usr/bin/env python3
"""Build an installable Linux tarball from a compiled hedit executable."""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/hedit')
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'dist')
    args = parser.parse_args()
    if platform.system() != 'Linux':
        parser.error('Linux packages must be built on Linux.')
    if not args.binary.is_file() or not os.access(args.binary, os.X_OK):
        parser.error('Build the executable first with cargo build --release --locked.')
    machine = platform.machine()
    arch = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(machine)
    if not arch:
        parser.error(f'Unsupported package architecture: {machine}')
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    name = f'hedit-{version}-linux-{arch}'
    args.out_dir.mkdir(parents=True, exist_ok=True)
    archive = args.out_dir / f'{name}.tar.gz'
    with tempfile.TemporaryDirectory(prefix='.hedit-package-', dir=args.out_dir) as work:
        stage = Path(work) / name
        stage.mkdir()
        shutil.copy2(args.binary, stage / 'hedit')
        for filename in ['README.md', 'LICENSE', 'NOTICE', 'ROADMAP.md', 'CONTRIBUTING.md']:
            shutil.copy2(ROOT / filename, stage / filename)
        for directory in ['assets', 'themes', 'docs', 'examples', 'contributor']:
            shutil.copytree(ROOT / directory, stage / directory)
        shutil.copy2(ROOT / 'packaging/linux/hedit.desktop', stage / 'hedit.desktop')
        shutil.copy2(ROOT / 'packaging/linux/install.sh', stage / 'install.sh')
        (stage / 'install.sh').chmod(0o755)
        # Commit a complete tarball in one rename; never leave half a download.
        temporary = Path(work) / archive.name
        with tarfile.open(temporary, 'w:gz') as output:
            output.add(stage, arcname=name)
        temporary.replace(archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = archive.with_name(archive.name + '.sha256')
    checksum.write_text(f'{digest}  {archive.name}\n')
    print(archive)
    print(checksum)

if __name__ == '__main__':
    main()
