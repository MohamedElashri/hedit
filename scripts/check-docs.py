#!/usr/bin/env python3
"""Check links and section anchors in hedit's documentation entry points."""
from html.parser import HTMLParser
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
files = [ROOT / name for name in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md']]


class HTMLLinks(HTMLParser):
    def __init__(self):
        super().__init__()
        self.targets = []

    def handle_starttag(self, tag, attrs):
        for key, value in attrs:
            if value and key in {'href', 'src', 'srcset'}:
                self.targets.append(value)


def anchors(path):
    """Generate heading IDs for the guides' GitHub-style section links."""
    result = set()
    counts = {}
    fenced = False
    for line in path.read_text().splitlines():
        if line.startswith(('```', '~~~')):
            fenced = not fenced
        if fenced or not re.match(r'^#{1,6} ', line):
            continue
        heading = re.sub(r'^#+ | +#+$', '', line).lower()
        slug = re.sub(r'[^\w\- ]', '', heading).replace(' ', '-')
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(f'{slug}-{count}' if count else slug)
    return result


errors = []
for path in files:
    if not path.is_file():
        errors.append(f'Missing guide: {path.name}')
        continue
    content = path.read_text()
    html = HTMLLinks()
    html.feed(content)
    targets = re.findall(r'\]\(([^)]+)\)', content) + html.targets
    for target in targets:
        target = target.split(' "', 1)[0].strip('<>')
        parsed = urlsplit(target)
        if parsed.scheme or parsed.netloc:
            continue
        resolved = path.parent / unquote(parsed.path) if parsed.path else path
        if not resolved.exists():
            errors.append(f'{path.relative_to(ROOT)}: missing {target}')
        elif parsed.fragment and resolved.suffix == '.md':
            if unquote(parsed.fragment) not in anchors(resolved):
                errors.append(f'{path.relative_to(ROOT)}: missing section {target}')
if errors:
    print('\n'.join(errors), file=sys.stderr)
    sys.exit(1)
print('Documentation links, assets, and section anchors passed.')
