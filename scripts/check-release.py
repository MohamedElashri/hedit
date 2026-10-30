#!/usr/bin/env python3
"""Validate a release tag and optionally export its dated changelog entry."""
import argparse
from datetime import date
import os
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def release_notes(changelog, version):
    pattern = rf'^## \[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})[ \t]*$'
    entries = list(re.finditer(pattern, changelog, re.MULTILINE))
    if len(entries) != 1:
        raise ValueError(f'CHANGELOG.md must have exactly one dated [{version}] entry.')
    entry = entries[0]
    date.fromisoformat(entry.group(1))
    body = re.split(r'^## ', changelog[entry.end():], maxsplit=1,
                    flags=re.MULTILINE)[0].strip()
    if not re.search(r'^-\s+\S', body, re.MULTILINE):
        raise ValueError(f'CHANGELOG.md [{version}] has no release notes.')
    return body + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--notes-file', type=Path,
                        help='Write this release\'s changelog entry to a UTF-8 file.')
    args = parser.parse_args(argv)
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    expected = f'v{version}'
    tag = os.environ.get('RELEASE_TAG', '')
    if not tag:
        sys.exit('Set RELEASE_TAG, for example: make release-check RELEASE_TAG=' + expected)
    if tag != expected:
        sys.exit(f'Release tag {tag!r} must match Cargo.toml: {expected}')
    try:
        notes = release_notes((ROOT / 'CHANGELOG.md').read_text(encoding='utf-8'), version)
    except ValueError as error:
        sys.exit(str(error))
    if args.notes_file:
        args.notes_file.write_text(notes, encoding='utf-8')
    print(f'Release tag and changelog match package version: {tag}')


if __name__ == '__main__':
    main()
