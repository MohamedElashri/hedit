#!/usr/bin/env python3
"""Export real README views, including SSH and checkpoint/restart recovery.

Requires a terminal-enabled build, OpenSSH server/client/keygen, and FFmpeg.
The disposable SSH server uses only loopback, generated keys, and private state.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def build(cargo, arguments, target):
    result = subprocess.run([*cargo, *arguments, '--message-format=json'], cwd=ROOT,
                            capture_output=True, text=True)
    artifacts = []
    for line in result.stdout.splitlines():
        row = json.loads(line)
        if row.get('reason') == 'compiler-message' and row['message']['level'] == 'error':
            print(row['message']['rendered'])
        if row.get('reason') == 'compiler-artifact' and row.get('executable') and row['target']['name'] == target:
            artifacts.append(Path(row['executable']))
    if result.returncode:
        raise RuntimeError(result.stderr)
    if len(artifacts) != 1:
        raise RuntimeError(f'Expected one {target} executable, found {artifacts}')
    return artifacts[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'docs/images')
    parser.add_argument('--work-dir', type=Path, default=os.environ.get('HEDIT_REMOTE_LIVE_DIR'),
                        help='SSH fixture parent on private durable storage; see developer guide')
    args = parser.parse_args()
    ffmpeg = shutil.which('ffmpeg')
    if ffmpeg is None or os.name != 'posix':
        parser.error('Use Linux/macOS with FFmpeg and OpenSSH installed')
    cargo = shlex.split(os.environ.get('CARGO', 'cargo'))
    editor = build(cargo, ['test', '--locked', '--bin', 'hedit', '--no-run'], 'hedit')
    helper = build(cargo, ['build', '--locked', '-p', 'hedit-remote'], 'hedit-remote')
    live = module('hedit_readme_ssh', 'test-remote-live.py')
    ppm = module('hedit_readme_png', 'ppm-to-png.py')
    live.ARGS = SimpleNamespace(work_dir=args.work_dir, debug_server=False)
    lab = live.Lab()
    try:
        lab.start()
        root = lab.base / 'atlas'
        root.mkdir(mode=0o700)
        fixture = dict(port=lab.port, config=str(lab.client_config), helper=str(helper),
                       state=str(lab.base / 'state'), root=str(root))
        with tempfile.TemporaryDirectory(prefix='hedit-readme-frames-') as frames:
            frames = Path(frames)
            env = dict(os.environ, HEDIT_PREVIEW_DIR=str(frames),
                       HEDIT_REMOTE_TEST_CONFIG=json.dumps(fixture))
            subprocess.run([str(editor), 'export_readme_', '--ignored', '--nocapture',
                            '--test-threads=1'], env=env, cwd=ROOT, check=True)
            args.output_dir.mkdir(parents=True, exist_ok=True)
            names = ['screenshot-dark', 'screenshot-light', 'git', 'markdown', 'search',
                     'split-panes', 'terminal', 'themes', 'ssh']
            for name in names:
                ppm.write_png(args.output_dir / f'{name}.png',
                              *ppm.read_ppm(frames / f'{name}.ppm'))
            ppm.write_png(args.output_dir / 'recovery.png',
                          *ppm.read_ppm(frames / 'recovery-03.ppm'))
            subprocess.run([ffmpeg, '-loglevel', 'error', '-y', '-framerate', '1/3',
                            '-start_number', '1', '-i', str(frames / 'recovery-%02d.ppm'),
                            '-filter_complex', 'scale=1280:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse',
                            '-loop', '0', str(args.output_dir / 'recovery.gif')], check=True)
    finally:
        lab.close()
    print(f'Exported 10 screenshots and the recovery animation to {args.output_dir}')


if __name__ == '__main__':
    main()
