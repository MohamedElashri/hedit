#!/usr/bin/env python3
"""Run pinned Zig with a build-local workaround for newer macOS SDK stubs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile


def macos_environment(executable, cache):
    env = os.environ.copy()
    if sys.platform != 'darwin':
        return env
    if explicit := env.get('SDKROOT'):
        sdk = Path(explicit).resolve()
    else:
        sdk = Path(subprocess.run(
            ['/usr/bin/xcrun', '--sdk', 'macosx', '--show-sdk-path'],
            check=True, capture_output=True, text=True).stdout.strip()).resolve()
    system = sdk / 'usr/lib/libSystem.tbd'
    # Recent SDKs advertise arm64e but omit arm64 in the top-level reexport.
    # Zig 0.15.2 requires an exact target match, unlike Apple's linker.
    header = system.read_text().split('install-name:', 1)[0]
    needs_system = 'arm64-macos' not in header and 'arm64e-macos' in header
    math = sdk / 'usr/include/math.h'
    math_source = math.read_text() if math.exists() else ''
    # SDK 27 delegates these macros to a newer Clang float.h. Zig's bundled
    # Clang does not implement that module-header protocol yet.
    needs_math = '__need_infinity_nan' in math_source
    if not needs_system and not needs_math:
        return env
    reported = subprocess.run([str(executable), 'env'], check=True,
                              capture_output=True, text=True).stdout
    match = re.search(r'\.lib_dir\s*=\s*("(?:\\.|[^"\\])*")', reported)
    if not match:
        raise RuntimeError('Cannot locate the pinned Zig compiler library directory.')
    bundled = Path(json.loads(match[1])).resolve() / 'libc/darwin/libSystem.tbd'
    # Use the definitions distributed with this exact Zig compiler. All SDK
    # headers, frameworks and other libraries still come from the selected SDK.
    digest = hashlib.sha256(b'sdk-view-v2' + str(sdk).encode() + system.read_bytes() + math_source.encode() +
                            str(bundled).encode() + bundled.read_bytes()).hexdigest()[:16]
    cache = Path(cache).resolve()
    cache.mkdir(parents=True, exist_ok=True)
    compatible = cache / digest
    if not compatible.exists():
        with tempfile.TemporaryDirectory(prefix='.sdk-', dir=cache) as temporary:
            stage = Path(temporary) / 'ready'
            libraries = stage / 'sdk/usr/lib'
            libraries.mkdir(parents=True)
            for entry in sdk.iterdir():
                if entry.name != 'usr':
                    (stage / 'sdk' / entry.name).symlink_to(entry)
            for entry in (sdk / 'usr').iterdir():
                if entry.name != 'lib':
                    target = stage / 'sdk/usr' / entry.name
                    if entry.name == 'include' and needs_math:
                        target.mkdir()
                        for include in entry.iterdir():
                            if include.name != 'math.h':
                                (target / include.name).symlink_to(include)
                        (target / 'math.h').write_text(math_source + '\n'
                            '#ifndef INFINITY\n#define INFINITY (__builtin_inff())\n#endif\n'
                            '#ifndef NAN\n#define NAN (__builtin_nanf(""))\n#endif\n')
                    else:
                        target.symlink_to(entry)
            for entry in (sdk / 'usr/lib').iterdir():
                source = bundled if entry.name == 'libSystem.tbd' and needs_system else entry
                (libraries / entry.name).symlink_to(source)
            binary = stage / 'bin/xcrun'
            binary.parent.mkdir()
            # Zig queries xcrun for its build runner as well as the library.
            # Scope this shim to this subprocess; all other queries pass through.
            binary.write_text(
                '#!/bin/sh\n'
                'if [ "$#" -eq 3 ] && [ "$1" = --sdk ] && '
                '[ "$2" = macosx ] && [ "$3" = --show-sdk-path ]; then\n'
                f'    printf \'%s\\n\' {shlex.quote(str(compatible / "sdk"))}\n'
                'else\n    exec /usr/bin/xcrun "$@"\nfi\n')
            binary.chmod(0o755)
            try:
                stage.rename(compatible)
            except OSError:
                if not compatible.is_dir():
                    raise
    env['PATH'] = str(compatible / 'bin') + os.pathsep + env.get('PATH', os.defpath)
    env['SDKROOT'] = str(compatible / 'sdk')
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--zig', required=True, type=Path)
    parser.add_argument('--sdk-cache', required=True, type=Path)
    parser.add_argument('arguments', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    arguments = args.arguments
    if arguments[:1] == ['--']:
        arguments = arguments[1:]
    try:
        env = macos_environment(args.zig, args.sdk_cache)
        result = subprocess.run([str(args.zig), *arguments], env=env)
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'Cannot run Ghostty\'s Zig compiler: {error}') from error
    raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
