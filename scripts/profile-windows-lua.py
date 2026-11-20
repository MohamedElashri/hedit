#!/usr/bin/env python3
"""Record real confined Windows fixture timings; CI timings are diagnostic only."""
import argparse
import json
import math
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--editor', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    editor = args.editor.resolve()
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        manifest, fixture = root / 'plugin.json', root / 'fixtures.json'
        manifest.write_text(json.dumps({
            'api_version': 2, 'id': 'latency',
            'permissions': ['selection.read', 'selection.replace'],
            'scripts': {'run.lua': 'return function(text) return text end'},
            'commands': [{'id': 'run', 'title': 'Latency', 'action': {'kind': 'lua', 'source': 'run.lua'}}],
        }), encoding='utf-8')
        results = {'conditions': 'CI diagnostics; warm filesystem caches; fresh confined process per fixture', 'samples': {}}
        for label, text in [('unicode', 'café\r\n東京'), ('64k', 'x' * 65536)]:
            fixture.write_text(json.dumps([{'command': 'latency.run', 'input': text, 'expected': text}]), encoding='utf-8')
            samples = []
            for index in range(23):
                start = time.perf_counter()
                reply = subprocess.run([str(editor), 'plugin', 'test', str(manifest), str(fixture)], capture_output=True, timeout=10)
                if reply.returncode:
                    raise RuntimeError(reply.stderr.decode('utf-8', errors='replace'))
                if index >= 3:
                    samples.append((time.perf_counter() - start) * 1000)
            results['samples'][label] = {'median_ms': statistics.median(samples), 'p95_ms': sorted(samples)[math.ceil(.95 * len(samples)) - 1], 'samples_ms': samples}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(results, indent=2), encoding='utf-8')
        print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
