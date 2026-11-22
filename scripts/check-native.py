#!/usr/bin/env python3
"""Keep the app native; allow browser scripts only in the documentation site."""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[1]
forbidden_suffixes = {".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"}
forbidden_names = {"package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lock", "bun.lockb", "node_modules"}

def scan(directory):
    for path in directory.iterdir():
        if path == root / '.worktrees':
            continue
        if path.name in {".git", "target", ".agents", ".codex", ".aws", "__pycache__"}:
            continue
        documentation_script = (path.suffix == '.js' and
                                path.parent == root / 'website/static/js')
        if (path.suffix in forbidden_suffixes and not documentation_script) or path.name in forbidden_names:
            yield path.relative_to(root)
        if path.is_dir() and not path.is_symlink():
            yield from scan(path)

def main():
    bad = list(scan(root))
    if bad:
        print("Native-only policy failed:\n" + "\n".join(map(str, bad)), file=sys.stderr)
        sys.exit(1)
    print("Native-only policy passed: no app frontend or Node build files.")


if __name__ == '__main__':
    main()
