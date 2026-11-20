#!/usr/bin/env python3
"""Prepare the shared editor executable as a confined macOS Lua worker."""
import argparse
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

IDENTIFIER = 'hedit.hedit.plugin-worker'
APP_NAME = 'hedit-plugin.app'


def prepare(editor, output, identity='-'):
    editor, output = Path(editor).resolve(), Path(output).resolve()
    if sys.platform != 'darwin':
        return None
    if not editor.is_file():
        raise ValueError(f'Build the editor before preparing its Lua helper: {editor}')
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.hedit-plugin-', dir=output.parent) as temporary:
        temporary = Path(temporary)
        app = temporary / APP_NAME
        contents = app / 'Contents'
        binary = contents / 'MacOS/hedit'
        binary.parent.mkdir(parents=True)
        shutil.copy2(editor, binary)
        (contents / 'Info.plist').write_bytes(plistlib.dumps({
            'CFBundleIdentifier': IDENTIFIER,
            'CFBundleExecutable': 'hedit',
            'CFBundleName': 'hedit Lua worker',
            'CFBundlePackageType': 'APPL',
            'LSMinimumSystemVersion': '11.0',
            'LSBackgroundOnly': True,
        }))
        entitlements = temporary / 'worker.entitlements'
        entitlements.write_bytes(plistlib.dumps({'com.apple.security.app-sandbox': True}))
        signing = ['codesign', '--force', '--sign', identity, '--options', 'runtime',
                   '--entitlements', str(entitlements)]
        if identity != '-':
            signing.append('--timestamp')
        subprocess.run([*signing, str(app)], check=True)
        subprocess.run(['codesign', '--verify', '--strict', str(app)], check=True)
        backup = temporary / 'previous.app'
        if output.exists():
            info = plistlib.loads((output / 'Contents/Info.plist').read_bytes())
            if info.get('CFBundleIdentifier') != IDENTIFIER:
                raise ValueError(f'Refusing to replace a different application: {output}')
            output.rename(backup)
        try:
            app.rename(output)
        except BaseException:
            if backup.exists():
                backup.rename(output)
            raise
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--editor', required=True, type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--identity', default='-')
    args = parser.parse_args()
    result = prepare(args.editor, args.output or args.editor.parent / APP_NAME, args.identity)
    if result:
        print(result)


if __name__ == '__main__':
    main()
