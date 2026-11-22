#!/usr/bin/env python3
"""Report and clean compiler output without removing tools, reports, or source."""
import argparse
from contextlib import ExitStack, contextmanager
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
LOCKS = ('.cargo-lock', '.cargo-build-lock', '.cargo-artifact-lock')
PROFILES = {'debug', 'release', 'debugging'}
AUXILIARY_TARGETS = {'remote-fault-tests', 'cargo-build'}


def git(*arguments):
    return subprocess.check_output(['git', '-C', str(ROOT), *arguments])


def worktrees():
    return [Path(field.removeprefix(b'worktree ').decode('utf-8'))
            for field in git('worktree', 'list', '--porcelain', '-z').split(b'\0')
            if field.startswith(b'worktree ')]


def shared_directory():
    common = Path(git('rev-parse', '--path-format=absolute', '--git-common-dir').decode().strip())
    return common.parent / 'target/cargo-build'


def require_directory(path):
    if path.is_symlink() or not path.is_dir():
        raise ValueError(f'Refusing a symlink or non-directory: {path}')


def cargo_profile(path):
    return path.name in PROFILES and path.is_dir() and any((path / name).exists() for name in LOCKS)


def profiles_in(target):
    """Recognize Cargo profiles, target triples, and nested Cargo target roots."""
    require_directory(target)
    found = []
    for child in sorted(target.iterdir()):
        if cargo_profile(child):
            require_directory(child)
            found.append(child)
        elif child.is_dir() and not child.is_symlink():
            # Architecture outputs and auxiliary builds (e.g. remote-fault-tests).
            # Never traverse arbitrary validation/report/tool directories.
            nested_cargo = child.name in AUXILIARY_TARGETS and (child / '.rustc_info.json').is_file()
            target_triple = re.fullmatch(
                r'[a-zA-Z0-9_]+-(apple-darwin|unknown-linux-(gnu|musl)|pc-windows-(msvc|gnu))', child.name) and any(
                cargo_profile(child / name) for name in PROFILES)
            if nested_cargo or target_triple:
                found.extend(profiles_in(child))
    return found


def targets(selected_worktrees, explicit=None):
    if explicit is not None:
        candidates = [Path(explicit).absolute()]
    else:
        candidates = [tree / relative for tree in selected_worktrees
                      for relative in ('target', 'fuzz/target')]
    result = []
    for path in candidates:
        if path.is_symlink():
            raise ValueError(f'Refusing a symlink target: {path}; select its real directory explicitly')
        if path.is_dir() and path.resolve() not in result:
            result.append(path.resolve())
    return result


def allocated_size(path):
    """Count allocated blocks, not logical sizes; do not follow symlinks."""
    total, seen = 0, set()
    pending = [path]
    while pending:
        current = pending.pop()
        try:
            info = current.lstat()
        except FileNotFoundError:
            continue
        identity = (info.st_dev, info.st_ino)
        if identity in seen:
            continue
        seen.add(identity)
        total += getattr(info, 'st_blocks', (info.st_size + 511) // 512) * 512
        if stat.S_ISDIR(info.st_mode):
            pending.extend(current.iterdir())
    return total


def size_text(amount):
    return f'{amount / (1024 ** 3):.2f} GiB' if amount >= 1024 ** 3 else f'{amount / (1024 ** 2):.1f} MiB'


@contextmanager
def profile_lock(profile):
    """Keep Cargo's existing lock inodes in place, including during cleanup."""
    require_directory(profile)
    with ExitStack() as stack:
        for name in LOCKS:
            path = profile / name
            if path.is_symlink():
                raise ValueError(f'Refusing a symlink Cargo lock: {path}')
            stream = stack.enter_context(path.open('a+b'))
            try:
                if os.name == 'nt':
                    import msvcrt
                    stream.seek(0)
                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    import fcntl
                    fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as error:
                raise ValueError(f'Cargo is using {profile}; stop that build and retry') from error
        yield


def plan_cleanup(target_dirs, kind):
    planned = []
    for target in target_dirs:
        for profile in profiles_in(target):
            if kind == 'dev' and profile.name == 'release':
                continue
            if kind == 'release' and profile.name != 'release':
                continue
            path = profile / 'incremental' if kind == 'incremental' else profile
            if path.is_symlink():
                raise ValueError(f'Refusing a symlink cleanup directory: {path}')
            if path.is_dir():
                planned.append((profile, path))
    return planned


def apply_cleanup(planned):
    # Acquire every selected profile before deleting anything. A busy build must
    # not leave an all-worktree request half-cleaned.
    with ExitStack() as stack:
        for profile in sorted({profile for profile, _ in planned}):
            stack.enter_context(profile_lock(profile))
        for profile, path in planned:
            require_directory(profile)
            require_directory(path)
            if path != profile:
                shutil.rmtree(path)
                continue
            for entry in profile.iterdir():
                if entry.name in LOCKS:
                    continue
                if entry.is_dir() and not entry.is_symlink():
                    shutil.rmtree(entry)
                else:
                    entry.unlink()


def report(target_dirs):
    total = 0
    for target in target_dirs:
        amount = allocated_size(target)
        total += amount
        print(f'{size_text(amount):>12}  {target}', flush=True)
        for profile in profiles_in(target):
            amount = allocated_size(profile)
            cache = profile / 'incremental'
            incremental = allocated_size(cache) if cache.is_dir() and not cache.is_symlink() else 0
            print(f'  {size_text(amount):>12}  {profile.relative_to(target)}'
                  f'  (incremental: {size_text(incremental)})', flush=True)
    print(f'Estimated generated storage: {size_text(total)}', flush=True)
    print('Shared blocks and filesystem snapshots can change the space reclaimed.')
    return total


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', nargs='?', choices=('report', 'clean', 'shared-dir'), default='report')
    parser.add_argument('--kind', choices=('incremental', 'dev', 'release', 'builds'), default='builds')
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--all-worktrees', action='store_true')
    group.add_argument('--target-dir', type=Path, help='Explicit compiler-output/intermediate directory')
    parser.add_argument('--apply', action='store_true', help='Delete the previewed compiler output')
    args = parser.parse_args(argv)
    if args.action == 'shared-dir':
        print(shared_directory())
        return
    try:
        trees = worktrees() if args.all_worktrees else [ROOT]
        directories = targets(trees, args.target_dir)
        free = shutil.disk_usage(ROOT).free
        print(f'Free on workspace volume: {size_text(free)}', flush=True)
        if args.action == 'report':
            report(directories)
            return
        planned = plan_cleanup(directories, args.kind)
        total = 0
        for _, path in planned:
            amount = allocated_size(path)
            total += amount
            print(f'{size_text(amount):>12}  {path}', flush=True)
        print(f'{"Cleaning" if args.apply else "Preview"}: {size_text(total)} of {args.kind} output.', flush=True)
        if not args.apply:
            print('No files removed. Add --apply to clean these paths.')
            return
        apply_cleanup(planned)
        print('Cleaned compiler output; source, toolchains, reports and session data were retained.')
        print(f'Free on workspace volume: {size_text(shutil.disk_usage(ROOT).free)}')
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
