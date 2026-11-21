#!/usr/bin/env python3
"""Measure confined broker callbacks and host work on disposable fixture files."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--trials', type=int, default=25)
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'target/plugin-validation/broker-profile')
    args = parser.parse_args()
    if not 1 <= args.trials <= 100:
        parser.error('--trials must be between 1 and 100')
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    paths = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'LICENSE',
             ROOT / 'crates/hedit-core/Cargo.toml', ROOT / 'crates/hedit-broker/Cargo.toml',
             ROOT / 'crates/hedit-plugin/Cargo.toml', Path(__file__).resolve()]
    for directory in ['crates/hedit-core/src', 'crates/hedit-broker/src',
                      'crates/hedit-broker/tests', 'crates/hedit-plugin/src']:
        paths += sorted((ROOT / directory).rglob('*.rs'))
        paths += sorted((ROOT / directory).rglob('*.lua'))
    hashes = {}
    for path in paths:
        content = path.read_bytes()
        relative = path.relative_to(ROOT)
        hashes[str(relative)] = hashlib.sha256(content).hexdigest()
        archived = output / 'source' / relative
        archived.parent.mkdir(parents=True, exist_ok=True)
        archived.write_bytes(content)
    build = subprocess.run(['cargo', 'test', '--locked', '--release', '-p', 'hedit-broker',
                            '--test', 'native_continuation', '--no-run', '--message-format=json'],
                           cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (output / 'build.log').write_text(build.stderr)
    build.check_returncode()
    executables = [message['executable'] for line in build.stdout.splitlines()
                   if (message := json.loads(line)).get('reason') == 'compiler-artifact'
                   and message.get('executable') and message['target']['name'] == 'native_continuation']
    if len(executables) != 1:
        raise RuntimeError('Expected one native continuation test executable.')
    result = subprocess.run([executables[0], '--measure', str(args.trials)], cwd=ROOT,
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (output / 'samples.jsonl').write_text(result.stdout)
    (output / 'run.log').write_text(result.stderr)
    result.check_returncode()
    samples = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
    groups = {}
    for sample in samples:
        groups.setdefault(sample['probe'], []).append(sample['elapsed_us'])
    summary = {}
    for key, values in groups.items():
        ordered = sorted(values)
        summary[key] = {'count': len(values), 'first_us': values[0], 'max_us': max(values),
                        **{f'p{int(q * 100)}_us': ordered[math.ceil(q * len(ordered)) - 1]
                           for q in [0.5, 0.95, 0.99]}}
    changed = [str(path.relative_to(ROOT)) for path in paths
               if hashlib.sha256(path.read_bytes()).hexdigest() != hashes[str(path.relative_to(ROOT))]]
    if changed:
        raise RuntimeError(f'Source changed during the probe; repeat with a stable tree: {changed}')
    metadata = {'platform': platform.platform(), 'machine': platform.machine(),
                'rustc': subprocess.check_output(['rustc', '-vV'], text=True),
                'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                'source_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
                'source_sha256': hashes,
                'build_env': {key: os.environ.get(key, '') for key in ['RUSTFLAGS', 'CARGO_TARGET_DIR', 'CC', 'CXX']},
                'trials': args.trials, 'summary': summary,
                'scope': 'Release build; two small Unicode/CRLF files; real confined callbacks, batch read, preview and immediate commit. No human dwell, physical UI/input latency, memory/allocation or large-workspace claim.'}
    (output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(json.dumps(summary, indent=2))
    print(f'Broker timing evidence: {output}')


if __name__ == '__main__':
    main()
