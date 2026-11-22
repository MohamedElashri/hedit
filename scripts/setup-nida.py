#!/usr/bin/env python3
"""Download the checksum-pinned Nida binary into target/docs-tools (no global install)."""
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    lock = json.loads((ROOT / 'website/nida.lock.json').read_text())
    system = platform.system().lower()
    architecture = {'aarch64': 'arm64', 'AMD64': 'x86_64'}.get(platform.machine(), platform.machine())
    key = f'{system}_{architecture}'
    if key not in lock['archives']:
        raise SystemExit(f'No pinned Nida release for {key}')
    windows = system == 'windows'
    archive = f"nida_{lock['version']}_{key}." + ('zip' if windows else 'tar.gz')
    url = f"https://github.com/MohamedElashri/nida/releases/download/v{lock['version']}/{archive}"
    binary = 'nida.exe' if windows else 'nida'
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(64 * 1024 * 1024 + 1)
    if len(data) > 64 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != lock['archives'][key]:
        raise SystemExit('Nida archive checksum mismatch')
    if windows:
        with zipfile.ZipFile(io.BytesIO(data)) as bundle:
            payload = bundle.read(binary)
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as bundle:
            member = bundle.getmember(binary)
            if not member.isfile() or member.size > 64 * 1024 * 1024:
                raise SystemExit('Invalid Nida binary entry')
            payload = bundle.extractfile(member).read()
    output = ROOT / 'target/docs-tools' / binary
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix('.tmp')
    temporary.write_bytes(payload)
    temporary.chmod(0o755)
    temporary.replace(output)
    print(output)


if __name__ == '__main__':
    main()
