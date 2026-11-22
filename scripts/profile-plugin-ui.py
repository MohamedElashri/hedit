#!/usr/bin/env python3
"""Record matched native-LSP UI, allocation and host-footprint workloads on macOS."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def quantiles(values):
    ordered = sorted(values)
    return {**{f'p{int(q*100)}': ordered[math.ceil(q*len(ordered))-1]
               for q in [0.5, 0.95, 0.99]}, 'max': max(ordered)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--frames', type=int, default=1200)
    parser.add_argument('--debug', action='store_true')
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'target/plugin-validation/scoped-ui')
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        parser.error('This probe uses macOS native process accounting.')
    if not 20 <= args.frames <= 3600:
        parser.error('Supply 20–3600 frames; the full workload uses 1200 or more.')
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    paths = [ROOT/'Cargo.toml', ROOT/'Cargo.lock', Path(__file__).resolve()]
    for directory in ['src', 'crates', 'schemas', 'examples/extensions']:
        paths += [path for path in (ROOT/directory).rglob('*')
                  if path.is_file() and path.suffix in {'.rs', '.lua', '.json', '.toml'}]
    paths += [path for path in (ROOT/'docs').rglob('*.md')]
    paths += [ROOT/name for name in ['USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md']]
    hashes = {}
    for path in sorted(set(paths)):
        content = path.read_bytes()
        relative = path.relative_to(ROOT)
        hashes[str(relative)] = hashlib.sha256(content).hexdigest()
        archived = output/'source'/relative
        archived.parent.mkdir(parents=True, exist_ok=True)
        archived.write_bytes(content)
    profile = [] if args.debug else ['--release']
    build = subprocess.run(['cargo', 'build', '--locked', '--no-default-features',
                            '--workspace', *profile], cwd=ROOT, text=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (output/'build.log').write_text(build.stdout+build.stderr)
    build.check_returncode()
    compile_test = subprocess.run(['cargo', 'test', '--locked', '--no-default-features',
                                  '--bin', 'hedit', *profile, '--no-run', '--message-format=json'],
                                 cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (output/'test-build.log').write_text(compile_test.stderr)
    compile_test.check_returncode()
    executables = [message['executable'] for line in compile_test.stdout.splitlines()
                   if (message := json.loads(line)).get('reason') == 'compiler-artifact'
                   and message.get('executable') and message['target']['name'] == 'hedit']
    if len(executables) != 1:
        raise RuntimeError('Expected one editor test executable.')
    environment = dict(os.environ, HEDIT_PLUGIN_UI_PROBE_OUT=str(output/'samples.json'),
                       HEDIT_PLUGIN_UI_PROBE_FRAMES=str(args.frames))
    result = subprocess.run([executables[0], 'profile_scoped_native_frames', '--ignored',
                             '--test-threads=1', '--nocapture'], cwd=ROOT, env=environment,
                            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    (output/'run.log').write_text(result.stdout+result.stderr)
    result.check_returncode()
    report = json.loads((output/'samples.json').read_text())
    groups = {}
    for workload in report['workloads']:
        groups.setdefault(workload['mode'], []).append(workload)
    summary = {}
    for mode, workloads in groups.items():
        samples = [sample for workload in workloads
                   for sample in workload['samples_us_allocations_requested_bytes']]
        summary[mode] = {
            'samples': len(samples),
            'cpu_frame_us': quantiles([sample[0] for sample in samples]),
            'ui_allocations': quantiles([sample[1] for sample in samples]),
            'ui_requested_bytes': quantiles([sample[2] for sample in samples]),
            'published_results': sum(w['published_results'] for w in workloads),
            'stop_ui_us': quantiles([w['stop_ui_us'] for w in workloads]),
            'stop_cleanup_us': quantiles([w['stop_cleanup_us'] for w in workloads]),
            'footprint_start_end_bytes': [
                [w['usage_before']['footprint_bytes'], w['usage_after']['footprint_bytes'],
                 w['usage_after_stop']['footprint_bytes']] for w in workloads],
            'self_cpu_us': [w['usage_after']['cpu_us']-w['usage_before']['cpu_us'] for w in workloads],
        }
    comparisons = {mode: summary[mode]['cpu_frame_us']['p95'] /
                    summary[baseline]['cpu_frame_us']['p95'] - 1
                   for mode, baseline in [('native_idle', 'disabled_idle'),
                                          ('typing_with_native', 'disabled_typing')]}
    changed = [name for name, digest in hashes.items()
               if hashlib.sha256((ROOT/name).read_bytes()).hexdigest() != digest]
    if changed:
        raise RuntimeError(f'Source changed during the probe: {changed}')
    metadata = {'platform': platform.platform(), 'machine': platform.machine(),
                'rustc': subprocess.check_output(['rustc', '-vV'], text=True),
                'hardware': {key: subprocess.check_output(['sysctl', '-n', key], text=True).strip()
                             for key in ['hw.model', 'hw.memsize', 'hw.physicalcpu',
                                         'machdep.cpu.brand_string']},
                'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                'source_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
                'source_sha256': hashes, 'frames_per_workload': args.frames,
                'profile': 'debug' if args.debug else 'release', 'default_features': False,
                'build_environment': {key: os.environ.get(key, '') for key in
                    ['RUSTFLAGS', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_RELEASE_LTO', 'CARGO_INCREMENTAL']},
                'test_executable_sha256': hashlib.sha256(Path(executables[0]).read_bytes()).hexdigest(),
                'summary': summary, 'p95_regression_fraction': comparisons,
                'ui_thread_qos': report['ui_thread_qos'], 'scope': report['scope']}
    (output/'metadata.json').write_text(json.dumps(metadata, indent=2)+'\n')
    print(json.dumps({'summary': summary, 'p95_regression_fraction': comparisons}, indent=2))


if __name__ == '__main__':
    main()
