#!/usr/bin/env python3
"""Reject release tags that do not match the package version."""
import os
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
expected = f'v{version}'
tag = os.environ.get('RELEASE_TAG', '')
if not tag:
    sys.exit('Set RELEASE_TAG, for example: make release-check RELEASE_TAG=' + expected)
if tag != expected:
    sys.exit(f'Release tag {tag!r} must match Cargo.toml: {expected}')
print(f'Release tag matches package version: {tag}')
