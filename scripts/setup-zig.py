#!/usr/bin/env python3
"""Select Ghostty's Zig compiler, installing an isolated copy when needed."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

# libghostty-vt-sys 0.2.2 pins Ghostty a887df42, whose build requires this version.
ZIG_VERSION = '0.15.2'
ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / 'target' / 'ghostty-toolchain'


def matches_version(executable):
    try:
        result = subprocess.run([str(executable), 'version'], check=False,
                                capture_output=True, text=True)
        return result.returncode == 0 and result.stdout.strip() == ZIG_VERSION
    except OSError:
        return False


def compiler_in(directory):
    return directory / 'ziglang' / ('zig.exe' if os.name == 'nt' else 'zig')


def ensure_zig(cache=CACHE):
    if explicit := os.environ.get('ZIG'):
        executable = Path(shutil.which(explicit) or explicit).resolve()
        if not matches_version(executable):
            raise RuntimeError(f'ZIG must name a Zig {ZIG_VERSION} executable, without arguments.')
        return executable
    installed = shutil.which('zig')
    if installed and matches_version(installed):
        return Path(installed).absolute()
    executable = compiler_in(cache)
    if matches_version(executable):
        return executable
    if cache.exists():
        raise RuntimeError(f'Incomplete Zig toolchain in {cache}. Remove that directory and retry, or set ZIG to a Zig {ZIG_VERSION} executable.')
    cache.parent.mkdir(parents=True, exist_ok=True)
    print(f'hedit: preparing Zig {ZIG_VERSION} under {cache}', file=sys.stderr)
    with tempfile.TemporaryDirectory(prefix='ghostty-zig-', dir=cache.parent) as temporary:
        staged = Path(temporary) / 'compiler'
        # --target works with externally managed Python installations and does
        # not modify the interpreter's packages. No global pip cache is needed.
        subprocess.run([sys.executable, '-m', 'pip', 'install', '--no-deps', '--no-input',
                        '--no-cache-dir', '--disable-pip-version-check',
                        '--target', str(staged), f'ziglang=={ZIG_VERSION}'],
                       check=True, stdout=sys.stderr)
        if not matches_version(compiler_in(staged)):
            raise RuntimeError(f'Downloaded compiler is not Zig {ZIG_VERSION}.')
        try:
            staged.rename(cache)
        except OSError:
            # Concurrent architecture builds may prepare the same toolchain.
            if not matches_version(executable):
                raise
    return executable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--print-path', action='store_true', help='Print only the compiler path for Cargo.')
    args = parser.parse_args()
    try:
        executable = ensure_zig()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'Cannot prepare Zig {ZIG_VERSION}: {error}\nInstall Python with pip, or set ZIG to an existing compiler.') from error
    if path_file := os.environ.get('GITHUB_PATH'):
        with open(path_file, 'a', encoding='utf-8') as stream:
            stream.write(f'{executable.parent}\n')
    if args.print_path:
        print(executable)
    else:
        print(f'Zig {ZIG_VERSION}: {executable}')


if __name__ == '__main__':
    main()
