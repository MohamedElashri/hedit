#!/usr/bin/env python3
"""Package, extract, install, and check the executable in an isolated prefix."""
import argparse
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from package_resources import copy_documentation

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']


def prepare_source(source):
    (source / 'scripts').mkdir(parents=True)
    for name in ['package-linux.py', 'package_resources.py']:
        shutil.copy2(ROOT / 'scripts' / name, source / 'scripts' / name)
    shutil.copy2(ROOT / 'Cargo.toml', source / 'Cargo.toml')
    copy_documentation(ROOT, source)
    for directory in ['themes', 'examples/cuda', 'packaging/linux']:
        shutil.copytree(ROOT / directory, source / directory)
    (source / 'examples/development_probe.rs').write_text('not a release resource')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/hedit')
    parser.add_argument('--helper', type=Path, default=ROOT / 'target/release/hedit-remote')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='hedit-package-smoke-') as work:
        work = Path(work)
        source = work / 'source'
        prepare_source(source)
        subprocess.run([sys.executable, str(source / 'scripts/package-linux.py'),
                        '--binary', str(args.binary.resolve()), '--helper', str(args.helper.resolve()), '--out-dir', str(work)], check=True)
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
        helper = prefix / 'bin/hedit-remote'
        assert subprocess.check_output([str(helper), '--version'], text=True).strip() == f'hedit-remote {VERSION}'
        for relative in ['share/applications/hedit.desktop',
                         'share/icons/hicolor/scalable/apps/hedit.svg',
                         'share/doc/hedit/README.md', 'share/doc/hedit/USER_GUIDE.md',
                         'share/doc/hedit/DEVELOPER_GUIDE.md', 'share/doc/hedit/LICENSE',
                         'share/doc/hedit/assets/hedit-lockup-light.svg',
                         'share/doc/hedit/assets/licenses/egui_commonmark-MIT.txt',
                         'share/doc/hedit/assets/licenses/STIX-Two-Math-OFL.txt',
                         'share/doc/hedit/assets/licenses/latex-rust-NOTICE.txt',
                         'share/doc/hedit/assets/licenses/latex-rust-dvipsnames.tsv',
                         'share/doc/hedit/assets/licenses/LPPL-1.3c.txt',
                         'share/hedit/themes/hedit-ink.json']:
            assert (prefix / relative).is_file(), relative
        assert (package / 'examples/cuda/vector_add.cu').is_file()
        assert {path.relative_to(package).as_posix() for path in (package / 'examples').rglob('*')
                if path.is_file()} == {
                    'examples/cuda/vector_add.cu', 'examples/extensions/markdown-task.json',
                    'examples/extensions/markdown-quote.json',
                }
        documents = ['CHANGELOG.md', 'vendor/egui_commonmark/PATCH.md', 'vendor/libghostty-vt-sys/PATCH.md',
                     'docs/extensions.md', 'examples/extensions/markdown-task.json',
                     'examples/extensions/markdown-quote.json']
        images = [path.relative_to(ROOT).as_posix() for path in (ROOT / 'docs/images').glob('*') if path.is_file()]
        for relative in documents + images:
            assert (package / relative).read_bytes() == (ROOT / relative).read_bytes(), relative
            assert (prefix / 'share/doc/hedit' / relative).read_bytes() == (ROOT / relative).read_bytes(), relative
    print('Linux package checksum, extraction, installation, and executable smoke test passed.')


if __name__ == '__main__':
    main()
