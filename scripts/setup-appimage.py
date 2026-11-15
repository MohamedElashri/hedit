#!/usr/bin/env python3
"""Fetch checksum-pinned AppImage tools; packaging never fetches a moving runtime."""
import argparse
import hashlib
from pathlib import Path
import platform
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
# Digests from the upstream GitHub release asset metadata.
TOOLS = {
    'x86_64': ('ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0',
               '2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d'),
    'aarch64': ('f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158',
                '00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444'),
}


def download(url, destination, digest):
    if destination.is_file() and hashlib.sha256(destination.read_bytes()).hexdigest() == digest:
        return
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temporary:
        path = Path(temporary.name)
        try:
            with urllib.request.urlopen(url, timeout=120) as response:
                while block := response.read(1024 * 1024):
                    temporary.write(block)
            temporary.flush()
            if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                raise RuntimeError(f'Checksum mismatch for {url}')
            path.chmod(0o755)
            path.replace(destination)
        finally:
            path.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, default=ROOT / 'target/appimage-tools')
    args = parser.parse_args()
    arch = platform.machine().lower()
    if platform.system() != 'Linux' or arch not in TOOLS:
        parser.error('Use native Linux x86_64 or aarch64')
    args.directory.mkdir(parents=True, exist_ok=True)
    tool_hash, runtime_hash = TOOLS[arch]
    download(f'https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-{arch}.AppImage',
             args.directory / 'appimagetool', tool_hash)
    download(f'https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-{arch}',
             args.directory / 'runtime', runtime_hash)
    print(args.directory)


if __name__ == '__main__':
    main()
