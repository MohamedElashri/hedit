#!/usr/bin/env python3
"""Verify all rendered local links, fragments, assets, and search-index routes."""
import argparse
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urljoin, urlsplit
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids, self.links, self.duplicates = set(), [], []
        self.meta, self.canonicals, self.structured, self.json_errors = {}, [], [], []
        self._json = None
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        if tag == 'meta':
            key = attributes.get('name') or attributes.get('property')
            if key:
                self.meta[key] = attributes.get('content', '')
            if key in {'og:image', 'twitter:image'}:
                self.links.append(attributes.get('content', ''))
        if tag == 'link' and 'canonical' in attributes.get('rel', '').split():
            self.canonicals.append(attributes.get('href', ''))
        if tag == 'script' and attributes.get('type') == 'application/ld+json':
            self._json = []
        for name, value in attrs:
            if name == 'id' and value:
                if value in self.ids:
                    self.duplicates.append(value)
                self.ids.add(value)
            if name in {'href', 'src', 'data-index'} and value:
                self.links.append(value)
            if name == 'srcset' and value:
                self.links.extend(item.strip().split()[0] for item in value.split(','))

    def handle_data(self, data):
        if self._json is not None:
            self._json.append(data)

    def handle_endtag(self, tag):
        if tag == 'script' and self._json is not None:
            try:
                self.structured.append(json.loads(''.join(self._json)))
            except json.JSONDecodeError as error:
                self.json_errors.append(str(error))
            self._json = None


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


def check_seo(public, base_url):
    errors, indexable = [], set()
    base_url = base_url.rstrip('/') + '/'
    required = {'description', 'author', 'robots', 'og:title', 'og:description', 'og:site_name',
                'og:url', 'og:image', 'og:image:alt', 'twitter:card', 'twitter:title',
                'twitter:description', 'twitter:image', 'twitter:image:alt'}
    for path in public.rglob('*.html'):
        text = path.read_text()
        page = Page(text)
        if 'main' not in page.ids:
            continue  # Nida's first-page aliases redirect to a canonical document.
        relative = path.relative_to(public).as_posix()
        route = relative.removesuffix('index.html')
        canonical = base_url + route
        if page.canonicals != [canonical] or page.meta.get('og:url') != canonical:
            errors.append(f'{relative}: incorrect canonical or Open Graph URL')
        for key in required:
            if not page.meta.get(key):
                errors.append(f'{relative}: missing {key} metadata')
        image = page.meta.get('og:image', '')
        if not image.startswith(base_url) or page.meta.get('twitter:image') != image:
            errors.append(f'{relative}: social images must use the absolute site URL')
        if not re.search(r'©\s+\d{4}\s+hedit contributors', text):
            errors.append(f'{relative}: missing copyright footer')
        noindex = route in {'search/', '404.html'}
        if ('noindex' in page.meta.get('robots', '')) != noindex:
            errors.append(f'{relative}: incorrect indexing policy')
        if page.json_errors:
            errors.append(f'{relative}: invalid JSON-LD: {page.json_errors}')
        if noindex:
            continue
        indexable.add(canonical)
        types = {item.get('@type'): item for item in page.structured if isinstance(item, dict)}
        if canonical == base_url:
            if types.get('WebSite', {}).get('url') != canonical:
                errors.append(f'{relative}: missing website structured data')
        else:
            if types.get('WebPage', {}).get('url') != canonical:
                errors.append(f'{relative}: missing webpage structured data')
            items = types.get('BreadcrumbList', {}).get('itemListElement', [])
            if (len(items) < 2 or [item.get('position') for item in items] != list(range(1, len(items) + 1))
                    or items[-1].get('item') != canonical):
                errors.append(f'{relative}: invalid breadcrumb structured data')
    namespace = '{http://www.sitemaps.org/schemas/sitemap/0.9}'
    sitemap = ET.parse(public / 'sitemap.xml')
    urls = [entry.text for entry in sitemap.findall(f'{namespace}url/{namespace}loc')]
    if len(urls) != len(set(urls)) or set(urls) != indexable:
        errors.append('Sitemap must contain exactly the canonical, indexable documentation pages')
    if f'Sitemap: {base_url}sitemap.xml' not in (public / 'robots.txt').read_text():
        errors.append('Generated robots.txt must reference the deployment sitemap')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--public', type=Path, default=ROOT / 'target/docs-site/public')
    parser.add_argument('--base-url', default='https://melashri.net/hedit/')
    args = parser.parse_args()
    errors, count = check(args.public.resolve(), args.base_url)
    errors += check_seo(args.public.resolve(), args.base_url)
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        raise SystemExit(1)
    print(f'Documentation site: {count} HTML pages; links, fragments, assets, search routes, and SEO checks passed.')


if __name__ == '__main__':
    main()
