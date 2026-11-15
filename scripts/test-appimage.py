#!/usr/bin/env python3
"""Exercise detached AppImage windows without FUSE; run under xvfb-run."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('appimage', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='hedit AppImage test ') as directory:
        work = Path(directory)
        env = dict(os.environ, WINIT_UNIX_BACKEND='x11', XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'scratch'))
        for name in ['runtime', 'scratch', 'config', 'data', 'cache', 'state']:
            (work / name).mkdir(mode=0o700)
        for kind in ['CONFIG', 'DATA', 'CACHE', 'STATE']:
            env[f'XDG_{kind}_HOME'] = str(work / kind.lower())
        for name in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'APPIMAGE', 'APPDIR', 'APPIMAGE_EXTRACT_AND_RUN']:
            env.pop(name, None)
        config = work / 'config/hedit'; config.mkdir()
        (config / 'settings.json').write_text(json.dumps(dict(terminal_enabled=False, terminal_onboarding_complete=True)))
        file = work / 'note.txt'; file.write_text('AppImage window test\n')

        def xdo(*arguments, check=True):
            return subprocess.run(['xdotool', *arguments], env=env, check=check, capture_output=True, text=True, timeout=10)

        def windows():
            result = xdo('search', '--onlyvisible', '--class', '^hedit$', check=False)
            assert result.returncode in [0, 1], result.stderr
            return set(result.stdout.split())

        def wait_for(check):
            deadline = time.monotonic() + 30
            while not check():
                if time.monotonic() > deadline:
                    logs = '\n'.join(p.read_text(errors='replace')[-2000:] for p in work.rglob('hedit.log'))
                    raise AssertionError(f'AppImage window check timed out\n{logs}')
                time.sleep(.1)

        def keys(window, sequence):
            xdo('windowfocus', '--sync', window)
            xdo('key', '--clearmodifiers', '--delay', '100', sequence)

        try:
            # The initial command returns, but its detached window must remain usable.
            subprocess.run([str(args.appimage.resolve()), '--appimage-extract-and-run', '--session-dir', str(work / 'session'), str(file)], env=env, check=True, timeout=30)
            wait_for(lambda: len(windows()) == 1)
            first, = windows()
            keys(first, 'ctrl+shift+n')
            wait_for(lambda: len(windows()) == 2)
            second, = windows() - {first}
            xdo('windowfocus', '--sync', second)
            xdo('type', '--clearmodifiers', '--delay', '25', 'Keep this AppImage note')
            keys(first, 'ctrl+q')
            wait_for(lambda: windows() == {second})
            # Opening another window after the original mount is gone catches stale paths.
            keys(second, 'ctrl+shift+n')
            wait_for(lambda: len(windows()) == 2)
            third, = windows() - {second}
            xdo('windowfocus', '--sync', third)
            xdo('type', '--clearmodifiers', '--delay', '25', 'Another AppImage note')
            for window in windows():
                keys(window, 'ctrl+q')
            wait_for(lambda: not windows())
            wait_for(lambda: not list((work / 'runtime').glob('hedit-appimage.*')))
            assert (work / 'session/session.json').is_file()
            sessions = list((work / 'windows').glob('*/session.json'))
            assert len(sessions) == 2, [str(p.relative_to(work)) for p in work.rglob('session.json')]
            print('AppImage detach, new windows, original-window exit, recovery checkpoints, and runtime cleanup passed without FUSE.')
        finally:
            for window in windows():
                result = xdo('getwindowpid', window, check=False)
                if result.returncode == 0:
                    try:
                        os.kill(int(result.stdout.strip()), signal.SIGTERM)
                    except ProcessLookupError:
                        pass


if __name__ == '__main__':
    main()
