#!/usr/bin/env python3
"""Run a bounded codec fuzz campaign with LLVM coverage and retain its evidence."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=int, default=300)
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'target/plugin-fuzz')
    parser.add_argument('--sanitizer', choices=['none', 'address'], default='none')
    args = parser.parse_args()
    if not 1 <= args.seconds <= 86400:
        parser.error('--seconds must be between 1 and 86400')
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    corpus = output / 'corpus'
    shutil.copytree(ROOT / 'fuzz/corpus/sdk_protocol', corpus, dirs_exist_ok=True)
    rustc = subprocess.check_output(['rustc', '-vV'], text=True)
    target = next(line.removeprefix('host: ') for line in rustc.splitlines() if line.startswith('host: '))
    flags = ('-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 '
             '-Cllvm-args=-sanitizer-coverage-inline-8bit-counters '
             '-Cllvm-args=-sanitizer-coverage-pc-table '
             '-Cllvm-args=-sanitizer-coverage-trace-compares')
    env = os.environ.copy()
    if args.sanitizer == 'address':
        flags += ' -Zsanitizer=address'
        env['CFLAGS'] = f'{env.get("CFLAGS", "")} -fsanitize=address'.strip()
        env['CXXFLAGS'] = f'{env.get("CXXFLAGS", "")} -fsanitize=address'.strip()
    env['RUSTFLAGS'] = f'{env.get("RUSTFLAGS", "")} {flags}'.strip()
    # An explicit target keeps host build scripts uninstrumented: they do not
    # link libFuzzer and cannot resolve its coverage callbacks.
    command = ['cargo', 'run', '--locked', '--manifest-path', str(ROOT / 'fuzz/Cargo.toml'),
               '--target', target, '--bin', 'sdk_protocol', '--', str(corpus),
               f'-artifact_prefix={output}{os.sep}', '-max_len=1048576', '-timeout=5',
               '-rss_limit_mb=1024', f'-max_total_time={args.seconds}']
    report = {'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(), 'rustc': rustc, 'command': command, 'rustflags': env['RUSTFLAGS'],
              'address_sanitizer': args.sanitizer == 'address',
              'native_build': {key: env.get(key, '') for key in ['CC', 'CXX', 'CFLAGS', 'CXXFLAGS', 'MACOSX_DEPLOYMENT_TARGET']}, 'scope': 'Codec-only LLVM coverage; no Lua or native confinement.'}
    (output / 'metadata.json').write_text(json.dumps(report, indent=2) + '\n')
    with (output / 'campaign.log').open('w') as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    print(f'Fuzz evidence: {output}')
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
