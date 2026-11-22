#!/usr/bin/env python3
"""Verify all rendered local links, fragments, assets, and search-index routes."""
import argparse
from html.parser import HTMLParser
import json
from pathlib import Path
import sys
from urllib.parse import unquote, urljoin, urlsplit

ROOT = Path(__file__).resolve().parents[1]


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids, self.links, self.duplicates = set(), [], []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        for name, value in attrs:
            if name == 'id' and value:
                if value in self.ids:
                    self.duplicates.append(value)
                self.ids.add(value)
            if name in {'href', 'src', 'data-index'} and value:
                self.links.append(value)
            if name == 'srcset' and value:
                self.links.extend(item.strip().split()[0] for item in value.split(','))


def check(public, base_url):
    base = urlsplit(base_url)
    prefix = base.path.rstrip('/')
    pages = {p: Page(p.read_text()) for p in public.rglob('*.html')}
    errors = []
    for path, page in pages.items():
        relative = path.relative_to(public).as_posix()
        current = base_url.rstrip('/') + '/' + relative
        for identifier in page.duplicates:
            errors.append(f'{relative}: duplicate id {identifier}')
        for target in page.links:
            parsed = urlsplit(urljoin(current, target))
            if parsed.scheme not in {'http', 'https'} or parsed.netloc != base.netloc:
                continue
            decoded = unquote(parsed.path)
            if prefix and not decoded.startswith(prefix + '/'):
                errors.append(f'{relative}: link escapes the deployment prefix: {target}')
                continue
            destination = (public / decoded.removeprefix(prefix).lstrip('/')).resolve()
            if not destination.is_relative_to(public.resolve()):
                errors.append(f'{relative}: link escapes the site: {target}')
                continue
            if destination.is_dir():
                destination /= 'index.html'
            if not destination.is_file():
                errors.append(f'{relative}: missing {target}')
            elif parsed.fragment and destination in pages and unquote(parsed.fragment) not in pages[destination].ids:
                errors.append(f'{relative}: missing fragment {target}')
    index = (public / 'search-index.js').read_text().strip()
    assignment = 'window.searchIndex = '
    if not index.startswith(assignment):
        errors.append('Unexpected Nida search index format')
    else:
        documents = json.loads(index.removeprefix(assignment).removesuffix(';'))['documentStore']['docs']
        for route in documents:
            if not (public / route.lstrip('/') / 'index.html').is_file():
                errors.append(f'Search index has missing route: {route}')
    return errors, len(pages)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--public', type=Path, default=ROOT / 'target/docs-site/public')
    parser.add_argument('--base-url', default='https://melashri.net/hedit/')
    args = parser.parse_args()
    errors, count = check(args.public.resolve(), args.base_url)
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        raise SystemExit(1)
    print(f'Documentation site: {count} HTML pages; local links, fragments, assets, and search routes passed.')


if __name__ == '__main__':
    main()
