#!/usr/bin/env python3
"""Assemble the shared offline guides and authored web pages, then run Nida."""
import argparse
import json
from pathlib import Path
import posixpath
import re
import shutil
import subprocess
import sys
import tomllib
from urllib.parse import unquote, urlsplit
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / 'website'
OUTPUT = ROOT / 'target/docs-site'
REPOSITORY = 'https://github.com/MohamedElashri/hedit'


def headings(text):
    """Read ATX headings outside code fences, with GitHub-style source anchors."""
    records, counts, fence = [], {}, None
    for number, line in enumerate(text.splitlines()):
        marker = re.match(r'^ {0,3}(`{3,}|~{3,})(.*)$', line)
        if marker:
            run, tail = marker.groups()
            if fence is None:
                fence = (run[0], len(run))
            elif run[0] == fence[0] and len(run) >= fence[1] and not tail.strip():
                fence = None
            continue
        match = re.match(r'^(#{1,6}) (.+?)(?: +#+)?$', line)
        if fence or not match:
            continue
        level, title = len(match[1]), match[2]
        slug = re.sub(r'[^\w\- ]', '', title.lower()).replace(' ', '-')
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        records.append((number, level, title, f'{slug}-{count}' if count else slug))
    return records


def extract(text, page):
    records, lines = headings(text), text.splitlines()
    if 'heading' in page:
        matches = [r for r in records if r[2] == page['heading']]
        if len(matches) != 1:
            raise ValueError(f"Expected one heading {page['heading']!r} in {page['source']}")
        top = matches[0]
    else:
        top = records[0]
        if top[1] != 1:
            raise ValueError(f"Missing document title in {page['source']}")
    start = top[0]
    end = next((r[0] for r in records if r[0] > start and r[1] <= top[1]), len(lines))
    excluded = set()
    for title in page.get('exclude', []):
        matches = [r for r in records if start < r[0] < end and r[2] == title]
        if len(matches) != 1:
            raise ValueError(f'Expected one excluded subsection {title!r}')
        item = matches[0]
        stop = next((r[0] for r in records if item[0] < r[0] < end and r[1] <= item[1]), end)
        excluded.update(range(item[0], stop))
    kept = [r for r in records if start < r[0] < end and r[0] not in excluded]
    by_line = {r[0]: r for r in kept}
    body = []
    shift = top[1] - 1
    for number in range(start + 1, end):
        if number in excluded:
            continue
        line = lines[number]
        if number in by_line:
            record = by_line[number]
            line = '#' * (record[1] - shift) + ' ' + record[2]
        body.append(line)
    body = '\n'.join(body).strip() + '\n'
    mapped = [(top[3], '')] + [(old[3], new[3]) for old, new in zip(kept, headings(body), strict=True)]
    return body, start + 1, mapped


def rewrite_target(target, source, routes, anchors, base_path, root=ROOT):
    parsed = urlsplit(target)
    if parsed.scheme or parsed.netloc or target.startswith('@/'):
        return target
    relative = posixpath.normpath(posixpath.join(posixpath.dirname(source), unquote(parsed.path))) if parsed.path else source
    fragment = unquote(parsed.fragment)
    if relative in routes:
        if fragment:
            if (relative, fragment) not in anchors:
                raise ValueError(f'{source}: unknown section {target}')
            destination, anchor = anchors[(relative, fragment)]
        else:
            destination, anchor = routes[relative], ''
        return '@/' + destination + (f'#{anchor}' if anchor else '')
    if not (root / relative).is_file():
        raise ValueError(f'{source}: missing link {target}')
    if relative.startswith('docs/images/'):
        return base_path + '/images/' + relative.removeprefix('docs/images/')
    if relative.startswith(('examples/extensions/', 'schemas/')):
        return base_path + '/downloads/' + relative
    return REPOSITORY + '/blob/main/' + relative + (f'#{parsed.fragment}' if parsed.fragment else '')


def rewrite_links(body, rewrite):
    """Leave example code intact while remapping prose links and HTML assets."""
    result, fence = [], None
    for line in body.splitlines(keepends=True):
        marker = re.match(r'^ {0,3}(`{3,}|~{3,})(.*)$', line.rstrip('\n'))
        if marker:
            run, tail = marker.groups()
            if fence is None:
                fence = (run[0], len(run))
            elif run[0] == fence[0] and len(run) >= fence[1] and not tail.strip():
                fence = None
            result.append(line)
            continue
        if not fence:
            line = re.sub(r'(\]\()([^\s)]+)(\))', lambda m: m[1] + rewrite(m[2]) + m[3], line)
            line = re.sub(r'((?:href|src|srcset)=")([^"]+)(")', lambda m: m[1] + rewrite(m[2]) + m[3], line)
        result.append(line)
    return ''.join(result)


def front_matter(page, source_line):
    fields = {key: page[key] for key in ('title', 'description', 'weight')}
    fields['template'] = 'page'
    data = '\n'.join(f'{key} = {json.dumps(value, ensure_ascii=False)}' for key, value in fields.items())
    source_url = f"{REPOSITORY}/edit/main/{page['source']}#L{source_line}"
    return f'+++\n{data}\n[extra]\nsource_url = {json.dumps(source_url)}\n+++\n\n'


def prepare(base_url=None):
    imports = tomllib.loads((SITE / 'imports.toml').read_text())
    config = tomllib.loads((SITE / 'config.toml').read_text())
    base_path = urlsplit(base_url or config['base_url']).path.rstrip('/')
    pages = imports['pages']
    routes, anchors, extracted = dict(imports['documents']), {}, []
    for page in pages:
        body, line, mapped = extract((ROOT / page['source']).read_text(), page)
        routes.setdefault(page['source'], page['path'])
        for anchor, local in mapped:
            key = (page['source'], anchor)
            if key in anchors:
                raise ValueError(f'Duplicate import: {key}')
            anchors[key] = (page['path'], local)
        extracted.append((page, body, line))
    if OUTPUT.is_symlink() or (ROOT / 'target').is_symlink():
        raise ValueError('Documentation output must not be a symlink')
    if OUTPUT.exists():
        shutil.rmtree(OUTPUT)
    OUTPUT.mkdir(parents=True)
    for name in ('content', 'templates', 'static'):
        shutil.copytree(SITE / name, OUTPUT / name)
    config_text = (SITE / 'config.toml').read_text()
    if base_url:
        config_text, count = re.subn(r'(?m)^base_url\s*=.*$',
                                     'base_url = ' + json.dumps(base_url.rstrip('/') + '/'), config_text)
        if count != 1:
            raise ValueError('Expected one base_url in the site configuration')
    (OUTPUT / 'config.toml').write_text(config_text)
    for page, body, line in extracted:
        destination = OUTPUT / 'content' / page['path']
        if destination.exists():
            raise ValueError(f"Authored/imported page collision: {page['path']}")
        destination.parent.mkdir(parents=True, exist_ok=True)
        body = rewrite_links(body, lambda target: rewrite_target(target, page['source'], routes, anchors, base_path))
        destination.write_text(front_matter(page, line) + body)
    for page in (SITE / 'content').rglob('*.md'):
        destination = OUTPUT / 'content' / page.relative_to(SITE / 'content')
        content = destination.read_text()
        end = content.index('+++', 3)
        source = 'website/content/' + page.relative_to(SITE / 'content').as_posix()
        extra = '\n[extra]\nsource_url = ' + json.dumps(REPOSITORY + '/edit/main/' + source) + '\n'
        destination.write_text(content[:end].rstrip() + extra + content[end:])
    shutil.copytree(ROOT / 'docs/images', OUTPUT / 'static/images')
    shutil.copy2(ROOT / 'assets/hedit.svg', OUTPUT / 'static/favicon.svg')
    for name in ('examples/extensions', 'schemas'):
        shutil.copytree(ROOT / name, OUTPUT / 'static/downloads' / name)
    tutorial = OUTPUT / 'static/examples/first-plugin'
    archive = OUTPUT / 'static/downloads/first-plugin.zip'
    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(tutorial.iterdir()):
            entry = zipfile.ZipInfo('first-plugin/' + path.name, date_time=(2026, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = 0o100644 << 16
            bundle.writestr(entry, path.read_bytes())
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    (OUTPUT / 'data').mkdir()
    (OUTPUT / 'data/project.json').write_text(json.dumps({'version': version}) + '\n')
    print(f'Assembled {len(list((OUTPUT / "content").rglob("*.md")))} pages in {OUTPUT}', flush=True)
    return OUTPUT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare-only', action='store_true', help='Assemble source for nida serve without building')
    parser.add_argument('--nida', default='nida', help='Path to the pinned Nida executable')
    parser.add_argument('--base-url', help='Override the deployment URL (including its path prefix)')
    args = parser.parse_args()
    source = prepare(args.base_url)
    if args.prepare_only:
        return
    expected = json.loads((SITE / 'nida.lock.json').read_text())['version']
    actual = subprocess.check_output([args.nida, 'version'], text=True).strip()
    if actual != f'nida version {expected}':
        raise SystemExit(f'Expected Nida {expected}; found {actual}. Run python3 scripts/setup-nida.py.')
    for command in ('check', 'build'):
        subprocess.run([args.nida, command, '--site', str(source)], check=True)
    subprocess.run([sys.executable, str(ROOT / 'scripts/check-docs-site.py'), '--base-url', args.base_url or
                    tomllib.loads((SITE / 'config.toml').read_text())['base_url']], check=True)


if __name__ == '__main__':
    main()
