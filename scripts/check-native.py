#!/usr/bin/env python3
"""Reject reintroduction of a web frontend or Node package/build files."""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[1]
forbidden_suffixes = {".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"}
forbidden_names = {"package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lock", "bun.lockb", "node_modules"}

def scan(directory):
    for path in directory.iterdir():
        if path.name in {".git", "target", ".agents", ".codex", ".aws", "__pycache__"}:
            continue
        if path.suffix in forbidden_suffixes or path.name in forbidden_names:
            yield path.relative_to(root)
        if path.is_dir() and not path.is_symlink():
            yield from scan(path)

bad = list(scan(root))
if bad:
    print("Native-only policy failed:\n" + "\n".join(map(str, bad)), file=sys.stderr)
    sys.exit(1)
print("Native-only policy passed: no Node/frontend files.")
