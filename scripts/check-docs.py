#!/usr/bin/env python3
"""Check local Markdown links and the user-guide table of contents."""
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
files = [ROOT / 'README.md', ROOT / 'CONTRIBUTING.md', ROOT / 'ROADMAP.md']
for directory in ['docs', 'contributor', 'assets']:
    files.extend((ROOT / directory).rglob('*.md'))
errors = []
for path in files:
    for target in re.findall(r'\]\(([^)]+)\)', path.read_text()):
        target = target.split(' "', 1)[0].strip('<>')
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc or not parsed.path:
            continue
        resolved = path.parent / unquote(parsed.path)
        if not resolved.exists():
            errors.append(f'{path.relative_to(ROOT)}: missing {target}')
hub = (ROOT / 'docs/README.md').read_text()
for path in (ROOT / 'docs').glob('*.md'):
    if path.name != 'README.md' and f']({path.name}' not in hub:
        errors.append(f'docs/README.md: no link to {path.name}')
if errors:
    print('\n'.join(errors), file=sys.stderr)
    sys.exit(1)
print('Documentation links and user-guide index passed.')
