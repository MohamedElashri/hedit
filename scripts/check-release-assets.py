#!/usr/bin/env python3
"""Reject missing, unexpected, or checksum-mismatched assets before publishing."""
import argparse
import hashlib
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def validate(directory, version):
    packages = [f'hedit-{version}-linux-{arch}.{kind}'
                for arch in ['x86_64', 'aarch64'] for kind in ['tar.gz', 'deb', 'rpm', 'AppImage']]
    packages += [f'hedit-{version}-macos-universal.zip', f'hedit-{version}-windows-x86_64.zip']
    expected = set(packages + [name + '.sha256' for name in packages])
    actual = {p.name for p in directory.iterdir()}
    if actual != expected:
        raise ValueError(f'Release assets differ: missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}')
    for name in packages:
        digest = hashlib.sha256((directory / name).read_bytes()).hexdigest()
        if (directory / (name + '.sha256')).read_text().split() != [digest, name]:
            raise ValueError(f'Invalid checksum: {name}')
    return len(packages)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, nargs='?', default=ROOT / 'dist')
    args = parser.parse_args()
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    try:
        count = validate(args.directory, version)
    except (ValueError, OSError) as error:
        parser.exit(1, f'{error}\n')
    print(f'All {count} release packages and their checksums passed.')


if __name__ == '__main__':
    main()
