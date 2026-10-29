#!/usr/bin/env python3
"""Package, extract, install, and check the executable in an isolated prefix."""
import argparse
import hashlib
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/hedit')
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix='hedit-package-smoke-') as work:
    work = Path(work)
    subprocess.run([sys.executable, str(ROOT / 'scripts/package-linux.py'),
                    '--binary', str(args.binary.resolve()), '--out-dir', str(work)], check=True)
    archive = next(work.glob('hedit-*.tar.gz'))
    checksum = archive.with_name(archive.name + '.sha256').read_text().split()[0]
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == checksum
    with tarfile.open(archive) as data:
        data.extractall(work / 'unpacked', filter='data')
    package = next((work / 'unpacked').iterdir())
    prefix = work / 'installed'
    subprocess.run(['sh', str(package / 'install.sh'), str(prefix)], check=True)
    result = subprocess.run([str(prefix / 'bin/hedit'), '--version'], check=True,
                            capture_output=True, text=True, timeout=10)
    assert result.stdout.strip() == f'hedit {VERSION}', result.stdout
    for relative in ['share/applications/hedit.desktop',
                     'share/icons/hicolor/scalable/apps/hedit.svg',
                     'share/doc/hedit/README.md', 'share/doc/hedit/USER_GUIDE.md',
                     'share/doc/hedit/DEVELOPER_GUIDE.md', 'share/doc/hedit/LICENSE',
                     'share/doc/hedit/assets/hedit-lockup-light.svg',
                     'share/doc/hedit/assets/licenses/STIX-Two-Math-OFL.txt',
                     'share/doc/hedit/assets/licenses/latex-rust-NOTICE.txt',
                     'share/doc/hedit/assets/licenses/latex-rust-dvipsnames.tsv',
                     'share/doc/hedit/assets/licenses/LPPL-1.3c.txt',
                     'share/hedit/themes/hedit-ink.json']:
        assert (prefix / relative).is_file(), relative
    assert (package / 'examples/cuda/vector_add.cu').is_file()
print('Linux package checksum, extraction, installation, and executable smoke test passed.')
